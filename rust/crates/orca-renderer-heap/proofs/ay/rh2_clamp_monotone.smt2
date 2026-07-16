; THEOREM (renderer-heap): more RAM never lowers the ceiling.
; For targets t1 <= t2, clamp(t1) <= clamp(t2). Since the target
; floor(totalGiB * 0.4) * 1024 is itself non-decreasing in total RAM (the elementary
; float fact, checked over the RAM tiers by the Rust tests), composing it with this
; order-preserving clamp gives "ceiling is monotone in RAM".
; Negation asserted; UNSAT == proved for ALL 0 <= t1 <= t2.
(set-logic QF_LIA)
(declare-const t1 Int)
(declare-const t2 Int)
(assert (>= t1 0))
(assert (>= t2 t1))
(assert (let ((mx1 (ite (>= t1 3072) t1 3072))
              (mx2 (ite (>= t2 3072) t2 3072)))
          (let ((c1 (ite (<= mx1 4096) mx1 4096))
                (c2 (ite (<= mx2 4096) mx2 4096)))
            (< c2 c1))))
(check-sat)
; EXPECT: unsat
