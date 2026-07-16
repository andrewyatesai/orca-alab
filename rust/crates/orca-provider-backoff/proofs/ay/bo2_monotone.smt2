; THEOREM (backoff unit): the throttle is non-decreasing in the failure streak.
; p1, p2 abstract 2^max(0, streak-1) at two streaks; since that exponential is
; itself non-decreasing in the streak (checked over the domain by the Rust
; is_non_decreasing_in_streak test), a larger streak gives p1 <= p2. This
; certifies the surrounding min(30000*p, 900000) preserves that order for ALL
; p1 <= p2 (both >= 1) — a longer failure run never shortens the wait.
; Negation asserted; UNSAT == proved for ALL 1 <= p1 <= p2.
(set-logic QF_LIA)
(declare-const p1 Int)
(declare-const p2 Int)
(assert (>= p1 1))
(assert (>= p2 p1))
(assert (let ((t1 (ite (<= (* 30000 p1) 900000) (* 30000 p1) 900000))
              (t2 (ite (<= (* 30000 p2) 900000) (* 30000 p2) 900000)))
          (< t2 t1)))
(check-sat)
; EXPECT: unsat
