; SPDX-License-Identifier: Apache-2.0
; Copyright 2026 Andrew Yates
;
; A1 NON-VACUITY control — interior index values are genuinely reachable (SAT).
; Expected: sat  (the encoder is not degenerate; real strict-interior indices occur).
(set-logic QF_BV)
(declare-const ring_head (_ BitVec 20))
(declare-const base (_ BitVec 20))
(declare-const len (_ BitVec 20))
(define-fun idx () (_ BitVec 20) (bvurem (bvadd ring_head base) len))
(assert (= len (_ bv8 20)))
(assert (bvugt idx (_ bv0 20)))                 ; 0 < idx
(assert (bvult idx (bvsub len (_ bv1 20))))     ;     idx < len-1  (strict interior)
(check-sat)
