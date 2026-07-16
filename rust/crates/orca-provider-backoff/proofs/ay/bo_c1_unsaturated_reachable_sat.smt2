; CONTROL (non-vacuity): the un-saturated branch is actually REACHABLE.
; There exists a multiplier p >= 1 for which throttle = 30000*p and stays strictly
; below the ceiling (p in [1, 29]). SAT proves the doubling schedule genuinely
; takes intermediate values — bo1/bo3 are not vacuously about a constant function.
(set-logic QF_LIA)
(declare-const p Int)
(assert (>= p 1))
(assert (let ((prod (* 30000 p)))
          (let ((thr (ite (<= prod 900000) prod 900000)))
            (and (= thr prod) (< thr 900000)))))
(check-sat)
; EXPECT: sat
