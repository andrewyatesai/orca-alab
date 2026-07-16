; CONTROL (non-vacuity, lower edge): the band bottom is REACHED.
; At the 7.5 GiB gate boundary the target is exactly t = 3072, and ceiling = 3072.
; SAT proves rh1's band is tight at the bottom too — the minimum reclaimed ceiling
; is the 3 GB floor, hit right at the gate.
(set-logic QF_LIA)
(declare-const t Int)
(assert (>= t 0))
(assert (let ((mx (ite (>= t 3072) t 3072)))
          (let ((clamp (ite (<= mx 4096) mx 4096)))
            (and (= t 3072) (= clamp 3072)))))
(check-sat)
; EXPECT: sat
