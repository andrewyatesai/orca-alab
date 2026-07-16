# orca-session-gc — ay proof certificate

Machine-checked safety certificate for the daemon session-history GC planner,
discharged by [`ay`](../../../../../..) (SAT/SMT/CHC solver) on hand-encoded
SMT-LIB2. This is the "machine-checked safety certificates on the emitted code" half
of the moonshot **E1** claim; the differential parity corpus
([`../../parity-corpus.txt`](../../parity-corpus.txt), run by BOTH the Rust core and
the TS `planSessionHistoryGc`) is the "regression-gated behavioral parity corpora"
half. The fs scan + `rmSync` executor around the planner is unchanged and covered by
the 12 `history-retention.test.ts` integration tests.

Run: `bash verify.sh` — exits 0 iff every obligation gets its expected verdict.

## Obligations — age-expiry decision

| file | kind | verdict | property |
|------|------|---------|----------|
| `ex1_live_never_expires` | theorem | unsat | a live dir is never age-expired |
| `ex2_toctou_floor_never_expires` | theorem | unsat | age < minDirAge ⇒ never expired (TOCTOU guard) |
| `ex3_unknown_liveness_unrestored_never_expires` | theorem | unsat | liveness unknown + not-ended ⇒ never expired (∞ retention) |
| `ex_c1_ended_expiry_reachable_sat` | control | sat | an ended dir past retention DOES expire |

## Obligations — size-cap eviction

| file | kind | verdict | property |
|------|------|---------|----------|
| `ev1_never_below_nonevictable` | theorem | unsat | remaining ≥ non-evictable bytes (live/recoverable never traded for disk) |
| `ev2_reaches_budget_when_enough` | theorem | unsat | enough evictable ⇒ remaining reaches the budget |
| `ev_step_monotone` | theorem | unsat | each eviction step never raises remaining |
| `ev_c1_eviction_reaches_budget_sat` | control | sat | eviction can bring the store under budget |

A theorem file asserts the **negation** of its property; `unsat` means no
counterexample exists over the whole input domain, i.e. proved ∀.

## Model ↔ Rust fidelity

The expire decision is modelled exactly as `should_expire_session_dir`
(`src/lib.rs`): `expire = ¬exempt ∧ over_retention` with
`exempt = isLive ∨ age < minDirAge` and `over_retention = (isEnded ∧ age>ended) ∨
(¬isEnded ∧ ¬livenessUnknown ∧ age>unrestored)` — the `¬isEnded ∧ livenessUnknown`
branch contributes nothing, encoding the TS `∞` retention (Rust `None`). Flags are
Bools; ages are ints; constants match the corpus (min 10 / ended 100 / unrestored
1000). The size-cap loop is abstracted by its byte accounting: `survivorBytes =
nonEvict + evictTotal`, the loop removes only evictable bytes
(`0 ≤ evicted ≤ evictTotal`), so the floor (ev1), achievability (ev2), and
monotonicity (ev_step) hold for all non-negative totals — the concrete oldest-first
ORDER is checked by the parity corpus (which includes tie-breaking + spare-live
cases). Fidelity to the running code is grounded by that corpus, replayed by both
implementations; the executor is exercised by the integration tests.
