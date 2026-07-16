; CONTROL (prove-AND-catch): the checker catches a deliberately WRONG bound.
; Suppose someone weakened the pause edge to `pending > HIGH-1` (off by one).
; That claim is FALSE — at pending == HIGH the real machine emits None, not
; Pause. ay must find that counterexample, proving the strict `> HIGH` boundary
; is load-bearing and the checker is not vacuously accepting.
; EXPECT: sat  (the counterexample pending == 262144 exists).
(set-logic QF_LIA)
(declare-const pending Int)
(assert (>= pending 0))
; the false claim's negation: pending > HIGH-1 yet the real action is not Pause.
(assert (> pending 262143))
(assert (not (= 1 (ite (> pending 262144) 1 0))))
(check-sat)
; EXPECT: sat
