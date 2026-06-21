; SPDX-License-Identifier: Apache-2.0
; Copyright 2026 The aterm Authors
;
; A1 side-condition — ring_head + base does NOT wrap (SAFE). By `ay` (20-bit model).
; Expected: unsat  (no wrap under the faithful precondition).
;
; ring_head is a ring index (< rows.len()) and base a scrollback offset, both
; bounded by total_lines; their sum stays in-domain. Precondition: ring_head <=
; MAX - base. Under it bvadd cannot carry, so the fast-path index is the TRUE
; (non-wrapped) modulo — strengthening row_index_in_bounds from "in bounds" to
; "in bounds AND the intended row".
(set-logic QF_BV)
(declare-const ring_head (_ BitVec 20))
(declare-const base (_ BitVec 20))
(assert (bvule ring_head (bvsub (bvnot (_ bv0 20)) base)))   ; ring_head <= MAX - base
(assert (bvult (bvadd ring_head base) ring_head))            ; negation: add wrapped (carry out)
(check-sat)
