; THEOREM (backoff unit): the throttle saturates exactly at the ceiling.
; Once the multiplier p reaches ceil(MAX/BASE) = ceil(900000/30000) = 30, the
; product 30000*p >= 900000, so min(30000*p, 900000) = 900000 — the backoff pins
; to the 15min ceiling and never grows past it (the property that makes the
; saturating u64 shift in the Rust impl safe: beyond this point the exact 2^exp is
; irrelevant, only that it is >= 30).
; Negation asserted; UNSAT == proved for ALL p >= 30.
(set-logic QF_LIA)
(declare-const p Int)
(assert (>= p 30))
(assert (let ((prod (* 30000 p)))
          (let ((thr (ite (<= prod 900000) prod 900000)))
            (not (= thr 900000)))))
(check-sat)
; EXPECT: unsat
