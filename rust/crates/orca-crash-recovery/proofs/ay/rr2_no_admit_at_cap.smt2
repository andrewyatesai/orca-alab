; THEOREM (renderer-recovery): at/above the cap, the attempt is rejected and the
; state is unchanged. allowed = (c < m); post-count = ite(c < m, c+1, c). When
; c >= m the breaker must return allowed=false AND leave the count at c (a rejected
; attempt is never recorded).
; Negation asserted (c >= m yet admitted or count changed); UNSAT == proved.
(set-logic QF_LIA)
(declare-const c Int)
(declare-const m Int)
(assert (>= m 1))
(assert (>= c 0))
(assert (and (>= c m)
             (or (< c m)
                 (not (= (ite (< c m) (+ c 1) c) c)))))
(check-sat)
; EXPECT: unsat
