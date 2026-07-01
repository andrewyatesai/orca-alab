; SPDX-License-Identifier: Apache-2.0
; Copyright 2026 Andrew Yates
;
; oom_bound — the buffer + segment byte-length add does not wrap. By `ay`.
; Expected: unsat  (the negation — the add wraps — is unsatisfiable under the
;                   stated preconditions, so the guard sees a TRUTHFUL `next`).
;
; WHY THIS MATTERS: oom_buffer_le_max assumes `next >= buffer` (no wrap). If
; `buffer + segment` could wrap usize, a huge segment would make `next` small,
; the guard `next > max` would falsely pass, and push_str would blow the bound.
; This obligation discharges that assumption from the real value ranges.
;
; PRECONDITIONS (faithful):
;   * P1 (carried invariant): buffer <= max on entry to a line. Initially the
;     buffer is empty (0); every prior arm left it <= max (see oom_buffer_le_max).
;   * max <= 2^63. The default is NDJSON_MAX_LINE_BYTES = 16 MiB and new() only
;     clamps up to >= 1; a caller-supplied cap is still far below 2^63.
;   * segment.len() <= isize::MAX = 2^63 - 1 (Rust's guaranteed max slice length).
; Then next = buffer + segment <= 2^63 + (2^63 - 1) = 2^64 - 1, representable in
; u64, so the add cannot wrap.
(set-logic QF_BV)
(declare-const buffer (_ BitVec 64))
(declare-const segment (_ BitVec 64))
(declare-const maxb (_ BitVec 64))
(assert (bvule buffer maxb))                              ; P1
(assert (bvule maxb (_ bv9223372036854775808 64)))       ; max <= 2^63
(assert (bvule segment (_ bv9223372036854775807 64)))    ; segment <= isize::MAX = 2^63 - 1
(define-fun next () (_ BitVec 64) (bvadd buffer segment))
(assert (bvult next buffer))                             ; negation: the add wrapped
(check-sat)
