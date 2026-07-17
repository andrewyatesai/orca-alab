# ts2rust Port Recipes — clearing trustc's W1 walls without losing faithfulness

Distilled from the 2026-07-16/17 Goal-A harvest that moved the census **54 → 90
TRUSTED** by recovering 36 "faithful-miss" kernels — ports that were already
W2-equivalent to their TS twin (0 differential divergences) but W1-INCOMPLETE.
None of those 36 needed a new verifier capability; each was a *formulation* fix.
This is the reusable playbook so the F3/F4 automation and future hand-ports skip
the walls from the start. Diagnose the exact blocker with
`trustc -Z trust-verify-output=json` (or `pnpm blocker-census` for the whole
corpus); do NOT guess which wall you're on.

## The safety net: the W2 differential gate protects faithfulness

Every recipe below is a **behaviour-preserving** rewrite — it changes only *how*
the value is produced, never *what* it is, so W1 clears while W2 stays 0-divergence.
You do not need to reason perfectly about faithfulness up front: a rewrite that
accidentally changes behaviour comes back **NOT-TRUSTED** (W2 finds the divergence)
and you revert. So "harder" candidates are safe to *attempt* — the cost is effort,
not a wrong kernel shipping. Attempt, verify, keep-or-revert.

## The five W1 walls (what actually blocks a faithful port)

| Wall | Signature in the JSON | Why | Recipe |
|---|---|---|---|
| **Absent-callee alloc** | `absent callee … to_string/String::from/collect … may panic` | An `extern "Rust"` allocation *can* unwind (OOM); trustc stays fail-closed (see below) | Return borrowed `&str`/`&'static str` — **remove** the allocation (§1) |
| **Absent-callee iterator** | `absent callee … Chars/Bytes/Split/EncodeUtf16/into_iter` | The iterator adapter body isn't lowered | Indexed `while` over `as_bytes()`/the slice with `.get()` (§2, §3) |
| **Drop-glue unwind** | `unsupported MIR Drop::UnsupportedUnwind` | An owned `String`/`Vec`'s drop has an un-modelled unwind path | Don't own it — borrow (§1, §6); genuine builders are residue |
| **Unsupported-MIR bounds** | `unsupported MIR … BoundsCheck` | `slice[i]` panics on OOB | Use `.get(i)` (returns `Option`, no panic obligation) |
| **Unsupported-MIR arith** | `unsupported MIR … ArithmeticSafety` | `i += 1` can overflow | `i = i.saturating_add(1)` (no overflow obligation) |

## The seven recovery patterns

1. **Substring/literal return → `&str` / `&'static str`.** A kernel whose value is
   a borrowed slice of the input (via `strip_*`/`trim`/`split_once` — all lowered)
   or a string literal but that calls `to_string()`/`String::from()`: change the
   return type and drop the allocation. `Option<String>`→`Option<&str>`,
   `String`→`&'static str` for literal maps.

2. **`.bytes().all(pred)` → indexed `while`.** Byte-identical by construction:
   ```rust
   let b = s.as_bytes(); let mut i = 0;
   while i < b.len() { if let Some(&c) = b.get(i) { if !pred(c) { return false; } } i = i.saturating_add(1); }
   true
   ```

3. **`for x in <slice / str-iter>` → indexed `while` + `.get()`.** Same body, same
   elements — only the absent iterator leaves. Works for `&[T]` slices and byte
   scans alike.

4. **Regex / structured scan → hand byte state-machine.** `\d+(\.\d+)*`, a UUID
   shape, "any `/`-segment == `..`" all become a small forward scan tracking a few
   flags/counters. `.` `/` etc. are ASCII, so byte boundaries coincide with the
   TS's split/code-unit boundaries.

5. **`RangeInclusive::contains(&c)` → explicit `c >= lo && c <= hi`.** `contains`
   leaves a runtime-checked obligation; the compare form proves clean.

6. **Owned-`String` enum → lifetime-parameterized borrow.**
   `enum R { Issue{ id: String } }` → `enum R<'a> { Issue{ id: &'a str } }`; serde
   serializes `&str` identically. (The gauntlet's generic/lifetime signature
   handling already accepts `fn f<'a>(…)`.)

7. **Compose the primitives.** A "complex" kernel is usually just several walls at
   once — a substring search, an N-way membership test, a byte-range equality — each
   of which is `.get()` + saturating index + byte compare. `str::contains(needle)`
   becomes a two-loop byte search (`matches_at`/`contains_sub`); an N-way blocklist
   membership becomes an indexed `while` over the const array calling a
   `range_eq(hay, start, end, needle)` helper. "Complex" ≠ "needs new capability".

## Faithfulness guards (where byte-level is NOT sound)

Byte-scanning is only faithful where it provably decides the same predicate as the
TS char/code-unit logic **for all inputs**, not just the fuzzed ones:

- **Counting/positioning a SPECIFIC ASCII code** ('\n', ' ', '.', '/', digits) is
  safe — that byte never appears inside a UTF-8 multibyte sequence.
- **Substring search / equality against a valid-UTF-8 needle** is safe — UTF-8
  self-synchronizes, so a byte match is a char-boundary match. This includes
  **non-ASCII needles**: UTF-8 is deterministic, so a specific char/range is an
  exact byte pattern (braille U+2800–28FF = `E2 A0..A3 80..BF`; `"π - "` =
  `CF 80 20 2D 20`). Corrected 2026-07-17 — an earlier revision wrongly listed
  non-ASCII targets as unsafe; probing recovered all of them.
- **Slicing is fine via `str::get(range)`** — it returns `Option` and generates
  ZERO obligations (probed; it's on the lowered list), so substring-returning and
  scalar-count-truncation kernels (`get(..byte_idx)` after a lead-byte count) are
  recoverable too.
- **Still genuinely char-level**: semantics that depend on UTF-16 *code-unit
  counts* where inputs can be non-ASCII (`.length` caps compared against
  code-unit positions), and broad-Unicode *classes* you can't enumerate cheaply
  (JS `\s` is enumerable but large — weigh effort vs a spec). When unsure:
  attempt it — W2 refutes an unfaithful rewrite; revert on NOT-TRUSTED.
  `_bug`/`_naive` soundness controls must stay NOT-TRUSTED — never "recover" them.

## The residue this playbook does NOT clear (owner-gated / research-scope)

- **Owned-`String` builders** (the dominant residue, ~115 kernels: `push`/`collect`
  that genuinely construct a new string). The absent-callee allocation wall is a
  **deliberate, soundness-critical, ABI-gated fail-closed boundary** in
  `trust_verify.rs` (`extern_abi_is_non_unwinding`): an out-of-bundle call's
  panic-freedom is discharged only for non-unwinding C-family ABIs. Clearing these
  means *axiomatizing std/alloc panic-freedom* (via `#[trust::ensures]` contracts or
  bundling std) — which **changes what TRUSTED guarantees** (is OOM-panic in
  scope?). That is an owner decision + a stage2 rebuild, not a formulation fix.
- **Loop-invariant synthesis / division & nonlinear theory** — genuine SMT work in
  `ay-chc/src/smt` (a *constant* divisor like `code / 8` already discharges; a
  variable divisor does not).
