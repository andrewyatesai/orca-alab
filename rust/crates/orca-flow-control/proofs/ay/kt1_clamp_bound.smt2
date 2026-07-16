; THEOREM (keep-tail unit): the keep-tail is always within [MIN, MAX].
; For any divide result x >= 0 (x abstracts floor(2M / max(1,n))), the clamped
; keep-tail min(512K, max(64K, x)) never falls below the 64K floor nor above the
; 512K cap — every backgrounded session keeps a bounded, non-starved tail.
; Negation asserted; UNSAT == proved for ALL x >= 0.
(set-logic QF_LIA)
(declare-const x Int)
(assert (>= x 0))
(assert (let ((m (ite (>= x 65536) x 65536)))
          (let ((kt (ite (<= m 524288) m 524288)))
            (or (< kt 65536) (> kt 524288)))))
(check-sat)
; EXPECT: unsat
