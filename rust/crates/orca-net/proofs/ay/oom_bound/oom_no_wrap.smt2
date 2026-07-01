; SPDX-License-Identifier: Apache-2.0
; Copyright 2026 Andrew Yates
;
; oom_bound — the buffer + segment byte-length add does not wrap. By `ay`.
; Expected: unsat  (the negation — the add wraps — is unsatisfiable, so the guard
;                   sees a TRUTHFUL `next`), for ANY max_line_bytes.
;
; WHY THIS MATTERS: oom_buffer_le_max assumes `next >= buffer` (no wrap). If
; `buffer + segment` could wrap usize, a huge segment would make `next` small,
; the guard `next > max` would falsely pass, and push_str would blow the bound.
; This obligation discharges that assumption from the code-TRUE value ranges.
;
; PRECONDITION (code-enforced, not caller-dependent): Rust guarantees the byte
; length of any `String`/`&str` is <= isize::MAX = 2^63 - 1 (the allocation
; limit). The retained buffer is a `String` and the fed segment is a `&str`
; slice, so BOTH lengths are bounded by isize::MAX REGARDLESS of max_line_bytes
; (new() applies only a lower clamp `.max(1)`, no upper clamp — see
; ndjson.rs:47). Then next = buffer + segment <= 2*(2^63 - 1) = 2^64 - 2, which
; is representable in u64, so the add cannot wrap. (This supersedes an earlier
; `max <= 2^63` premise that new() did not enforce.)
(set-logic QF_BV)
(declare-const buffer (_ BitVec 64))
(declare-const segment (_ BitVec 64))
(assert (bvule buffer (_ bv9223372036854775807 64)))    ; buffer.len() <= isize::MAX = 2^63 - 1
(assert (bvule segment (_ bv9223372036854775807 64)))   ; segment.len() <= isize::MAX = 2^63 - 1
(define-fun next () (_ BitVec 64) (bvadd buffer segment))
(assert (bvult next buffer))                            ; negation: the add wrapped
(check-sat)
