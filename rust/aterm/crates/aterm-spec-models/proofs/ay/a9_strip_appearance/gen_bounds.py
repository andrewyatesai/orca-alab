#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 The aterm Authors
"""Generate the a9_strip_appearance SMT obligation bundle, literal-faithful.

Every constant is derived from the same builtin table as scheme.rs, so the
emitted .smt2 files cannot drift from hand-transcription. Run, then `ay solve`
each file and check the expected verdict via verify.sh.
"""
import os
from decimal import Decimal, getcontext, ROUND_FLOOR, ROUND_CEILING
getcontext().prec = 60

OUT = os.path.dirname(os.path.abspath(__file__))

# (name, fg, bg, selection, appearance)  — mirrors crates/aterm-types/src/scheme.rs
B = [
    ("Default",          0xD0D0D0, 0x111318, 0x264F78, "dark"),
    ("Dracula",          0xF8F8F2, 0x282A36, 0x44475A, "dark"),
    ("Nord",             0xD8DEE9, 0x2E3440, 0x4C566A, "dark"),
    ("Tokyo Night",      0xC0CAF5, 0x1A1B26, 0x283457, "dark"),
    ("Catppuccin Mocha", 0xCDD6F4, 0x1E1E2E, 0x585B70, "dark"),
    ("Gruvbox Dark",     0xEBDBB2, 0x282828, 0x665C54, "dark"),
    ("Solarized Dark",   0x839496, 0x002B36, 0x073642, "dark"),
    ("One Dark",         0xABB2BF, 0x21252B, 0x323844, "dark"),
    ("Solarized Light",  0x657B83, 0xFDF6E3, 0xEEE8D5, "light"),
    ("Gruvbox Light",    0x3C3836, 0xFBF1C7, 0xBDAE93, "light"),
    ("Catppuccin Latte", 0x4C4F69, 0xEFF1F5, 0xACB0BE, "light"),
    ("GitHub Light",     0x1F2328, 0xFFFFFF, 0xB6E3FF, "light"),
]
THRESH = 150000      # luma1000 threshold (0.299r+0.587g+0.114b > 150.0, *1000)

def ch(x): return ((x >> 16) & 0xff, (x >> 8) & 0xff, x & 0xff)
def luma1000(x):
    r, g, b = ch(x); return 299*r + 587*g + 114*b

# ---- sound WCAG luminance bounds (monotone sRGB transfer, outward rounding) ----
Q = Decimal(10) ** -12
SCALE = Decimal(10) ** 12
def lin(c):
    s = Decimal(c) / Decimal(255)
    if s <= Decimal("0.04045"): return s / Decimal("12.92")
    return ((s + Decimal("0.055")) / Decimal("1.055")) ** Decimal("2.4")
def lin_lo(c): return lin(c).quantize(Q, rounding=ROUND_FLOOR)
def lin_hi(c): return lin(c).quantize(Q, rounding=ROUND_CEILING)
CW = (Decimal("0.2126"), Decimal("0.7152"), Decimal("0.0722"))
def lum_lo(x):
    r, g, b = ch(x); return CW[0]*lin_lo(r) + CW[1]*lin_lo(g) + CW[2]*lin_lo(b)
def lum_hi(x):
    r, g, b = ch(x); return CW[0]*lin_hi(r) + CW[1]*lin_hi(g) + CW[2]*lin_hi(b)
def lum(x):
    r, g, b = ch(x); return CW[0]*lin(r) + CW[1]*lin(g) + CW[2]*lin(b)
# DIRECTIONAL final rounding so the scaled integer stays a SOUND outward bound:
# floor for a lower bound, ceil for an upper bound. (to_integral_value defaults to
# HALF_EVEN, which can round a bound the UNSOUND way at the 1e-12 grid.)
def inum_lo(d): return int((d * SCALE).to_integral_value(rounding=ROUND_FLOOR))
def inum_hi(d): return int((d * SCALE).to_integral_value(rounding=ROUND_CEILING))

HEADER = ("; SPDX-License-Identifier: Apache-2.0\n"
          "; Copyright 2026 The aterm Authors\n;\n")

def write(name, body):
    with open(os.path.join(OUT, name), "w") as f:
        f.write(HEADER + body)
    print("wrote", name)

# helpers to emit a disjunction over the builtin set --------------------------
def bg_eq(x):
    r, g, b = ch(x); return f"(and (= r {r}) (= g {g}) (= bb {b}))"
def tuple_eq(name, fg, bg, sel, ap):
    fr, fgc, fb = ch(fg); br, bgc, bb = ch(bg)
    T = 10 if ap == "light" else 16
    return (f"(and (= fr {fr}) (= fg {fgc}) (= fb {fb}) "
            f"(= br {br}) (= bg {bgc}) (= bb {bb}) (= T {T}))")

DARK  = [x for x in B if x[4] == "dark"]
LIGHT = [x for x in B if x[4] == "light"]

# 1) partition_dark  (UNSAT) --------------------------------------------------
body = "(set-logic QF_LIA)\n(declare-const r Int)(declare-const g Int)(declare-const bb Int)\n"
body += "; bg ranges over the 8 dark builtins; assert it classifies LIGHT (the bug).\n"
body += "(assert (or\n" + "\n".join("  " + bg_eq(x[2]) for x in DARK) + "))\n"
body += f"(assert (> (+ (* 299 r) (* 587 g) (* 114 bb)) {THRESH}))\n(check-sat)\n"
write("partition_dark.smt2",
      "; a9 — every DARK builtin background classifies DARK under bg_is_light.\n"
      "; Expected: unsat (no dark builtin lands in the light branch).\n"
      "; FAITHFUL SOURCE: crates/aterm-gui/src/tab_bar.rs bg_is_light\n"
      ";   luma = 0.299r+0.587g+0.114b > 150.0  <=>  299r+587g+114b > 150000.\n"
      "; Backgrounds: crates/aterm-types/src/scheme.rs (the 8 Appearance::Dark schemes).\n;\n" + body)

# 2) partition_light  (UNSAT) -------------------------------------------------
body = "(set-logic QF_LIA)\n(declare-const r Int)(declare-const g Int)(declare-const bb Int)\n"
body += "; bg ranges over the 4 light builtins; assert it classifies DARK (the bug).\n"
body += "(assert (or\n" + "\n".join("  " + bg_eq(x[2]) for x in LIGHT) + "))\n"
body += f"(assert (<= (+ (* 299 r) (* 587 g) (* 114 bb)) {THRESH}))\n(check-sat)\n"
write("partition_light.smt2",
      "; a9 — every LIGHT builtin background classifies LIGHT under bg_is_light.\n"
      "; Expected: unsat (no light builtin lands in the dark branch).\n"
      "; FAITHFUL SOURCE: tab_bar.rs bg_is_light; scheme.rs (the 4 Appearance::Light schemes).\n;\n" + body)

# 3) dark_factors_unchanged  (UNSAT) — whole dark region byte-identical -------
body = ("(set-logic QF_LIA)\n"
        "(declare-const r Int)(declare-const g Int)(declare-const bb Int)\n"
        "(assert (and (>= r 0)(<= r 255)(>= g 0)(<= g 255)(>= bb 0)(<= bb 255)))\n"
        "(define-fun luma () Int (+ (* 299 r) (* 587 g) (* 114 bb)))\n"
        f"(define-fun islight () Bool (> luma {THRESH}))\n"
        "; strip_colors resolves (active_t, inactive_t); legacy/dark factors are (16,40).\n"
        "(define-fun active_t   () Int (ite islight 10 16))\n"
        "(define-fun inactive_t () Int (ite islight 30 40))\n"
        "; Negation: a DARK-classified colour whose resolved factors differ from legacy.\n"
        f"(assert (<= luma {THRESH}))\n"
        "(assert (or (not (= active_t 16)) (not (= inactive_t 40))))\n(check-sat)\n")
write("dark_factors_unchanged.smt2",
      "; a9 — KEY NO-REGRESSION THEOREM. On the ENTIRE dark-classified region\n"
      ";   (luma <= 150000, all r,g,b in 0..255) strip_colors resolves EXACTLY the\n"
      ";   legacy factors (active_t=16, inactive_t=40). Same factors + same blend\n"
      ";   closure => byte-identical output to the pre-appearance code for every dark\n"
      ";   theme (existing, future, or user). Expected: unsat.\n"
      "; FAITHFUL SOURCE: tab_bar.rs strip_colors\n"
      ";   let (active_t, inactive_t) = if bg_is_light(bg) {(0.10,0.30)} else {(0.16,0.40)};\n;\n" + body)

# 4) active_distinct  (UNSAT) — active card != body for every builtin --------
#   active_ch == body_ch  <=>  T*|fg-body| < 50  (round-half-away, t=T/100, *100).
disj = "\n".join("  " + tuple_eq(*x) for x in B)
body = ("(set-logic QF_LIA)\n"
        "(declare-const fr Int)(declare-const fg Int)(declare-const fb Int)\n"
        "(declare-const br Int)(declare-const bg Int)(declare-const bb Int)\n"
        "(declare-const T  Int)\n"
        "; |d| via a case-free bound: T*|x-y| < 50  <=>  (T*(x-y) < 50 AND T*(y-x) < 50).\n"
        "(define-fun same ((x Int)(y Int)) Bool\n"
        "  (and (< (* T (- x y)) 50) (< (* T (- y x)) 50)))\n"
        "(assert (or\n" + disj + "))\n"
        "; Negation: a builtin whose active card equals the body in ALL THREE channels.\n"
        "(assert (and (same br fr) (same bg fg) (same bb fb)))\n(check-sat)\n")
write("active_distinct.smt2",
      "; a9 — the active-tab card is a DISTINCT surface from the body for EVERY\n"
      ";   builtin (else the focused tab vanishes into the strip). active card =\n"
      ";   blend(body, fg, t); the integer test T*|fg-body| < 50 (t=T/100) is a SOUND\n"
      ";   over-approximation of 'this channel is unchanged' (rounded move < 0.5): exact\n"
      ";   for T=16, and for T=10 it can only OVER-predict a change at the |fg-body|=5\n"
      ";   boundary (the f32 literal 0.10 != 1/10), which only makes this UNSAT harder.\n"
      ";   Every builtin has min |fg-body| = 96, far from that boundary. Expected: unsat\n"
      ";   (no builtin is unchanged in all three channels). DIVISION-FREE.\n"
      "; FAITHFUL SOURCE: tab_bar.rs strip_colors active_bg=blend(theme.bg,theme.fg,active_t)\n"
      ";   + the blend mix() round; T=16 (dark) / 10 (light) per appearance.\n;\n" + body)

# 5) raise_direction  (UNSAT) — division-free luma surrogate -----------------
def dir_eq(name, fg, bg, sel, ap):
    fr, fgc, fb = ch(fg); br, bgc, bb = ch(bg)
    isl = 1 if ap == "light" else 0
    return (f"(and (= fr {fr}) (= fg {fgc}) (= fb {fb}) "
            f"(= br {br}) (= bg {bgc}) (= bb {bb}) (= isl {isl}))")
disj = "\n".join("  " + dir_eq(*x) for x in B)
body = ("(set-logic QF_LIA)\n"
        "(declare-const fr Int)(declare-const fg Int)(declare-const fb Int)\n"
        "(declare-const br Int)(declare-const bg Int)(declare-const bb Int)\n"
        "(declare-const isl Int)\n"
        "(define-fun lf () Int (+ (* 299 fr) (* 587 fg) (* 114 fb)))\n"
        "(define-fun lb () Int (+ (* 299 br) (* 587 bg) (* 114 bb)))\n"
        "(assert (or\n" + disj + "))\n"
        "; Negation: appearance and the body->fg luma move disagree.\n"
        ";   dark  (isl=0) must raise  (lf > lb);  light (isl=1) must recede (lf < lb).\n"
        "(assert (or (and (= isl 0) (<= lf lb)) (and (= isl 1) (>= lf lb))))\n(check-sat)\n")
write("raise_direction.smt2",
      "; a9 — the active card raises in the correct direction per appearance: on dark\n"
      ";   themes it is brighter than the body, on light themes darker. Since the card\n"
      ";   is body blended TOWARD fg (t in (0,1)) and luma is a positive-weight linear\n"
      ";   combination, the card's luma lies strictly between body and fg luma (A5\n"
      ";   no-overshoot, proofs/ay/blend_in_gamut_case{A,B}). So the direction equals\n"
      ";   sign(luma(fg)-luma(body)), proved here over all builtins. Expected: unsat.\n"
      "; FAITHFUL SOURCE: tab_bar.rs strip_colors_raise_direction_follows_appearance test.\n;\n" + body)

# 6) selection_legible  (UNSAT) — WCAG contrast>=3.0 via sound bounds ---------
def leg_tuple(name, fg, bg, sel, ap):
    lf, ls = lum(fg), lum(sel)
    light, dark = (fg, sel) if lf >= ls else (sel, fg)
    return inum_lo(lum_lo(light)), inum_hi(lum_hi(dark))
rows = [leg_tuple(*x) for x in B]
disj = "\n".join(f"  (and (= llo {llo}) (= dhi {dhi}))" for (llo, dhi) in rows)
body = ("(set-logic QF_LIA)\n"
        "; Luminances scaled to integers over 1e12 (exact on the proof's 12-digit grid).\n"
        "; llo = SOUND lower bound on the LIGHTER of {fg,selection}; dhi = SOUND upper\n"
        "; bound on the DARKER. WCAG: contrast = (Llight+0.05)/(Ldark+0.05); >= 3.0\n"
        "; <=> Llight >= 3*Ldark + 0.10. Using llo<=Llight and dhi>=Ldark, llo >= 3*dhi\n"
        "; + 0.10 SOUNDLY implies contrast >= 3.0. (0.10*1e12 = 100000000000.)\n"
        "(declare-const llo Int)(declare-const dhi Int)\n"
        "(assert (or\n" + disj + "))\n"
        "; Negation: a builtin whose sound bound fails the >=3.0 contrast certificate.\n"
        "(assert (< llo (+ (* 3 dhi) 100000000000)))\n(check-sat)\n")
write("selection_legible.smt2",
      "; a9 — on EVERY builtin the default foreground stays legible OVER the selection\n"
      ";   surface: WCAG contrast(fg, selection) >= 3.0:1 (aterm paints selection as a\n"
      ";   BACKGROUND only, so selected text keeps its fg). Proved via SOUND rational\n"
      ";   luminance bounds (monotone sRGB transfer, outward-rounded) so the obligation\n"
      ";   is linear/division-free. Guards the light-theme dark-fg->light-surface\n"
      ";   selection substitutions. Expected: unsat.\n"
      "; FAITHFUL SOURCE: aterm-types/src/lib.rs contrast(); scheme.rs selection values;\n"
      ";   the selection_is_legible_over_foreground test (exact f32 cross-check).\n;\n" + body)

# 7) partition_nonvacuity_sat  (SAT) -----------------------------------------
body = ("(set-logic QF_LIA)\n(declare-const r Int)(declare-const g Int)(declare-const bb Int)\n"
        "; The light-set encoding is genuinely satisfiable (not a contradictory disjunction).\n"
        "(assert (or\n" + "\n".join("  " + bg_eq(x[2]) for x in LIGHT) + "))\n"
        f"(assert (> (+ (* 299 r) (* 587 g) (* 114 bb)) {THRESH}))\n(check-sat)\n")
write("partition_nonvacuity_sat.smt2",
      "; a9 CONTROL — non-vacuity: a real light builtin classifies light (the light\n"
      ";   region + its set encoding are non-empty, so partition_light is not vacuous).\n"
      "; Expected: sat.\n;\n" + body)

# 8) catches_threshold_regression_sat  (SAT) ---------------------------------
nord = next(x for x in B if x[0] == "Nord")[2]
r, g, b = ch(nord)
body = ("(set-logic QF_LIA)\n(declare-const r Int)(declare-const g Int)(declare-const bb Int)\n"
        f"; Nord bg #2e3440 = ({r},{g},{b}); its luma1000 = 51574 is the MAX over dark.\n"
        f"(assert (and (= r {r}) (= g {g}) (= bb {b})))\n"
        "; A regression that lowered the threshold to 51000 (into the dark band) would\n"
        "; reclassify Nord as light and change its chrome. Witnessed: luma1000 > 51000.\n"
        "(assert (> (+ (* 299 r) (* 587 g) (* 114 bb)) 51000))\n(check-sat)\n")
write("catches_threshold_regression_sat.smt2",
      "; a9 CONTROL — prove-AND-catch: the partition margin is REAL. Nord (the brightest\n"
      ";   dark builtin) sits at luma1000=51574, so any threshold <= 51574 would\n"
      ";   misclassify a shipping dark theme. sat here proves the proof is sensitive to\n"
      ";   a downward threshold regression. Expected: sat.\n;\n" + body)

# 9) legible_catches_false_floor_sat  (SAT) ----------------------------------
#   A too-strong floor (contrast>=4.0) FAILS for the tight case (Solarized Light 3.64).
soll = next(x for x in B if x[0] == "Solarized Light")
llo, dhi = leg_tuple(*soll)
body = ("(set-logic QF_LIA)\n(declare-const llo Int)(declare-const dhi Int)\n"
        f"; Solarized Light is the TIGHTEST builtin (exact contrast 3.636).\n"
        f"(assert (= llo {llo}))(assert (= dhi {dhi}))\n"
        "; A deliberately FALSE floor of contrast>=4.0 (llo >= 4*dhi + 0.15*1e12) does\n"
        "; NOT hold here: witnessed by its negation being satisfiable.\n"
        "(assert (< llo (+ (* 4 dhi) 150000000000)))\n(check-sat)\n")
write("legible_catches_false_floor_sat.smt2",
      "; a9 CONTROL — prove-AND-catch: the legibility bound is TIGHT, not vacuous. The\n"
      ";   checker rejects an over-strong floor (contrast>=4.0) on the tightest builtin\n"
      ";   (Solarized Light, exact 3.636), so selection_legible's >=3.0 is a real,\n"
      ";   non-trivial certificate. Expected: sat.\n;\n" + body)

print("\nAll obligations written to", OUT)
