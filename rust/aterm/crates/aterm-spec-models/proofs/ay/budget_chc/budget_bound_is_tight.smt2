; SPDX-License-Identifier: Apache-2.0
; Copyright 2026 Andrew Yates
;
; ============================================================================
; A8 TIGHTNESS CONTROL — the honest bound n+k is the LEAST upper bound (UNSAFE here).
; Discharged by `ay` as a CHC problem.
; Expected verdict: unsat  (the strictly-tighter claim `b <= n+k-1` is VIOLATED =>
;                           its error state `b > n+k-1` is REACHABLE; ay prints a
;                           counterexample). This certifies our SAFE bound is EXACT,
;                           i.e. b <= n+k is proved but no smaller constant works.
; ============================================================================
;
; This is the EXACT faithful SAFE transition system from budget_safe.smt2 (guarded
; push + eviction). The only change is the safety query is tightened by one byte:
;   `b > n+k-1 => false`  (claims b <= n+k-1).
; A push of step=k from the stable boundary b=n reaches b=n+k > n+k-1, so the
; tighter bound is reachable and ay returns unsat. Together with budget_safe.smt2
; (sat at b <= n+k), this proves n+k is the LEAST upper bound on the budgeted
; (hot+warm) byte count along the evicting-push path: honest AND tight.

(set-logic HORN)

(declare-fun inv (Int Int Int) Bool)

(assert (forall ((b Int) (n Int) (k Int))
  (=> (and (= b 0) (>= n 1) (>= k 1))
      (inv b n k))))

(assert (forall ((b Int) (n Int) (k Int) (step Int) (b2 Int))
  (=> (and (inv b n k) (<= b n) (>= step 1) (<= step k) (= b2 (+ b step)))
      (inv b2 n k))))

(assert (forall ((b Int) (n Int) (k Int) (b3 Int))
  (=> (and (inv b n k) (>= b3 0) (<= b3 n))
      (inv b3 n k))))

; Safety: the TOO-TIGHT bound  b <= n+k-1  (one byte tighter than the truth).
(assert (forall ((b Int) (n Int) (k Int))
  (=> (and (inv b n k) (> b (+ (+ n k) (- 1))))
      false)))

(check-sat)
