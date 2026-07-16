# orca-provider-backoff — ay proof certificate

Machine-checked safety certificate for the provider rate-limit refetch-backoff
decision, discharged by [`ay`](../../../../../..) (SAT/SMT/CHC solver) on
hand-encoded SMT-LIB2. This is the "machine-checked safety certificates on the
emitted code" half of the moonshot **E1** claim; the differential parity corpus
([`../../parity-corpus.txt`](../../parity-corpus.txt), run by BOTH the Rust core
and the TS production sizing) is the "regression-gated behavioral parity corpora"
half. Together they prove the *spec* is correct (here) and both *implementations*
are equivalent to it (there).

Run: `bash verify.sh` — exits 0 iff every obligation gets its expected verdict.

## Obligations

| file | kind | verdict | property |
|------|------|---------|----------|
| `bo1_throttle_bound` | theorem | unsat | throttle ∈ [30s, 15min] for every multiplier p ≥ 1 |
| `bo2_monotone` | theorem | unsat | p1 ≤ p2 ⇒ throttle(p1) ≤ throttle(p2) (non-decreasing in streak) |
| `bo3_saturates` | theorem | unsat | p ≥ ⌈MAX/BASE⌉ = 30 ⇒ throttle = MAX (pins to the ceiling) |
| `bo_c1_unsaturated_reachable_sat` | control | sat | the doubling takes intermediate values (bo1/bo3 not vacuous) |
| `bo_c2_floor_tight_sat` | control | sat | the 30s BASE floor is reached (band tight; strict `>BASE` is false) |

A theorem file asserts the **negation** of its property; `unsat` means no
counterexample exists over the whole input domain, i.e. proved ∀.

## Model ↔ Rust fidelity

The SMT encodes `active_failure_refetch_throttle_ms`'s decision
(`src/lib.rs`) — `min(BASE·2^max(0,streak-1), MAX)` with `BASE = 30000`,
`MAX = 900000`. The exponential `2^max(0,streak-1)` is abstracted as a free
**`p ≥ 1`** (every power of two is ≥ 1, and the real streaks give a subset of the
powers of two), so `BASE·p` is *linear* in the solver and the min/clamp bounds
hold for **all** p, not sampled streaks. The exponential's own
non-decreasing-in-streak behavior (needed to lift bo2 from "clamp preserves order"
to "throttle rises with the streak") is the elementary fact
`e1 ≤ e2 ⇒ 2^e1 ≤ 2^e2`, checked over the domain by the Rust
`is_non_decreasing_in_streak` test.

**Two distinct obligations, intentionally separated:**
- *Mathematical correctness of the spec* (unbounded integers) — proved here by ay.
- *Overflow-safety of the finite-width implementation* — the Rust `1u64 << exp`
  must never overflow/panic for any `u32` streak. That is a property of the
  `checked_shl(...).unwrap_or(u64::MAX)` + `saturating_mul` construction, evidenced
  by `stays_in_the_backoff_band` / `saturates_and_stays_saturated` calling the
  function at `u32::MAX`. bo3 is what makes it *sound* to saturate: past p = 30 the
  exact multiplier is irrelevant, only that it is ≥ 30, so clamping a huge shift to
  `u64::MAX` yields the same `MAX` result as the exact arithmetic.

Each model's fidelity to the *running* code is grounded downstream by the parity
corpus, whose rows are replayed by both implementations — so a drift between the
proved spec and the shipped code would surface as a corpus failure.
