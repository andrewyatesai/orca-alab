; SPDX-License-Identifier: Apache-2.0
; Copyright 2026 Andrew Yates
;
; line_scan_bounds — SINGLE-SCAN line splitting index arithmetic is in-bounds and
;   underflow-free. By `ay`.
; Expected: unsat  (the negation is unsatisfiable => the bounds hold for all inputs
;                   with start <= nl < len).
;
; FAITHFUL SOURCE (crates/orca-git/src/status_stream.rs:76-81, update):
;     while let Some(rel) = memchr::memchr(0x0A, &text[start..]) {
;         let nl = start + rel;                                    // start <= nl < len
;         let end = if nl > start && text[nl - 1] == 0x0D { nl - 1 } else { nl };
;         self.parse_line(&text[start..end]);                      // needs start <= end <= nl
;         start = nl + 1;                                          // next start' = nl+1 <= len
;     }
;
; THEOREM:  given start <= nl < len and cr = (text[nl-1]==0x0D),
;   end = ite(cr && nl>start, nl-1, nl) satisfies
;     start <= end <= nl < len,  no usize underflow on nl-1,  and  nl+1 <= len.
(set-logic QF_BV)
(declare-const start (_ BitVec 32))
(declare-const nl (_ BitVec 32))
(declare-const len (_ BitVec 32))
(declare-const cr Bool)                                  ; text[nl-1] == 0x0D
(assert (bvule start nl))                                ; memchr returns nl at/after start
(assert (bvult nl len))                                  ; nl is a valid buffer index
(define-fun strip () Bool (and cr (bvugt nl start)))     ; the CR-strip guard
(define-fun end () (_ BitVec 32) (ite strip (bvsub nl (_ bv1 32)) nl))
(define-fun ok () Bool
  (and (bvule start end)                                 ; start <= end
       (bvule end nl)                                    ; end <= nl
       (bvult nl len)                                    ; nl < len (carried)
       (=> strip (bvuge nl (_ bv1 32)))                  ; strip => nl>=1 => nl-1 no underflow
       (bvule (bvadd nl (_ bv1 32)) len)))               ; next start' = nl+1 <= len, no wrap
(assert (not ok))                                        ; negation
(check-sat)
