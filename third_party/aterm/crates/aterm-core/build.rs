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

/// Declared minimum optimization floor for this hot-path crate.
///
/// Prototype of the Trust feature `#![trust::min_opt_level(3)]`. aterm-core sits
/// on the terminal's hot path; building it at a low/size opt-level (e.g. `s`/`z`)
/// silently drops throughput ~2.6x (measured: 165 -> ~430 MB/s regression when a
/// host workspace's `[profile.release] opt-level = "z"` leaked into the engine).
///
/// Override with `ATERM_MIN_OPT_LEVEL` (set to `0` to disable the guard entirely).
const MIN_OPT_LEVEL_FLOOR: u8 = 3;

/// Guard: in release builds, fail loudly if the active `opt-level` is below the
/// declared floor. Cargo exposes the resolved level via `OPT_LEVEL` and the
/// profile via `PROFILE`. Debug builds are intentionally unaffected.
fn check_min_opt_level() {
    println!("cargo:rerun-if-env-changed=ATERM_MIN_OPT_LEVEL");

    // Only enforce in release builds; debug/dev builds are intentionally exempt.
    if std::env::var("PROFILE").as_deref() != Ok("release") {
        return;
    }

    // Read the declared floor (env-overridable). `0` disables the guard.
    let floor: u8 = std::env::var("ATERM_MIN_OPT_LEVEL")
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(MIN_OPT_LEVEL_FLOOR);
    if floor == 0 {
        return;
    }

    // OPT_LEVEL is one of "0".."3" or "s"/"z". The size-optimized levels ("s"/"z")
    // are the regression we guard against, so they always count as below the floor.
    let opt = std::env::var("OPT_LEVEL").unwrap_or_default();
    let too_low = match opt.as_str() {
        "s" | "z" => true,
        n => n.parse::<u8>().map(|lvl| lvl < floor).unwrap_or(false),
    };

    if too_low {
        panic!(
            "aterm-core is on the hot path but is being compiled at opt-level={opt}; \
             terminal throughput drops ~2.6x. Set opt-level={floor} for this crate \
             (e.g. add to your top-level Cargo.toml:\n\n    \
             [profile.release.package.aterm-core]\n    opt-level = {floor}\n\n\
             To override the declared floor, set ATERM_MIN_OPT_LEVEL (0 disables the guard)."
        );
    }
}

fn main() {
    // Hot-path optimization-level guard (prototype of `#![trust::min_opt_level]`).
    check_min_opt_level();

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
