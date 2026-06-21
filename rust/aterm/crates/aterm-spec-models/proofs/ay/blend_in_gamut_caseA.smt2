; A5+ in-gamut, CASE A (bg <= fg), single-multiplier reduction.
; num = bg*(255-t) + fg*t = 255*bg + t*(fg-bg).  With d = fg-bg >= 0 (this case),
; the bracket  255*bg <= num <= 255*fg  (== bg <= mix <= fg after /255) is exactly
;     0 <= t*d <= 255*d.
; Only ONE variable*variable product (t*d); 255*d is a constant multiply.
; Assert the NEGATION; UNSAT == bg <= mix <= fg for all bg<=fg, t in 0..=255.
(set-logic QF_BV)
(declare-const bg (_ BitVec 18))
(declare-const fg (_ BitVec 18))
(declare-const t  (_ BitVec 18))
(define-fun c255 () (_ BitVec 18) (_ bv255 18))
(assert (bvule bg c255))
(assert (bvule fg c255))
(assert (bvule t  c255))
(assert (bvule bg fg))                       ; CASE A
(define-fun d   () (_ BitVec 18) (bvsub fg bg))
(define-fun td  () (_ BitVec 18) (bvmul t d))
(assert (bvugt td (bvmul c255 d)))           ; negation of  t*d <= 255*d  (lower 0<=t*d trivial in BV)
(check-sat)
