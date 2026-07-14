; SPDX-License-Identifier: Apache-2.0
; Copyright 2026 Andrew Yates
;
; decode_total FAITHFULNESS — the Rust `(value & 0xFF) as u8` byte reproduces the
;   TS decoder's `parseInt(octal, 8) & 0xFF` (a Uint8Array / String.fromCharCode
;   wrap) EXACTLY, so the port is byte-identical to src/shared/git-cquoted-path.ts.
;   By `ay`.
; Expected: unsat  (the negation is unsatisfiable => for ALL 1-3 digit octal
;                   inputs, masking equals reduction mod 256).
;
; The TS wrote each escape as `bytes.push(parseInt(octal, 8) & 0xFF)` into a
; Uint8Array; JS `& 0xFF` on an integer is reduction mod 256. Rust emits
; `(value & 0xFF) as u8`. This obligation pins  (v & 0xFF) == (v mod 256)  over the
; whole syntactic input range v in 0..=511, i.e. the two decoders agree on every
; producible byte (the load-bearing equivalence the byte-run rewrite relies on).
;
; THEOREM:  for all d0,d1,d2 in 0..=7,  (v & 0xFF) == (v mod 256).
(set-logic QF_BV)
(declare-const d2 (_ BitVec 32))
(declare-const d1 (_ BitVec 32))
(declare-const d0 (_ BitVec 32))
(assert (bvule d2 (_ bv7 32)))
(assert (bvule d1 (_ bv7 32)))
(assert (bvule d0 (_ bv7 32)))
(define-fun v () (_ BitVec 32)
  (bvadd (bvmul (bvadd (bvmul d2 (_ bv8 32)) d1) (_ bv8 32)) d0))
; negation: the mask disagrees with reduction mod 256 for some producible v.
(assert (not (= (bvand v (_ bv255 32)) (bvurem v (_ bv256 32)))))
(check-sat)
