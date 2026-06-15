// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors
//
//! Tier-0 for DERIVED specs: the TLA+ generated from a Rust `Model` (one source)
//! is exhaustively model-checked by the real `ty` binary.
//!
//! This is the derivation half of `docs/RFC-ty-embed-derived-tla.md`: no
//! hand-written `.tla` — `Model::to_tla()` produces the module and `.to_cfg()` the
//! config, and `ty check` proves the invariants hold over the whole bounded state
//! space. Change the model, the spec changes, and this re-checks the new spec.
//! Drift is impossible by construction. Both the single-action ring AND the
//! two-action cursor (which exercises `UNCHANGED` + a disjunctive `Next`) are
//! checked, so the derivation is shown to generalize.
//!
//! VERIFICATION GATE (honesty ratchet): absent `ty`, this test FAILS by default.
//! A missing checker can never read as `ok` — that would make the "formally
//! verified" claim a silent no-op. The ONLY way to get a non-failing run without
//! `ty` is the explicit opt-out `ATERM_ALLOW_SKIP_TY=1`, which skips VISIBLY.
//! `ATERM_REQUIRE_TY=1` is still honored (hard fail) for back-compat, but hard
//! fail is now the DEFAULT regardless.

use aterm_spec::derive::{
    cursor_model, evict_full_model, kernel_model, ring_model, snapshot_model, subscribe_model,
    transact_model, Model,
};
use std::path::PathBuf;
use std::process::Command;

fn find_ty() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("TY_BIN") {
        let p = PathBuf::from(p);
        if p.exists() {
            return Some(p);
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        for rel in ["trust/first-party/ty/target/release/ty", "ty/target/release/ty"] {
            let p = PathBuf::from(&home).join(rel);
            if p.exists() {
                return Some(p);
            }
        }
    }
    let out = Command::new("sh").arg("-c").arg("command -v ty").output().ok()?;
    if out.status.success() {
        let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !p.is_empty() {
            return Some(PathBuf::from(p));
        }
    }
    None
}

/// VERIFICATION GATE. Returns `Some(ty)` when the checker is present. When it is
/// ABSENT, the default is a HARD FAILURE (panic): a missing `ty` must NEVER let a
/// formal-verification test report `ok`. The ONLY non-failing absent-`ty` outcome
/// is the explicit opt-out `ATERM_ALLOW_SKIP_TY=1`, which returns `None` so the
/// caller skips VISIBLY. `ATERM_REQUIRE_TY=1` is still accepted (hard fail) for
/// back-compat, but it is now redundant — hard fail is the default.
fn ty_or_skip(label: &str) -> Option<PathBuf> {
    match find_ty() {
        Some(t) => Some(t),
        None => {
            let msg = "`ty` not found (set TY_BIN, build ~/ty, or put `ty` on PATH)";
            // ATERM_REQUIRE_TY=1 (CI) always hard-fails on absence and cannot be
            // suppressed — its strict semantics take precedence over the opt-out.
            let require = std::env::var("ATERM_REQUIRE_TY").is_ok();
            if !require && std::env::var("ATERM_ALLOW_SKIP_TY").is_ok() {
                eprintln!(
                    "SKIP (ATERM_ALLOW_SKIP_TY=1): {msg}; {label} NOT model-checked this run \
                     — the formal-verification claim is UNVERIFIED for this run."
                );
                return None;
            }
            // Default: absence is a hard failure (a missing checker must never read ok).
            panic!(
                "VERIFICATION GATE: {msg}. {label} could NOT be model-checked, so this \
                 test FAILS rather than silently reporting ok. Install/build `ty` to verify, \
                 or set ATERM_ALLOW_SKIP_TY=1 to explicitly (and visibly) skip."
            );
        }
    }
}

/// Generate the model's spec + cfg and assert `ty check` succeeds.
fn assert_model_checks(ty: &PathBuf, m: &Model) {
    let dir = std::env::temp_dir().join(format!("aterm-derive-{}-{}", m.name, std::process::id()));
    std::fs::create_dir_all(&dir).expect("mk tempdir");
    let spec = dir.join(format!("{}.tla", m.name));
    let cfg = dir.join(format!("{}.cfg", m.name));
    std::fs::write(&spec, m.to_tla()).expect("write derived spec");
    std::fs::write(&cfg, m.to_cfg()).expect("write derived cfg");

    let out = Command::new(ty)
        .arg("check")
        .arg(&spec)
        .arg("--config")
        .arg(&cfg)
        .output()
        .expect("run ty check");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.status.success(),
        "ty check FAILED on DERIVED {} spec\n--- generated {}.tla ---\n{}\n--- ty output ---\n{combined}",
        m.name,
        m.name,
        m.to_tla()
    );
    let _ = std::fs::remove_dir_all(&dir);
    eprintln!("derived {} spec model-checked clean by ty.", m.name);
}

#[test]
fn derived_ring_spec_model_checks() {
    let Some(ty) = ty_or_skip("derived ring spec") else { return };
    assert_model_checks(&ty, &ring_model());
}

#[test]
fn derived_cursor_spec_model_checks() {
    // Exercises the multi-action / UNCHANGED generation path through `ty`.
    let Some(ty) = ty_or_skip("derived cursor spec") else { return };
    assert_model_checks(&ty, &cursor_model());
}

#[test]
fn derived_evict_full_spec_model_checks() {
    // The FUNCTION-VALUED faithful ring: proves EvictOldestContiguous over a
    // live: [1..MaxSeq -> BOOLEAN] set — the property the scalar ring can't express.
    let Some(ty) = ty_or_skip("derived EvictFull spec") else { return };
    assert_model_checks(&ty, &evict_full_model());
}

/// A model using the `Buggy` convention: `ty` must PROVE its invariant at the
/// committed `Buggy=0`, and find a COUNTEREXAMPLE at `Buggy=1` — so the invariant
/// is non-trivial AND genuinely catches the bug. Both spec + cfg are derived.
fn assert_proves_and_catches(ty: &PathBuf, m: &Model) {
    let dir = std::env::temp_dir().join(format!("aterm-{}-{}", m.name, std::process::id()));
    std::fs::create_dir_all(&dir).expect("mk tempdir");
    let spec = dir.join(format!("{}.tla", m.name));
    std::fs::write(&spec, m.to_tla()).expect("write spec");

    let run = |cfg_name: &str, cfg: String| -> (bool, String) {
        let cfgp = dir.join(cfg_name);
        std::fs::write(&cfgp, cfg).expect("write cfg");
        let out = Command::new(ty)
            .arg("check")
            .arg(&spec)
            .arg("--config")
            .arg(&cfgp)
            .output()
            .expect("run ty check");
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        (out.status.success(), combined)
    };

    let (ok, out) = run("ok.cfg", m.to_cfg());
    assert!(ok, "derived {} (Buggy=0) must model-check clean\n{out}", m.name);
    let (bug_ok, bug_out) = run("bug.cfg", m.to_cfg_with(&[("Buggy", 1)]));
    assert!(!bug_ok, "{} (Buggy=1) MUST yield a counterexample\n{bug_out}", m.name);

    let _ = std::fs::remove_dir_all(&dir);
    eprintln!("derived {}: invariant proven (Buggy=0) and caught (Buggy=1 -> counterexample).", m.name);
}

#[test]
fn derived_subscribe_proves_and_catches_silent_loss() {
    let Some(ty) = ty_or_skip("derived subscribe spec") else { return };
    assert_proves_and_catches(&ty, &subscribe_model());
}

#[test]
fn derived_transact_proves_and_catches_lost_update() {
    let Some(ty) = ty_or_skip("derived transact spec") else { return };
    assert_proves_and_catches(&ty, &transact_model());
}

#[test]
fn derived_kernel_proves_and_catches_gap() {
    let Some(ty) = ty_or_skip("derived kernel spec") else { return };
    assert_proves_and_catches(&ty, &kernel_model());
}

#[test]
fn derived_snapshot_proves_and_catches_leak() {
    let Some(ty) = ty_or_skip("derived snapshot spec") else { return };
    assert_proves_and_catches(&ty, &snapshot_model());
}
