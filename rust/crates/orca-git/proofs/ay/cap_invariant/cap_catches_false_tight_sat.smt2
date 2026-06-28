; SPDX-License-Identifier: Apache-2.0
; Copyright 2026 Andrew Yates
;
; cap_invariant PROVE-AND-CATCH control — catches the FALSE tighter bound limit+1.
; Expected: sat  (the buffer can exceed limit+1, reaching limit+2, so a claimed
;   "buffer <= limit+1" bound is FALSE — a boundary MM line overshoots a too-tight
;   limit+1 by reaching limit+2). This makes cap_buffer_le_limit_plus_2 non-vacuous:
;   limit+2 is the LEAST upper bound, not loose.
(set-logic QF_BV)
(declare-const c (_ BitVec 32))
(declare-const k (_ BitVec 32))
(declare-const limit (_ BitVec 32))
(assert (bvule c limit))                                         ; P1
(assert (bvule k (_ bv2 32)))                                    ; <= 2 pushes / line
(assert (bvule limit (bvsub (bvnot (_ bv0 32)) (_ bv2 32))))
(define-fun buffered () (_ BitVec 32) (bvadd c k))
(assert (bvugt buffered (bvadd limit (_ bv1 32))))   ; buffer > limit+1 : reachable => caught
(check-sat)
