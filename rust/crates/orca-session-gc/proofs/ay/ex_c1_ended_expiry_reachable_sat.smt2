; CONTROL (non-vacuity): an ended dir past retention DOES expire.
; There is an age at which a non-live, non-recent, ended dir is age-expired
; (age > endedRetention = 100). SAT proves ex1..ex3 are not vacuously about a GC
; that never expires anything — the reaper genuinely fires.
(set-logic QF_LIA)
(declare-const age Int)
(assert (let ((exempt (or false (< age 10))))
          (and (not exempt)
               (or (and true (> age 100))
                   (and (not true) (not false) (> age 1000))))))
(check-sat)
; EXPECT: sat
