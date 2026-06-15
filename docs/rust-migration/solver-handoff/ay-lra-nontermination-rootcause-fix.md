# LRA non-termination: ROOT CAUSE + algorithmic fix (replaces the timeout hack)

**Date:** 2026-06-14. **Status:** root cause confirmed end-to-end; sound fix designed; implementing.
The `AY_DIRECT_SOLVE_TIMEOUT_MS` deadline is a *guardrail*, not a fix — it converts a hang into a
non-answer. This makes the LRA decision procedure **converge by default** so the deadline never fires
on a solvable problem.

## It is NOT Simplex cycling

ay-lra is **dual simplex over rationals with bound propagation** (`simplex/solve.rs:151`
`dual_simplex_with_max_iters`). Bland's anti-cycling rule is already present
(`simplex/solve.rs:867`, `BLAND_THRESHOLD=1000`) with `CHECK_PIVOT_BUDGET=10_000` and
`GLOBAL_PIVOT_BUDGET=2_000_000`. **The simplex terminates.** Dynamic atom/bound refinement is disabled
(`implied_refinement.rs:402` is an empty body). So none of the classic causes (pivot cycling, FM blowup,
unbounded atom creation) apply.

## Root cause: an implied-bound propagation handshake that never quiesces

The spinning loop is the **outer CDCL loop** at `ay-sat/src/solver/solve/theory_backend.rs:405`. Every
inner fixpoint is individually bounded (`MAX_TIGHTENS_PER_VAR=4`, `max_fixpoint_iters`,
`MAX_FIXPOINT_ITERS=8`). The bug is that each outer round re-enters theory propagation and the theory
reports "I made progress" **forever**:

1. `compute_implied_bounds` populates its returned `newly_bounded` set for **any** tightening
   (`implied_bounds.rs:1170,1198`) with **no budget cap** — only the in-call cascade frontier
   `round_newly_bounded` respects `MAX_TIGHTENS_PER_VAR`. The cross-negation identity block
   (`implied_bounds.rs:1220-1306`) has **no tighten budget at all**.
2. On a u64-scale `start + len` offset obligation (`len ≤ u64::MAX`, plus an offset-equality pair
   `start ≤ x ∧ x ≤ start`), bounds tighten monotonically by ever-smaller exact-rational amounts
   (bignum spill, `hotframes.txt:23`, 654 samples). So `newly_bounded` is **non-empty on every call**.
3. The fixpoint hits its cap with non-empty `touched_rows` →
   `fixpoint_continuation_needed = reached_cap && !touched_rows.is_empty()` (`check_atoms.rs:904`) →
   `propagate_direct_touched_rows_pending = true` (`check_atoms.rs:922`).
4. That keeps `has_pending_analysis()` permanently true (`theory_solver/mod.rs:64`), which **defeats the
   only quiescence guard** (`ay-dpll/.../propagate.rs:345`), so `propagate_impl` runs another round.
5. `propagated_atoms` dedup is cleared on every pop/backtrack (`lifecycle_scope.rs:121,353,446`), so the
   same `TermId`s (47,72,73,76,77…) are re-asserted at decision level 0 each round. CDCL `continue`s
   (`theory_backend.rs:718`) → spins forever.

**Missing invariant:** nothing says "if the tightened-variable set hasn't changed across N propagate
rounds, stop." And **nothing polls a deadline** inside these fixpoints (confirmed: no `should_stop` in
`implied_bounds.rs`/`check_atoms.rs`/`propagation.rs`) — which is exactly why only the external
wall-clock timeout can stop it today.

## The fix (sound): a per-state no-progress round guard

Implied bounds are an **optimization** (`implied_bounds.rs:68-69` says so). Refusing to derive a
maximally-tight one is never unsound — feasibility is decided independently by the bounded, sound
`dual_simplex`. So: when the **same tightened-variable set repeats** for `MAX_IMPLIED_NOPROGRESS_ROUNDS`
(64) consecutive continuation rounds, stop seeding the cascade and let the handshake quiesce.

In `run_post_simplex_propagation` (`check_atoms.rs`), replace the bare
`self.propagate_direct_touched_rows_pending = fixpoint_continuation_needed;` (line 922) with a guard
that tracks an order-independent signature of `all_newly_bounded` (a `HashSet<u32>`) across calls on two
new solver fields (`implied_noprogress_streak: u32`, `last_implied_bound_signature: u64`):

- continuation && signature == last  → streak += 1
- continuation && signature changed  → streak = 0, last = signature   (genuine progress resets)
- !continuation                      → streak = 0                       (natural quiesce)
- streak ≥ 64                         → continuation = false; touched_rows.clear()  (stop oscillation)

**Why it never false-trips converging problems:** a converging cascade bounds *new* variables each round
(signature changes → streak resets) and then returns empty (continuation false → streak 0). The streak
only climbs when the *same* set re-tightens with no new variables — the pathological oscillation. The
streak is NOT reset after tripping, so a recurring no-progress state re-trips immediately; only real
progress clears it. Regression-safe for the long-bound-chain benchmarks (sc-6, simple_startup,
windowreal) because those bound new vars per round.

**Defense in depth (optional follow-up):** thread a cumulative implied-bound-derivation budget /
`should_stop` poll into the fixpoint loops (`check_atoms.rs:868`, `theory_solver/propagation.rs:613`) so
*any* other uncapped feedback path (e.g. the cross-negation block) also degrades to "no more
propagations" → final `check()` decides. Same soundness argument.

## Validation

Repro (the original hang): `orca-core`'s `agent_notification_id::build_agent_notification_id`
(`usize start+len`) spins with `sample` pinned in `compute_implied_bounds`/`run_post_simplex_propagation`.
Minimal SMT2 (from `loop-atoms.txt`):
```
(declare-const start Int)(declare-const len Int)(declare-const x Int)
(assert (<= 0 start))(assert (<= 0 len))(assert (<= len 18446744073709551615))
(assert (<= x start))(assert (<= start x))
(assert (or (< (+ start len) 0) (< 18446744073709551615 (+ start len))))
(check-sat)
```
Post-fix: returns a verdict by default in ms **with no timeout set**; the streak caps out once and the
extension quiesces. Then the `AY_DIRECT_SOLVE_TIMEOUT_MS` deadline is pure defense-in-depth.

Credit: root-cause traced by a dedicated investigation agent across ay-sat/ay-dpll/ay-theories-lra.
