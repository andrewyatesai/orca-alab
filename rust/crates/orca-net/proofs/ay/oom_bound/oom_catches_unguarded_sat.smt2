; SPDX-License-Identifier: Apache-2.0
; Copyright 2026 Andrew Yates
;
; oom_bound PROVE-AND-CATCH control — the guard is load-bearing. By `ay`.
; Expected: sat  (WITHOUT the `next > max` guard, a push can drive the buffer past
;   max: buffer = 0, segment = max + 1 gives new_buffer = max + 1 > max, and this
;   is a genuine overshoot — the add does not wrap). This refutes the claim that
;   push_str is safe unconditionally; it is safe ONLY because line 85 gates it.
;   Pairs with oom_buffer_le_max (unsat) per the prove-and-catch contract.
(set-logic QF_BV)
(declare-const buffer (_ BitVec 64))
(declare-const segment (_ BitVec 64))
(declare-const maxb (_ BitVec 64))
(define-fun new_buffer () (_ BitVec 64) (bvadd buffer segment))
(assert (bvuge new_buffer buffer))         ; no wrap — a real overshoot, not an artifact
(assert (bvult maxb (bvnot (_ bv0 64))))   ; max < u64::MAX so max+1 exists
(assert (bvugt new_buffer maxb))           ; exceeds max — reachable with NO guard
(check-sat)
