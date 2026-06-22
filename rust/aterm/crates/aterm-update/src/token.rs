// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0

//! Resolving the GitHub token used to read the PRIVATE release repo.
//!
//! The token is **per-machine** and must NEVER be compiled into the shipped
//! (signed) binary — that would distribute one credential to every user. It is
//! provisioned out of band (see `docs/RELEASING.md`) and resolved at runtime, in
//! order:
//!
//! 1. `$ATERM_UPDATE_TOKEN` — explicit env override (CI / power users);
//! 2. the macOS **keychain** generic-password item `aterm-update-token`
//!    (`security find-generic-password -s aterm-update-token -w`);
//! 3. a `0600` file `…/aterm/update-token` under Application Support.
//!
//! A fine-grained PAT with read-only **Contents** permission on the repo is
//! sufficient. The token is never logged, and never placed on a command line — it
//! reaches curl through stdin (see `github::curl_auth`).

use std::path::Path;
use std::process::Command;

/// Resolve the token, or `None` when none is provisioned (the updater then stays
/// idle rather than hammering the API unauthenticated against a private repo).
/// `support_dir` is `…/Library/Application Support/aterm` (the `Updates` parent).
pub fn resolve(support_dir: &Path) -> Option<String> {
    if let Some(v) = std::env::var_os("ATERM_UPDATE_TOKEN") {
        let s = v.to_string_lossy().trim().to_string();
        if !s.is_empty() {
            return Some(s);
        }
    }
    if let Some(t) = from_keychain() {
        return Some(t);
    }
    from_file(&support_dir.join("update-token"))
}

/// `security find-generic-password -s aterm-update-token -w` → the secret on
/// stdout. Returns `None` if the item is absent or the tool fails.
fn from_keychain() -> Option<String> {
    let out = Command::new("/usr/bin/security")
        .args(["find-generic-password", "-s", "aterm-update-token", "-w"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!s.is_empty()).then_some(s)
}

/// Read a token from a `0600`-or-tighter file, refusing one that is
/// group/other-readable (a leaked credential file is worse than no updates).
fn from_file(path: &Path) -> Option<String> {
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::metadata(path).ok()?;
    if meta.mode() & 0o077 != 0 {
        crate::warn(&format!(
            "{} is group/other-accessible; ignoring (chmod 600 it)",
            path.display()
        ));
        return None;
    }
    let s = std::fs::read_to_string(path).ok()?.trim().to_string();
    (!s.is_empty()).then_some(s)
}
