; SPDX-License-Identifier: Apache-2.0
; Copyright 2026 Andrew Yates
;
; A1 PROVE-AND-CATCH control — catches the FALSE tighter bound idx <= len-2.
; Expected: sat  (idx = len-1 is reachable, refuting idx <= len-2). This makes
; row_index_in_bounds non-vacuous: len is the LEAST upper bound, so the proved
; `idx < len` is EXACT, not loose.
(set-logic QF_BV)
(declare-const ring_head (_ BitVec 20))
(declare-const base (_ BitVec 20))
(declare-const len (_ BitVec 20))
(assert (not (= len (_ bv0 20))))
(define-fun idx () (_ BitVec 20) (bvurem (bvadd ring_head base) len))
(assert (bvuge idx (bvsub len (_ bv1 20))))     ; idx >= len-1 : reachable => false bound caught
(check-sat)
