; SPDX-License-Identifier: Apache-2.0
; Copyright 2026 Andrew Yates
;
; A7 non-vacuity witness — the shiftable domain is NON-EMPTY and the shift map
;   genuinely moves a key. Discharged by `ay`.
; Expected: sat  (a concrete shiftable key `c` exists with ShiftSpec(c) != c, e.g.
;                 c=0x32 '2' -> 0x40 '@'). Guards shift_is_effective.smt2 from being
;                 vacuously `unsat` over an empty/degenerate domain.
;
; The `unsat` of shift_is_effective ("no shiftable key is fixed") is only
; meaningful if shiftable keys EXIST and at least one is actually moved by the
; map. This file exhibits that witness, completing the prove-and-catch pair.
(set-logic QF_BV)
(declare-const c (_ BitVec 8))

(define-fun IsShiftable () Bool
  (or (and (bvule #x61 c) (bvule c #x7a))
      (= c #x31)(= c #x32)(= c #x33)(= c #x34)(= c #x35)(= c #x36)(= c #x37)
      (= c #x38)(= c #x39)(= c #x30)
      (= c #x60)(= c #x2d)(= c #x3d)(= c #x5b)(= c #x5d)(= c #x5c)
      (= c #x3b)(= c #x27)(= c #x2c)(= c #x2e)(= c #x2f)))

(define-fun ShiftSpec () (_ BitVec 8)
  (ite (and (bvule #x61 c) (bvule c #x7a)) (bvsub c #x20)
  (ite (= c #x31) #x21  (ite (= c #x32) #x40  (ite (= c #x33) #x23
  (ite (= c #x34) #x24  (ite (= c #x35) #x25  (ite (= c #x36) #x5e
  (ite (= c #x37) #x26  (ite (= c #x38) #x2a  (ite (= c #x39) #x28
  (ite (= c #x30) #x29  (ite (= c #x60) #x7e  (ite (= c #x2d) #x5f
  (ite (= c #x3d) #x2b  (ite (= c #x5b) #x7b  (ite (= c #x5d) #x7d
  (ite (= c #x5c) #x7c  (ite (= c #x3b) #x3a  (ite (= c #x27) #x22
  (ite (= c #x2c) #x3c  (ite (= c #x2e) #x3e  (ite (= c #x2f) #x3f
  c)))))))))))))))))))))))

; SAT: at least one shiftable key is genuinely moved by the shift map.
(assert IsShiftable)
(assert (distinct ShiftSpec c))
(check-sat)
