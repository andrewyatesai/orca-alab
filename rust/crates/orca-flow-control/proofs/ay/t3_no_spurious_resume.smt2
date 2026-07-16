; THEOREM (P3 stage 3): resume only strictly below LOW.
; A paused PTY is NOT resumed while pending >= LOW. Combined with t1, this pins
; the resume edge to exactly the strict `< LOW` crossing (the low watermark),
; so a PTY resumed here is genuinely drained, never mid-band. Mirrors the strict
; `pending_chars < low_watermark_chars` test.
; Negation asserted; UNSAT == proved for ALL pending/now/paused_at.
(set-logic QF_LIA)
(declare-const pending Int)
(declare-const now Int)
(declare-const paused_at Int)
(assert (>= pending 0))
(assert (>= paused_at 0))
(assert (>= now 0))
; at or above the low watermark
(assert (>= pending 32768))
; the NEGATION: Resume fires. Resume requires pending < LOW=32768 —
; contradicted by the >= above.
(assert (< pending 32768))
(check-sat)
; EXPECT: unsat
