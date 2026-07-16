# orca-flow-control — ay proof certificate (P3 stage 3)

Machine-checked safety certificate for the producer PTY flow-control decision,
discharged by [`ay`](../../../../../..) (SAT/SMT/CHC solver) on hand-encoded
SMT-LIB2. This is the "machine-checked safety certificates on the emitted code"
half of the moonshot **E1** claim; the differential parity corpus
([`../../parity-corpus.txt`](../../parity-corpus.txt), run by BOTH the Rust core
and the TS production controller) is the "regression-gated behavioral parity
corpora" half. Together they prove: the *spec* is correct (here) and both
*implementations* are equivalent to it (there).

Run: `bash verify.sh` — exits 0 iff every obligation gets its expected verdict.

## Obligations

| file | kind | verdict | property |
|------|------|---------|----------|
| `t1_no_flap` | theorem | unsat | paused + pending ∈ [LOW,HIGH] ⇒ no action (anti-flap hysteresis) |
| `t2_reassert_gated` | theorem | unsat | paused + flooding + elapsed < REASSERT ⇒ no re-Pause |
| `t3_no_spurious_resume` | theorem | unsat | paused + pending ≥ LOW ⇒ not Resume (strict low edge) |
| `t4_unpaused_pause_iff_over_high` | theorem | unsat | unpaused: Pause ⇔ pending > HIGH (strict high edge) |
| `c1_reassert_reachable_sat` | control | sat | the reassert path is reachable (t1/t2 not vacuous) |
| `c2_catches_off_by_one_sat` | control | sat | a `> HIGH-1` off-by-one bound IS caught |

A theorem file asserts the **negation** of its property; `unsat` means no
counterexample exists over the whole (non-negative) input domain, i.e. proved ∀.

## Model ↔ Rust fidelity

The SMT encodes `update()`'s decision exactly (`src/lib.rs`), with the same
constants (HIGH=262144, LOW=32768, REASSERT=5000) and the same operators
(strict `>` HIGH to pause, strict `<` LOW to resume, `>=` interval to reassert).
`elapsed` models the Rust `now_ms.saturating_sub(paused_at)` as
`ite(now >= paused_at, now - paused_at, 0)`. The model's fidelity to the *running*
code is grounded downstream by the parity corpus, whose steps exercise these exact
edges and are replayed byte-for-byte by both implementations — so a drift between
the proved spec and the shipped code would surface as a corpus failure.
