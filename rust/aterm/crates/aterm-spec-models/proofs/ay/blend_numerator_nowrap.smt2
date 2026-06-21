; WIDTH-FIDELITY premise for blend_in_gamut.smt2.
; Discharges the claim "numerator < 2^18, so an 18-bit model does not wrap and is
; bit-identical to the real u32 path on the domain bg,fg,t in 0..=255".
; Computed at the REAL width (BitVec 32, == Rust u32) so the bound itself cannot
; be an artefact of a too-narrow model.
;     numerator = bg*(255-t) + fg*t,   claim: numerator <= 130050  (0x0001fc02)
; Assert the NEGATION; UNSAT == the bound holds, justifying the 18-bit narrowing.
(set-logic QF_BV)
(declare-const bg (_ BitVec 32))
(declare-const fg (_ BitVec 32))
(declare-const t  (_ BitVec 32))
(assert (bvule bg #x000000ff))
(assert (bvule fg #x000000ff))
(assert (bvule t  #x000000ff))
(define-fun num () (_ BitVec 32)
  (bvadd (bvmul bg (bvsub #x000000ff t)) (bvmul fg t)))
(assert (bvugt num #x0001fc02)) ; 130050
(check-sat)
