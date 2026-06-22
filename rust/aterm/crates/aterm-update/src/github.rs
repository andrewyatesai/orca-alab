// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0

//! The network layer: talk to the PRIVATE GitHub Releases API over `curl`, find
//! the newest release carrying an `aterm-appcast.toml`, and — if it is strictly
//! newer than the running build — download + verify its DMG and stage it.
//!
//! Private repos require the API (the `releases/latest/download/…` browser
//! shortcut needs web auth), so we authenticate every request with the per-machine
//! token (see [`crate::token`]) and download asset bytes via the asset API URL
//! with `Accept: application/octet-stream` (curl `-L` follows the 302 to storage
//! and drops the `Authorization` header on the cross-host redirect by default).
//!
//! The token is fed to curl through STDIN ([`curl_auth`], `curl --config -`), never
//! on argv, so it is not exposed to same-user processes via `ps`.

use std::path::Path;
use std::process::Command;

use serde::Deserialize;

use crate::manifest::Manifest;
use crate::{OWNER, PINNED_TEAM_ID, REPO, bundle, install, paths::Staging, token, verify};

/// A GitHub Release (subset). Unknown fields are ignored.
#[derive(Debug, Deserialize)]
struct Release {
    #[serde(default)]
    assets: Vec<Asset>,
}

/// A release asset (subset).
#[derive(Debug, Deserialize)]
struct Asset {
    name: String,
    /// The asset's API URL (`…/releases/assets/<id>`), used for the octet download.
    url: String,
    #[serde(default)]
    size: u64,
}

impl Release {
    fn asset(&self, name: &str) -> Option<&Asset> {
        self.assets.iter().find(|a| a.name == name)
    }
}

/// Background check + stage. Returns the staged version string on success, or
/// `None` when nothing newer is available / the updater is idle. Errors are
/// transient/operational (network, parse) and are logged by the caller.
pub fn check_and_stage(
    current_build: u64,
    _current_version: &str,
) -> Result<Option<String>, String> {
    // Only stage for a real installed bundle (a dev build has nothing to swap).
    if bundle::resolve().is_none() {
        return Ok(None);
    }
    let staging = Staging::resolve().ok_or("could not resolve Updates dir")?;
    // The Application Support dir is the Updates dir's parent.
    let support = staging.root.parent().ok_or("no support dir")?.to_path_buf();
    let Some(tok) = token::resolve(&support) else {
        // No token provisioned → stay idle (a private repo can't be read).
        return Ok(None);
    };

    // List recent releases (newest first) and pick the one with the highest
    // build_number that carries an appcast — robust whether or not releases are
    // marked prerelease (unlike `/releases/latest`, which skips prereleases).
    let url = format!("https://api.github.com/repos/{OWNER}/{REPO}/releases?per_page=20");
    let body = api_get(&url, &tok)?;
    let releases: Vec<Release> =
        serde_json::from_slice(&body).map_err(|e| format!("parse releases JSON: {e}"))?;

    let mut best: Option<(Manifest, usize)> = None;
    for (i, rel) in releases.iter().enumerate() {
        let Some(asset) = rel.asset("aterm-appcast.toml") else {
            continue;
        };
        let bytes = match download_bytes(&asset.url, &tok) {
            Ok(b) => b,
            Err(e) => {
                crate::warn(&format!("fetch appcast for release #{i}: {e}"));
                continue;
            }
        };
        let text = String::from_utf8_lossy(&bytes);
        match Manifest::parse(&text) {
            Ok(m) => {
                if best
                    .as_ref()
                    .is_none_or(|(b, _)| m.build_number > b.build_number)
                {
                    best = Some((m, i));
                }
            }
            Err(e) => crate::warn(&format!("parse appcast for release #{i}: {e}")),
        }
    }

    let Some((manifest, rel_idx)) = best else {
        return Ok(None); // no release carries a manifest
    };

    // Downgrade gate: never stage an older-or-equal build.
    if manifest.build_number <= current_build {
        return Ok(None);
    }
    // If a newer build is already staged, don't re-download it.
    if let Some(r) = crate::manifest::Ready::read(&staging.ready)
        && r.build_number >= manifest.build_number
    {
        return Ok(None);
    }

    // Locate + download the DMG named by the manifest, from the same release.
    let release = &releases[rel_idx];
    let dmg_asset = release
        .asset(&manifest.dmg)
        .ok_or_else(|| format!("release has no asset named {:?}", manifest.dmg))?;

    let part = staging.download.join(format!("{}.part", manifest.dmg));
    let dmg = staging.download.join(&manifest.dmg);
    let _ = std::fs::remove_file(&part);
    download_to(&dmg_asset.url, &tok, &part)?;

    // Size sanity (when the API reported one), then atomically name it final.
    if dmg_asset.size != 0 {
        let got = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
        if got != dmg_asset.size {
            let _ = std::fs::remove_file(&part);
            return Err(format!(
                "DMG size mismatch: got {got} bytes, expected {}",
                dmg_asset.size
            ));
        }
    }
    std::fs::rename(&part, &dmg).map_err(|e| format!("finalize download: {e}"))?;

    // Integrity: SHA-256 must equal the manifest.
    let got = verify::sha256_file(&dmg)?;
    if !got.eq_ignore_ascii_case(&manifest.sha256) {
        let _ = std::fs::remove_file(&dmg);
        return Err(format!(
            "DMG sha256 mismatch: got {got}, manifest {}",
            manifest.sha256
        ));
    }

    // Mount, extract, verify (codesign/team-id/spctl), publish the ready marker.
    install::stage_from_dmg(&staging, &dmg, &manifest, PINNED_TEAM_ID)?;
    // The verified bundle is the artifact now; reclaim the DMG.
    let _ = std::fs::remove_file(&dmg);

    Ok(Some(manifest.version))
}

/// Run curl with `args`, feeding the secret `Authorization` header through STDIN
/// (`curl --config -`) so the token NEVER appears in argv — argv is world-visible
/// to same-user processes via `ps`. Returns the completed process output.
fn curl_auth(args: &[&str], token: &str) -> Result<std::process::Output, String> {
    use std::io::Write;
    use std::process::Stdio;
    let mut child = Command::new("/usr/bin/curl")
        .args(args)
        .args(["-H", "User-Agent: aterm-update"])
        .args(["--config", "-"]) // read more options (the auth header) from stdin
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("spawn curl: {e}"))?;
    {
        let mut stdin = child.stdin.take().ok_or("curl stdin unavailable")?;
        stdin
            .write_all(format!("header = \"Authorization: Bearer {token}\"\n").as_bytes())
            .map_err(|e| format!("write curl config: {e}"))?;
    } // drop stdin → EOF so curl proceeds
    child
        .wait_with_output()
        .map_err(|e| format!("curl wait: {e}"))
}

/// GET a GitHub API JSON resource, returning the raw body bytes.
fn api_get(url: &str, token: &str) -> Result<Vec<u8>, String> {
    let out = curl_auth(
        &[
            "-fsSL",
            "--retry",
            "2",
            "--max-time",
            "30",
            "-H",
            "Accept: application/vnd.github+json",
            "-H",
            "X-GitHub-Api-Version: 2022-11-28",
            url,
        ],
        token,
    )?;
    if !out.status.success() {
        return Err(format!(
            "curl GET {} failed ({}): {}",
            url,
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(out.stdout)
}

/// Download a SMALL asset's bytes (the manifest) into memory, size-capped so a
/// rogue/oversized asset can't be buffered whole.
fn download_bytes(asset_url: &str, token: &str) -> Result<Vec<u8>, String> {
    let out = curl_auth(
        &[
            "-fsSL",
            "--retry",
            "2",
            "--max-time",
            "60",
            "--max-filesize",
            "5000000", // 5 MB — an appcast is a few KB
            "-H",
            "Accept: application/octet-stream",
            asset_url,
        ],
        token,
    )?;
    if !out.status.success() {
        return Err(format!(
            "curl asset download failed ({}): {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(out.stdout)
}

/// Download an asset (the DMG) to a file, following the storage redirect.
fn download_to(asset_url: &str, token: &str, dest: &Path) -> Result<(), String> {
    let dest_s = dest.to_str().ok_or("non-UTF-8 destination path")?;
    let out = curl_auth(
        &[
            "-fSL",
            "--retry",
            "2",
            "--max-time",
            "600",
            "-H",
            "Accept: application/octet-stream",
            "-o",
            dest_s,
            asset_url,
        ],
        token,
    )?;
    if !out.status.success() {
        return Err(format!(
            "curl download failed ({}): {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(())
}
