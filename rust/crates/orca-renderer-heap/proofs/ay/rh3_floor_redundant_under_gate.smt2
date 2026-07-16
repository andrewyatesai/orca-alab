; THEOREM (renderer-heap): the FLOOR clamp is redundant under the RAM gate.
; The 7.5 GiB gate means the target only reaches the sizing when
; floor(totalGiB * 0.4) * 1024 >= floor(7.5 * 0.4) * 1024 = floor(3.0) * 1024 = 3072.
; So on the reachable path t >= 3072 and max(3072, t) = t — the floor never fires;
; it is a defensive belt-and-suspenders, and the effective clamp is min(4096, t).
; This documents an intended dead branch, not a bug.
; Negation asserted; UNSAT == proved for ALL t >= 3072.
(set-logic QF_LIA)
(declare-const t Int)
(assert (>= t 3072))
(assert (not (= (ite (>= t 3072) t 3072) t)))
(check-sat)
; EXPECT: unsat
