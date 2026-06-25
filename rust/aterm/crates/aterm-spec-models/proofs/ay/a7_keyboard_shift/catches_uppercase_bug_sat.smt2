; SPDX-License-Identifier: Apache-2.0
; Copyright 2026 Andrew Yates
;
; A7 PROVE-AND-CATCH control — catches the ORIGINAL "Shift doesn't work" bug: the
;   pre-a2742d7 legacy shift `c.to_ascii_uppercase()` DISAGREES with the shift
;   spec on at least one shiftable key. Discharged by `ay`.
; Expected: sat  (a shiftable key exists where Upper(c) != ShiftSpec(c) — the
;                 buggy uppercase-only map is provably WRONG; `ay` returns a model,
;                 a digit/symbol such as c=0x32 '2': Upper=0x32 '2', Spec=0x40 '@').
;
; WHY THIS GUARDS shift_is_effective.smt2:
;   That lemma proves Shift is effective on the FIXED map. This control proves the
;   property has TEETH against the actual historical defect: the function the code
;   used to ship (`to_ascii_uppercase`, identity on non-letters) does NOT satisfy
;   the spec, so the spec would have REJECTED it. If this ever turned `unsat` the
;   spec would have gone vacuous (it would accept the bug).
;
; This is also the exact formal shape of the symptom: there is a key that Shift
; must change (ShiftSpec(c) != c) but uppercase leaves fixed (Upper(c) == c).
;
; FAITHFUL SOURCE: the pre-fix branch in encode_legacy.rs read
;     let output = if SHIFT { c.to_ascii_uppercase() } else { c };
;   `Upper` below models `to_ascii_uppercase`: a-z -> upper, everything else
;   (every digit and symbol) unchanged.
(set-logic QF_BV)
(declare-const c (_ BitVec 8))

(define-fun IsShiftable () Bool
  (or (and (bvule #x61 c) (bvule c #x7a))
      (= c #x31)(= c #x32)(= c #x33)(= c #x34)(= c #x35)(= c #x36)(= c #x37)
      (= c #x38)(= c #x39)(= c #x30)
      (= c #x60)(= c #x2d)(= c #x3d)(= c #x5b)(= c #x5d)(= c #x5c)
      (= c #x3b)(= c #x27)(= c #x2c)(= c #x2e)(= c #x2f)))

; The BUGGY map: `to_ascii_uppercase` — uppercases letters, identity otherwise.
(define-fun Upper () (_ BitVec 8)
  (ite (and (bvule #x61 c) (bvule c #x7a)) (bvsub c #x20) c))

; The SPEC map (faithful to `shifted_character`).
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

; SAT means: a shiftable key where the buggy uppercase map disagrees with the
; spec exists => the bug is caught, the spec is non-vacuous.
(assert IsShiftable)
(assert (distinct Upper ShiftSpec))
(check-sat)
