; THEOREM (stream-split): clamp never leaves the target surrogate pair split.
; In the non-guard case the result r = ite(H(b) & L(c), end-1, end), where b =
; units[end-1], c = units[end] are free UTF-16 code units. The safety guarantee: it
; is never the case that r == end WHILE (b,c) is a high/low pair — i.e. the clamp
; always pulls a pair-splitting index back. (H = 0xd800..0xdbff = 55296..56319,
; L = 0xdc00..0xdfff = 56320..57343.)
; Negation asserted; UNSAT == proved for ALL code-unit values.
(set-logic QF_LIA)
(declare-const b Int)
(declare-const c Int)
(declare-const end Int)
(assert (and (>= b 0) (<= b 65535) (>= c 0) (<= c 65535)))
(assert (let ((hb (and (>= b 55296) (<= b 56319)))
              (lc (and (>= c 56320) (<= c 57343))))
          (let ((r (ite (and hb lc) (- end 1) end)))
            (and (= r end) hb lc))))
(check-sat)
; EXPECT: unsat
