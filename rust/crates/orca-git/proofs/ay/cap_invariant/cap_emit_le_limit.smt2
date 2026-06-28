; SPDX-License-Identifier: Apache-2.0
; Copyright 2026 Andrew Yates
;
; cap_invariant — the unified status parser EMITS at most `limit` entries. By `ay`.
; Expected: unsat  (the negation is unsatisfiable => the bound holds for ALL inputs
;                   with limit != 0).
;
; FAITHFUL SOURCE (crates/orca-git/src/status_stream.rs:107-108, into_result):
;     let keep = self.count.min(limit);
;     self.entries.into_iter().take(keep).collect()
;   The emitted vector length is min(count, limit); for limit != 0 it is <= limit
;   UNCONDITIONALLY (independent of any per-line invariant).
;
; THEOREM:  for all count, all limit != 0,  min(count, limit) <= limit.
; WIDTH: 32-bit QF_BV counter arithmetic (counts are usize on a 64-bit host but
;   bounded far below 2^32 in any real worktree; min(a,b) <= b is width-uniform).
(set-logic QF_BV)
(declare-const count (_ BitVec 32))
(declare-const limit (_ BitVec 32))
(assert (not (= limit (_ bv0 32))))                              ; cap enabled
(define-fun emitted () (_ BitVec 32) (ite (bvule count limit) count limit)) ; min
(assert (bvugt emitted limit))                                  ; negation: emitted > limit
(check-sat)
