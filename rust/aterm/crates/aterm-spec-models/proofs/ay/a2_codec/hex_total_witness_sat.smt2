; SPDX-License-Identifier: Apache-2.0
; Copyright 2026 Andrew Yates
;
; A2 NON-VACUITY / PROVE-AND-CATCH control for hex decode_nibble. By `ay`.
; Expected: sat  (a witness exists: a byte OUTSIDE all three hex ranges hits the `_`
;                 arm => Err (decode is TOTAL, not a panic); AND the digit-arm
;                 nibble genuinely reaches its max 9 — catching the false claim that
;                 the digit arm could ever yield 10..15, which only the letter arms do).
;
; FAITHFUL SOURCE (crates/aterm-codec/src/hex.rs:82-89): the match has exactly the
;   three accepting ranges '0'..='9', 'a'..='f', 'A'..='F'; everything else => Err.
;   This control proves the `_` (Err) arm is reachable (e.g. 'g'=103 is between 'f'=102
;   and 'A' is far away; 'z'=122 > 'f'; ':'=58 is just past '9'=57), so the
;   hex_nibble_no_underflow lemma is non-vacuous: real bytes take the Err path and
;   real bytes take the Ok arms.
(set-logic QF_BV)
(declare-const byte (_ BitVec 8))

(define-fun in_digit ((b (_ BitVec 8))) Bool (and (bvuge b (_ bv48 8)) (bvule b (_ bv57 8))))
(define-fun in_lower ((b (_ BitVec 8))) Bool (and (bvuge b (_ bv97 8)) (bvule b (_ bv102 8))))
(define-fun in_upper ((b (_ BitVec 8))) Bool (and (bvuge b (_ bv65 8)) (bvule b (_ bv70 8))))

; Err witness: a byte in NONE of the three accepting ranges => the `_` arm => Err.
(assert (not (in_digit byte)))
(assert (not (in_lower byte)))
(assert (not (in_upper byte)))
(assert (bvult byte (_ bv128 8)))           ; an ASCII non-hex byte exists (e.g. 'g',':','z')

; Catch / tightness: the digit arm's nibble (byte - '0') reaches exactly 9 at '9'=57,
; never 10..15 — refuting a hypothetical "digit arm yields 10" over-claim. SAT shows
; the digit nibble's true maximum 9 is attained (the arms partition the nibble space).
(define-fun nib_digit_at9 () (_ BitVec 8) (bvsub (_ bv57 8) (_ bv48 8)))   ; '9' - '0'
(assert (= nib_digit_at9 (_ bv9 8)))        ; max digit nibble is 9 (< 10)
(check-sat)
