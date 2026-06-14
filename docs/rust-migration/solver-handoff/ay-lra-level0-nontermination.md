# ay-lra / ay-lia level-0 propagation non-termination on u64-overflow atoms

**Status:** owner-side core-solver bug (ay-dpll / ay-lra / ay-lia). Reproduced deterministically
against the Orca workload on Trust stage2 built 2026-06-14 00:27 (trust-mc `be05d7f14`).
**Impact:** a single obligation can spin `trustc` at 100% CPU indefinitely (observed >23h in an
earlier run) — a verifier must never be able to hang on one obligation, so this is a
correctness-of-the-tool invariant, not just a performance issue.

## Symptom

While surveying `orca-core`, `trustc --crate-name orca_core` pins one core at 100% CPU and never
returns. `stderr` streams the same theory atoms forever:

```
WARN ay_dpll::extension::propagate  asserting theory atom at level 0, term=TermId(47), value=true,  term_str=(= _36#19 Int(1))
WARN ay_dpll::extension::propagate  asserting theory atom at level 0, term=TermId(76), value=false, term_str=(< (+ start#29 _43#21) Int(0))
WARN ay_dpll::extension::propagate  asserting theory atom at level 0, term=TermId(77), value=true,  term_str=(< Int(18446744073709551615) (+ start#29 _43#21))
WARN ay_dpll::extension::propagate  asserting theory atom at level 0, term=TermId(73), value=true,  term_str=(<= _43#21 Int(18446744073709551615))
...
```

The same `TermId`s are re-asserted at **decision level 0** in a tight cycle (full dump:
`ay-lra-level0-loop-atoms.txt`). The propagation never reaches a fixpoint, so the CDCL loop above it
never advances past level 0.

## Hot stack (sampled, `sample <pid> 5`; full trace: `ay-lra-level0-loop-sample.txt`)

```
ay_dpll::executor::check_sat::Executor::check_sat
 → check_sat_internal → solve_current_assertions_with_quantifier_support
 → ay_dpll::executor::theories::lia::Executor::solve_lia_incremental_inner
 → ay_sat::solver::solve::Solver::solve_interruptible_with_extension_raw
 → ay_sat::solver::solve::Solver::cdcl_loop_impl                       (2345/3279 samples)
 → ay_sat theory_callback ExtensionCallback::propagate                 (2342)
 → ay_dpll::extension::propagate::TheoryExtension<LiaSolver>::propagate_impl   (2285)  ← the re-assert loop
 → ay_lia::theory_impl::LiaSolver::check_during_propagate              (1557)
 → ay_lra::theory_solver::LraSolver::check_during_propagate            (451+339, self-recursive)
 → ay_lra::propagation::var_atoms::LraSolver::propagate_var_atoms      (cycle: +280→+1136→+1228→+280)
 → ay_lra::simplex::solve::LraSolver::dual_simplex_propagate
```

`propagate_var_atoms` shows self-recursive frames and `check_during_propagate` calls itself — the
LRA→LIA propagation handshake is re-deriving bounds without converging.

**Refined hot spot (dedup'd frame counts, `ay-lra-level0-loop-hotframes.txt`):** the time inside
`check_during_propagate` is dominated by
`ay_lra::implied_bounds::LraSolver::compute_implied_bounds` (1139) and
`ay_lra::check_atoms::LraSolver::run_post_simplex_propagation` (698), with
`ay_lra::rational::Rational::to_big` (654) — i.e. the implied-bound computation has **spilled into
arbitrary-precision (bignum) rational arithmetic** and keeps producing fresh bounds. With the
`u64::MAX = 18446744073709551615` literal in the constraint system, `compute_implied_bounds` →
`run_post_simplex_propagation` → `compute_implied_bounds` cycles, each pass tightening/oscillating a
bound at u64-scale magnitude without ever reaching a fixpoint. That is the concrete non-termination.

## Trigger (semantic)

Every looping atom is part of an **unsigned-64-bit overflow obligation**:

| atom | meaning |
| --- | --- |
| `(<= Int(0) _43#21)` , `(<= _43#21 Int(18446744073709551615))` | `_43` is a `u64`/`usize` (range `[0, u64::MAX]`) |
| `(< (+ start#29 _43#21) Int(0))` | the synthesized "add underflows below 0" check |
| `(< Int(18446744073709551615) (+ start#29 _43#21))` | the synthesized "add overflows u64::MAX" check |

The obligation is encoded in **LIA (unbounded integers)** with the literal `u64::MAX =
18446744073709551615` as an explicit bound, rather than in the **bit-vector theory** where the same
`a + b` wrap check is finite and decidable. Feeding `start + len` with `len ≤ u64::MAX` into the LRA
simplex makes `propagate_var_atoms` chase a bound it can tighten forever (`start ≤ x`, `x ≤ start`,
`0 ≤ start`, re-derive…) without a fixpoint.

## Why the existing watchdogs do NOT catch it

- The trust-mc typed-CHC/PDR watchdog (`run_native_solve_within_deadline`, native.rs:1326,
  `be05d7f14`) wraps the **typed CHC/PDR** solve. This hang is on a **direct `check_sat`**:
  `ay_bindings::execute_direct → incremental::run_check_sat → Solver::check_sat_with_details`. That
  path has no thread-level deadline.
- `TRUST_VERIFY_FN_BUDGET_MS` is checked at **obligation boundaries**; this single obligation never
  returns, so the budget check is never reached.
- The solver is invoked via `solve_interruptible_with_extension_raw` with a `make_should_stop`
  callback — but `propagate_impl` / `propagate_var_atoms` do **not poll `should_stop`** between
  iterations, so even the interruptible entry point can't break this loop.

## Two independent fixes

1. **Owner-side (ay-lra/ay-dpll), the real bug:** give `propagate_var_atoms` /
   `check_during_propagate` a fixpoint/no-progress guard, and/or poll `should_stop` inside the
   propagation loop so the existing interruption mechanism can fire. A non-converging theory
   propagation at level 0 should degrade to Unknown/Timeout, never spin.
2. **Verifier-side (Trust, and the higher-value lever):** route **u64/usize overflow obligations to
   the bit-vector theory**, not LIA-with-a-u64::MAX-literal. This both avoids the loop *and* unblocks
   the #1 frontier from the gap log (unsigned-64-bit arithmetic is currently unverifiable). The BV
   `bvadd`-overflow encoding is finite and the dominating-guard machinery already exists for the mul
   lane (`v2_bv_mul_dominating_guard_constraints`).

## Reproduce

```
cd ~/orc/rust
TRUST_VERIFY_SURVEY=1 TRUST_VERIFY_POLICY=verify-example-corpus \
  ~/trust/build/aarch64-apple-darwin/stage3-tools-bin/aarch64-apple-darwin/tcargo-trust \
  trust check -p orca-core --format json --allow-l0-gaps
```

Hangs while verifying an `orca-core` function whose body does usize string-offset arithmetic
(`start + len`); the obligation immediately preceding the loop in the log is
`agent_notification_id::build_agent_notification_id`
(`crates/orca-core/src/agent_notification_id.rs:16`). Bound any run with
`tools/trust-survey/survey-orca-verify.sh`, which caps each obligation, each function, and the whole
process.
