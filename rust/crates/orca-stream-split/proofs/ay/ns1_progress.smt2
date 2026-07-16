; THEOREM (stream-split): next_safe_split_index always makes forward progress.
; When there is data left to split (start < len), the result is strictly greater
; than start — so the chunking loop that calls it can never stall, even when a
; single astral code point exceeds the byte budget. next = min(len, start+1) = start+1
; here, and the result is next or next+1, both > start.
; Negation asserted; UNSAT == proved for ALL start < len and code-unit values.
(set-logic QF_LIA)
(declare-const start Int)
(declare-const len Int)
(declare-const c0 Int)
(declare-const c1 Int)
(assert (and (>= c0 0) (<= c0 65535) (>= c1 0) (<= c1 65535)))
(assert (and (>= start 0) (< start len)))
(assert (let ((next (ite (<= (+ start 1) len) (+ start 1) len)))
          (let ((cond (and (< next len)
                           (>= c0 55296) (<= c0 56319)
                           (>= c1 56320) (<= c1 57343))))
            (let ((result (ite cond (+ next 1) next)))
              (<= result start)))))
(check-sat)
; EXPECT: unsat
