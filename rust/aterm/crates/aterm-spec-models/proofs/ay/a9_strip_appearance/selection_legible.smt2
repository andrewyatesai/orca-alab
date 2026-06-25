; SPDX-License-Identifier: Apache-2.0
; Copyright 2026 Andrew Yates
;
; a9 — on EVERY builtin the default foreground stays legible OVER the selection
;   surface: WCAG contrast(fg, selection) >= 3.0:1 (aterm paints selection as a
;   BACKGROUND only, so selected text keeps its fg). Proved via SOUND rational
;   luminance bounds (monotone sRGB transfer, outward-rounded) so the obligation
;   is linear/division-free. Guards the light-theme dark-fg->light-surface
;   selection substitutions. Expected: unsat.
; FAITHFUL SOURCE: aterm-types/src/lib.rs contrast(); scheme.rs selection values;
;   the selection_is_legible_over_foreground test (exact f32 cross-check).
;
(set-logic QF_LIA)
; Luminances scaled to integers over 1e12 (exact on the proof's 12-digit grid).
; llo = SOUND lower bound on the LIGHTER of {fg,selection}; dhi = SOUND upper
; bound on the DARKER. WCAG: contrast = (Llight+0.05)/(Ldark+0.05); >= 3.0
; <=> Llight >= 3*Ldark + 0.10. Using llo<=Llight and dhi>=Ldark, llo >= 3*dhi
; + 0.10 SOUNDLY implies contrast >= 3.0. (0.10*1e12 = 100000000000.)
(declare-const llo Int)(declare-const dhi Int)
(assert (or
  (and (= llo 630757136346) (= dhi 73600993776))
  (and (= llo 935020667973) (= dhi 64736052214))
  (and (= llo 727246741250) (= dhi 92326841280))
  (and (= llo 600400993409) (= dhi 35952190527))
  (and (= llo 676039241000) (= dhi 107267300347))
  (and (= llo 715395559824) (= dhi 111191661743))
  (and (= llo 282071120058) (= dhi 30768519744))
  (and (= llo 442603043753) (= dhi 39238116162))
  (and (= llo 806944646107) (= dhi 185713128362))
  (and (= llo 431975047304) (= dhi 40553472842))
  (and (= llo 435390295658) (= dhi 81483849152))
  (and (= llo 721032535564) (= dhi 16465710544))))
; Negation: a builtin whose sound bound fails the >=3.0 contrast certificate.
(assert (< llo (+ (* 3 dhi) 100000000000)))
(check-sat)
