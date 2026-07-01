<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Andrew Yates -->

# oom_bound — the NDJSON splitter is memory-bounded (discharged by `ay`)

Re-checkable certificate for the byte-budget on Orca's daemon-socket NDJSON line
splitter (`orca_net::NdjsonSplitter`, the Rust port of `createNdjsonParser` in
`src/main/daemon/ndjson.ts`). The daemon socket is local but persistent; a peer
that never sends a newline must not grow the parser buffer without bound, so a
per-line UTF-8 byte budget drops oversized lines and resyncs at the next newline.

Anchored to [`rust/PROOF_CARRYING_PERFORMANCE.md`](../../../../PROOF_CARRYING_PERFORMANCE.md)
(the proof-boundary / `assert_proves_and_catches` contract).

**Discharged by `ay` (the Trust SAT/SMT solver) — Trust, not kani.** Run
`bash verify.sh` → exits 0 iff every obligation gets its expected verdict (or
`ay` is absent, in which case the bundle is **skipped, not failed**).

## Faithful source

`crates/orca-net/src/ndjson.rs:84-95` (the growth site, `feed`):

```rust
let next_line_bytes = self.buffer.len() + segment.len();
if next_line_bytes > self.max_line_bytes {
    out.push(NdjsonEvent::Oversized { observed_bytes: next_line_bytes });
    self.buffer.clear();
    // ... resync ...
}
self.buffer.push_str(segment);   // ONLY reached when next_line_bytes <= max
```

`push_str` is the only place the buffer grows, and it runs only when the guard is
false (`next <= max`). Every other arm — oversized, discarding, and the
newline `mem::take` — leaves the buffer at `0`. So after any iteration the
retained buffer is `<= max_line_bytes`; by induction, after any `feed`.

## What is proved

| File | Verdict | Obligation |
|---|---|---|
| `oom_buffer_le_max.smt2` | **unsat** | guard passed (`next <= max`) and no wrap ⇒ post-push `buffer + segment <= max` |
| `oom_no_wrap.smt2` | **unsat** | `buffer, segment <= isize::MAX` (the Rust `String`/`&str` allocation limit) ⇒ `buffer + segment` does not wrap usize, for ANY `max_line_bytes` |
| `oom_nonvacuity_sat.smt2` | **sat** | `buffer = 0, segment = max` reaches exactly `max` — the bound is tight, not loose |
| `oom_catches_unguarded_sat.smt2` | **sat** | without the guard, `buffer > max` is reachable — the guard is load-bearing |

**Prove-and-catch:** the two `unsat` theorems are paired with a `sat` non-vacuity
witness (the bound is tight) and a `sat` catch (the guard is necessary), per
`assert_proves_and_catches`.

## Width

64-bit `QF_BV`. `buffer.len()` / `segment.len()` / `max_line_bytes` are `usize`,
which is 64-bit on every desktop target the daemon runs on (macOS, Linux,
Windows x64/arm64), so the width is faithful — this is NOT a narrowed model.

## Honest scope — what this does NOT prove

- This bounds the splitter's **retained partial-line buffer** — the unbounded-growth
  OOM vector. It does not bound a caller that accumulates the emitted line
  `String`s itself; those are handed off and dropped per line.
- **Parity with the live TS `createNdjsonParser`** (that the Rust splitter is a
  faithful drop-in) is verified by the dual-run test
  `src/main/daemon/ndjson-napi-parity.test.ts`, not by this proof. This bundle
  proves only the byte-budget inequality the splitter relies on.
- `new()` applies only a lower clamp (`max_line_bytes.max(1)`), no upper clamp, so
  `max_line_bytes` can be any `usize`. `oom_no_wrap` therefore does NOT lean on any
  cap on `max` — it derives no-wrap from the `isize::MAX` length guarantee that
  `String`/`&str` already satisfy, so the bound holds for every reachable state.
