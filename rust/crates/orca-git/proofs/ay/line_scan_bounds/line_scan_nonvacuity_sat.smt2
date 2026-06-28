; SPDX-License-Identifier: Apache-2.0
; Copyright 2026 Andrew Yates
;
; line_scan_bounds NON-VACUITY / load-bearing control — the nl>start guard is needed.
; By `ay`.
; Expected: sat  (an empty/bare-\n record where start == nl is reachable, and there
;   end collapses to start WITHOUT computing nl-1 even when a CR is claimed). This
;   proves the `nl > start` guard is load-bearing: without it, start==nl==0 would
;   evaluate nl-1 and underflow usize.
(set-logic QF_BV)
(declare-const start (_ BitVec 32))
(declare-const nl (_ BitVec 32))
(declare-const len (_ BitVec 32))
(declare-const cr Bool)
(assert (bvule start nl))
(assert (bvult nl len))
(assert (= start nl))                                    ; empty record: \n at the record start
(assert cr)                                              ; even claiming a CR precedes nl
(define-fun strip () Bool (and cr (bvugt nl start)))     ; FALSE because nl == start
(define-fun end () (_ BitVec 32) (ite strip (bvsub nl (_ bv1 32)) nl))
(assert (= end start))                                   ; end collapses to start (no nl-1)
(check-sat)
