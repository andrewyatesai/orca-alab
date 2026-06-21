; SPDX-License-Identifier: Apache-2.0
; Copyright 2026 The aterm Authors
;
; A6 side-condition — atlas.height + HEADROOM does NOT wrap u32 (SAFE). By `ay`.
; Expected: unsat  (no wrap under the faithful precondition).
;
; FAITHFUL SOURCE (crates/aterm-gpu/src/renderer.rs):
;   :1405  let tex_h = (atlas.height + ATLAS_GROW_HEADROOM).min(max_tex_dim);
;   The `+ ATLAS_GROW_HEADROOM` is a plain u32 add; if it wrapped, the .min would
;   clamp a SMALL wrapped value and silently under-size (a correctness bug — though
;   NOT the device-abort one). This obligation shows the add is on the true value.
;
; FAITHFUL PRECONDITION (all three hold in the real tree):
;   (P1) atlas.height <= max_tex_dim
;        The packer bounds occupied_height to cap_h (= max_tex_dim):
;          :431  if atlas.occupied_height() > cap_h { ...rollback...; break; }
;          :486  if need_h > cap_h { return None; }   (grow guard)
;        and :1537 passes cap_h = device.limits().max_texture_dimension_2d.
;   (P2) max_tex_dim <= u32::MAX - HEADROOM
;        Real max_texture_dimension_2d is a small device limit (wgpu downlevel min
;        2048, modern GPUs <= ~16384 << u32::MAX-256). Modeled as the explicit
;        assumption max <= 0xFFFFFEFF (= u32::MAX - 256). This is the load-bearing,
;        on-the-record device fact; it is NOT a universal claim over all u32.
;
; THEOREM:  under (P1) /\ (P2),  h + HEADROOM does not carry out of 32 bits
;     i.e.  h + HEADROOM >= h   (unsigned: no wrap),  so .min sees the TRUE sum.
;
; WIDTH: native 32-bit u32; the precondition (P2) is exactly the u32 headroom margin.
(set-logic QF_BV)
(declare-const h (_ BitVec 32))                 ; atlas.height
(declare-const max (_ BitVec 32))               ; max_texture_dimension_2d
(define-fun HEADROOM () (_ BitVec 32) (_ bv256 32))
(define-fun U32MAX () (_ BitVec 32) (bvnot (_ bv0 32)))
(assert (bvule h max))                                   ; (P1) packer bound
(assert (bvule max (bvsub U32MAX HEADROOM)))             ; (P2) device limit margin
(assert (bvult (bvadd h HEADROOM) h))                    ; negation: add wrapped (carry out)
(check-sat)
