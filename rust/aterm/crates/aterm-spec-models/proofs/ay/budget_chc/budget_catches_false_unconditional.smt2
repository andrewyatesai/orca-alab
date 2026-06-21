; SPDX-License-Identifier: Apache-2.0
; Copyright 2026 The aterm Authors
;
; ============================================================================
; A8 NON-VACUITY CONTROL — the checker CATCHES the FALSE unconditional bound.
; Discharged by `ay` as a CHC problem.
; Expected verdict: unsat  (the FALSE claim `b <= n` is violated => its error
;                           state `b > n` is REACHABLE; ay prints a counterexample).
; ============================================================================
;
; WHY THIS CONTROL EXISTS (the doc's honesty discipline):
;   The tempting BUT FALSE theorem is the *unconditional* `budgeted_bytes <= memory_budget`.
;   It is false: eviction (handle_memory_pressure, disk_backed_tiers.rs:69) runs AFTER a
;   push, so a single push transiently overshoots the budget by up to k bytes before
;   eviction restores it (hot_tier.rs:48 does `budgeted_bytes += step` unconditionally).
;
;   This file takes the EXACT SAME faithful SAFE transition system as budget_safe.smt2
;   (guarded push + eviction) but asserts the OVER-STRONG safety query `b > n => false`
;   (i.e. claims b <= n always). ay returns UNSAT with a counterexample where one push
;   from the stable boundary b=n reaches b = n + step > n.
;
;   Pairing this (unsat: the false bound is caught) with budget_safe.smt2
;   (sat: the honest bound b <= n+k holds) is the prove-AND-catch non-vacuity that
;   the `assert_proves_and_catches` convention requires: our SAFE proof is not
;   passing merely because the bound is trivially loose.

(set-logic HORN)

(declare-fun inv (Int Int Int) Bool)

; Init: identical to the faithful SAFE model.
(assert (forall ((b Int) (n Int) (k Int))
  (=> (and (= b 0) (>= n 1) (>= k 1))
      (inv b n k))))

; Consecution: guarded push (FAITHFUL — same as budget_safe.smt2).
(assert (forall ((b Int) (n Int) (k Int) (step Int) (b2 Int))
  (=> (and (inv b n k) (<= b n) (>= step 1) (<= step k) (= b2 (+ b step)))
      (inv b2 n k))))

; Consecution: eviction restores stable b<=n (FAITHFUL — same as budget_safe.smt2).
(assert (forall ((b Int) (n Int) (k Int) (b3 Int))
  (=> (and (inv b n k) (>= b3 0) (<= b3 n))
      (inv b3 n k))))

; Safety: the OVER-STRONG / FALSE unconditional bound  b <= n  (no +k slack).
; This MUST be caught: a single guarded push from b=n reaches n+step > n.
(assert (forall ((b Int) (n Int) (k Int))
  (=> (and (inv b n k) (> b n))
      false)))

(check-sat)
