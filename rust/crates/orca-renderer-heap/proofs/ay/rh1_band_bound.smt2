; THEOREM (renderer-heap): the RAM-tier ceiling is always within [FLOOR, CAP].
; t abstracts the target floor(totalGiB * 0.4) * 1024 (a non-negative whole number);
; proving the clamp bound for ALL integer t >= 0 is strictly stronger than for the
; reachable targets. ceiling = min(4096, max(3072, t)) never drops below the 3 GB
; floor nor exceeds V8's ~4 GB pointer-compression cage.
; Negation asserted; UNSAT == proved for ALL t >= 0.
(set-logic QF_LIA)
(declare-const t Int)
(assert (>= t 0))
(assert (let ((mx (ite (>= t 3072) t 3072)))
          (let ((clamp (ite (<= mx 4096) mx 4096)))
            (or (< clamp 3072) (> clamp 4096)))))
(check-sat)
; EXPECT: unsat
