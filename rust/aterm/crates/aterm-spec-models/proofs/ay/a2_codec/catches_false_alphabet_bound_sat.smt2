; SPDX-License-Identifier: Apache-2.0
; Copyright 2026 The aterm Authors
;
; A2 PROVE-AND-CATCH control — catches a deliberately-FALSE tighter alphabet bound,
;   and serves as the non-vacuity witness that the masked index 63 is reachable.
;   Discharged by `ay`.
; Expected: sat  (the false bound is refuted: (n>>k)&0x3F can EQUAL 63, so the only
;                 correct upper bound is 64; idx < 64 is EXACT, not loose/vacuous).
;
; WHY THIS GUARDS encoder_alphabet_index_inbounds.smt2:
;   That lemma proves ((n>>k)&0x3F) < 64 (unsat negation). A bound proof is only
;   meaningful if the bound is TIGHT — if the index could never reach 63, the claim
;   "< 64" would be needlessly loose (and a stricter true bound would exist). Here
;   we show 63 IS attained, so 64 is the LEAST upper bound: alphabet[63] ('/' in the
;   standard alphabet) is genuinely emitted, and the [u8; 64] table must have all 64
;   slots. The false tighter bound  idx <= 62  (i.e. idx < 63)  is therefore CAUGHT.
;
; FAITHFUL SOURCE: base64.rs:120  out.push(alphabet[(n & 0x3F) as usize]); with the
;   low 6 bits of n all set, (n & 0x3F) = 63 — the last alphabet slot is reachable.
(set-logic QF_BV)
(declare-const n (_ BitVec 32))
(declare-const k (_ BitVec 32))
(assert (bvult k (_ bv32 32)))
(define-fun idx () (_ BitVec 32) (bvand (bvlshr n k) (_ bv63 32)))   ; (n>>k) & 0x3F
; The FALSE tighter bound is  idx <= 62  (idx < 63). We assert its negation, idx = 63;
; SAT means the false bound is violated => 63 is reachable => 64 is the least bound.
(assert (= idx (_ bv63 32)))
(check-sat)
