; THEOREM (keep-tail unit): the clamp preserves order in the divide result.
; For x1 >= x2 >= 0, clamp(x1) >= clamp(x2). Since floor(2M/max(1,n)) is itself
; non-increasing in the session count n (fewer chars per session as more compete),
; composing it with this order-preserving clamp gives the keep-tail's
; monotone-in-n property (the Rust test checks that composite over n=1..200; this
; certifies the clamp leg for ALL divide results, not just the sampled ones).
; Negation asserted; UNSAT == proved for ALL x1 >= x2 >= 0.
(set-logic QF_LIA)
(declare-const x1 Int)
(declare-const x2 Int)
(assert (>= x2 0))
(assert (>= x1 x2))
(assert (let ((m1 (ite (>= x1 65536) x1 65536))
              (m2 (ite (>= x2 65536) x2 65536)))
          (let ((kt1 (ite (<= m1 524288) m1 524288))
                (kt2 (ite (<= m2 524288) m2 524288)))
            (< kt1 kt2))))
(check-sat)
; EXPECT: unsat
