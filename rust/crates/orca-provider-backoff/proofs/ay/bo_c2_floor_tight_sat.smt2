; CONTROL (non-vacuity / catches a false strict bound): the BASE floor is REACHED.
; There exists a multiplier p >= 1 for which throttle = 30000 exactly (p = 1, the
; first-failure wait). SAT proves bo1's band [30000, 900000] is tight at the bottom
; and that a would-be off-by-one spec of `throttle > 30000` (strict) is FALSE.
(set-logic QF_LIA)
(declare-const p Int)
(assert (>= p 1))
(assert (let ((prod (* 30000 p)))
          (let ((thr (ite (<= prod 900000) prod 900000)))
            (= thr 30000))))
(check-sat)
; EXPECT: sat
