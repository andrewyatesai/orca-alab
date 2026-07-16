; CONTROL (non-vacuity): eviction can actually bring the store under budget.
; There exists an over-budget store with a single evictable dir (bytes b) whose
; removal brings remaining to <= budget. SAT proves ev1/ev2 are not vacuous — the
; size cap genuinely evicts and reaches the target.
(set-logic QF_LIA)
(declare-const nonEvict Int)
(declare-const evictTotal Int)
(declare-const budget Int)
(declare-const b Int)
(assert (and (>= nonEvict 0) (> evictTotal 0) (>= budget 0)))
(assert (> (+ nonEvict evictTotal) budget))
(assert (and (> b 0) (<= b evictTotal)))
(assert (<= (- (+ nonEvict evictTotal) b) budget))
(check-sat)
; EXPECT: sat
