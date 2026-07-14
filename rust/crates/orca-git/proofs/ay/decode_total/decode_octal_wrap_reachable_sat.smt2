; SPDX-License-Identifier: Apache-2.0
; Copyright 2026 Andrew Yates
;
; decode_total NON-VACUITY — the syntactic overflow region \400..\777 is reachable
;   and there the `& 0xFF` wrap does real work (drops the high bit), so the mask in
;   the totality/faithfulness theorems is load-bearing, not vacuous. By `ay`.
; Expected: sat  (witness v = 511 = \777: v > 255 AND (v & 0xFF) = 255 = v - 256).
;
; A single UTF-8 byte is 0..=255 (\0..\377), so real git output never needs the
; wrap; but \400..\777 are SYNTACTICALLY valid 3-octal-digit escapes the decoder
; must still total-ize. This control proves that region is reachable and that the
; mask genuinely reduces it (v & 0xFF = v - 256 for v in 256..=511) — the guard the
; totality theorem (decode_octal_total, unsat) and the faithfulness theorem
; (decode_octal_mask_matches_uint8, unsat) would be vacuous without.
(set-logic QF_BV)
(declare-const d2 (_ BitVec 32))
(declare-const d1 (_ BitVec 32))
(declare-const d0 (_ BitVec 32))
(assert (bvule d2 (_ bv7 32)))
(assert (bvule d1 (_ bv7 32)))
(assert (bvule d0 (_ bv7 32)))
(define-fun v () (_ BitVec 32)
  (bvadd (bvmul (bvadd (bvmul d2 (_ bv8 32)) d1) (_ bv8 32)) d0))
(assert (bvugt v (_ bv255 32)))                                 ; overflow region reachable
(assert (= (bvand v (_ bv255 32)) (bvsub v (_ bv256 32))))      ; and the mask wraps by one 256-period
(check-sat)
