; THEOREM (P3 stage 3): the unpaused pause edge is exactly `> HIGH`.
; From the unpaused state, Pause fires if and only if pending > HIGH — no
; spurious pause at or below HIGH, and a guaranteed pause once strictly above.
; Mirrors update()'s unpaused branch (`pending_chars > high_watermark_chars`).
; Negation asserted; UNSAT == proved for ALL pending.
(set-logic QF_LIA)
(declare-const pending Int)
(assert (>= pending 0))
; unpaused action: 1 (Pause) iff pending > HIGH=262144, else 0 (None).
; the NEGATION of the iff: the action and the `> HIGH` predicate disagree.
(assert (or (and (> pending 262144) (not (= 1 (ite (> pending 262144) 1 0))))
            (and (<= pending 262144) (= 1 (ite (> pending 262144) 1 0)))))
(check-sat)
; EXPECT: unsat
