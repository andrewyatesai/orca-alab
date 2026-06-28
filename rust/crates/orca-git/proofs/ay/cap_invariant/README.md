<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Andrew Yates -->

# cap_invariant — the unified status parser is memory-bounded (discharged by `ay`)

Re-checkable certificate for the cap on Orca's one status/diff record scanner
(`StatusPorcelainParser`). The scanner is shared by the local streaming path
(`status.rs::parse_porcelain_v2_status`) and the relay one-shot
(`status_stream.rs::parse_status_porcelain`), so the cap is applied **during** the
scan — this is the fix for the relay's old full-materialize-then-truncate.

Anchored to [`rust/PROOF_CARRYING_PERFORMANCE.md`](../../../../PROOF_CARRYING_PERFORMANCE.md)
(the proof-boundary / `assert_proves_and_catches` contract).

**Discharged by `ay` (the Trust SAT/SMT solver) — Trust, not kani.** Run
`bash verify.sh` → exits 0 iff every obligation gets its expected verdict (or
`ay` is absent, in which case the bundle is **skipped, not failed**).

## Faithful source

`crates/orca-git/src/status_stream.rs:82-85` (the per-line stop-check, `update`):

```rust
if limit != 0 && self.count > limit { self.stopped = true; return true; }
```

`crates/orca-git/src/status_stream.rs:107-108` (the emit slice, `into_result`):

```rust
let keep = self.count.min(limit);
self.entries.into_iter().take(keep).collect()
```

The stop-check runs **once per line, after** that line's pushes; a type-1/2 `MM`
line pushes at most 2 entries (staged + unstaged). So on entry to any line,
`count <= limit` (we did not stop on the previous line) — the carried precondition
**P1**.

## What is proved

| File | Verdict | Obligation |
|---|---|---|
| `cap_emit_le_limit.smt2` | **unsat** | `min(count, limit) <= limit` for **all** inputs with `limit != 0` (the emitted Vec length is bounded, unconditionally) |
| `cap_buffer_le_limit_plus_2.smt2` | **unsat** | under **P1** (`c <= limit`) and `<= 2` pushes/line: `buffered = c + k <= limit + 2`, and the add does not wrap |
| `cap_nonvacuity_sat.smt2` | **sat** | `c = limit, k = 2` reaches exactly `limit + 2` — the bound is tight, not loose |
| `cap_catches_false_tight_sat.smt2` | **sat** | `buffered > limit + 1` is reachable, refuting the too-tight `limit + 1` bound (a boundary MM line overshoots it by reaching `limit + 2`) |

**Prove-and-catch:** the two `unsat` bound theorems are paired with a `sat`
non-vacuity witness and a `sat` catch of the false tighter bound, per
`assert_proves_and_catches`.

## Width

32-bit `QF_BV` counter arithmetic. Counts are `usize` on a 64-bit host but are
bounded far below `2^32` in any real worktree; `min(a,b) <= b` and `a + b` bound
arithmetic are width-uniform, so the narrowing does not weaken the theorem.

## Honest scope — what this does NOT prove

- This arithmetic bounds the **parsed-entries `Vec`** (the structured rows Orca
  keeps in memory). It does **not** prove the relay's V8 **giant-string OOM** is
  gone: that crash is the *upstream one-shot `stdout` buffer* being materialized
  before parsing — a separate concern handled by `isGitBufferOverflowError`, not
  by this proof. The cap fix only bounds the entries `Vec`.
- **"The relay materialize bug is fixed"** — i.e. that the relay one-shot now caps
  *during* the scan, identically to the local streaming path — is verified by the
  **dual-run regression test**
  (`status_stream.rs::tests::cap_during_scan_matches_streaming_local_path`), not by
  this proof. This bundle proves only the counter inequalities the test relies on.
