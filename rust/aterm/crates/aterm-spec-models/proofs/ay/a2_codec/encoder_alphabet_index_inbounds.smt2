; SPDX-License-Identifier: Apache-2.0
; Copyright 2026 Andrew Yates
;
; A2 — base64 encoder's alphabet index is ALWAYS in-bounds (no panic). By `ay`.
; Expected: unsat  (the negation is unsatisfiable => the bound holds for ALL n,k).
;
; FAITHFUL SOURCE (crates/aterm-codec/src/base64.rs:116-120, 127-139):
;     let n = (u32::from(c0) << 16) | (u32::from(c1) << 8) | u32::from(c2);
;     out.push(alphabet[((n >> 18) & 0x3F) as usize]);   // and >>12, >>6, >>0
;   `alphabet` is a fixed [u8; 64]. The index `((n >> k) & 0x3F) as usize` masks
;   with 0x3F = 63, so the result is ALWAYS <= 63 < 64 for EVERY u32 n and every
;   shift k. The alphabet lookup never panics.
;
; THEOREM:  for all n: u32 and every shift k in {18,12,6,0},
;     ((n >> k) & 0x3F)  <  64.
;   We quantify over k symbolically (k < 32, the only well-defined shift range)
;   to prove the masked index is in-bounds for ANY shift, which strictly covers
;   the four concrete shifts the code uses.
(set-logic QF_BV)
(declare-const n (_ BitVec 32))
(declare-const k (_ BitVec 32))
(assert (bvult k (_ bv32 32)))                          ; any in-range shift amount
(define-fun idx () (_ BitVec 32) (bvand (bvlshr n k) (_ bv63 32)))   ; (n>>k) & 0x3F
(assert (bvuge idx (_ bv64 32)))                        ; negation: idx >= 64
(check-sat)
