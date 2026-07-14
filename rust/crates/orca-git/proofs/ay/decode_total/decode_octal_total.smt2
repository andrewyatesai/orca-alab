; SPDX-License-Identifier: Apache-2.0
; Copyright 2026 Andrew Yates
;
; decode_total — the C-quote octal escape decode is TOTAL and DROP-FREE for the
;   byte-accumulation arm: every 1-3 digit octal escape yields exactly one u8, so
;   the decoder never panics and never silently drops an escape. By `ay`.
; Expected: unsat  (the negation is unsatisfiable => for ALL 1-3 digit octal
;                   inputs the byte cast fits u8 and the parse cannot overflow).
;
; FAITHFUL SOURCE (crates/orca-core/src/git_cquoted_path.rs:41-68, octal arm):
;     let mut octal = String::new(); octal.push(chars[index]);   // 1..=3 octal digits
;     ... while octal.len() < 3 && chars[index+1].is_digit(8) { octal.push(...) }
;     if let Ok(value) = u32::from_str_radix(&octal, 8) {
;         bytes.push((value & 0xFF) as u8);                       // one byte per escape
;     }
;   Each digit di is in 0..=7, most-significant first, so v = ((d2*8 + d1)*8 + d0)
;   is in 0..=511. Since 511 < u32::MAX, `u32::from_str_radix(_,8)` NEVER overflows
;   => the `if let Ok` always takes the Ok branch => the escape is never dropped.
;   The emitted byte is b = v & 0xFF, always in 0..=255, so `(value & 0xFF) as u8`
;   is a total cast. (from_utf8_lossy over the accumulated bytes is total by the
;   Rust std guarantee: invalid runs decode to U+FFFD, never a panic.)
;
; THEOREM:  for all d0,d1,d2 in 0..=7,  v <= 511  AND  (v & 0xFF) <= 255
;           (parse cannot overflow u32, and the byte cast fits u8).
(set-logic QF_BV)
(declare-const d2 (_ BitVec 32))    ; first / most-significant octal digit
(declare-const d1 (_ BitVec 32))
(declare-const d0 (_ BitVec 32))
(assert (bvule d2 (_ bv7 32)))
(assert (bvule d1 (_ bv7 32)))
(assert (bvule d0 (_ bv7 32)))
; v = ((d2*8 + d1)*8 + d0)   ; in 0..=511
(define-fun v () (_ BitVec 32)
  (bvadd (bvmul (bvadd (bvmul d2 (_ bv8 32)) d1) (_ bv8 32)) d0))
; b = v & 0xFF  (the pushed byte)
(define-fun b () (_ BitVec 32) (bvand v (_ bv255 32)))
; negation: parse overflows the 0..=511 value range OR the byte does not fit u8.
(assert (or (bvugt v (_ bv511 32)) (bvugt b (_ bv255 32))))
(check-sat)
