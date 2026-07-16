; THEOREM (session-gc): each eviction step is non-increasing in remaining bytes.
; Evicting a dir subtracts its (non-negative) byte count, so remaining' = remaining
; - b <= remaining. Combined with ev1's floor and the loop's `stop when remaining <=
; budget` guard, this makes the loop terminate at a well-defined remaining.
; Negation asserted (a step that raised remaining); UNSAT == proved for ALL b >= 0.
(set-logic QF_LIA)
(declare-const remaining Int)
(declare-const b Int)
(assert (>= b 0))
(assert (> (- remaining b) remaining))
(check-sat)
; EXPECT: unsat
