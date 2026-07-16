; CONTROL (non-vacuity): the clamp actually FIRES.
; There exist code units b, c (a high surrogate followed by a low surrogate) for
; which the result is pulled back to end-1. SAT proves cs1/cs2 are not vacuously
; about a function that always returns `end` — the surrogate fix genuinely triggers.
(set-logic QF_LIA)
(declare-const b Int)
(declare-const c Int)
(declare-const end Int)
(assert (and (>= b 0) (<= b 65535) (>= c 0) (<= c 65535)))
(assert (let ((hb (and (>= b 55296) (<= b 56319)))
              (lc (and (>= c 56320) (<= c 57343))))
          (let ((r (ite (and hb lc) (- end 1) end)))
            (and hb lc (= r (- end 1))))))
(check-sat)
; EXPECT: sat
