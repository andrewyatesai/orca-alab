; A5+ in-gamut, CASE B (fg <= bg), single-multiplier reduction (mirror of case A).
; num = 255*bg - t*d  with d = bg-fg >= 0 (this case). The bracket
;     255*fg <= num <= 255*bg   (== fg <= mix <= bg after /255)
; reduces, as in case A, to   0 <= t*d <= 255*d   (the non-trivial half: t*d <= 255*d).
; Cases A+B together cover all bg,fg orderings => min(bg,fg) <= mix <= max(bg,fg).
; Assert the NEGATION; UNSAT == fg <= mix <= bg for all fg<=bg, t in 0..=255.
(set-logic QF_BV)
(declare-const bg (_ BitVec 18))
(declare-const fg (_ BitVec 18))
(declare-const t  (_ BitVec 18))
(define-fun c255 () (_ BitVec 18) (_ bv255 18))
(assert (bvule bg c255))
(assert (bvule fg c255))
(assert (bvule t  c255))
(assert (bvule fg bg))                       ; CASE B
(define-fun d   () (_ BitVec 18) (bvsub bg fg))
(define-fun td  () (_ BitVec 18) (bvmul t d))
(assert (bvugt td (bvmul c255 d)))           ; negation of  t*d <= 255*d
(check-sat)
