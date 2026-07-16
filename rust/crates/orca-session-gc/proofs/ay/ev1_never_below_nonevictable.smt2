; THEOREM (session-gc): size eviction never drops below the non-evictable floor.
; survivorBytes = nonEvict + evictTotal (bytes of exempt/live/unknown-liveness dirs
; that are never evicted, plus evictable dirs). The loop only ever removes evictable
; bytes (0 <= evicted <= evictTotal), so remaining = survivorBytes - evicted is
; always >= nonEvict — live/recoverable sessions' bytes are never traded for disk.
; Negation asserted; UNSAT == proved for ALL non-negative byte totals.
(set-logic QF_LIA)
(declare-const nonEvict Int)
(declare-const evictTotal Int)
(declare-const evicted Int)
(assert (and (>= nonEvict 0) (>= evictTotal 0)))
(assert (and (>= evicted 0) (<= evicted evictTotal)))
(assert (< (- (+ nonEvict evictTotal) evicted) nonEvict))
(check-sat)
; EXPECT: unsat
