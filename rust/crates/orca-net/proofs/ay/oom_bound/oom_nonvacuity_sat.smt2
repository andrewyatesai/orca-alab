; SPDX-License-Identifier: Apache-2.0
; Copyright 2026 Andrew Yates
;
; oom_bound NON-VACUITY control — the `<= max` buffer bound is TIGHT. By `ay`.
; Expected: sat  (a real feed reaches exactly max: buffer = 0, segment = max, so
;   the guard passes with next = max and push leaves new_buffer = max). Without
;   this witness the bound could be vacuously loose; this proves max is genuinely
;   reachable — a single line of exactly max bytes fills the buffer to the brim
;   before its terminating newline.
(set-logic QF_BV)
(declare-const buffer (_ BitVec 64))
(declare-const segment (_ BitVec 64))
(declare-const maxb (_ BitVec 64))
(define-fun next () (_ BitVec 64) (bvadd buffer segment))
(assert (bvule next maxb))                 ; guard passed
(assert (bvuge next buffer))               ; no wrap
(assert (bvugt maxb (_ bv0 64)))           ; non-trivial cap
(define-fun new_buffer () (_ BitVec 64) (bvadd buffer segment))
(assert (= new_buffer maxb))               ; buffer reaches exactly max
(check-sat)
