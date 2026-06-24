; SPDX-License-Identifier: Apache-2.0
; Copyright 2026 The aterm Authors
;
; a9 — every DARK builtin background classifies DARK under bg_is_light.
; Expected: unsat (no dark builtin lands in the light branch).
; FAITHFUL SOURCE: crates/aterm-gui/src/tab_bar.rs bg_is_light
;   luma = 0.299r+0.587g+0.114b > 150.0  <=>  299r+587g+114b > 150000.
; Backgrounds: crates/aterm-types/src/scheme.rs (the 8 Appearance::Dark schemes).
;
(set-logic QF_LIA)
(declare-const r Int)(declare-const g Int)(declare-const bb Int)
; bg ranges over the 8 dark builtins; assert it classifies LIGHT (the bug).
(assert (or
  (and (= r 17) (= g 19) (= bb 24))
  (and (= r 40) (= g 42) (= bb 54))
  (and (= r 46) (= g 52) (= bb 64))
  (and (= r 26) (= g 27) (= bb 38))
  (and (= r 30) (= g 30) (= bb 46))
  (and (= r 40) (= g 40) (= bb 40))
  (and (= r 0) (= g 43) (= bb 54))
  (and (= r 33) (= g 37) (= bb 43))))
(assert (> (+ (* 299 r) (* 587 g) (* 114 bb)) 150000))
(check-sat)
