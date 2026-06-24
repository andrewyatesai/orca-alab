; SPDX-License-Identifier: Apache-2.0
; Copyright 2026 The aterm Authors
;
; a9 — the active-tab card is a DISTINCT surface from the body for EVERY
;   builtin (else the focused tab vanishes into the strip). active card =
;   blend(body, fg, t); the integer test T*|fg-body| < 50 (t=T/100) is a SOUND
;   over-approximation of 'this channel is unchanged' (rounded move < 0.5): exact
;   for T=16, and for T=10 it can only OVER-predict a change at the |fg-body|=5
;   boundary (the f32 literal 0.10 != 1/10), which only makes this UNSAT harder.
;   Every builtin has min |fg-body| = 96, far from that boundary. Expected: unsat
;   (no builtin is unchanged in all three channels). DIVISION-FREE.
; FAITHFUL SOURCE: tab_bar.rs strip_colors active_bg=blend(theme.bg,theme.fg,active_t)
;   + the blend mix() round; T=16 (dark) / 10 (light) per appearance.
;
(set-logic QF_LIA)
(declare-const fr Int)(declare-const fg Int)(declare-const fb Int)
(declare-const br Int)(declare-const bg Int)(declare-const bb Int)
(declare-const T  Int)
; |d| via a case-free bound: T*|x-y| < 50  <=>  (T*(x-y) < 50 AND T*(y-x) < 50).
(define-fun same ((x Int)(y Int)) Bool
  (and (< (* T (- x y)) 50) (< (* T (- y x)) 50)))
(assert (or
  (and (= fr 208) (= fg 208) (= fb 208) (= br 17) (= bg 19) (= bb 24) (= T 16))
  (and (= fr 248) (= fg 248) (= fb 242) (= br 40) (= bg 42) (= bb 54) (= T 16))
  (and (= fr 216) (= fg 222) (= fb 233) (= br 46) (= bg 52) (= bb 64) (= T 16))
  (and (= fr 192) (= fg 202) (= fb 245) (= br 26) (= bg 27) (= bb 38) (= T 16))
  (and (= fr 205) (= fg 214) (= fb 244) (= br 30) (= bg 30) (= bb 46) (= T 16))
  (and (= fr 235) (= fg 219) (= fb 178) (= br 40) (= bg 40) (= bb 40) (= T 16))
  (and (= fr 131) (= fg 148) (= fb 150) (= br 0) (= bg 43) (= bb 54) (= T 16))
  (and (= fr 171) (= fg 178) (= fb 191) (= br 33) (= bg 37) (= bb 43) (= T 16))
  (and (= fr 101) (= fg 123) (= fb 131) (= br 253) (= bg 246) (= bb 227) (= T 10))
  (and (= fr 60) (= fg 56) (= fb 54) (= br 251) (= bg 241) (= bb 199) (= T 10))
  (and (= fr 76) (= fg 79) (= fb 105) (= br 239) (= bg 241) (= bb 245) (= T 10))
  (and (= fr 31) (= fg 35) (= fb 40) (= br 255) (= bg 255) (= bb 255) (= T 10))))
; Negation: a builtin whose active card equals the body in ALL THREE channels.
(assert (and (same br fr) (same bg fg) (same bb fb)))
(check-sat)
