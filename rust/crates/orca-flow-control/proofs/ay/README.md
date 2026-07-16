# orca-flow-control — ay proof certificate (P3 stage 3)

Machine-checked safety certificates for the two flow-control decision cores in
this crate, discharged by [`ay`](../../../../../..) (SAT/SMT/CHC solver) on
hand-encoded SMT-LIB2. This is the "machine-checked safety certificates on the
emitted code" half of the moonshot **E1** claim; the differential parity corpora
([`../../parity-corpus.txt`](../../parity-corpus.txt) and
[`../../keep-tail-parity-corpus.txt`](../../keep-tail-parity-corpus.txt), each run
by BOTH the Rust core and the TS production code) are the "regression-gated
behavioral parity corpora" half. Together they prove: each *spec* is correct
(here) and both *implementations* are equivalent to it (there).

Run: `bash verify.sh` — exits 0 iff every obligation gets its expected verdict.

## Obligations — producer controller (`ProducerFlowController`)

| file | kind | verdict | property |
|------|------|---------|----------|
| `t1_no_flap` | theorem | unsat | paused + pending ∈ [LOW,HIGH] ⇒ no action (anti-flap hysteresis) |
| `t2_reassert_gated` | theorem | unsat | paused + flooding + elapsed < REASSERT ⇒ no re-Pause |
| `t3_no_spurious_resume` | theorem | unsat | paused + pending ≥ LOW ⇒ not Resume (strict low edge) |
| `t4_unpaused_pause_iff_over_high` | theorem | unsat | unpaused: Pause ⇔ pending > HIGH (strict high edge) |
| `c1_reassert_reachable_sat` | control | sat | the reassert path is reachable (t1/t2 not vacuous) |
| `c2_catches_off_by_one_sat` | control | sat | a `> HIGH-1` off-by-one bound IS caught |

## Obligations — keep-tail sizing (`background_session_keep_tail_chars`)

| file | kind | verdict | property |
|------|------|---------|----------|
| `kt1_clamp_bound` | theorem | unsat | keep_tail ∈ [64K, 512K] for every divide result (no starve / no overshoot) |
| `kt2_drop_cap_bound` | theorem | unsat | drop_cap = 2·keep_tail ∈ [128K, 1M] (bounded backlog) |
| `kt3_clamp_monotone` | theorem | unsat | x1 ≥ x2 ⇒ clamp(x1) ≥ clamp(x2) (the monotone-in-n leg) |
| `kt_c1_floor_active_sat` | control | sat | the 64K floor is reached — kt1's band is tight below |
| `kt_c2_cap_active_sat` | control | sat | the 512K cap is reached — kt1's band is tight above |

A theorem file asserts the **negation** of its property; `unsat` means no
counterexample exists over the whole (non-negative) input domain, i.e. proved ∀.

## Model ↔ Rust fidelity

**Producer:** the SMT encodes `update()`'s decision exactly (`src/lib.rs`), with
the same constants (HIGH=262144, LOW=32768, REASSERT=5000) and the same operators
(strict `>` HIGH to pause, strict `<` LOW to resume, `>=` interval to reassert).
`elapsed` models the Rust `now_ms.saturating_sub(paused_at)` as
`ite(now >= paused_at, now - paused_at, 0)`.

**Keep-tail:** the SMT abstracts the exact term `floor(2M / max(1,n))` as a free
`x >= 0` (u64 division is total and non-negative) and encodes the surrounding
`min(512K, max(64K, x))` clamp exactly (`src/keep_tail.rs`), so kt1/kt2 hold for
*every* possible divide result, not just sampled n. kt3 certifies only the clamp's
order-preservation; the divide's own non-increasing-in-n behavior is the elementary
arithmetic fact `n1 ≤ n2 ⇒ floor(B/n1) ≥ floor(B/n2)`, checked over n=1..200 by the
Rust `keep_tail_is_non_increasing_in_session_count` test.

Each model's fidelity to the *running* code is grounded downstream by its parity
corpus, whose steps/points are replayed by both implementations — so a drift
between a proved spec and the shipped code would surface as a corpus failure.
