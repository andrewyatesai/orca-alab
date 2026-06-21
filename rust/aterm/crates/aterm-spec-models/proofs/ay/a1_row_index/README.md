<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The aterm Authors -->

# A1 — `row_index()` returns an in-bounds index (discharged by `ay`)

Re-checkable certificate for initiative **A1** (the roadmap's declared beachhead):
the engine's single most-executed lookup, `row_index()` on the `display_offset==0`
fast path, returns a physical row index that is always `< rows.len()` — so the
panic-indexing sites `storage.rs:400` / `:417` are provably in-bounds and may be
converted to `get_unchecked`.

**Discharged by `ay` (the Trust SAT/SMT solver) — Trust, not kani.** Run
`bash verify.sh` → exits 0 iff all four obligations get their expected verdict.

## Faithful source

`crates/aterm-grid/src/grid/state/storage.rs:219` (fast path):

```rust
return Some((self.ring_head + base) % self.rows.len());
```

`rows` is asserted non-empty at entry (`!self.rows.is_empty()`), so `len != 0`.

## What is proved

| File | Verdict | Obligation |
|---|---|---|
| `row_index_in_bounds.smt2` | **unsat** | `(ring_head + base) % len < len` for **all** `len != 0` (the `x % n < n` postcondition; index in bounds) |
| `row_index_no_overflow.smt2` | **unsat** | under `ring_head <= MAX - base`, the add does **not** wrap — the fast-path index is the *intended* (non-wrapped) row, not merely some in-bounds row |
| `row_index_nonvacuity_sat.smt2` | **sat** | strict-interior indices (`0 < idx < len-1`) are genuinely reachable — the encoder is not degenerate |
| `row_index_catches_false_tight.smt2` | **sat** | `idx = len-1` is reachable, refuting the false tighter bound `idx <= len-2` — so `len` is the **least** upper bound and `idx < len` is exact |

**Prove-and-catch non-vacuity:** the `unsat` bound proof is paired with a `sat`
non-vacuity witness and a `sat` catch of the false tighter bound, per the
`assert_proves_and_catches` discipline.

## Width

Modeled at **20-bit** (`len <= 1_048_575`). The property `x % n < n` is
**width-uniform** — true at every bit width — so narrowing from `usize`'s 64 bits
does not weaken it; it only keeps `ay`'s symbolic-divisor `bvurem` out of the
divider-bit-blast frontier that times out at 64-bit (the same routing lesson
`proofs/ay/README.md` records for A5). aterm's physical `rows.len()` =
`visible_rows + scrollback capacity` is bounded far below `2^20` in any real
configuration, so the model covers the entire reachable domain.

## Honest scope — what this does NOT prove

- This is the **arithmetic / bounds** half of A1. The full `get_unchecked`
  license also needs the **single-borrow lexical lemma**: within the exclusive
  `&mut self` borrow in `row_mut_with_effective_cols`, `rows.len()` does not change
  between computing `idx` and using it (`rows` is `pub`, so this is a *lexical*
  argument, **not** a struct invariant — state it that way). That side-condition is
  a Rust-level fact; its natural machine-checker is the **Trust deductive verifier**
  (`trustc` `#[ensures]`/`#[requires]`), **not** kani. On this box `trustc` is
  present and proves *simple* postconditions (e.g. `result >= 0`), but **empirically
  (2026-06-20) returns `Unsupported` for symbolic-comparison contracts** like
  `result < len` (trust-wp: "comparison contains symbolic or unsupported operands"),
  so the deductive path cannot yet discharge this here. The borrow lemma is therefore
  held as the documented precondition for the `get_unchecked` conversion. The
  arithmetic fact above (the load-bearing index bound) is fully discharged on `ay`.
- Scope the conversion strictly to `:400` / `:417` first (they already panic-index,
  so it is zero behavior change). The fallible `.get()/.get_mut()` at `:369/:379`
  are intentionally `None`-returning on the `display_offset>0` path — out of scope.
