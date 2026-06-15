# LRA Zeno non-termination — root cause pinned + the principled fix

_2026-06-15. Supersedes the "owner-side hand-off" framing: the owner directed this
be fixed in-tree. This is the algorithm-level analysis behind the work-budget fix._

## The hang, precisely

`ay_lra::implied_bounds::compute_implied_bounds` does GMP-bignum rational arithmetic
that grows without bound on u64-offset obligations (orca-core `build_agent_notification_id`
is the canonical reproducer). A `sample` pins the CPU inside `to_rug` / `mul_add_assign`.

Two candidate shapes — **a single non-returning call** vs **the outer DPLL(T) loop
re-entering**. The code rules out the first:

- Within one call, the cascade frontier (`round_newly_bounded`) only grows when
  `tighten_count[vi] <= MAX_TIGHTENS_PER_VAR` (=4, per-call, reset each call —
  implied_bounds.rs:91-92, 1222, 1249).
- So a single call performs at most ~`4 * num_vars` cascade rounds, then
  `round_newly_bounded` stays empty and the loop breaks (1367). **One call always
  terminates.**

Therefore the non-termination is the **outer loop re-entering `compute_implied_bounds`
unboundedly**, each call terminating but doing ever-more-expensive bignum work.

## Why the owner's Zeno throttle (#8857) doesn't hold across the outer loop

The throttle (`accept_replacing_tighten`, implied_bounds.rs:1469-1490) caps *replacing*
tightenings per variable at `IMPLIED_TIGHTEN_STREAK_CAP = 8`; beyond that a tightening
is accepted only if it crosses an unassigned atom threshold (`bound_crosses_unassigned_atom`).
`implied_tighten_streak` persists across calls — **except** it is reset:

- to 0 for each changed variable on incremental overlay (implied_bounds.rs:131-132), and
- entirely via `implied_tighten_streak.fill(0)` on every full-scan overlay (line 141),

both gated by `direct_bounds_changed_since_implied`. In the DPLL(T) search the solver
constantly asserts/retracts direct bounds (decisions, propagations, backtracking), so the
streak is reset frequently and the cap-of-8 **never accumulates across outer-loop
re-entries**. The Zeno cascade restarts each time, with the rational denominators from the
*persisted* `implied_bounds` (lines 60-69) carried forward — so each re-entry is strictly
more expensive than the last. Unbounded work, growing per-call cost.

## Two fixes

### 1. Entry-guard work budget (shipped in ay branch `lra-implied-work-budget`, the current build)

`compute_implied_bounds` counts its own invocations in `implied_work_done` (persists for
the solver lifetime — NOT reset on pop; a fresh solver is built per obligation, so this is a
per-obligation budget). Once `implied_work_done >= implied_work_budget` it returns
immediately (empty `newly_bounded`, `converged: true`), **skipping the cascade entirely**.
The solver stays responsive so the wall-clock deadline can fire and the propagation handshake
quiesces. Sound: implied bounds only strengthen propagation; feasibility is the bounded dual
simplex's verdict, so skipping them never flips a result.

**Limitation:** it's a *call-count* proxy. Per-call cost grows across re-entries, so a
call-count budget caps the number of calls but not total work — a budget high enough to prove
hard obligations may still permit very expensive late calls. The default (4M) is effectively
unreachable; the working value is empirical (test starts at 20000).

### 2. Cumulative replacing-tighten cap (the principled "works by default" fix — next build if #1's proxy is too coarse)

Bound the actual Zeno operation, not call count. Add a solver-lifetime counter
`replacing_tighten_done` incremented in `accept_replacing_tighten` whenever it returns true
for a *replacing* tightening; once it exceeds a budget, `accept_replacing_tighten` returns
`false` unconditionally. This:

- bounds the exact bignum-growing operation (replacing tightenings with ballooning denominators),
- is **not** defeated by the streak resets (it's a separate cumulative counter, never reset),
- quiesces the cascade at the source: bounds stop being replaced → denominators stop growing
  → re-entries become cheap → the deadline/fixpoint resolves,
- is purely additive and sound by the same argument (discarding a derived bound only weakens
  propagation).

This is the smaller, more targeted change and the better default. The entry-guard remains as
a coarse outer backstop.

## Test plan (build #67, ay `1bc24189af82`)

`tools/trust-survey/focused-hang-test.sh build_agent_notification_id 20000 180` — one
function, low budget, AY direct-solve timeout DISABLED (termination proven by budget alone),
perl-alarm backstop. Exit 142 ⇒ still hangs (diagnosis wrong / SAT-core). Otherwise ⇒
entry-guard terminates it; then tune the budget and run the full orca-core survey to measure
the gap with var×const + Unsize + Zeno + budget combined.
