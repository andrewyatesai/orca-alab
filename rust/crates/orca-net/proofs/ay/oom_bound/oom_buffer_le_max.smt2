; SPDX-License-Identifier: Apache-2.0
; Copyright 2026 Andrew Yates
;
; oom_bound — the NDJSON splitter's retained buffer stays <= max_line_bytes. By `ay`.
; Expected: unsat  (the negation is unsatisfiable => the bound holds for ALL inputs
;                   that reach the push, i.e. whenever the guard passed and the add
;                   did not wrap).
;
; FAITHFUL SOURCE (crates/orca-net/src/ndjson.rs:84-95, feed):
;     let next_line_bytes = self.buffer.len() + segment.len();   ; = next
;     if next_line_bytes > self.max_line_bytes { ...drop/clear... continue/return }
;     self.buffer.push_str(segment);        ; only reached when next <= max
;   push_str is the ONLY place the buffer grows. It runs only on the guard's
;   false branch, so new_buffer = buffer + segment = next <= max. Every other
;   arm (oversized, discarding, newline take) clears the buffer to 0 <= max.
;
; THEOREM:  when the guard passed (next <= max) and the add did not wrap
;   (next >= buffer), the post-push buffer = buffer + segment <= max.
; Buffer/segment lengths are usize = 64-bit on every desktop target the daemon
; runs on, so 64-bit QF_BV is width-faithful. The no-wrap precondition is
; discharged independently by oom_no_wrap.smt2.
(set-logic QF_BV)
(declare-const buffer (_ BitVec 64))    ; buffer.len() before the push
(declare-const segment (_ BitVec 64))   ; segment.len() (UTF-8 bytes)
(declare-const maxb (_ BitVec 64))      ; max_line_bytes
(define-fun next () (_ BitVec 64) (bvadd buffer segment))
(assert (bvule next maxb))              ; guard PASSED (line 85 false) => push runs
(assert (bvuge next buffer))            ; add did not wrap (see oom_no_wrap)
(define-fun new_buffer () (_ BitVec 64) (bvadd buffer segment))
(assert (bvugt new_buffer maxb))        ; negation: buffer exceeds max after push
(check-sat)
