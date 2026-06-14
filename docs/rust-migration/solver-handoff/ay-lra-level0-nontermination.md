# ay-lra / ay-lia level-0 propagation non-termination on u64-overflow atoms

> **THE HANG IS RESOLVED (validated 2026-06-14, build #40).** A small, sound, opt-in fix makes the
> direct-SMT path bound itself: `execute_direct`'s `ExecutionContext::new` now reads
> `AY_DIRECT_SOLVE_TIMEOUT_MS` and calls `solver.set_timeout`, which enables the ay solver's OWN
> deadline→`should_stop`→`theory_backend.rs:531` abort (that budget check was added specifically for
> theory-heavy QF_LRA). A non-converging propagation degrades to **Unknown** instead of spinning —
> sound (never Proved). **Default behavior unchanged when the env is unset.** Patch:
> `execute-direct-timeout-fix.patch` (committed to ay branch `survey-execute-direct-timeout`, base
> `beeb0d6`). **Proof:** same build #40 binary + same source — with the env set, the orca-core survey
> COMPLETES (927 obligations, 2m46s); with it unset, `trustc` spins 100% CPU for 84s+ stuck in
> `ay_lra::compute_implied_bounds` (sampled). The only variable is the env var.
>
> Two things remain OWNER-SIDE (this fix bounds the symptom, soundly, but does not cure them):
> 1. **The propagation still doesn't converge** — `compute_implied_bounds`/`run_post_simplex_propagation`
>    should reach a fixpoint (or poll `should_stop`) so the deadline isn't the only thing that stops it.
> 2. **u64/usize overflow still can't be PROVED** (gap-log lever #1) — it now times out to Unknown
>    rather than proving; deciding these linear u64 formulas is the real capability gap.
>
> NOT recommended (and still rejected by design): routing add/sub overflow to BV (completeness loss,
> see below). Pushing the timeout fix to ay origin main / advancing the trust→ay pin is deferred — ay
> is pinned 89 commits behind its own main, so advancing it is a separate, riskier decision.

**Status:** owner-side core-solver bug (ay-dpll / ay-lra / ay-lia). Reproduced deterministically
against the Orca workload on Trust stage2 built 2026-06-14 00:27 (trust-mc `be05d7f14`).
**Impact:** a single obligation can spin `trustc` at 100% CPU indefinitely (observed >23h in an
earlier run) — a verifier must never be able to hang on one obligation, so this is a
correctness-of-the-tool invariant, not just a performance issue. **Now bounded by the fix above.**

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

## The fix is owner-side (ay-dpll ↔ ay-lra propagation termination)

**The right fix is a no-progress/round guard in the theory-propagation handshake**, NOT a change to
how the verifier encodes the obligation. Concretely: the `ay_sat::cdcl_loop_impl → TheoryCallback::
propagate → ay_dpll::extension::propagate_impl → ay_lra::check_during_propagate` round loop must
detect that it is re-asserting atoms already assigned at the current decision level (the WARN dump
shows duplicate `TermId`s) and stop — degrading to Unknown/Timeout rather than spinning. The codebase
already uses caps of exactly this flavor nearby (`dual_simplex_with_max_iters`, `MAX_RECURSIVE_CALLS
= 256`, the `#8256` propagation-fixpoint count at `theory_solver/propagation.rs:595`, and
`expr_split_seen_count >= 50` in `extension/propagate.rs`); this handshake needs an analogous
same-level round cap and/or a `should_stop` poll inside the loop (`propagate_impl` /
`propagate_var_atoms` currently never poll the `make_should_stop` callback they are handed).

### Why NOT to "just route add/sub to the BV theory"

Tempting, but wrong — and the Trust verifier already deliberately rejects it. `trust-vcgen`
generate.rs:2554-2582 routes **only MUL** to the BV lane and keeps unsigned **add/sub on the
Int/LIA path on purpose**, because the Int path conjoins the operands' preconditions, dominating
guards, and block-defs (`input_range_constraint`, `v2_formula_with_block_defs`,
`conjoin_arg_type_ranges`) — which let a *precondition-bounded* add/sub PROVE. The BV
`v2_unsigned_bv_overflow_formula` uses FRESH unconstrained operands (`__trust_ovf_bv_*`, sorts must
not collide), so those guards are dropped and provably-safe code false-FAILs. So BV-routing add/sub
would trade a solver-termination bug for a pervasive completeness regression. The encoding is sound
and intentional; the LRA propagation just has to terminate on it.

(The connection to gap-log lever #1 — "u64/usize arithmetic unverifiable" — stands, but the lever is
*also* about the solver actually deciding these linear u64 formulas, which today it cannot because it
hangs. Fix the propagation termination first; the verifiability follows on the same obligations.)

## Likely a regression (worth bisecting)

The same `orca-core` crate surveyed **clean to completion at gap-log build #29** (2026-06-09): 287
functions, 1280 obligations, 7.8 MB of deterministic per-obligation JSON, no hang. It hangs now (build
#39, 2026-06-14). Between those points the solver core advanced ~133 commits (the unsigned/signed
BV-mul overflow migration and associated LRA/propagation tuning — `#8003/#8187/#8255/#8256/#8707/#8810`
markers are all in this window). The `build_agent_notification_id`-class functions are not new, so the
non-termination most likely entered with that recent arithmetic-encoding / LRA-propagation work rather
than being a longstanding limitation. A bisect across that range against the repro below should localize
the regressing commit quickly.

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
