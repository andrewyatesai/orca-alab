; THEOREM (renderer-recovery): reset always re-opens the breaker (no permanent
; lockout). After reset the in-window count is 0, and for any max >= 1 the next
; attempt is allowed (0 < m). This is the liveness counterpart to the rr1/rr2
; safety bounds: the breaker throttles a loop but never wedges recovery shut.
; Negation asserted (count 0 yet not allowed); UNSAT == proved for ALL m >= 1.
(set-logic QF_LIA)
(declare-const c Int)
(declare-const m Int)
(assert (>= m 1))
(assert (= c 0))
(assert (not (< c m)))
(check-sat)
; EXPECT: unsat
