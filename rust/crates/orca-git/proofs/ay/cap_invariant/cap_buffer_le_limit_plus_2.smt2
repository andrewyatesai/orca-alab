; SPDX-License-Identifier: Apache-2.0
; Copyright 2026 Andrew Yates
;
; cap_invariant — the parsed-entries Vec BUFFERS at most limit+2 entries. By `ay`.
; Expected: unsat  (the negation is unsatisfiable => the bound holds under the
;                   stated precondition P1, for all inputs).
;
; FAITHFUL SOURCE (crates/orca-git/src/status_stream.rs:82-85, update):
;     if limit != 0 && self.count > limit { self.stopped = true; return true; }
;   The stop-check runs ONCE PER LINE, AFTER the line's pushes. So entering any
;   line, count <= limit (we did not stop on the previous line) — this is the
;   carried precondition P1. A type-1/2 "MM" line pushes at most 2 entries/line.
;
; THEOREM (under P1):  for c <= limit (count before the line) and k <= 2 (pushes
;   this line),  buffered = c + k <= limit + 2,  AND the add does not wrap.
; Precondition limit <= MAX-2 keeps limit+2 in range (the real cap is tiny).
(set-logic QF_BV)
(declare-const c (_ BitVec 32))        ; count BEFORE the line (P1: c <= limit)
(declare-const k (_ BitVec 32))        ; entries pushed by this line (<= 2)
(declare-const limit (_ BitVec 32))
(assert (bvule c limit))                                         ; P1 (stated precondition)
(assert (bvule k (_ bv2 32)))                                    ; <= 2 pushes / line
(assert (bvule limit (bvsub (bvnot (_ bv0 32)) (_ bv2 32))))     ; limit <= MAX-2 (no +2 wrap)
(define-fun buffered () (_ BitVec 32) (bvadd c k))
; negation: buffer exceeds limit+2, OR the add wrapped (buffered < c).
(assert (or (bvugt buffered (bvadd limit (_ bv2 32)))
            (bvult buffered c)))
(check-sat)
