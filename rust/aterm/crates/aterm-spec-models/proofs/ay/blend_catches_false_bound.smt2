; CATCH control (the `Buggy=1` half of prove-and-catch).
; A deliberately FALSE bound: claim mix <= 200 always. The solver MUST refute it
; with a counterexample, proving the checker can actually find blend violations
; (so the UNSAT verdicts in the real theorems are meaningful, not solver no-ops).
; Expected: SAT, with a model where mix > 200 (e.g. bg=fg=255 => mix=255).
(set-logic QF_BV)
(declare-const bg (_ BitVec 32))
(declare-const fg (_ BitVec 32))
(declare-const t  (_ BitVec 32))
(assert (bvule bg #x000000ff))
(assert (bvule fg #x000000ff))
(assert (bvule t  #x000000ff))
(define-fun mix () (_ BitVec 32)
  (bvudiv (bvadd (bvmul bg (bvsub #x000000ff t)) (bvmul fg t)) #x000000ff))
(assert (bvugt mix #x000000c8)) ; mix > 200 : a witness exists, so SAT
(check-sat)
(get-value (bg fg t mix))
