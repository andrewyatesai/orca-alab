// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! The single shared **verification-gate policy** for aterm's conformance and
//! spec-link tests (the "honesty ratchet").
//!
//! Every conformance test in the workspace shells out to one of two Trust-toolchain
//! binaries — `ty` (the TLA+ model checker) or `trust-ir` (the Trust-native
//! `spec-link` cross-referencer). Historically each test crate carried its own
//! verbatim copy of a `find_ty`/`ty_required` pair that PANICKED when the binary was
//! absent (no skip path, no opt-out). That made `cargo test` hard-fail on any
//! machine that does not have the Trust toolchain built locally — CI, fresh clones,
//! and outside contributors all went red even though they changed nothing.
//!
//! This module replaces "always panic when absent" with a **three-way policy**, kept
//! in ONE place so it cannot drift between crates:
//!
//! 1. **Binary present** (unchanged): return its path; the test runs the checker and
//!    enforces the result fail-closed, exactly as before. No real check is weakened.
//! 2. **Binary absent, default**: print a LOUD, unmissable warning to stderr and
//!    return `None` so the caller SKIPS the Trust-dependent assertions. The skip is
//!    visible — it is NEVER a silent pass.
//! 3. **Binary absent, `ATERM_REQUIRE_TRUST=1`**: keep the original mandatory PANIC
//!    (fatal-on-absence). CI and the toolchain owner's box set this so a missing
//!    binary is a hard failure, preserving the ratchet where the toolchain is
//!    expected to exist.
//!
//! ## Caller idiom
//!
//! ```ignore
//! #[test]
//! fn real_thing_conforms() {
//!     let Some(ty) = aterm_spec::verify::ty_or_skip("Thing conformance") else {
//!         return; // absent + default: loud skip already printed; do not assert.
//!     };
//!     // ... use `ty` to run + enforce the conformance check ...
//! }
//! ```
//!
//! ## The env override
//!
//! `ATERM_REQUIRE_TRUST=1` makes an absent binary FATAL (panic) instead of a loud
//! skip. Any other value (or unset) selects the loud-skip default. Set it in CI and
//! on machines where the Trust toolchain is expected, so verification is mandatory
//! there.

use std::path::PathBuf;
use std::process::Command;

/// The env var that flips absent-binary behavior from loud-skip (default) to fatal
/// panic. Set `ATERM_REQUIRE_TRUST=1` in CI / on the toolchain owner's box.
pub const REQUIRE_ENV: &str = "ATERM_REQUIRE_TRUST";

/// `true` when `ATERM_REQUIRE_TRUST=1` — verification is then MANDATORY: an absent
/// `ty`/`trust-ir` is a hard (panic) failure rather than a loud skip.
pub fn require_trust() -> bool {
    std::env::var(REQUIRE_ENV).map(|v| v == "1").unwrap_or(false)
}

/// Discover the `ty` binary by a fixed canonical path search — NO env vars, NO flags.
/// Tries, in order: the Trust first-party submodule release build, the Trust stage2
/// build, a standalone `~/ty` checkout, then `ty` on `PATH`. Returns the first that
/// exists. Reading `$HOME` is the only environment access; there is no path override.
pub fn find_ty() -> Option<PathBuf> {
    if let Ok(home) = std::env::var("HOME") {
        for rel in [
            "trust/first-party/ty/target/release/ty",
            "trust/build/host/stage2/bin/ty",
            "ty/target/release/ty",
        ] {
            let p = PathBuf::from(&home).join(rel);
            if p.exists() {
                return Some(p);
            }
        }
    }
    cmd_path("ty")
}

/// Discover the `trust-ir` binary (TRUST_NATIVE_TLA, Phase 3). Mirrors [`find_ty`]:
/// the released first-party build, a standalone `~/trust-ir` checkout, then
/// `trust-ir` on `PATH`.
pub fn find_trust_ir() -> Option<PathBuf> {
    if let Ok(home) = std::env::var("HOME") {
        for rel in [
            "trust/first-party/trust-ir/target/release/trust-ir",
            "trust-ir/target/release/trust-ir",
        ] {
            let p = PathBuf::from(&home).join(rel);
            if p.exists() {
                return Some(p);
            }
        }
    }
    cmd_path("trust-ir")
}

/// `command -v <bin>` lookup on `PATH`; `None` if not found.
fn cmd_path(bin: &str) -> Option<PathBuf> {
    let out = Command::new("sh").arg("-c").arg(format!("command -v {bin}")).output().ok()?;
    if out.status.success() {
        let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !p.is_empty() {
            return Some(PathBuf::from(p));
        }
    }
    None
}

/// VERIFICATION GATE for `ty` (the honesty ratchet), three-way:
///
/// * present  → `Some(path)`: run + enforce (unchanged).
/// * absent + `ATERM_REQUIRE_TRUST=1` → **panic** (fatal-on-absence, the old behavior).
/// * absent + default → print a LOUD stderr warning and return `None` so the caller
///   SKIPS the Trust-dependent assertions. The skip is visible; it is never a silent
///   pass.
///
/// `label` names the check (e.g. `"EventLog conformance"`) so both the panic and the
/// skip warning say exactly what was/was not verified.
#[must_use]
pub fn ty_or_skip(label: &str) -> Option<PathBuf> {
    gate("ty", "~/trust/first-party/ty/target/release/ty", find_ty(), label)
}

/// VERIFICATION GATE for `trust-ir` (`spec-link`), three-way — see [`ty_or_skip`].
#[must_use]
pub fn trust_ir_or_skip(label: &str) -> Option<PathBuf> {
    gate(
        "trust-ir",
        "~/trust/first-party/trust-ir/target/release/trust-ir",
        find_trust_ir(),
        label,
    )
}

/// Shared three-way decision. `found` is the result of the per-binary discovery.
fn gate(bin: &str, build_hint: &str, found: Option<PathBuf>, label: &str) -> Option<PathBuf> {
    if let Some(p) = found {
        return Some(p);
    }
    if require_trust() {
        panic!(
            "VERIFICATION GATE ({REQUIRE_ENV}=1): `{bin}` not found; build it at {build_hint} \
             (or put it on PATH) — verification is MANDATORY. {label} could NOT be \
             model-checked / spec-linked, so this test FAILS rather than silently reporting ok."
        );
    }
    eprintln!(
        "\n⚠️  VERIFICATION SKIPPED: `{bin}` not found at {build_hint} — Trust toolchain not \
         installed; this run did NOT model-check `{label}`.\n    \
         Build Trust ({bin}) or set {REQUIRE_ENV}=1 to make this fatal. (Skipping the \
         Trust-dependent assertions for `{label}`; the test reports ok but was NOT verified.)\n"
    );
    None
}
