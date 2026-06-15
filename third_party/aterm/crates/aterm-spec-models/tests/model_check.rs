// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors
//
// Model-checking as a test (ROADMAP WS-I): drives the `ty` TLA+ explicit-state
// checker over every spec in `specs/` and asserts the invariants hold. This
// makes formal model-checking a first-class `cargo test` + CI artifact rather
// than a manual side ritual.
//
// `ty` is located via (in order): $TY_BIN, the Trust first-party submodule
// (~/trust/first-party/ty/target/release/ty), ~/ty/target/release/ty, then `ty`
// on PATH. If not found, the test FAILS by default (hard failure) — a missing
// checker must NEVER let the model-check claim report `ok`. The ONLY non-failing
// absent-`ty` outcome is the explicit opt-out $ATERM_ALLOW_SKIP_TY=1, which skips
// VISIBLY. $ATERM_REQUIRE_TY=1 (CI) is still honored as a hard fail for
// back-compat, but hard fail is now the DEFAULT.
// This is the honesty ratchet: the model-check claim is only green when `ty`
// actually ran. The default search includes the Trust submodule path so a
// standard `cargo test` model-checks the specs with no extra configuration
// (batteries-on); when `ty` is genuinely absent the test fails loudly instead of
// silently skipping.

use std::path::{Path, PathBuf};
use std::process::Command;

fn find_ty() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("TY_BIN") {
        let p = PathBuf::from(p);
        if p.exists() {
            return Some(p);
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        // The canonical location: `ty` is a Trust first-party submodule. A standard
        // checkout builds it here, so the model-check runs by default.
        let trust_ty =
            PathBuf::from(&home).join("trust/first-party/ty/target/release/ty");
        if trust_ty.exists() {
            return Some(trust_ty);
        }
        let p = PathBuf::from(home).join("ty/target/release/ty");
        if p.exists() {
            return Some(p);
        }
    }
    if let Ok(out) = Command::new("sh").arg("-c").arg("command -v ty").output() {
        if out.status.success() {
            let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !p.is_empty() {
                return Some(PathBuf::from(p));
            }
        }
    }
    None
}

fn specs_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("specs")
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
            let msg = "`ty` TLA+ checker not found (set TY_BIN, build ~/ty, or put `ty` on PATH)";
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

#[test]
fn ty_model_checks_every_spec() {
    let Some(ty) = ty_or_skip("specs") else { return };

    let dir = specs_dir();
    let mut checked = 0usize;
    let mut names: Vec<String> = Vec::new();
    for entry in std::fs::read_dir(&dir).expect("read specs/") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("tla") {
            continue;
        }
        let cfg = path.with_extension("cfg");
        assert!(cfg.exists(), "spec {path:?} has no matching .cfg");

        let out = Command::new(&ty)
            .arg("check")
            .arg(&path)
            .arg("--config")
            .arg(&cfg)
            .output()
            .unwrap_or_else(|e| panic!("failed to run {ty:?}: {e}"));

        assert!(
            out.status.success(),
            "ty check FAILED for {path:?}\n--- stdout ---\n{}\n--- stderr ---\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
        );
        eprintln!("ty: {} — invariants hold", path.file_name().unwrap().to_string_lossy());
        names.push(path.file_name().unwrap().to_string_lossy().into_owned());
        checked += 1;
    }
    assert!(checked > 0, "no .tla specs found in {dir:?}");
    // The kernel-family ledger: every spec named here MUST exist and be checked.
    for required in [
        "Kernel.tla",    // event-log spine: seq == len, gap-free monotonic
        "Subscribe.tla", // poll(): subscriber no-silent-loss / gap on fall-behind
        "Snapshot.tla",  // snapshot(): view == history-prefix(N) isolation
        "Transact.tla",  // transact(): atomic commit-or-nothing, no lost update
        "Evict.tla",     // ring cap: len <= K, oldest-contiguous eviction, gap iff evicted
    ] {
        assert!(names.iter().any(|n| n == required), "required spec missing: {required}");
    }
    eprintln!("ty model-checked {checked} spec(s)");
}
