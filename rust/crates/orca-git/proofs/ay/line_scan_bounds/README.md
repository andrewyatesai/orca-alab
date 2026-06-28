<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Andrew Yates -->

# line_scan_bounds — single-scan line-split index arithmetic (discharged by `ay`)

Re-checkable certificate for the index arithmetic of the status scanner's
newline-splitting loop: given the next `0x0A` at `nl` with `start <= nl < len`,
the CR-stripped record end and the next record start are all in-bounds and
underflow-free.

Anchored to [`rust/PROOF_CARRYING_PERFORMANCE.md`](../../../../PROOF_CARRYING_PERFORMANCE.md).

**Discharged by `ay` (the Trust SAT/SMT solver) — Trust, not kani.** Run
`bash verify.sh` → exits 0 iff every obligation gets its expected verdict (or
`ay` is absent, in which case the bundle is **skipped, not failed**).

## Faithful source

`crates/orca-git/src/status_stream.rs:76-81` (the line-split loop in `update`):

```rust
while let Some(rel) = memchr::memchr(0x0A, &text[start..]) {
    let nl = start + rel;                                     // start <= nl < len
    let end = if nl > start && text[nl - 1] == 0x0D { nl - 1 } else { nl };
    self.parse_line(&text[start..end]);                      // needs start <= end <= nl
    start = nl + 1;                                          // next start' = nl+1 <= len
}
```

## What is proved

| File | Verdict | Obligation |
|---|---|---|
| `line_scan_in_bounds.smt2` | **unsat** | given `start <= nl < len` and `cr = (text[nl-1]==0x0D)`, `end = ite(cr && nl>start, nl-1, nl)` satisfies `start <= end <= nl < len`, the `nl-1` subtraction never underflows, and the next `start' = nl+1 <= len` (no wrap) |
| `line_scan_nonvacuity_sat.smt2` | **sat** | with `start == nl` (an empty / bare-`\n` record) the guard `nl > start` is false, so `end` collapses to `start` **without** computing `nl-1` — proving the guard is load-bearing (prevents a `usize` underflow at `start==nl==0`) |
| `line_scan_catches_false_strip_sat.smt2` | **sat** | with no CR, `end == nl` is reachable, refuting the false tighter bound "a CR is always stripped" (`end <= nl-1`) — so `end <= nl` is exact |

**Prove-and-catch:** the `unsat` bounds theorem is paired with a `sat` non-vacuity
/ load-bearing-guard witness and a `sat` catch of the false always-strip bound, per
`assert_proves_and_catches`.

## Honest scope — what this does NOT prove

- This proves **only the index arithmetic of single-buffer line splitting**. It
  does **not** license `get_unchecked`: that needs an unproven **single-borrow
  lexical lemma** (`text` is not aliased/resized between computing `nl` and slicing
  `text[start..end]`), which `trustc` returns `Unsupported` for. That lemma is held
  as a documented **precondition** — so the code keeps **checked slices**.
- It does **not** cover **carry concatenation across `update()` calls** (the
  `carry + chunk` re-assembly across chunk boundaries) — that is an inductive / CHC
  property, not flat `QF_BV`.
- It does **not** cover the **intra-line, space-delimited FIELD-offset arithmetic**
  (`parts.slice(8)` / `slice(9)`, tab-split rename paths) inside `parse_line`; those
  use checked `.min(len)` slicing, not modeled here.
