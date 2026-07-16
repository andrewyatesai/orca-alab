; THEOREM (session-gc): a dir younger than the TOCTOU floor is never expired.
; A dir created between the liveness snapshot and the scan must not be reaped, so
; anything with age < minDirAge (10) is exempt regardless of ended/liveness state.
; Negation asserted (age < floor yet expired); UNSAT == proved for ALL flags.
(set-logic QF_LIA)
(declare-const age Int)
(declare-const isEnded Bool)
(declare-const lu Bool)
(assert (< age 10))
(assert (let ((exempt (or false (< age 10))))
          (and (not exempt)
               (or (and isEnded (> age 100))
                   (and (not isEnded) (not lu) (> age 1000))))))
(check-sat)
; EXPECT: unsat
