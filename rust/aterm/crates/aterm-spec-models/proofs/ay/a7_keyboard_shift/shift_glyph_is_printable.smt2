; SPDX-License-Identifier: Apache-2.0
; Copyright 2026 Andrew Yates
;
; A7 — TOTALITY/WELL-FORMEDNESS: every shiftable key's shifted glyph is a single
;      PRINTABLE ASCII byte (0x20..=0x7e). Discharged by `ay`.
; Expected: unsat  (the negation — "some shiftable key shifts to a control or
;                   non-ASCII byte" — is unsatisfiable; the map stays printable).
;
; This is the keyboard analogue of A2's `encoder_output_ascii`: it certifies the
; shift map never produces a control byte (< 0x20) or a non-ASCII byte (> 0x7e),
; so `encode_character_legacy`'s single-byte `output.encode_utf8(..)` for the
; bare-Shift branch always emits one well-formed printable glyph.
;
; FAITHFUL SOURCE: crates/aterm-types/src/keyboard/encode.rs:390-420
;   (`shifted_character`); crates/aterm-types/src/keyboard/encode_legacy.rs:43
;   (`super::shifted_character(c, modifiers).unwrap_or(c)` -> single-byte output).
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

; Negation: a shiftable key whose shifted glyph is OUTSIDE printable ASCII.
; UNSAT => every shifted glyph is in 0x20..=0x7e.
(assert IsShiftable)
(assert (or (bvult ShiftSpec #x20) (bvugt ShiftSpec #x7e)))
(check-sat)
