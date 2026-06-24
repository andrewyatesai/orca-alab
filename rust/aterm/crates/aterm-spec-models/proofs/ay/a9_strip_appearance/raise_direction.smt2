; SPDX-License-Identifier: Apache-2.0
; Copyright 2026 The aterm Authors
;
; a9 — the active card raises in the correct direction per appearance: on dark
;   themes it is brighter than the body, on light themes darker. Since the card
;   is body blended TOWARD fg (t in (0,1)) and luma is a positive-weight linear
;   combination, the card's luma lies strictly between body and fg luma (A5
;   no-overshoot, proofs/ay/blend_in_gamut_case{A,B}). So the direction equals
;   sign(luma(fg)-luma(body)), proved here over all builtins. Expected: unsat.
; FAITHFUL SOURCE: tab_bar.rs strip_colors_raise_direction_follows_appearance test.
;
(set-logic QF_LIA)
(declare-const fr Int)(declare-const fg Int)(declare-const fb Int)
(declare-const br Int)(declare-const bg Int)(declare-const bb Int)
(declare-const isl Int)
(define-fun lf () Int (+ (* 299 fr) (* 587 fg) (* 114 fb)))
(define-fun lb () Int (+ (* 299 br) (* 587 bg) (* 114 bb)))
(assert (or
  (and (= fr 208) (= fg 208) (= fb 208) (= br 17) (= bg 19) (= bb 24) (= isl 0))
  (and (= fr 248) (= fg 248) (= fb 242) (= br 40) (= bg 42) (= bb 54) (= isl 0))
  (and (= fr 216) (= fg 222) (= fb 233) (= br 46) (= bg 52) (= bb 64) (= isl 0))
  (and (= fr 192) (= fg 202) (= fb 245) (= br 26) (= bg 27) (= bb 38) (= isl 0))
  (and (= fr 205) (= fg 214) (= fb 244) (= br 30) (= bg 30) (= bb 46) (= isl 0))
  (and (= fr 235) (= fg 219) (= fb 178) (= br 40) (= bg 40) (= bb 40) (= isl 0))
  (and (= fr 131) (= fg 148) (= fb 150) (= br 0) (= bg 43) (= bb 54) (= isl 0))
  (and (= fr 171) (= fg 178) (= fb 191) (= br 33) (= bg 37) (= bb 43) (= isl 0))
  (and (= fr 101) (= fg 123) (= fb 131) (= br 253) (= bg 246) (= bb 227) (= isl 1))
  (and (= fr 60) (= fg 56) (= fb 54) (= br 251) (= bg 241) (= bb 199) (= isl 1))
  (and (= fr 76) (= fg 79) (= fb 105) (= br 239) (= bg 241) (= bb 245) (= isl 1))
  (and (= fr 31) (= fg 35) (= fb 40) (= br 255) (= bg 255) (= bb 255) (= isl 1))))
; Negation: appearance and the body->fg luma move disagree.
;   dark  (isl=0) must raise  (lf > lb);  light (isl=1) must recede (lf < lb).
(assert (or (and (= isl 0) (<= lf lb)) (and (= isl 1) (>= lf lb))))
(check-sat)
