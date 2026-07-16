; THEOREM (renderer-recovery): the breaker never records more than `max` attempts.
; Inductive safety step. c is the in-window attempt count after pruning, m is
; max_recoveries. Assuming the invariant holds before (0 <= c <= m), a register
; leaves the count at ite(c < m, c+1, c) — still <= m. Since prune only removes
; and the count starts at 0, induction gives "at most m attempts in any window".
; Negation asserted; UNSAT == proved for ALL 0 <= c <= m, m >= 1.
(set-logic QF_LIA)
(declare-const c Int)
(declare-const m Int)
(assert (>= m 1))
(assert (and (>= c 0) (<= c m)))
(assert (> (ite (< c m) (+ c 1) c) m))
(check-sat)
; EXPECT: unsat
