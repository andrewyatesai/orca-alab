; THEOREM (gpu-fallback): a crash outside the post-launch window is a no-op.
; A crash with ms_since_launch < 0 or > window_ms makes `active` false, so it
; never counts and never engages fallback — a one-off GPU hiccup hours into a
; session is ignored, only a launch-time burst matters.
; Negation asserted (out of window yet should-fire or count changed); UNSAT.
(set-logic QF_LIA)
(declare-const m Int)
(declare-const w Int)
(declare-const k Int)
(declare-const t Int)
(declare-const e Int)
(assert (and (>= t 1) (>= k 0) (>= w 0) (or (= e 0) (= e 1))))
(assert (or (< m 0) (> m w)))
(assert (let ((active (and (= e 0) (>= m 0) (<= m w))))
          (let ((should (and active (>= (+ k 1) t)))
                (kpost (ite (and (= e 0) (>= m 0) (<= m w)) (+ k 1) k)))
            (or should (not (= kpost k))))))
(check-sat)
; EXPECT: unsat
