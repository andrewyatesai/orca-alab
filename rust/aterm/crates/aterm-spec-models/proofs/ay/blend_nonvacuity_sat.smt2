; NON-VACUITY control (mirrors the `assert_proves_and_catches` discipline).
; If the encoder were broken (e.g. always returns 0), the UNSAT proofs would be
; vacuous. This asserts a CONCRETE expected interior value and must be SAT:
;     bg=0, fg=255, t=128  =>  mix = (0*127 + 255*128)/255 = 32640/255 = 128
; SAT here proves the bvudiv encoding computes real, non-trivial blend values.
(set-logic QF_BV)
(declare-const bg (_ BitVec 32))
(declare-const fg (_ BitVec 32))
(declare-const t  (_ BitVec 32))
(assert (= bg #x00000000))
(assert (= fg #x000000ff))
(assert (= t  #x00000080))
(define-fun mix () (_ BitVec 32)
  (bvudiv (bvadd (bvmul bg (bvsub #x000000ff t)) (bvmul fg t)) #x000000ff))
(assert (= mix #x00000080)) ; 128
(check-sat)
(get-value (mix))
