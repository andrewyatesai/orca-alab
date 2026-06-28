; SPDX-License-Identifier: Apache-2.0
; Copyright 2026 Andrew Yates
;
; line_scan_bounds PROVE-AND-CATCH control — catches the FALSE claim "a CR is always
;   stripped" (end < nl always). By `ay`.
; Expected: sat  (when no CR precedes nl, end == nl is reachable, so the tighter bound
;   end <= nl-1 is FALSE). This makes line_scan_in_bounds' "end <= nl" non-vacuous:
;   end actually reaches nl on the common (no-CRLF) path.
(set-logic QF_BV)
(declare-const start (_ BitVec 32))
(declare-const nl (_ BitVec 32))
(declare-const len (_ BitVec 32))
(declare-const cr Bool)
(assert (bvule start nl))
(assert (bvult nl len))
(assert (not cr))                                        ; no CR before nl (LF-only record)
(define-fun strip () Bool (and cr (bvugt nl start)))
(define-fun end () (_ BitVec 32) (ite strip (bvsub nl (_ bv1 32)) nl))
(assert (= end nl))                                      ; end == nl reachable => false bound caught
(check-sat)
