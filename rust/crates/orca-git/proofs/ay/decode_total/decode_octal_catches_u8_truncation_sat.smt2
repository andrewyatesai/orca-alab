; SPDX-License-Identifier: Apache-2.0
; Copyright 2026 Andrew Yates
;
; decode_total PROVE-AND-CATCH control — catches the latent `octal as u8` per-byte
;   truncation trap. By `ay`.
; Expected: sat  (v > 255 is reachable: e.g. \777 = 511 > 255). A per-byte `as u8`
;   path would wrap 256..511 back into 0..255, corrupting \400..\777. Because v can
;   exceed 255, modeling the escape PER-VALUE (char::from_u32 over the full 0..=511)
;   is load-bearing — the u8 trap is real and is what this catches.
(set-logic QF_BV)
(declare-const d2 (_ BitVec 32))
(declare-const d1 (_ BitVec 32))
(declare-const d0 (_ BitVec 32))
(assert (bvule d2 (_ bv7 32)))
(assert (bvule d1 (_ bv7 32)))
(assert (bvule d0 (_ bv7 32)))
(define-fun v () (_ BitVec 32)
  (bvadd (bvmul (bvadd (bvmul d2 (_ bv8 32)) d1) (_ bv8 32)) d0))
(assert (bvugt v (_ bv255 32)))        ; v > 255 reachable => the u8-truncation trap is real
(check-sat)
