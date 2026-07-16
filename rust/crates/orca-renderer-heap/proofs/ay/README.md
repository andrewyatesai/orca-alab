# orca-renderer-heap — ay proof certificate

Machine-checked safety certificate for the renderer heap-ceiling RAM-tier decision,
discharged by [`ay`](../../../../../..) (SAT/SMT/CHC solver) on hand-encoded
SMT-LIB2. This is the "machine-checked safety certificates on the emitted code" half
of the moonshot **E1** claim; the differential parity corpus
([`../../parity-corpus.txt`](../../parity-corpus.txt), run by BOTH the Rust core and
the TS production sizing) is the "regression-gated behavioral parity corpora" half.

Run: `bash verify.sh` — exits 0 iff every obligation gets its expected verdict.

## Obligations

| file | kind | verdict | property |
|------|------|---------|----------|
| `rh1_band_bound` | theorem | unsat | ceiling ∈ [3072, 4096] for every target t ≥ 0 |
| `rh2_clamp_monotone` | theorem | unsat | t1 ≤ t2 ⇒ clamp(t1) ≤ clamp(t2) (ceiling monotone in RAM) |
| `rh3_floor_redundant_under_gate` | theorem | unsat | t ≥ 3072 (the gated invariant) ⇒ max(3072, t) = t (floor is dead code) |
| `rh_c1_cap_active_sat` | control | sat | the CAP fires at t ≥ 4096 (band tight at the top) |
| `rh_c2_gate_bottom_sat` | control | sat | the band bottom t = 3072 is reached at the 7.5 GiB gate (tight) |

A theorem file asserts the **negation** of its property; `unsat` means no
counterexample exists over the whole input domain, i.e. proved ∀.

## Model ↔ Rust fidelity, and the float/integer split

The SMT abstracts the target `t = floor(totalGiB * 0.4) * 1024` (a non-negative
whole number) as a **free integer**, so the surrounding clamp
`min(4096, max(3072, t))` is proved over all t in QF_LIA — no floating-point theory.
The reachable path actually has `t ≥ 3072` (the 7.5 GiB gate forces
`floor(totalGiB * 0.4) ≥ 3`), which is why `rh3` can show the 3072 floor is a
redundant defensive clamp and the real work is the 4096 cap.

The **float layer** — that JS `Number` and Rust `f64` compute the *same* `t` from
the same byte count (both IEEE-754 doubles: the `/ 2^30`, `* 0.4`, `floor`, and the
final exact `as u32` all agree bit-for-bit) — is not an ay obligation; it is pinned
by the differential parity corpus, whose RAM-tier rows are computed by both the Rust
core and the real TS `computeRendererHeapCeilingMb` (parser included). So a drift in
either the arithmetic or the clamp surfaces as a corpus failure.
