; THEOREM (stream-split): a surrogate pair beginning at start is skipped WHOLE.
; If units[start] is a high surrogate, units[start+1] its low surrogate, and there
; is room (start+1 < len), next_safe_split_index returns start+2 — both halves land
; in the same chunk, so a lone astral code point is never left straddling the split.
; Negation asserted; UNSAT == proved for ALL such start and pair values.
(set-logic QF_LIA)
(declare-const start Int)
(declare-const len Int)
(declare-const c0 Int)
(declare-const c1 Int)
(assert (and (>= c0 0) (<= c0 65535) (>= c1 0) (<= c1 65535)))
(assert (>= start 0))
(assert (< (+ start 1) len))
(assert (and (>= c0 55296) (<= c0 56319)))
(assert (and (>= c1 56320) (<= c1 57343)))
(assert (let ((next (ite (<= (+ start 1) len) (+ start 1) len)))
          (let ((cond (and (< next len)
                           (>= c0 55296) (<= c0 56319)
                           (>= c1 56320) (<= c1 57343))))
            (let ((result (ite cond (+ next 1) next)))
              (not (= result (+ start 2)))))))
(check-sat)
; EXPECT: unsat
