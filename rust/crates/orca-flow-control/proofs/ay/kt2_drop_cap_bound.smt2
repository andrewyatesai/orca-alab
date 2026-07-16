; THEOREM (keep-tail unit): the drop cap is always within [2*MIN, 2*MAX].
; drop_cap = 2 * keep_tail, and keep_tail in [64K, 512K] (see kt1), so the queue a
; backgrounded session may grow to before thinning is bounded in [128K, 1M] for
; every session count — no unbounded backlog, no sub-floor thrash.
; Negation asserted; UNSAT == proved for ALL x >= 0.
(set-logic QF_LIA)
(declare-const x Int)
(assert (>= x 0))
(assert (let ((m (ite (>= x 65536) x 65536)))
          (let ((kt (ite (<= m 524288) m 524288)))
            (let ((dc (* 2 kt)))
              (or (< dc 131072) (> dc 1048576))))))
(check-sat)
; EXPECT: unsat
