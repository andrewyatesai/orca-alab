// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! The single shared **verification gate** for aterm's conformance and spec-link
//! tests — the honesty ratchet, batteries ON.
//!
//! Every conformance test in the workspace shells out to one of two Trust-toolchain
//! binaries: `ty` (the TLA+ model checker) or `trust-ir` (the Trust-native
//! `spec-link` cross-referencer). Verification is **always required** — there is no
//! env var, no flag, and no skip path. If the toolchain is not built, the test FAILS
//! (panics) with a one-line build hint, never a silent (or "loud-skip") false `ok`.
//! Build the toolchain once and every gate enforces, fail-closed, automatically.
//!
//! ## The checker is part of Trust
//!
//! `ty`/`trust-ir` live in the Trust toolchain (`~/trust/first-party/{ty,trust-ir}`),
//! NOT a standalone checkout. Build them once:
//!
//! ```sh
//! cargo build --release -p tla-cli   # in ~/trust/first-party/ty       -> ty
//! cargo build --release              # in ~/trust/first-party/trust-ir -> trust-ir
//! ```
//!
//! [`find_ty`]/[`find_trust_ir`] then discover them automatically at their canonical
//! release paths — or anywhere the full-toolchain bootstrap (`build/<triple>/…`)
//! dropped them, or on `PATH`. `$HOME` is the only environment access; there is no
//! path override and nothing to remember to set.
//!
//! ## Caller idiom
//!
//! ```ignore
//! #[test]
//! fn real_thing_conforms() {
//!     let ty = aterm_spec::verify::ty("Thing conformance");
//!     // ... use `ty` to run + enforce the conformance check, fail-closed ...
//! }
//! ```

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

/// Discover the Trust `ty` model-checker. Searches, in order: the canonical
/// first-party cargo build, any full-toolchain bootstrap stage build, then `ty` on
/// `PATH`. `$HOME` is the only environment access.
#[must_use]
pub fn find_ty() -> Option<PathBuf> {
    find_trust_bin("ty", "ty/target/release/ty")
}

/// Discover the Trust `trust-ir` `spec-link` cross-referencer. Mirrors [`find_ty`].
#[must_use]
pub fn find_trust_ir() -> Option<PathBuf> {
    find_trust_bin("trust-ir", "trust-ir/target/release/trust-ir")
}

/// Shared discovery: the canonical `~/trust/first-party/<rel>` cargo build, then any
/// matching tool the full-toolchain bootstrap left under `~/trust/build/<triple>/…`,
/// then `<bin>` on `PATH`.
fn find_trust_bin(bin: &str, first_party_rel: &str) -> Option<PathBuf> {
    if let Ok(home) = std::env::var("HOME") {
        let home = PathBuf::from(home);
        let canonical = home.join("trust/first-party").join(first_party_rel);
        if canonical.exists() {
            return Some(canonical);
        }
        if let Some(p) = scan_trust_bootstrap(&home.join("trust/build"), bin) {
            return Some(p);
        }
    }
    cmd_path(bin)
}

/// Best-effort scan of the full-toolchain bootstrap output
/// (`~/trust/build/<triple>/{stage2-tools-bin/<triple>,stage1/bin}/<bin>`) — the
/// layout `x.py`/bootstrap produces when the whole Trust compiler is built.
fn scan_trust_bootstrap(build: &Path, bin: &str) -> Option<PathBuf> {
    for entry in std::fs::read_dir(build).ok()?.flatten() {
        let triple_dir = entry.path();
        let triple = entry.file_name();
        for cand in [
            triple_dir.join("stage2-tools-bin").join(&triple).join(bin),
            triple_dir.join("stage1").join("bin").join(bin),
        ] {
            if cand.exists() {
                return Some(cand);
            }
        }
    }
    None
}

/// `command -v <bin>` lookup on `PATH`; `None` if not found.
fn cmd_path(bin: &str) -> Option<PathBuf> {
    let out = Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {bin}"))
        .output()
        .ok()?;
    if out.status.success() {
        let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !p.is_empty() {
            return Some(PathBuf::from(p));
        }
    }
    None
}

/// Locate the Trust `ty` model-checker for `label`, or PANIC with a build hint.
/// Verification is ALWAYS required — no env var, no skip. A conformance test that
/// cannot reach `ty` FAILS rather than reporting a false `ok`.
#[must_use]
pub fn ty(label: &str) -> PathBuf {
    require(
        "ty",
        "cargo build --release -p tla-cli   (in ~/trust/first-party/ty)",
        find_ty(),
        label,
    )
}

/// Locate the Trust `trust-ir` `spec-link` tool for `label`, or PANIC with a build
/// hint. Always required — see [`ty`].
#[must_use]
pub fn trust_ir(label: &str) -> PathBuf {
    require(
        "trust-ir",
        "cargo build --release   (in ~/trust/first-party/trust-ir)",
        find_trust_ir(),
        label,
    )
}

/// The gate: return the discovered path, or PANIC. There is no skip and no opt-out —
/// the honesty ratchet, batteries-on.
fn require(bin: &str, build_hint: &str, found: Option<PathBuf>, label: &str) -> PathBuf {
    found.unwrap_or_else(|| {
        panic!(
            "VERIFICATION GATE: Trust `{bin}` not found — `{label}` could NOT be \
             model-checked / spec-linked. Build the Trust toolchain once: {build_hint}. \
             Verification is always required; this test FAILS rather than reporting a \
             false ok."
        )
    })
}
