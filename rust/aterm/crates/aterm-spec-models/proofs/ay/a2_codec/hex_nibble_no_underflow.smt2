; SPDX-License-Identifier: Apache-2.0
; Copyright 2026 The aterm Authors
;
; A2 — hex decode_nibble's subtractions never underflow and yield a value < 16. By `ay`.
; Expected: unsat  (the negation is unsatisfiable => for EVERY byte matched by an arm,
;                   the subtraction is in-range and the nibble is a valid 4-bit value).
;
; FAITHFUL SOURCE (crates/aterm-codec/src/hex.rs:82-89):
;     fn decode_nibble(byte: u8, pos: usize) -> Result<u8, DecodeError> {
;         match byte {
;             b'0'..=b'9' => Ok(byte - b'0'),          // b'0'=48, b'9'=57
;             b'a'..=b'f' => Ok(byte - b'a' + 10),     // b'a'=97, b'f'=102
;             b'A'..=b'F' => Ok(byte - b'A' + 10),     // b'A'=65, b'F'=70
;             _ => Err(DecodeError::InvalidByte(pos, byte)),
;         }
;     }
;   Each subtraction is GUARDED by the range match, so it can never underflow:
;     - digit arm:  byte >= 48 (=b'0')  => byte - 48 does not underflow; result 0..9.
;     - lower arm:  byte >= 97 (=b'a')  => byte - 97 does not underflow; (+10) gives 10..15.
;     - upper arm:  byte >= 65 (=b'A')  => byte - 65 does not underflow; (+10) gives 10..15.
;   In every arm the produced nibble is in 0..=15 (< 16). The `_` arm returns Err,
;   so decode_nibble is TOTAL (Ok/Err, never panics). Note hex's whole subtraction
;   pipeline runs in u8 (no widening); we model byte as an 8-bit BV.
;
; THEOREM:  for every byte in any of the three matched ranges, the corresponding
;   wrapping-aware subtraction does NOT underflow (byte >= subtrahend) AND the
;   resulting nibble value is < 16.
(set-logic QF_BV)
(declare-const byte (_ BitVec 8))

; --- Arm membership predicates (the match guards), inclusive ranges. ---
(define-fun in_digit () Bool (and (bvuge byte (_ bv48 8)) (bvule byte (_ bv57 8))))   ; '0'..='9'
(define-fun in_lower () Bool (and (bvuge byte (_ bv97 8)) (bvule byte (_ bv102 8))))  ; 'a'..='f'
(define-fun in_upper () Bool (and (bvuge byte (_ bv65 8)) (bvule byte (_ bv70 8))))   ; 'A'..='F'

; --- The nibble each arm computes (exactly the source expressions). ---
(define-fun nib_digit () (_ BitVec 8) (bvsub byte (_ bv48 8)))                        ; byte - b'0'
(define-fun nib_lower () (_ BitVec 8) (bvadd (bvsub byte (_ bv97 8)) (_ bv10 8)))     ; byte - b'a' + 10
(define-fun nib_upper () (_ BitVec 8) (bvadd (bvsub byte (_ bv65 8)) (_ bv10 8)))     ; byte - b'A' + 10

; Per-arm correctness: guard => (no underflow) AND (nibble < 16).
; "No underflow" for an unsigned u8 subtraction byte - s is exactly byte >= s; the
; guards force this, so we state it positively for each arm.
(define-fun arm_digit_ok () Bool
  (=> in_digit (and (bvuge byte (_ bv48 8)) (bvult nib_digit (_ bv16 8)))))
(define-fun arm_lower_ok () Bool
  (=> in_lower (and (bvuge byte (_ bv97 8)) (bvult nib_lower (_ bv16 8)))))
(define-fun arm_upper_ok () Bool
  (=> in_upper (and (bvuge byte (_ bv65 8)) (bvult nib_upper (_ bv16 8)))))

; negation: SOME arm is taken yet its subtraction underflows or the nibble is >= 16.
(assert (not (and arm_digit_ok arm_lower_ok arm_upper_ok)))
(check-sat)
