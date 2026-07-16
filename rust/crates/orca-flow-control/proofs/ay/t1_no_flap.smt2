; THEOREM (P3 stage 3): anti-flap hysteresis.
; A paused PTY whose pending sits in the band [LOW, HIGH] emits NO action —
; neither resume nor a reassert-pause. This is what stops the drain from
; flapping pause/resume once per flush slice. Mirrors update()'s paused branch
; in orca-flow-control/src/lib.rs (and pty-producer-flow-control.ts).
; Negation asserted; UNSAT == proved for ALL pending/now/paused_at.
(set-logic QF_LIA)
(declare-const pending Int)
(declare-const now Int)
(declare-const paused_at Int)
; realistic input domain (chars and clocks are non-negative)
(assert (>= pending 0))
(assert (>= paused_at 0))
(assert (>= now 0))
; in the hysteresis band: LOW <= pending <= HIGH  (LOW=32768, HIGH=262144)
(assert (>= pending 32768))
(assert (<= pending 262144))
; the NEGATION: some action fires anyway. action != None iff
;   pending < LOW            (resume), or
;   pending > HIGH AND elapsed >= REASSERT   (reassert-pause)
; elapsed models the Rust saturating_sub(now, paused_at); REASSERT=5000.
(assert (or (< pending 32768)
            (and (> pending 262144)
                 (>= (ite (>= now paused_at) (- now paused_at) 0) 5000))))
(check-sat)
; EXPECT: unsat
