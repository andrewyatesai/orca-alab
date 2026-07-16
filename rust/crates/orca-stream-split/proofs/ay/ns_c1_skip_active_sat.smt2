; CONTROL (non-vacuity): the whole-pair skip actually FIRES.
; There exist a start and code units for which next_safe_split_index returns
; start+2 (the pair-skip branch). SAT proves ns1/ns2 are not vacuously about a
; function that always advances by exactly one.
(set-logic QF_LIA)
(declare-const start Int)
(declare-const len Int)
(declare-const c0 Int)
(declare-const c1 Int)
(assert (and (>= c0 0) (<= c0 65535) (>= c1 0) (<= c1 65535)))
(assert (>= start 0))
(assert (< (+ start 1) len))
(assert (let ((next (+ start 1)))
          (let ((cond (and (< next len)
                           (>= c0 55296) (<= c0 56319)
                           (>= c1 56320) (<= c1 57343))))
            (and cond (= (ite cond (+ next 1) next) (+ start 2))))))
(check-sat)
; EXPECT: sat
