; SPDX-License-Identifier: Apache-2.0
; Copyright 2026 The aterm Authors
;
; ============================================================================
; A8 — aterm scrollback evicting-push BYTE-BUDGET inductive invariant (SAFE).
; Discharged by `ay` as a CHC (constrained Horn clause) problem.
; Expected verdict: sat  (an inductive invariant EXISTS  ==>  the system is SAFE;
;                         ay prints the synthesized invariant as an ay-chc-cert).
; ============================================================================
;
; FAITHFUL SOURCE SEMANTICS (verified against the live tree 2026-06-20):
;   STATE  b = budgeted_bytes = hot.budgeted_bytes() + warm.budgeted_bytes()
;            (disk_backed.rs:425, sync_accounting). COLD TIER IS EXCLUDED.
;          n = memory_budget, always >= 1 (set_memory_budget: budget.max(1),
;            disk_backed.rs:405).
;          k = K_max, the max bytes a single line contributes on PUSH; k >= 1.
;   PUSH   budgeted_bytes += line.memory_used()    (hot_tier.rs:45-50)
;            i.e. b += step, with 1 <= step <= k.
;   over_budget()  <=>  b > n                       (lib.rs:409-410)
;   EVICTION (handle_memory_pressure, disk_backed_tiers.rs:69, run after each push):
;            while over_budget() && warm.block_count() > 0 { evict_warm_to_cold(); }
;            evict_warm_to_cold moves a warm block to COLD (excluded) => b decreases.
;            The loop ends at a STABLE state with b <= n WHENEVER warm had blocks to
;            give (the only case modeled here — see scope caveat in README.md).
;
; HONEST INDUCTIVE PROPERTY (matches ay-chc bounded-increment shape
;   x' = x + K under guard x <= N  ==>  x <= N + K):
;   b advances by one push (+step, step <= k) only from an at-or-under-budget
;   STABLE state (b <= n), and eviction then restores the stable bound.
;   Therefore the OBSERVABLE PEAK byte count obeys:
;
;            b  <=  n + k        (overshoot by at most one max push before eviction)
;
;   This is the OOM-impossibility bound: the evicting-push path can never grow the
;   budgeted (hot+warm) byte count without limit. We do NOT claim the (false)
;   unconditional b <= n: a single push can transiently overshoot by up to k.
;
; ENCODING (single-variable bounded-increment; n,k are frame-constant parameters
; carried through the invariant so the theorem holds for ALL budgets n>=1 and
; ALL max line sizes k>=1, not one fixed instance):
;
;   inv(b, n, k) is the reachable set of OBSERVABLE peak byte counts.
;   * init        : b=0, n>=1, k>=1                              => inv
;   * push+evict  : inv(b,n,k) /\ stable(b<=n) /\ 1<=step<=k
;                   /\ b2 = b + step                             => inv(b2,n,k)
;                   (the peak after a push from a stable state)
;     ...then eviction returns to a stable state b<=n, re-enabling the guard.
;     Modeling the post-eviction stable value as any b3 with 0<=b3<=n keeps the
;     guard live for the next round (eviction can land anywhere at-or-under budget).
;   * safety      : inv(b,n,k) /\ b > n + k                      => false
;
; UNSAT here would mean the bound is violable; sat (with a synthesized inv such as
; b <= n+k) certifies the OOM-impossibility theorem.

(set-logic HORN)

; inv(b, n, k): b reachable observable peak; n=budget(>=1); k=K_max(>=1).
(declare-fun inv (Int Int Int) Bool)

; --- Initiation: fresh scrollback, empty budgeted byte count. ---
(assert (forall ((b Int) (n Int) (k Int))
  (=> (and (= b 0) (>= n 1) (>= k 1))
      (inv b n k))))

; --- Consecution: PUSH (+step, 1<=step<=k) from a STABLE at-or-under-budget
;     state (b<=n) yields the observable peak b2=b+step. ---
(assert (forall ((b Int) (n Int) (k Int) (step Int) (b2 Int))
  (=> (and (inv b n k)
           (<= b n)            ; guard: push advances only from a STABLE state
           (>= step 1)         ; a line contributes at least 1 byte
           (<= step k)         ; ... and at most K_max
           (= b2 (+ b step)))
      (inv b2 n k))))

; --- Consecution: EVICTION restores a STABLE post-round state b3 with b3<=n
;     (warm had blocks to evict; cold-excluded => b drops). This re-arms the
;     guard for the next push round. ---
(assert (forall ((b Int) (n Int) (k Int) (b3 Int))
  (=> (and (inv b n k)
           (>= b3 0)
           (<= b3 n))          ; eviction lands at-or-under budget
      (inv b3 n k))))

; --- Safety: the OBSERVABLE peak byte count never exceeds n + k. ---
(assert (forall ((b Int) (n Int) (k Int))
  (=> (and (inv b n k) (> b (+ n k)))
      false)))

(check-sat)
