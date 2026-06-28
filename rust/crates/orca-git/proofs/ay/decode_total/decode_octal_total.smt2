; SPDX-License-Identifier: Apache-2.0
; Copyright 2026 Andrew Yates
;
; decode_total — the C-quote octal escape decode is TOTAL (char::from_u32 is always
;   Some; the decoder never panics and never silently drops an octal escape). By `ay`.
; Expected: unsat  (the negation is unsatisfiable => every octal escape value is a
;                   valid Unicode scalar, for ALL 1-3 digit octal inputs).
;
; FAITHFUL SOURCE (crates/orca-core/src/git_cquoted_path.rs:41-53, octal arm):
;     c if ('0'..='7').contains(&c) => {            // up to 3 octal digits
;         ... u32::from_str_radix(&octal, 8).ok().and_then(char::from_u32) ...
;     }
;   Each digit di is in 0..=7, and the value is v = ((d2*8 + d1)*8 + d0) (most-
;   significant digit first), so v in 0..=511. Since 511 < 0xD800 (the low surrogate
;   floor) <= 0x10FFFF, char::from_u32(v) is ALWAYS Some. This pins char PER-VALUE
;   (0..=511), NOT per-byte: it refutes any `octal as u8` per-byte truncation that
;   would corrupt \400..\777.
;
; THEOREM:  for all d0,d1,d2 in 0..=7,  v <= 511  AND  v < 0xD800
;           (so v is a valid scalar and from_u32(v) is Some).
(set-logic QF_BV)
(declare-const d2 (_ BitVec 32))    ; first / most-significant octal digit
(declare-const d1 (_ BitVec 32))
(declare-const d0 (_ BitVec 32))
(assert (bvule d2 (_ bv7 32)))
(assert (bvule d1 (_ bv7 32)))
(assert (bvule d0 (_ bv7 32)))
; v = ((d2*8 + d1)*8 + d0)
(define-fun v () (_ BitVec 32)
  (bvadd (bvmul (bvadd (bvmul d2 (_ bv8 32)) d1) (_ bv8 32)) d0))
; negation: v out of the always-valid range (v > 511 OR v >= 0xD800 = 55296).
(assert (or (bvugt v (_ bv511 32)) (bvuge v (_ bv55296 32))))
(check-sat)
