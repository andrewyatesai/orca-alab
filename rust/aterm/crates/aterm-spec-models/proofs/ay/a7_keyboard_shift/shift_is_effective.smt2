; SPDX-License-Identifier: Apache-2.0
; Copyright 2026 The aterm Authors
;
; A7 — LOAD-BEARING LEMMA: holding Shift CHANGES every shiftable key. For every
;      key `c` that has a distinct shifted glyph, the shift map does not return
;      the unshifted byte:  ShiftSpec(c) != c.  Discharged by `ay`.
; Expected: unsat  (the negation — "some shiftable key is left UNCHANGED by Shift"
;                   — is unsatisfiable; so Shift is effective on the whole row).
;
; THIS IS THE PROPERTY THE "Shift doesn't work" BUG VIOLATED. Pre-a2742d7 the
; legacy encoder shifted with `c.to_ascii_uppercase()`, the identity on every
; non-letter, so Shift+2 returned '2' (== the input) — a direct violation of
; `ShiftSpec(c) != c`. Note this lemma needs NO knowledge of the exact glyph: it
; forbids the bug class regardless of which symbol each key maps to.
;
; FAITHFUL SOURCE (crates/aterm-types/src/keyboard/encode.rs:390-420
;   `shifted_character`, the single shift map both the legacy and Kitty paths use
;   after a2742d7; encode_legacy.rs routes the bare-Shift branch through it):
;     'a'..='z' => to_ascii_uppercase   (c - 0x20)
;     '1'=>'!' '2'=>'@' '3'=>'#' '4'=>'$' '5'=>'%' '6'=>'^' '7'=>'&' '8'=>'*'
;     '9'=>'(' '0'=>')' '`'=>'~' '-'=>'_' '='=>'+' '['=>'{' ']'=>'}' '\'=>'|'
;     ';'=>':' '\''=>'"' ','=>'<' '.'=>'>' '/'=>'?'
(set-logic QF_BV)

; `c` ranges over all bytes; we constrain it to the SHIFTABLE keys below.
(declare-const c (_ BitVec 8))

; The shiftable domain: the lower-case letter range OR a key in the symbol row.
(define-fun IsShiftable () Bool
  (or (and (bvule #x61 c) (bvule c #x7a))                                  ; 'a'..'z'
      (= c #x31)(= c #x32)(= c #x33)(= c #x34)(= c #x35)(= c #x36)(= c #x37); 1-7
      (= c #x38)(= c #x39)(= c #x30)                                       ; 8 9 0
      (= c #x60)(= c #x2d)(= c #x3d)(= c #x5b)(= c #x5d)(= c #x5c)         ; ` - = [ ] backslash
      (= c #x3b)(= c #x27)(= c #x2c)(= c #x2e)(= c #x2f)))                 ; ; ' , . /

; ShiftSpec: the US-QWERTY shifted glyph. Letters via (c - 0x20); symbols via an
; ite-chain; identity default (non-shiftable keys, excluded by IsShiftable here).
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

; Negation of the theorem: a shiftable key that Shift leaves UNCHANGED.
; UNSAT => no such key => Shift is effective on every shiftable key.
(assert IsShiftable)
(assert (= ShiftSpec c))
(check-sat)
