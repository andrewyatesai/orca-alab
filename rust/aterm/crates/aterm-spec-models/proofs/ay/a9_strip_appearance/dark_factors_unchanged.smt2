; SPDX-License-Identifier: Apache-2.0
; Copyright 2026 The aterm Authors
;
; a9 — KEY NO-REGRESSION THEOREM. On the ENTIRE dark-classified region
;   (luma <= 150000, all r,g,b in 0..255) strip_colors resolves EXACTLY the
;   legacy factors (active_t=16, inactive_t=40). Same factors + same blend
;   closure => byte-identical output to the pre-appearance code for every dark
;   theme (existing, future, or user). Expected: unsat.
; FAITHFUL SOURCE: tab_bar.rs strip_colors
;   let (active_t, inactive_t) = if bg_is_light(bg) {(0.10,0.30)} else {(0.16,0.40)};
;
(set-logic QF_LIA)
(declare-const r Int)(declare-const g Int)(declare-const bb Int)
(assert (and (>= r 0)(<= r 255)(>= g 0)(<= g 255)(>= bb 0)(<= bb 255)))
(define-fun luma () Int (+ (* 299 r) (* 587 g) (* 114 bb)))
(define-fun islight () Bool (> luma 150000))
; strip_colors resolves (active_t, inactive_t); legacy/dark factors are (16,40).
(define-fun active_t   () Int (ite islight 10 16))
(define-fun inactive_t () Int (ite islight 30 40))
; Negation: a DARK-classified colour whose resolved factors differ from legacy.
(assert (<= luma 150000))
(assert (or (not (= active_t 16)) (not (= inactive_t 40))))
(check-sat)
