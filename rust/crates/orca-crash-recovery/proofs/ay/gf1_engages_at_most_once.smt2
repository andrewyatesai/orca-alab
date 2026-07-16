; THEOREM (gpu-fallback): once engaged, every later crash is a no-op.
; Model of record_gpu_crash: active = (e=0) & (m>=0) & (m<=w); should = active &
; (k+1 >= t); post-count = ite(active, k+1, k). If already engaged (e=1) then
; active is false, so should=false AND the count is unchanged — the latch fires at
; most once, so the caller relaunches at most once.
; Negation asserted (engaged yet should-fire or count changed); UNSAT == proved.
(set-logic QF_LIA)
(declare-const m Int)
(declare-const w Int)
(declare-const k Int)
(declare-const t Int)
(declare-const e Int)
(assert (and (>= t 1) (>= k 0) (or (= e 0) (= e 1))))
(assert (= e 1))
(assert (let ((active (and (= e 0) (>= m 0) (<= m w))))
          (let ((should (and active (>= (+ k 1) t)))
                (kpost (ite (and (= e 0) (>= m 0) (<= m w)) (+ k 1) k)))
            (or should (not (= kpost k))))))
(check-sat)
; EXPECT: unsat
