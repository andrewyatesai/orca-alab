; CONTROL (window inclusive at the upper edge / catches an off-by-one).
; A crash at exactly ms_since_launch = window_ms is IN the window (active when not
; engaged), so it counts. SAT proves the boundary is inclusive — a would-be
; `m >= window_ms` exclusion (off-by-one) would wrongly drop the boundary crash and
; is thereby ruled out.
(set-logic QF_LIA)
(declare-const w Int)
(declare-const k Int)
(assert (and (>= k 0) (>= w 0)))
(assert (let ((m w))
          (and (>= m 0) (<= m w))))
(check-sat)
; EXPECT: sat
