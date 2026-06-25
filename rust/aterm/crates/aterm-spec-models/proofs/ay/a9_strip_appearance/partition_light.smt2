; SPDX-License-Identifier: Apache-2.0
; Copyright 2026 Andrew Yates
;
; a9 — every LIGHT builtin background classifies LIGHT under bg_is_light.
; Expected: unsat (no light builtin lands in the dark branch).
; FAITHFUL SOURCE: tab_bar.rs bg_is_light; scheme.rs (the 4 Appearance::Light schemes).
;
(set-logic QF_LIA)
(declare-const r Int)(declare-const g Int)(declare-const bb Int)
; bg ranges over the 4 light builtins; assert it classifies DARK (the bug).
(assert (or
  (and (= r 253) (= g 246) (= bb 227))
  (and (= r 251) (= g 241) (= bb 199))
  (and (= r 239) (= g 241) (= bb 245))
  (and (= r 255) (= g 255) (= bb 255))))
(assert (<= (+ (* 299 r) (* 587 g) (* 114 bb)) 150000))
(check-sat)
