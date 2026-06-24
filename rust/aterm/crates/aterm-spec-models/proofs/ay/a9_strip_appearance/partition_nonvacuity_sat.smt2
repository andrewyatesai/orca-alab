; SPDX-License-Identifier: Apache-2.0
; Copyright 2026 The aterm Authors
;
; a9 CONTROL — non-vacuity: a real light builtin classifies light (the light
;   region + its set encoding are non-empty, so partition_light is not vacuous).
; Expected: sat.
;
(set-logic QF_LIA)
(declare-const r Int)(declare-const g Int)(declare-const bb Int)
; The light-set encoding is genuinely satisfiable (not a contradictory disjunction).
(assert (or
  (and (= r 253) (= g 246) (= bb 227))
  (and (= r 251) (= g 241) (= bb 199))
  (and (= r 239) (= g 241) (= bb 245))
  (and (= r 255) (= g 255) (= bb 255))))
(assert (> (+ (* 299 r) (* 587 g) (* 114 bb)) 150000))
(check-sat)
