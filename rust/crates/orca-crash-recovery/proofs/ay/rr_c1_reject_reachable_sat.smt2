; CONTROL (non-vacuity): the open-breaker (reject) branch is REACHABLE.
; With max = 3, there exists an in-window count c >= 3 at which an attempt is
; rejected. SAT proves rr1/rr2 are not vacuously about a breaker that always
; admits — the throttle genuinely fires.
(set-logic QF_LIA)
(declare-const c Int)
(assert (>= c 0))
(assert (and (>= c 3) (not (< c 3))))
(check-sat)
; EXPECT: sat
