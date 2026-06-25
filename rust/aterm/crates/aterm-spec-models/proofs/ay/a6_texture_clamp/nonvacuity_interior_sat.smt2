; SPDX-License-Identifier: Apache-2.0
; Copyright 2026 Andrew Yates
;
; A6 NON-VACUITY control — the clamped height takes a real INTERIOR value (SAT).
; By `ay`.
; Expected: sat  (the common steady-state case: the atlas is NOT near the device
;                 limit, so tex_h = atlas.height + HEADROOM strictly BELOW max — the
;                 .min is a no-op and the headroom is genuinely allocated). This
;                 shows the model is not degenerate: height_clamp_le_limit is not
;                 unsat merely because tex_h is pinned to a boundary.
;
; FAITHFUL SOURCE (crates/aterm-gpu/src/renderer.rs):
;   :1405  let tex_h = (atlas.height + ATLAS_GROW_HEADROOM).min(max_tex_dim);
;   For every real workload, max_tex_dim (>= 2048, often 16384) is far above the
;   packed height, so .min is the identity and tex_h = atlas.height + 256, an
;   interior value strictly between 0 and the device limit.
;
; THEOREM (witness): EXISTS h, max with  h <= max,  add does not wrap,  AND
;   0 < tex_h < max   where tex_h = (h+HEADROOM).min(max)  AND  tex_h = h+HEADROOM
;   (i.e. the clamp did NOT fire — true interior allocation).
;   A concrete witness: h = 1000, max = 16384 => tex_h = 1256 (interior, .min no-op).
(set-logic QF_BV)
(declare-const h (_ BitVec 32))                 ; atlas.height
(declare-const max (_ BitVec 32))               ; max_texture_dimension_2d
(define-fun HEADROOM () (_ BitVec 32) (_ bv256 32))
(define-fun U32MAX () (_ BitVec 32) (bvnot (_ bv0 32)))
(define-fun sum () (_ BitVec 32) (bvadd h HEADROOM))
(define-fun tex_h () (_ BitVec 32) (ite (bvule sum max) sum max))
(assert (bvule h max))                                   ; packer bound
(assert (bvule max (bvsub U32MAX HEADROOM)))             ; no wrap (true value)
(assert (bvugt tex_h (_ bv0 32)))                        ; 0 < tex_h
(assert (bvult tex_h max))                               ;     tex_h < max  (strict interior)
(assert (= tex_h sum))                                   ; clamp did NOT fire (headroom real)
(check-sat)
