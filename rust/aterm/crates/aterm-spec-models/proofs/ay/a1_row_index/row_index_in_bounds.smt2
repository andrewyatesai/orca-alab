; SPDX-License-Identifier: Apache-2.0
; Copyright 2026 The aterm Authors
;
; A1 — row_index() fast-path returns an IN-BOUNDS index (SAFE). Discharged by `ay`.
; Expected: unsat  (the negation is unsatisfiable => the bound holds for ALL inputs
;                   in the modeled domain).
;
; FAITHFUL SOURCE (crates/aterm-grid/src/grid/state/storage.rs:219, display_offset==0):
;     return Some((self.ring_head + base) % self.rows.len());
;   rows is non-empty (storage.rs asserts !rows.is_empty()) => len != 0.
;
; THEOREM:  for all ring_head, base, and all len != 0,
;     (ring_head + base) % len  <  len
;   so the returned physical row index is always < rows.len(); the panic-index
;   sites storage.rs:400 / :417 are provably in-bounds (licenses get_unchecked).
;
; WIDTH (20-bit, len <= 1048575): the property `x % n < n` is WIDTH-UNIFORM
;   (true at every bit width); narrowing from usize's 64 bits only bounds ay's
;   divider-circuit cost (the symbolic-divisor bvurem bit-blasts into ay's
;   nonlinear-BV frontier at 64-bit — the same timeout A5's README records, fixed
;   the same way: route by reformulation/narrowing). aterm's physical rows.len()
;   = visible_rows + scrollback capacity is bounded FAR below 2^20 in any real
;   configuration, so the model covers the entire reachable domain.
(set-logic QF_BV)
(declare-const ring_head (_ BitVec 20))
(declare-const base (_ BitVec 20))
(declare-const len (_ BitVec 20))
(assert (not (= len (_ bv0 20))))                       ; rows non-empty
(define-fun idx () (_ BitVec 20) (bvurem (bvadd ring_head base) len))
(assert (bvuge idx len))                                ; negation: idx >= len
(check-sat)
