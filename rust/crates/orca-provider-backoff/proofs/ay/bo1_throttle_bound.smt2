; THEOREM (backoff unit): the refetch throttle is always within [BASE, MAX].
; p abstracts 2^max(0, streak-1) (a power of two, so p >= 1); the actual streak
; values give a subset of {1,2,4,...}, so proving the bound for ALL integer p >= 1
; is strictly stronger. throttle = min(30000*p, 900000) never drops below the 30s
; base nor exceeds the 15min ceiling — a failing provider always waits a bounded,
; non-zero interval before the next active-window refetch.
; Negation asserted; UNSAT == proved for ALL p >= 1.
(set-logic QF_LIA)
(declare-const p Int)
(assert (>= p 1))
(assert (let ((prod (* 30000 p)))
          (let ((thr (ite (<= prod 900000) prod 900000)))
            (or (< thr 30000) (> thr 900000)))))
(check-sat)
; EXPECT: unsat
