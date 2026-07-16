; THEOREM (gpu-fallback): fallback engages only at/after the threshold.
; When record_gpu_crash reports should_engage, the post-count is >= threshold — the
; latch can never trip before `threshold` in-window crashes have accumulated.
; Negation asserted (engaged with post-count < threshold); UNSAT == proved.
(set-logic QF_LIA)
(declare-const m Int)
(declare-const w Int)
(declare-const k Int)
(declare-const t Int)
(declare-const e Int)
(assert (and (>= t 1) (>= k 0) (>= w 0) (or (= e 0) (= e 1))))
(assert (let ((active (and (= e 0) (>= m 0) (<= m w))))
          (let ((should (and active (>= (+ k 1) t)))
                (kpost (ite (and (= e 0) (>= m 0) (<= m w)) (+ k 1) k)))
            (and should (< kpost t)))))
(check-sat)
; EXPECT: unsat
