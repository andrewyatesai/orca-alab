; SPDX-License-Identifier: Apache-2.0
; Copyright 2026 The aterm Authors
;
; a9 CONTROL — prove-AND-catch: the partition margin is REAL. Nord (the brightest
;   dark builtin) sits at luma1000=51574, so any threshold <= 51574 would
;   misclassify a shipping dark theme. sat here proves the proof is sensitive to
;   a downward threshold regression. Expected: sat.
;
(set-logic QF_LIA)
(declare-const r Int)(declare-const g Int)(declare-const bb Int)
; Nord bg #2e3440 = (46,52,64); its luma1000 = 51574 is the MAX over dark.
(assert (and (= r 46) (= g 52) (= bb 64)))
; A regression that lowered the threshold to 51000 (into the dark band) would
; reclassify Nord as light and change its chrome. Witnessed: luma1000 > 51000.
(assert (> (+ (* 299 r) (* 587 g) (* 114 bb)) 51000))
(check-sat)
