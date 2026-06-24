; SPDX-License-Identifier: Apache-2.0
; Copyright 2026 The aterm Authors
;
; a9 CONTROL — prove-AND-catch: the legibility bound is TIGHT, not vacuous. The
;   checker rejects an over-strong floor (contrast>=4.0) on the tightest builtin
;   (Solarized Light, exact 3.636), so selection_legible's >=3.0 is a real,
;   non-trivial certificate. Expected: sat.
;
(set-logic QF_LIA)
(declare-const llo Int)(declare-const dhi Int)
; Solarized Light is the TIGHTEST builtin (exact contrast 3.636).
(assert (= llo 806944646107))(assert (= dhi 185713128362))
; A deliberately FALSE floor of contrast>=4.0 (llo >= 4*dhi + 0.15*1e12) does
; NOT hold here: witnessed by its negation being satisfiable.
(assert (< llo (+ (* 4 dhi) 150000000000)))
(check-sat)
