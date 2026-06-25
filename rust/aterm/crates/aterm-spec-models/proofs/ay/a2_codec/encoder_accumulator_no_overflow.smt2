; SPDX-License-Identifier: Apache-2.0
; Copyright 2026 Andrew Yates
;
; A2 — base64 encoder's u32 accumulator never overflows. By `ay`.
; Expected: unsat  (the negation is unsatisfiable => no overflow for ALL inputs).
;
; FAITHFUL SOURCE (crates/aterm-codec/src/base64.rs:116):
;     let n = (u32::from(c0) << 16) | (u32::from(c1) << 8) | u32::from(c2);
;   Each of c0,c1,c2 is a byte (u8 <= 255), widened to u32 via u32::from BEFORE
;   the shift, so the shifts happen in u32 space. The maximum accumulator is
;   (255<<16)|(255<<8)|255 = 0x00FF_FFFF = 16_777_215 < 2^32, so the u32 `n`
;   never overflows (the OR of disjoint byte lanes fits in 24 bits).
;
; THEOREM:  for all c0,c1,c2 with c0,c1,c2 <= 255 (modeled as 8-bit BVs widened
;   to u32 by zero-extension, exactly as `u32::from`), the value
;     n = (c0<<16) | (c1<<8) | c2
;   satisfies  n <= 0x00FF_FFFF  (equivalently the top 8 bits are zero: no carry
;   into bit 24 or above; the u32 add/OR did not wrap).
(set-logic QF_BV)
(declare-const c0 (_ BitVec 8))
(declare-const c1 (_ BitVec 8))
(declare-const c2 (_ BitVec 8))
; u32::from(cN): zero-extend the byte to 32 bits (a u8->u32 cast is zero-fill).
(define-fun w0 () (_ BitVec 32) ((_ zero_extend 24) c0))
(define-fun w1 () (_ BitVec 32) ((_ zero_extend 24) c1))
(define-fun w2 () (_ BitVec 32) ((_ zero_extend 24) c2))
(define-fun n () (_ BitVec 32)
  (bvor (bvor (bvshl w0 (_ bv16 32)) (bvshl w1 (_ bv8 32))) w2))
(assert (bvugt n (_ bv16777215 32)))                    ; negation: n > 0x00FFFFFF
(check-sat)
