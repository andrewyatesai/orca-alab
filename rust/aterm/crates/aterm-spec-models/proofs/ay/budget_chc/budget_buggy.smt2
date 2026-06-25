; SPDX-License-Identifier: Apache-2.0
; Copyright 2026 Andrew Yates
;
; ============================================================================
; A8 PROVE-AND-CATCH CONTROL — evicting-push with EVICTION REMOVED (UNSAFE).
; Discharged by `ay` as a CHC problem.
; Expected verdict: unsat  (the OOM error state b > n+k is REACHABLE; ay prints a
;                           counterexample trace). This proves the A8 theorem is
;                           NON-VACUOUS: removing the real eviction safeguard
;                           genuinely makes unbounded budget growth (OOM) reachable.
; ============================================================================
;
; THE INJECTED BUG (a REAL failure mode, not a syntactic tweak):
;   The faithful system in budget_safe.smt2 keeps b bounded because (a) a push only
;   advances from a STABLE at-or-under-budget state and (b) handle_memory_pressure
;   (disk_backed_tiers.rs:69) evicts warm->cold after every push, restoring b<=n.
;
;   Here we delete BOTH safeguards:
;     * the eviction consecution clause is GONE (no warm->cold reclamation), and
;     * the push guard `b<=n` is GONE, so push fires from ANY reached b.
;   This models a scrollback that never runs handle_memory_pressure (or whose
;   eviction is a no-op): every push does `budgeted_bytes += step` (hot_tier.rs:48)
;   with nothing ever decreasing b.
;
; CONSEQUENCE: b = 0, step, 2*step, ... grows without bound. For any fixed budget n
; and max line k, after enough pushes b exceeds n + k. The safety query
; `b > n + k` is therefore REACHABLE  ==>  ay returns unsat with a counterexample.
;
; (Same init and same safety query as budget_safe.smt2; only the transition is
; weakened by removing eviction + the stability guard.)

(set-logic HORN)

(declare-fun inv (Int Int Int) Bool)

; --- Initiation: identical to the SAFE model. ---
(assert (forall ((b Int) (n Int) (k Int))
  (=> (and (= b 0) (>= n 1) (>= k 1))
      (inv b n k))))

; --- Consecution: UNGUARDED push. No `b<=n` guard => push fires from any reached
;     state. NO eviction clause exists to restore b. b grows unboundedly. ---
(assert (forall ((b Int) (n Int) (k Int) (step Int) (b2 Int))
  (=> (and (inv b n k)
           (>= step 1)         ; a line contributes at least 1 byte
           (<= step k)         ; ... and at most K_max
           (= b2 (+ b step)))
      (inv b2 n k))))

; (NO eviction consecution clause: warm->cold reclamation has been removed.)

; --- Safety: same OOM target as the SAFE model. ---
(assert (forall ((b Int) (n Int) (k Int))
  (=> (and (inv b n k) (> b (+ n k)))
      false)))

(check-sat)
