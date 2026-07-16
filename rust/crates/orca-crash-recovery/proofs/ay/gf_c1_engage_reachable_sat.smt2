; CONTROL (non-vacuity): the fallback latch can actually TRIP.
; With threshold = 3 and not yet engaged (e=0), there exist an in-window crash and
; a prior count k with k+1 >= 3 for which should_engage is true. SAT proves gf1/gf3
; are not vacuously about a latch that never fires.
(set-logic QF_LIA)
(declare-const m Int)
(declare-const w Int)
(declare-const k Int)
(assert (and (>= k 0) (>= w 0)))
(assert (let ((active (and (>= m 0) (<= m w))))
          (and active (>= (+ k 1) 3))))
(check-sat)
; EXPECT: sat
