# orca-crash-recovery — ay proof certificate

Machine-checked safety certificates for the two crash-recovery decision cores in
this crate, discharged by [`ay`](../../../../../..) (SAT/SMT/CHC solver) on
hand-encoded SMT-LIB2. This is the "machine-checked safety certificates on the
emitted code" half of the moonshot **E1** claim; the differential parity corpora
([`../../renderer-recovery-parity-corpus.txt`](../../renderer-recovery-parity-corpus.txt)
and [`../../gpu-fallback-parity-corpus.txt`](../../gpu-fallback-parity-corpus.txt),
each an operation trace replayed by BOTH the Rust core and the TS class) are the
"regression-gated behavioral parity corpora" half.

Run: `bash verify.sh` — exits 0 iff every obligation gets its expected verdict.

## Obligations — renderer-recovery rate limiter (`RendererRecoveryCircuitBreaker`)

| file | kind | verdict | property |
|------|------|---------|----------|
| `rr1_never_exceeds_max` | theorem | unsat | inductive safety: post-count ≤ max ⇒ at most `max` attempts in any window |
| `rr2_no_admit_at_cap` | theorem | unsat | c ≥ max ⇒ rejected AND count unchanged (a rejected attempt is never recorded) |
| `rr3_reset_reopens` | theorem | unsat | c = 0 (post-reset), max ≥ 1 ⇒ allowed (no permanent lockout — liveness) |
| `rr_c1_reject_reachable_sat` | control | sat | the open-breaker/reject branch is reachable |
| `rr_c2_admit_reachable_sat` | control | sat | the admit branch is reachable and grows the count |

## Obligations — GPU one-shot fallback latch (`GpuCrashFallbackTracker`)

| file | kind | verdict | property |
|------|------|---------|----------|
| `gf1_engages_at_most_once` | theorem | unsat | already engaged ⇒ no-op (relaunch at most once) |
| `gf2_window_gate` | theorem | unsat | crash outside [0, window] ⇒ no-op |
| `gf3_no_engage_below_threshold` | theorem | unsat | engaged ⇒ post-count ≥ threshold |
| `gf_c1_engage_reachable_sat` | control | sat | the latch can actually trip |
| `gf_c2_upper_boundary_inclusive_sat` | control | sat | m = window counts (inclusive edge; catches an `m ≥ window` off-by-one) |

A theorem file asserts the **negation** of its property; `unsat` means no
counterexample exists over the whole input domain, i.e. proved ∀.

## Model ↔ Rust fidelity

Both cores make integer-only decisions, so each SMT models the pruned/accumulated
in-window count as a **free integer** and encodes the decision exactly
(`src/renderer_recovery.rs`, `src/gpu_fallback.rs`) in QF_LIA — no floats, no
bit-vectors. The rate limiter's per-step safety (rr1) is the *inductive step* of the
trace property "at most `max` attempts in any window `W`": the stored list starts
empty and `register` only pushes when the pruned length is below `max`, so the
length is an invariant ≤ `max`; prune (strict `>` cutoff) only removes. The latch's
`gf1`/`gf3` pin the one-shot + threshold-floor guarantees. Each model's fidelity to
the *running* code is grounded downstream by its parity corpus, whose operation
trace is replayed step-for-step by both implementations — so a drift between a
proved spec and the shipped code would surface as a corpus failure.
