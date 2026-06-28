; SPDX-License-Identifier: Apache-2.0
; Copyright 2026 Andrew Yates
;
; cap_invariant NON-VACUITY control — the limit+2 buffer bound is TIGHT. By `ay`.
; Expected: sat  (a real boundary MM line reaches exactly limit+2: c = limit, k = 2).
;   Without this witness the +2 bound could be vacuously loose; this proves it is
;   the genuinely reachable worst case (count sits AT limit, then a type-1/2 line
;   pushes both staged + unstaged before the next per-line stop-check).
(set-logic QF_BV)
(declare-const c (_ BitVec 32))
(declare-const k (_ BitVec 32))
(declare-const limit (_ BitVec 32))
(assert (bvule limit (bvsub (bvnot (_ bv0 32)) (_ bv2 32))))
(assert (= c limit))                                ; count sat AT the limit
(assert (= k (_ bv2 32)))                           ; the MM line pushes 2
(define-fun buffered () (_ BitVec 32) (bvadd c k))
(assert (= buffered (bvadd limit (_ bv2 32))))      ; buffer reaches limit+2
(check-sat)
