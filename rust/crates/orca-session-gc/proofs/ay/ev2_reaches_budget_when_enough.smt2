; THEOREM (session-gc): when enough is evictable, the store reaches the budget.
; If the evictable bytes cover the overage (evictTotal >= survivorBytes - budget),
; then removing all evictable bytes leaves remaining = nonEvict <= budget — so the
; oldest-first loop, which stops as soon as remaining <= budget, is guaranteed to
; reach the cap. (survivorBytes = nonEvict + evictTotal.)
; Negation asserted; UNSAT == proved for ALL non-negative byte totals.
(set-logic QF_LIA)
(declare-const nonEvict Int)
(declare-const evictTotal Int)
(declare-const budget Int)
(assert (and (>= nonEvict 0) (>= evictTotal 0) (>= budget 0)))
(assert (>= evictTotal (- (+ nonEvict evictTotal) budget)))
(assert (> (- (+ nonEvict evictTotal) evictTotal) budget))
(check-sat)
; EXPECT: unsat
