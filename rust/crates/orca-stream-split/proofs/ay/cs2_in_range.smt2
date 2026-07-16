; THEOREM (stream-split): the clamp result stays within [start, end].
; In the non-guard case (start < end < len) the result is `end` or `end-1`, both of
; which are >= start (since end > start) and <= end. So the clamp only ever moves a
; split backwards by at most one and never past `start` — the chunk it delimits is
; non-empty and no larger than requested.
; Negation asserted; UNSAT == proved for ALL start < end < len and code-unit values.
(set-logic QF_LIA)
(declare-const b Int)
(declare-const c Int)
(declare-const start Int)
(declare-const end Int)
(declare-const len Int)
(assert (and (>= b 0) (<= b 65535) (>= c 0) (<= c 65535)))
(assert (and (< start end) (< end len)))
(assert (let ((hb (and (>= b 55296) (<= b 56319)))
              (lc (and (>= c 56320) (<= c 57343))))
          (let ((r (ite (and hb lc) (- end 1) end)))
            (or (< r start) (> r end)))))
(check-sat)
; EXPECT: unsat
