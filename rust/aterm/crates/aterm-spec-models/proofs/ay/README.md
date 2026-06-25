<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Andrew Yates -->

# A5 — coverage-blend proof bundle (discharged by `ay`)

Re-checkable certificate for initiative **A5** of
[`PROOF_CARRYING_PERFORMANCE.md`](../../../../../PROOF_CARRYING_PERFORMANCE.md):
the CPU coverage-blend `blend()` at `crates/aterm-render/src/lib.rs:2257-2266`.

Run `bash verify.sh` (locates `ay` even while the trust sysroot rebuilds). It exits 0
iff every obligation gets its expected verdict. **No `trust-mc` needed** — these are
hand-encoded SMT-LIB2 discharged directly by `ay` (the SAT/SMT/CHC solver).

## What is proved

Per channel the real code computes, in u32: `mix = (bg*(255-t) + fg*t) / 255`, then packs
`(mix_r<<16) | (mix_g<<8) | mix_b` with **no `& 0xff` mask** (`lib.rs:2266`).

| File | Verdict | Theorem (negation asserted; UNSAT ⇒ holds ∀ bg,fg,t ∈ 0..=255) |
|---|---|---|
| `blend_endpoint_exact` | UNSAT | `t=0 ⇒ mix=bg` and `t=255 ⇒ mix=fg` (A5 committed scope: hard-edge/Powerline path is bit-exact) |
| `blend_in_gamut_caseA` | UNSAT | `bg≤fg ⇒ min(bg,fg) ≤ mix ≤ max(bg,fg)` |
| `blend_in_gamut_caseB` | UNSAT | `fg≤bg ⇒ min(bg,fg) ≤ mix ≤ max(bg,fg)` |
| `blend_numerator_nowrap` | UNSAT | numerator ≤ 130050 < 2¹⁸ (justifies the 18-bit model's fidelity to u32) |
| `blend_nonvacuity_sat` | SAT | a real interior value exists (`mix=128`) — encoder is not trivial |
| `blend_catches_false_bound` | SAT | the checker catches a deliberately false bound (`mix≤200`) |

**The PCP payoff.** cases A+B ⇒ `0 ≤ mix ≤ 255` ⇒ the unmasked `<<8`/`<<16` packing at
`lib.rs:2266` cannot bleed across channels. The *absence of a `& 0xff` guard there is now a
discharged theorem*, not an unstated assumption. The two SAT controls give the
prove-AND-catch non-vacuity the `assert_proves_and_catches` convention requires.

**Honest scope (unchanged from the doc).** This models the blend *arithmetic only*. It says
nothing about atlas packing, UV/rasterization, the two render passes, or `Rgba8Unorm`
readback — the device-dependent path. **Do not** delete `gpu_matches_cpu.rs` on the strength
of this lemma; keep the lemma and the tests.

## Engine-frontier note (why the proof is shaped this way)

The naïve monolithic encodings of the no-overshoot theorem **do not discharge** on `ay`:

- 32-bit `bvudiv` form → **timeout** (two symbolic 32-bit multipliers + a divider bit-blast
  into an intractable SAT instance).
- 18-bit `bvudiv` form → **timeout** (narrower, still has the divider).
- 18-bit division-free form → **timeout** (two symbolic multipliers).
- `QF_NIA` (integer) form → **`unknown (unsupported arithmetic)`** — `ay` has no nonlinear
  integer-division decision procedure.

This is `ay`'s nonlinear-BV frontier (consistent with the doc's note that its algebraic
engine bails on BitVec sorts). The discharge comes from **reformulation**, not a bigger
hammer: the algebra `num = 255·bg + t·(fg−bg)` exposes a *single* variable multiplier, and a
case-split on the color ordering removes the `min/max` `ite`. Each case then reduces to the
one-multiplier monotonicity fact `0 ≤ t·d ≤ 255·d` (`d = |fg−bg|`), which `ay` proves in well
under a second. The width narrowing to 18 bits is itself justified by `blend_numerator_nowrap`.

Lesson for the broader program: when `ay` stalls, **route by reformulation** (eliminate
dividers, factor to fewer variable multipliers, case-split away `ite`s) before escalating up
the engine ladder.
