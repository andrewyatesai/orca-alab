; SPDX-License-Identifier: Apache-2.0
; Copyright 2026 The aterm Authors
;
; A6 PROVE-AND-CATCH control — the .min CLAMP is load-bearing (UNSAFE if removed).
; By `ay`.
; Expected: sat  (a witness exists where the UNCLAMPED height exceeds the limit =>
;                 device-abort reachable; the .min at :1405 is what prevents it).
;
; FAITHFUL SOURCE (crates/aterm-gpu/src/renderer.rs):
;   :1405  let tex_h = (atlas.height + ATLAS_GROW_HEADROOM).min(max_tex_dim);
;   Consider the COUNTERFACTUAL where the `.min(max_tex_dim)` is dropped:
;       tex_h_unclamped = atlas.height + ATLAS_GROW_HEADROOM
;   Even under the faithful packer bound (atlas.height <= max_tex_dim), the +HEADROOM
;   can push the height strictly above max_tex_dim (exactly when the atlas is packed
;   near-full: height in (max-256, max]). Creating that texture aborts the device.
;
; THEOREM (negative test): EXISTS h, max with  h <= max  (packer bound holds)
;   AND  h + HEADROOM > max   (unclamped height exceeds the device limit).
;   sat => the device-abort is REACHABLE without the clamp => the clamp is the
;   load-bearing safeguard, not dead code. (Contrast height_clamp_le_limit, which is
;   unsat: WITH the .min the same h,max can never exceed the limit.)
;
; WIDTH: native 32-bit u32. A concrete witness: h = max = 2048 => sum = 2304 > 2048.
(set-logic QF_BV)
(declare-const h (_ BitVec 32))                 ; atlas.height
(declare-const max (_ BitVec 32))               ; max_texture_dimension_2d
(define-fun HEADROOM () (_ BitVec 32) (_ bv256 32))
(define-fun U32MAX () (_ BitVec 32) (bvnot (_ bv0 32)))
(assert (bvule h max))                                   ; faithful packer bound holds
(assert (bvule max (bvsub U32MAX HEADROOM)))             ; and the add does not wrap (true value)
(define-fun tex_h_unclamped () (_ BitVec 32) (bvadd h HEADROOM))
(assert (bvugt tex_h_unclamped max))                     ; UNCLAMPED height exceeds the limit
(check-sat)
