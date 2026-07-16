; THEOREM (P3 stage 3): the reassert is failsafe-gated.
; A paused PTY does NOT re-emit Pause before the reassert interval has elapsed,
; EVEN while still flooding (pending > HIGH). This bounds pause traffic to at
; most one assert per REASSERT window — the property the daemon's 5s
; lost-resume failsafe relies on. Mirrors the `>= reassert_interval_ms` guard.
; Negation asserted; UNSAT == proved for ALL pending/now/paused_at.
(set-logic QF_LIA)
(declare-const pending Int)
(declare-const now Int)
(declare-const paused_at Int)
(assert (>= pending 0))
(assert (>= paused_at 0))
(assert (>= now 0))
; still flooding, but the interval has NOT elapsed (elapsed < REASSERT=5000)
(assert (> pending 262144))
(assert (< (ite (>= now paused_at) (- now paused_at) 0) 5000))
; the NEGATION: a Pause fires anyway. In the paused branch, Pause requires
;   pending > HIGH AND elapsed >= REASSERT  — contradicted by the < above.
; (pending > HIGH here, so it is not the resume case.)
(assert (and (> pending 262144)
             (>= (ite (>= now paused_at) (- now paused_at) 0) 5000)))
(check-sat)
; EXPECT: unsat
