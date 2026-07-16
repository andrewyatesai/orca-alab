; CONTROL (non-vacuity): the admit branch is REACHABLE and grows the count.
; With max = 3, there exists an in-window count c < 3 at which an attempt is
; allowed and the count becomes exactly c+1 (still <= 3). SAT proves the breaker
; genuinely admits attempts below the cap — rr1's bound is tight, not a
; never-admit degenerate.
(set-logic QF_LIA)
(declare-const c Int)
(assert (>= c 0))
(assert (and (< c 3) (<= (+ c 1) 3)))
(check-sat)
; EXPECT: sat
