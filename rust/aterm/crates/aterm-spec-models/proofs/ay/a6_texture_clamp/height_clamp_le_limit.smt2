; SPDX-License-Identifier: Apache-2.0
; Copyright 2026 Andrew Yates
;
; A6 — GPU atlas texture HEIGHT is clamped to the device limit (SAFE). By `ay`.
; Expected: unsat  (the negation is unsatisfiable => the bound holds for ALL inputs
;                   in the modeled domain).
;
; FAITHFUL SOURCE (crates/aterm-gpu/src/renderer.rs):
;   :56    const ATLAS_GROW_HEADROOM: u32 = 256;
;   :1404  let max_tex_dim = device.limits().max_texture_dimension_2d;
;   :1405  let tex_h = (atlas.height + ATLAS_GROW_HEADROOM).min(max_tex_dim);
;   :1408  size: Extent3d { width: atlas.width, height: tex_h, .. }
;   An oversized texture aborts the wgpu device (lost-device DoS). The created
;   texture's height is exactly `tex_h`.
;
; THEOREM:  for all h (= atlas.height) and all max (= max_tex_dim),
;     (h + HEADROOM).min(max)  <=  max
;   so the texture height passed to create_texture is ALWAYS <= the device limit;
;   the device-abort-on-oversized-texture is IMPOSSIBLE for the height/headroom path.
;   This relies ONLY on Rust's u32::min semantics: `a.min(b) <= b` unconditionally.
;
; WIDTH (32-bit, native u32): u32::min is modeled as unsigned BV min (ite on bvule).
;   No divider / nonlinear-BV op appears, so native 32-bit is fast — do NOT narrow.
;   NOTE: this obligation is about the .min CLAMP alone and holds even if the
;   `h + HEADROOM` add were to wrap; the separate height_no_overflow.smt2 proves the
;   add does not wrap under the faithful precondition, so the clamp sees the TRUE value.
(set-logic QF_BV)
(declare-const h (_ BitVec 32))                 ; atlas.height
(declare-const max (_ BitVec 32))               ; max_texture_dimension_2d
(define-fun HEADROOM () (_ BitVec 32) (_ bv256 32))
(define-fun sum () (_ BitVec 32) (bvadd h HEADROOM))
; u32::min(sum, max) = if sum <= max then sum else max
(define-fun tex_h () (_ BitVec 32) (ite (bvule sum max) sum max))
(assert (bvugt tex_h max))                       ; negation: clamped height EXCEEDS limit
(check-sat)
