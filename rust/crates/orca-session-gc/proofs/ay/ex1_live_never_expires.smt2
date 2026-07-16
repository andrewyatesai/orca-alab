; THEOREM (session-gc): a LIVE session dir is never age-expired.
; The expire decision is: expire = ¬exempt ∧ over_retention, where
; exempt = isLive ∨ age < minDirAge. With isLive = true the dir is exempt, so it can
; never be expired no matter how old — a running session's recovery data is safe.
; Negation asserted (live yet expired); UNSAT == proved for ALL age / flags.
(set-logic QF_LIA)
(declare-const age Int)
(declare-const isEnded Bool)
(declare-const lu Bool)
(assert (let ((exempt (or true (< age 10))))
          (and (not exempt)
               (or (and isEnded (> age 100))
                   (and (not isEnded) (not lu) (> age 1000))))))
(check-sat)
; EXPECT: unsat
