; SPDX-License-Identifier: Apache-2.0
; Copyright 2026 Andrew Yates
;
; decode_total NON-VACUITY control — the top of the octal range is reachable. By `ay`.
; Expected: sat  (v = 511 is a real input: \777 = d2=d1=d0=7). Without it the
;   totality bound could be vacuous; this proves 511 is genuinely the max value.
(set-logic QF_BV)
(declare-const d2 (_ BitVec 32))
(declare-const d1 (_ BitVec 32))
(declare-const d0 (_ BitVec 32))
(assert (bvule d2 (_ bv7 32)))
(assert (bvule d1 (_ bv7 32)))
(assert (bvule d0 (_ bv7 32)))
(define-fun v () (_ BitVec 32)
  (bvadd (bvmul (bvadd (bvmul d2 (_ bv8 32)) d1) (_ bv8 32)) d0))
(assert (= v (_ bv511 32)))            ; \777 reachable => max octal value is real
(check-sat)
