<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The aterm Authors -->

# A6 — GPU atlas texture-height device-limit clamp (discharged by `ay`)

Re-checkable certificate for initiative **A6**: the aterm GPU glyph-atlas texture is
allocated with a **height clamped to the device's max 2D texture dimension**, so the
`+headroom` growth can never push the created texture past what the GPU allows.
Creating an oversized texture **aborts the wgpu device** (lost-device DoS). Proving
the clamped height is always `<= max_texture_dimension_2d` turns that abort into an
**impossibility for the proven (height/headroom) scope**.

**Discharged by `ay` (the Trust SAT/SMT solver) — Trust, not kani.** Hand-encoded
SMT-LIB2 in **`QF_BV`** (`u32` modeled as native 32-bit bitvectors). Run
`bash verify.sh` → exits 0 iff all five obligations get their expected verdict.

**Polarity (plain SMT, not CHC):** each `.smt2` asserts the **negation** of its
theorem. `unsat` = the theorem holds for **all** inputs in the modeled domain;
`sat` = a witness / counterexample exists.

## Faithful source (verified against the live tree, 2026-06-20)

`crates/aterm-gpu/src/renderer.rs`:

| Concept | Source |
|---|---|
| `HEADROOM` = `ATLAS_GROW_HEADROOM` = `256` | `:56` |
| `max` = `max_tex_dim` = `device.limits().max_texture_dimension_2d` | `:1404` |
| `tex_h = (atlas.height + ATLAS_GROW_HEADROOM).min(max_tex_dim)` — clamp **after** `+headroom` | `:1405` |
| created texture height is exactly `tex_h` (`Extent3d { width: atlas.width, height: tex_h, .. }`) | `:1408` |
| `h` = `atlas.height` is bounded `<= cap_h` **before** this point by the packer rollback / grow guard | `:431`, `:486` |
| `cap_h` passed to the packer **is** `max_texture_dimension_2d` | `:1537` |
| atlas **width** is the fixed constant `ATLAS_WIDTH = 1024` (no width clamp) | `:47`, `:408` |

## Honest property statement

For every `atlas.height` (`h`) and every device limit `max_texture_dimension_2d`
(`max`), the height the renderer passes to `create_texture`,

> **`tex_h = (h + 256).min(max)  <=  max`**,

with the side-fact that, under the faithful precondition (`h <= max` from the packer
**and** `max <= u32::MAX − 256` from real device limits), the `h + 256` add **does
not wrap**, so the clamp operates on the **true** (non-wrapped) value. Therefore the
created texture's height never exceeds the device's max 2D texture dimension, and the
**oversized-texture device-abort is impossible along the height/headroom path** in the
proven scope.

## What is proved

| File | Verdict | Obligation |
|---|---|---|
| `height_clamp_le_limit.smt2` | **unsat** | `(h + HEADROOM).min(max) <= max` for **all** `h, max` (u32::min semantics: `a.min(b) <= b` unconditionally — device-abort impossible) |
| `height_no_overflow.smt2` | **unsat** | under `h <= max` (packer bound) **and** `max <= u32::MAX − HEADROOM` (real device-limit margin), `h + HEADROOM` does **not** wrap u32 — so `.min` sees the true value, not a wrapped under-size |
| `width_within_limit.smt2` | **unsat** | the **documented assumption** made explicit: under `assume(max >= 2048)` (wgpu downlevel min), `ATLAS_WIDTH (1024) <= max`. Stated as a precondition, **not** a proof that `1024 <= every` device's limit |
| `clamp_is_load_bearing.smt2` | **sat** | the **unclamped** form `tex_h = h + HEADROOM` **can** exceed `max` even under the packer bound (witness `h = max = 2048 ⇒ 2304 > 2048`) — device-abort reachable ⇒ the `.min` is load-bearing, not dead code |
| `nonvacuity_interior_sat.smt2` | **sat** | `tex_h` takes a real **interior** value (clamp a no-op, headroom genuinely allocated; witness `h = 1000, max = 16384 ⇒ tex_h = 1256`) ⇒ the model is not degenerate, the `unsat` above is not boundary-pinned |

**Prove-and-catch non-vacuity** (the `assert_proves_and_catches` discipline): the
three `unsat` bound proofs are paired with two `sat` controls —
`clamp_is_load_bearing` (removing the `.min` makes the device-abort reachable, so the
safeguard is load-bearing) and `nonvacuity_interior_sat` (the clamped height genuinely
takes a strict-interior value, so the bound proof is not passing because `tex_h` is
trivially pinned to a boundary or the model is degenerate).

## Honest scope — what this does NOT prove

- **Clamps are HEIGHT-ONLY.** The atlas **width** is the compile-time constant
  `ATLAS_WIDTH = 1024` (`:47`, used at `:408`/`:1408`); there is **no runtime clamp on
  width**. This bundle does **not** prove `1024 <= max_texture_dimension_2d` for every
  device. That fact rides on **wgpu's downlevel minimum (`>= 2048`)** and is carried
  here only as the explicit `width_within_limit` **assumption** (`assume(max >= 2048)`),
  not as a discharged property of the code. On a hypothetical device reporting
  `max_texture_dimension_2d < 1024`, the constant-width texture would itself be
  oversized — outside the proven scope.
- **Device-limit margin is an assumption, not a fact about the code.** `height_no_overflow`
  assumes `max_tex_dim <= u32::MAX − 256`. This holds for every real GPU
  (`max_texture_dimension_2d` is at most ~16384 on current backends), but a device that
  *reported* `max` within 256 of `u32::MAX` could make `h + 256` wrap. The renderer does
  not defensively reject such a (physically impossible) report; the proof states this as
  the precondition (P2), it does not eliminate it.
- **Packer bound (`atlas.height <= max_tex_dim`) is taken as given here.** It is
  established by separate code — the build-time rollback (`:431`) and the grow-guard
  (`:486`) — which this SMT bundle does **not** re-verify; it is consumed as the faithful
  precondition (P1) for `height_no_overflow`. (`height_clamp_le_limit` itself needs no
  precondition: `.min` guarantees the bound even if the add wrapped.)
- **Scoped strictly to the texture *height/headroom* dimension fed to `create_texture`
  at `:1406`.** It says nothing about the `write_texture` upload extent (`:1434`, which
  uses `atlas.height`, not `tex_h`), byte-buffer sizing, format/bpp correctness, or any
  other wgpu validation. Only: the created texture's **height** never exceeds the device
  limit, so the oversized-texture lost-device abort cannot fire along this path.

## Provenance

This is pitched as a **NEW** property (GPU device-abort / DoS-impossibility for the
height clamp). It is **not** drawn from any "Bug-3 backlog"; no such provenance exists.

## Engine-frontier note

This obligation lives in `ay`'s comfort zone: pure quantifier-free bitvector logic with
`bvadd`, `bvule`, and an `ite` (modeling `u32::min`) — **no divider / `bvurem` and no
nonlinear op**, so native **32-bit** width is fast (each query returns in well under a
second). Unlike A1's `x % n < n` (where the symbolic-divisor `bvurem` forced a narrow
20-bit model to stay off `ay`'s divider frontier), here there is no reason to narrow:
modeling `u32` at its true 32-bit width keeps the proof faithful at zero cost.
