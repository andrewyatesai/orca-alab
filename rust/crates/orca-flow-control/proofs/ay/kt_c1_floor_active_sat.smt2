; CONTROL (non-vacuity, lower edge): the 64K floor is actually REACHED.
; There exists a divide result x < 64K for which keep_tail = 64K (the max() floor
; clamp fires). SAT proves kt1's band [64K, 512K] is TIGHT at the bottom — the
; bound is the sharpest one, not a vacuous over-approximation.
(set-logic QF_LIA)
(declare-const x Int)
(assert (>= x 0))
(assert (let ((m (ite (>= x 65536) x 65536)))
          (let ((kt (ite (<= m 524288) m 524288)))
            (and (< x 65536) (= kt 65536)))))
(check-sat)
; EXPECT: sat
