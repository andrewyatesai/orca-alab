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

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("aterm-tok-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d.join("update-token")
    }

    #[test]
    fn file_token_accepts_0600() {
        let p = tmp("ok");
        std::fs::write(&p, "github_pat_secret\n").unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(from_file(&p).as_deref(), Some("github_pat_secret"));
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn file_token_refuses_group_or_other_readable() {
        for mode in [0o644u32, 0o640, 0o604, 0o660] {
            let p = tmp(&format!("m{mode:o}"));
            std::fs::write(&p, "leakable").unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode)).unwrap();
            assert!(from_file(&p).is_none(), "mode {mode:o} must be refused");
            let _ = std::fs::remove_dir_all(p.parent().unwrap());
        }
    }

    #[test]
    fn missing_file_is_none() {
        assert!(from_file(std::path::Path::new("/nonexistent/aterm/update-token")).is_none());
    }
}
