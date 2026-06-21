; A5 (scoped) — endpoint exactness of the CPU coverage-blend.
; The procedural / Powerline / hard-edged-glyph path drives coverage t in {0,255}
; only (stem_darken at lib.rs:2237 fixes endpoints). There the blend must be EXACT:
;     t = 0   => mix(bg,fg,0)   == bg
;     t = 255 => mix(bg,fg,255) == fg
; Same faithful u32/BitVec32 encoding as blend_in_gamut.smt2.
; Assert the NEGATION; UNSAT == exact at both endpoints for all bg,fg in 0..=255.
(set-logic QF_BV)
(declare-const bg (_ BitVec 32))
(declare-const fg (_ BitVec 32))
(assert (bvule bg #x000000ff))
(assert (bvule fg #x000000ff))

(define-fun mix ((b (_ BitVec 32)) (f (_ BitVec 32)) (t (_ BitVec 32))) (_ BitVec 32)
  (bvudiv (bvadd (bvmul b (bvsub #x000000ff t)) (bvmul f t)) #x000000ff))

; negation of:  mix(bg,fg,0) == bg  AND  mix(bg,fg,255) == fg
(assert (or (not (= (mix bg fg #x00000000) bg))
            (not (= (mix bg fg #x000000ff) fg))))
(check-sat)
