; CONTROL (non-vacuity): the reassert-pause path is REACHABLE, not dead code.
; If this were unsat, t1/t2 would be vacuously true over an empty domain. A
; witness (still flooding AND the failsafe interval elapsed) must exist.
; EXPECT: sat.
(set-logic QF_LIA)
(declare-const pending Int)
(declare-const now Int)
(declare-const paused_at Int)
(assert (>= pending 0))
(assert (>= paused_at 0))
(assert (>= now 0))
; a real reassert: pending > HIGH and elapsed >= REASSERT
(assert (> pending 262144))
(assert (>= (ite (>= now paused_at) (- now paused_at) 0) 5000))
(check-sat)
; EXPECT: sat
