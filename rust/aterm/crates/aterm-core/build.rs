// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors
//! Build script for aterm-core
//!
//! Embeds git SHA and build timestamp into the FFI build info struct.
//!
//! ## Git SHA
//!
//! Auto-detected from `git rev-parse HEAD`. Override with `ATERM_GIT_SHA` env var
//! for CI/reproducible builds. When neither is available, `git_sha` is NULL.
//!
//! ## Build Timestamp
//!
//! Set `SOURCE_DATE_EPOCH` (standard env var) for reproducible builds.
//! When unset, `build_timestamp_unix` is 0.

use std::process::Command;

fn main() {
    // Re-run when git HEAD changes
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/refs/");
    println!("cargo:rerun-if-env-changed=ATERM_GIT_SHA");
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");

    // Git SHA: prefer explicit env var, fall back to auto-detect
    let sha = std::env::var("ATERM_GIT_SHA")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            Command::new("git")
                .args(["rev-parse", "HEAD"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        });

    if let Some(sha) = sha {
        println!("cargo:rustc-env=ATERM_GIT_SHA_EMBEDDED={sha}");
        println!("cargo:rustc-cfg=has_git_sha");
    }

    // Emit build timestamp if SOURCE_DATE_EPOCH is set (standard reproducible builds var)
    if let Ok(epoch) = std::env::var("SOURCE_DATE_EPOCH")
        && let Ok(timestamp) = epoch.parse::<u64>()
    {
        println!("cargo:rustc-env=ATERM_BUILD_TIMESTAMP={timestamp}");
        println!("cargo:rustc-cfg=has_build_timestamp");
    }
}
