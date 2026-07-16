; THEOREM (session-gc): with liveness UNKNOWN, a not-ended dir is never expired.
; When the daemon liveness probe fails (liveness unknown), a not-ended dir might
; belong to a live-but-unreattached session, so its retention is ∞ — it is never
; age-expired at any age. (isEnded = false, lu = true, and past the TOCTOU floor.)
; Negation asserted; UNSAT == proved for ALL age >= minDirAge.
(set-logic QF_LIA)
(declare-const age Int)
(assert (>= age 10))
(assert (let ((exempt (or false (< age 10))))
          (and (not exempt)
               (or (and false (> age 100))
                   (and (not false) (not true) (> age 1000))))))
(check-sat)
; EXPECT: unsat
