; CONTROL (non-vacuity, upper edge / catches a false strict bound): the 512K cap
; is actually REACHED. There exists a divide result x >= 512K for which
; keep_tail = 512K (the min() cap clamp fires). SAT proves kt1's band is TIGHT at
; the top too — and that a would-be off-by-one spec of `keep_tail < 512K` (strict)
; is FALSE, exactly the class of bug the parity corpus + kt1 together rule out.
(set-logic QF_LIA)
(declare-const x Int)
(assert (>= x 0))
(assert (let ((m (ite (>= x 65536) x 65536)))
          (let ((kt (ite (<= m 524288) m 524288)))
            (and (>= x 524288) (= kt 524288)))))
(check-sat)
; EXPECT: sat
