<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Andrew Yates -->

# A9 — appearance-aware tab strip + light schemes (discharged by `ay`)

Re-checkable certificate for initiative **A9** of
[`PROOF_CARRYING_PERFORMANCE.md`](../../../../../../PROOF_CARRYING_PERFORMANCE.md):
the appearance-aware tab strip `strip_colors` / `bg_is_light`
(`crates/aterm-gui/src/tab_bar.rs`) and the bundled light colour schemes
(`crates/aterm-types/src/scheme.rs`).

Run `bash verify.sh` (locates `ay` even while the trust sysroot rebuilds). It exits 0
iff every obligation gets its expected verdict. **No `trust-mc` needed** — these are
hand-encoded SMT-LIB2 discharged directly by `ay` (the Trust SAT/SMT solver), exactly
like the A5 coverage-blend and A7 keyboard-shift bundles.

## What is proved

The strip derives its tones from the theme itself: `bg_is_light(bg)` is
`0.299·r + 0.587·g + 0.114·b > 150.0`, and `strip_colors` resolves
`(active_t, inactive_t) = if bg_is_light(bg) { (0.10, 0.30) } else { (0.16, 0.40) }`,
then `active_bg = blend(bg, fg, active_t)`. The 12 built-in schemes — the `Default`
scheme (served from `ColorScheme::default()`) plus the 11-entry `BUILTINS` registry,
**8 dark** (including `Default`) and **4 light** — supply the concrete
`(fg, bg, selection)` constants.

| File | Verdict | Theorem (negation asserted; UNSAT ⇒ holds over the stated scope) |
|---|---|---|
| `partition_dark` | UNSAT | every `Appearance::Dark` builtin bg classifies **dark** (`luma1000 ≤ 150000`) |
| `partition_light` | UNSAT | every `Appearance::Light` builtin bg classifies **light** (`luma1000 > 150000`) |
| `dark_factors_unchanged` | UNSAT | **no-regression**: over the *whole* dark-classified region (`luma ≤ 150000`, all `r,g,b ∈ 0..255`) the resolved factors are *exactly* the legacy `(16, 40)` |
| `active_distinct` | UNSAT | the active card ≠ the body for every builtin (the focused tab never vanishes) |
| `raise_direction` | UNSAT | the card raises per appearance — brighter than the body on dark, darker on light |
| `selection_legible` | UNSAT | WCAG `contrast(fg, selection) ≥ 3.0` for every builtin (sound luminance bounds) |
| `partition_nonvacuity_sat` | SAT | a real light builtin exists — the set encoding is not a contradiction |
| `catches_threshold_regression_sat` | SAT | a threshold lowered to 51000 would misclassify **Nord** (`luma1000 = 51574`) |
| `legible_catches_false_floor_sat` | SAT | an over-strong floor (`≥ 4.0`) fails the tightest builtin (Solarized Light, 3.636) |

**The payoff.** `dark_factors_unchanged` is the load-bearing one: same factors fed to the
same `blend` closure ⇒ **byte-identical output** to the pre-appearance code for *every* dark
theme — existing, future, or a user theme file — so the refactor provably cannot regress the
dark path. The three SAT controls give the prove-AND-catch non-vacuity the
`assert_proves_and_catches` convention requires: the partition margin is real (Nord is the
tight bound) and the legibility floor is tight, not vacuously large.

## Faithful model (why the encoding matches the f32 code)

* **Classifier (integer vs f32).** `bg_is_light` compares an f32 luma to `150.0`; the model
  compares `299r + 587g + 114b` to `150000` (×1000). These agree for every builtin with
  enormous margin: the brightest dark bg is **Nord 51574** and the darkest light bg is
  **Gruvbox Light 239202**, both ~90 000 luma-units from the threshold — far beyond any f32
  rounding of the three coefficients (≤ 1 unit in 150 000). `partition_dark`/`partition_light`
  certify each builtin sits deep in its half; the threshold sub-ulp behaviour is irrelevant
  to the actual data.
* **Distinctness without a divider.** The integer test `T·|fg−body| < 50` (`t = T/100`) is a
  **sound over-approximation** of "this channel is unchanged" (rounded move `< 0.5`): exact for
  `T = 16`, and for `T = 10` it can only *over*-predict a change at the `|fg−body| = 5` boundary
  (the f32 literal `0.10 ≠ 1/10`) — which only makes the UNSAT *harder*, never unsound. Every
  builtin's nearest channel is `|fg−body| = 96`, far from that boundary (so the model is exact on
  the actual data). This keeps `active_distinct` linear — **no `bvudiv`**, dodging the
  divider-bit-blast frontier the A5 README documents.
* **Direction via a luma surrogate.** The card is `body` blended *toward* `fg` (`t ∈ (0,1)`),
  and luma is a positive-weight linear combination, so the card's luma lies strictly between
  `body` and `fg` luma (the A5 `blend_in_gamut_case{A,B}` no-overshoot lemma, per channel).
  `raise_direction` therefore proves the division-free surrogate `sign(luma(fg) − luma(body))`
  matches the appearance, and the A5 lemma carries it to the rounded card.
* **WCAG contrast via sound bounds.** `contrast = (L_light + 0.05)/(L_dark + 0.05) ≥ 3.0`
  iff `L_light ≥ 3·L_dark + 0.10`. The sRGB transfer (`((c+0.055)/1.055)^2.4`) is irrational
  and outside SMT, so each luminance is bracketed by **outward-rounded 12-digit rationals**
  (the transfer is monotone, so `floor`/`ceil` of the exact value are sound bounds). Feeding
  the lighter colour's *lower* bound and the darker's *upper* bound into the linear inequality
  makes `selection_legible ≥ 3.0` a sound, division-free QF_LIA obligation. The lower bounds
  are floored and the upper bounds ceiled (directional rounding, so each stays on the sound
  side). All nine `.smt2` files are emitted byte-for-byte by `gen_bounds.py` in this directory
  (`python3 gen_bounds.py`), so the literals cannot drift from hand-transcription; they are
  cross-checked by the exact f32 `selection_is_legible_over_foreground` unit test in `scheme.rs`.

## Honest scope

This certifies the strip's **colour arithmetic and the classification/legibility invariants**
over the bundled palettes (plus the dark region in full). It says nothing about glyph
rasterisation, the composed `RenderInput`, or mouse hit-testing — those keep their unit tests
(`tab_bar::tests`). Keep the lemmas **and** the tests: the SMT proves the all-inputs algebra,
the tests pin the concrete f32 evaluation the model abstracts.
