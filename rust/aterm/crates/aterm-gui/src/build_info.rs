// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Andrew Yates

//! Build provenance — version, git commit, build timestamp, and a content signature
//! of the running binary. Surfaced in two places: the macOS About panel
//! ([`crate::menu::show_about_panel`]) and over the control socket
//! (`aterm-ctl version`, see [`crate::control`]).
//!
//! `VERSION`/`GIT_COMMIT`/`BUILD_TIME` are stamped at compile time by `build.rs`.
//! [`binary_signature`] is computed at runtime from the actual executable, so it
//! reflects the EXACT shipped bytes (the `.app`'s signed binary hashes differently
//! from the bare `target/release` binary — which is correct: it is what's running).

use std::hash::Hasher;
use std::sync::OnceLock;

/// Semantic version, from Cargo's `[package] version`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Short git commit the binary was built from — with a `-dirty` suffix when the
/// working tree had uncommitted changes. `"unknown"` when git was unavailable at
/// build time (e.g. a source tarball). Stamped by `build.rs`.
pub const GIT_COMMIT: &str = env!("ATERM_GIT_COMMIT");

/// UTC build timestamp (RFC3339), or `"unknown"`. Stamped by `build.rs`.
pub const BUILD_TIME: &str = env!("ATERM_BUILD_TIME");

/// Monotonic build number = commit depth (`git rev-list --count HEAD`). It increments
/// exactly when a build is from a LATER commit (a descendant has strictly more
/// ancestors) and is stable across rebuilds of the same commit; `"0"` without git.
/// Stamped by `build.rs` and used as the macOS `CFBundleVersion`.
pub const BUILD_NUMBER: &str = env!("ATERM_BUILD_NUMBER");

/// A content signature of the RUNNING binary: a 16-hex FxHash of `current_exe()`,
/// computed once and cached. Identifies the exact bytes that shipped; `"unknown"` if
/// the executable can't be read.
///
/// This is a build FINGERPRINT, not a cryptographic attestation — it uses the
/// workspace's non-cryptographic FxHash to avoid pulling in a crypto dependency. It
/// is enough to tell two builds apart and to confirm "the binary I'm running is the
/// one I shipped", which is its purpose in the About panel and `aterm-ctl version`.
#[must_use]
pub fn binary_signature() -> &'static str {
    static SIG: OnceLock<String> = OnceLock::new();
    SIG.get_or_init(|| {
        std::env::current_exe()
            .and_then(std::fs::read)
            .map(|bytes| {
                let mut h = aterm_hash::FxHasher::default();
                h.write(&bytes);
                format!("{:016x}", h.finish())
            })
            .unwrap_or_else(|_| "unknown".to_string())
    })
    .as_str()
}

/// One-line build summary for the About panel's version field, e.g.
/// `0.1.0 (build 1234) · a1b2c3d4e5f6 · built 2026-06-18T16:00:00Z · sig 1a2b3c4d5e6f7a8b`.
/// Used only by the macOS About panel; unused on other platforms.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[must_use]
pub fn about_line() -> String {
    format!(
        "{VERSION} (build {BUILD_NUMBER}) · {GIT_COMMIT} · built {BUILD_TIME} · sig {}",
        binary_signature()
    )
}

/// The control-socket (`aterm-ctl version`) response line: a stable, greppable
/// `key=value` form so scripts can parse the running build's provenance.
#[must_use]
pub fn control_line() -> String {
    format!(
        "OK version={VERSION} build={BUILD_NUMBER} commit={GIT_COMMIT} built={BUILD_TIME} signature={}\n",
        binary_signature()
    )
}
