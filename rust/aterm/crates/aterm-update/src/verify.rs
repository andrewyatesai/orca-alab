// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0

//! Authenticity verification of a candidate `.app`, run both at stage time and
//! again at apply time (TOCTOU defence — the staged copy sits on disk between the
//! two). Mirrors the checks `apps/aterm-mac/notarize.sh` performs, in order of
//! cheapest/most-local first, and fails CLOSED: any error, non-zero exit, or
//! unparseable output is a rejection.

use std::path::Path;
use std::process::Command;

/// Full apply-/stage-time gate: structural codesign seal, Team-ID pin, and
/// Gatekeeper/notarization acceptance. `expected_team` is [`crate::PINNED_TEAM_ID`].
/// Returns `Ok(())` only if every check passes.
pub fn verify_bundle(app: &Path, expected_team: &str) -> Result<(), String> {
    codesign_verify(app)?;
    let team = team_id(app)?;
    if team != expected_team {
        return Err(format!(
            "Team ID mismatch: bundle is signed by {team:?}, expected {expected_team:?}"
        ));
    }
    spctl_assess(app)?;
    Ok(())
}

/// `codesign --verify --deep --strict` — the signature seal is intact and nothing
/// inside the bundle has been modified since signing.
fn codesign_verify(app: &Path) -> Result<(), String> {
    let out = Command::new("/usr/bin/codesign")
        .args(["--verify", "--deep", "--strict", "--verbose=2"])
        .arg(app)
        .output()
        .map_err(|e| format!("spawn codesign: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "codesign --verify failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

/// Extract the signing **Team Identifier** from a bundle. `codesign -dv` writes its
/// descriptive output to **stderr**, hence the merge; we scan for the
/// `TeamIdentifier=...` line. A `not set` value (ad-hoc / unsigned) is rejected.
pub fn team_id(app: &Path) -> Result<String, String> {
    let out = Command::new("/usr/bin/codesign")
        .args(["-d", "--verbose=4"])
        .arg(app)
        .output()
        .map_err(|e| format!("spawn codesign -d: {e}"))?;
    // -dv prints to stderr regardless of success; combine both streams.
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let team = text
        .lines()
        .find_map(|l| l.strip_prefix("TeamIdentifier="))
        .map(str::trim)
        .ok_or_else(|| "codesign output had no TeamIdentifier line".to_string())?;
    if team.is_empty() || team == "not set" {
        return Err("bundle is ad-hoc/unsigned (TeamIdentifier not set)".to_string());
    }
    Ok(team.to_string())
}

/// `spctl -a -t exec` — Gatekeeper/notarization acceptance for a *runnable app*
/// (`-t exec`, not the DMG-install `-t install`). Reads the stapled notarization
/// ticket from the bundle, so it succeeds offline.
fn spctl_assess(app: &Path) -> Result<(), String> {
    let out = Command::new("/usr/sbin/spctl")
        .args(["-a", "-t", "exec", "-vvv"])
        .arg(app)
        .output()
        .map_err(|e| format!("spawn spctl: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "spctl assessment rejected the bundle: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

/// `shasum -a 256 <file>` → lowercase hex digest. Used to verify a downloaded DMG
/// against the manifest (shelling `shasum` keeps the crate crypto-free, matching
/// the build scripts).
pub fn sha256_file(path: &Path) -> Result<String, String> {
    let out = Command::new("/usr/bin/shasum")
        .args(["-a", "256"])
        .arg(path)
        .output()
        .map_err(|e| format!("spawn shasum: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "shasum failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    stdout
        .split_whitespace()
        .next()
        .map(|h| h.to_ascii_lowercase())
        .ok_or_else(|| "shasum produced no digest".to_string())
}
