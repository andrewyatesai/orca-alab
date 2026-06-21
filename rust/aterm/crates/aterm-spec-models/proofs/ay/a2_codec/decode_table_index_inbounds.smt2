; SPDX-License-Identifier: Apache-2.0
; Copyright 2026 The aterm Authors
;
; A2 — base64 decode_byte's table lookup is ALWAYS in-bounds (no panic). By `ay`.
; Expected: unsat  (the negation is unsatisfiable => the bound holds for ALL u8).
;
; FAITHFUL SOURCE (crates/aterm-codec/src/base64.rs:219-220):
;     fn decode_byte(table: &[u8; 256], byte: u8, pos: usize) -> Result<u8, DecodeError> {
;         let val = table[byte as usize];
;   `byte: u8` ranges over 0..=255; `table` is a fixed [u8; 256]. The index
;   `byte as usize` is therefore ALWAYS strictly < 256, so the bracket lookup
;   never panics. decode_byte is total (returns Ok/Err, never panics).
;
; THEOREM:  for all byte: u8,  (byte as usize)  <  256.
;   Model u8 as an 8-bit BV and zero-extend to a 32-bit "usize" index (matching
;   `byte as usize`). 8-bit values zero-extended are exactly 0..=255 < 256.
(set-logic QF_BV)
(declare-const byte (_ BitVec 8))
; index = byte as usize  (zero-extend u8 -> 32-bit; usize cast of a u8 is zero-fill)
(define-fun index () (_ BitVec 32) ((_ zero_extend 24) byte))
(assert (bvuge index (_ bv256 32)))                     ; negation: index >= 256
(check-sat)
