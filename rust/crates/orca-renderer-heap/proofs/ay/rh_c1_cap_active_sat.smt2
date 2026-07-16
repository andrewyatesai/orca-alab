; CONTROL (non-vacuity, upper edge): the CAP actually fires.
; There exists a target t >= 4096 (RAM >= ~10 GiB) for which ceiling = 4096 — the
; min() cap clamps. SAT proves rh1's band is tight at the top and the cap is not a
; vacuous over-approximation.
(set-logic QF_LIA)
(declare-const t Int)
(assert (>= t 0))
(assert (let ((mx (ite (>= t 3072) t 3072)))
          (let ((clamp (ite (<= mx 4096) mx 4096)))
            (and (>= t 4096) (= clamp 4096)))))
(check-sat)
; EXPECT: sat
