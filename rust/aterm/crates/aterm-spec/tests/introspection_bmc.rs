// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Andrew Yates
//
//! Tier-1 (interpreter-driven) bounded model check of the introspection /
//! recursive-stacking safety models — the SAME `Model`s the `ty` binary checks at
//! Tier-0 (`derived_ring_ty.rs`), here driven through the embedded executable
//! interpreter so the verification runs WITHOUT the external `ty` toolchain.
//!
//! This is genuine model-checking, not example-testing: [`bmc`] enumerates the
//! ENTIRE bounded reachable state space by BFS over `Model::successors` (which
//! fans out the nondeterministic `\in lo..hi` picks exactly as `ty`'s existential
//! search does) and asserts every invariant at every reachable state. Each model
//! uses the `Buggy` convention: the invariant must HOLD across the whole space at
//! `Buggy = 0` and a counterexample state must be REACHABLE at `Buggy = 1` — so the
//! property is both true and non-trivial (it genuinely catches the audit defect).
//!
//! Findings modelled: M1 dispatch completeness, M2 relay-teardown liveness,
//! S1 proxy-registry leak. See docs/TRUST-introspection-audit-detection.md.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

// The 7 introspection models are iterated via `harness::instances()`, not named here.
use aterm_spec::derive::Model;

/// A copy of `m` with its `Buggy` constant set to `b` (the interpreter reads
/// constants from `m.consts`, so flipping it here drives the buggy variant).
fn with_buggy(m: &Model, b: i64) -> Model {
    let mut m = m.clone();
    for c in &mut m.consts {
        if c.0 == "Buggy" {
            c.1 = b;
        }
    }
    m
}

/// Exhaustive bounded model check: BFS the reachable state space via
/// `Model::successors` over every action, checking every invariant at every state.
/// Returns `Ok(n_states)` if all invariants hold everywhere, or
/// `Err((violating_state, invariant_name))` at the first violation.
fn bmc(m: &Model) -> Result<usize, (BTreeMap<&'static str, i64>, &'static str)> {
    let key = |s: &BTreeMap<&'static str, i64>| -> Vec<(&'static str, i64)> {
        s.iter().map(|(k, v)| (*k, *v)).collect()
    };
    let mut seen: BTreeSet<Vec<(&'static str, i64)>> = BTreeSet::new();
    let mut q: VecDeque<BTreeMap<&'static str, i64>> = VecDeque::new();
    let init = m.init_state();
    seen.insert(key(&init));
    q.push_back(init);
    let mut n = 0usize;
    while let Some(st) = q.pop_front() {
        n += 1;
        assert!(
            n < 100_000,
            "{} state space unexpectedly large — tighten bounds",
            m.name
        );
        for inv in &m.invariants {
            if !m.check_invariant(inv.name, &st) {
                return Err((st, inv.name));
            }
        }
        for a in &m.actions {
            for ns in m.successors(a.name, &st) {
                if seen.insert(key(&ns)) {
                    q.push_back(ns);
                }
            }
        }
    }
    Ok(n)
}

/// The Tier-1 analogue of `derived_ring_ty::assert_proves_and_catches`: the
/// invariant holds across the WHOLE bounded space at `Buggy = 0`, and a
/// counterexample is reachable at `Buggy = 1`.
fn proves_and_catches(m: &Model) {
    match bmc(&with_buggy(m, 0)) {
        Ok(n) => eprintln!(
            "{}: invariant proven over {n} reachable states (Buggy=0).",
            m.name
        ),
        Err((st, inv)) => panic!("{} invariant `{inv}` VIOLATED at {st:?} (Buggy=0)", m.name),
    }
    match bmc(&with_buggy(m, 1)) {
        Ok(n) => panic!(
            "{} (Buggy=1) MUST yield a counterexample but invariant held over {n} states \
             — the property is trivial / does not catch the defect",
            m.name
        ),
        Err((st, inv)) => {
            eprintln!(
                "{}: invariant `{inv}` correctly CAUGHT at {st:?} (Buggy=1).",
                m.name
            )
        }
    }
}

/// The introspection property suite is the shared instance table; this binary is
/// the Tier-1 (interpreter-BMC) driver over it (the Tier-0 `ty` driver lives in
/// `derived_ring_ty.rs`). Adding a property = one row in `harness::instances()`.
#[path = "common/harness.rs"]
mod harness;

/// A reachable state with NO successor under ANY action, that is also NOT a
/// declared-final state, is a DEADLOCK — the interpreter twin of `ty`'s
/// CHECK_DEADLOCK. `Model::successors` returns an empty Vec for a disabled guard,
/// so a wedge is a BFS-reachable state where every action yields no successor.
fn find_deadlock(
    m: &Model,
    is_final: impl Fn(&BTreeMap<&'static str, i64>) -> bool,
) -> Option<BTreeMap<&'static str, i64>> {
    let key = |s: &BTreeMap<&'static str, i64>| -> Vec<(&'static str, i64)> {
        s.iter().map(|(k, v)| (*k, *v)).collect()
    };
    let mut seen: BTreeSet<Vec<(&'static str, i64)>> = BTreeSet::new();
    let mut q: VecDeque<BTreeMap<&'static str, i64>> = VecDeque::new();
    let init = m.init_state();
    seen.insert(key(&init));
    q.push_back(init);
    while let Some(st) = q.pop_front() {
        let mut any_succ = false;
        for a in &m.actions {
            for ns in m.successors(a.name, &st) {
                any_succ = true;
                if seen.insert(key(&ns)) {
                    q.push_back(ns);
                }
            }
        }
        if !any_succ && !is_final(&st) {
            return Some(st); // stuck, and not a legitimate work-complete terminal
        }
    }
    None
}

/// THE UMBRELLA (Tier-1, no toolchain): every property-combinator instance is
/// verified by the interpreter — a `Safety` invariant exhaustively over the bounded
/// reachable space (`proves_and_catches`), a `Liveness` instance via the
/// no-successor wedge check (`find_deadlock`, deadlock-free@Buggy=0 / wedge@Buggy=1).
/// Iterated from the ONE shared table; a new property adds a row there, not a fn here.
#[test]
fn property_classes_prove_and_catch_under_bmc() {
    for inst in harness::instances() {
        match inst.class {
            harness::Class::Safety => proves_and_catches(&inst.model),
            harness::Class::Liveness { is_final } => {
                assert!(
                    find_deadlock(&with_buggy(&inst.model, 0), is_final).is_none(),
                    "{} Buggy=0 must be deadlock-free",
                    inst.model.name
                );
                let wedge = find_deadlock(&with_buggy(&inst.model, 1), is_final);
                assert!(
                    wedge.is_some(),
                    "{} Buggy=1 must reach a wedge",
                    inst.model.name
                );
                eprintln!(
                    "{}: wedge caught at {:?} (Buggy=1).",
                    inst.model.name,
                    wedge.unwrap()
                );
            }
        }
    }
}
