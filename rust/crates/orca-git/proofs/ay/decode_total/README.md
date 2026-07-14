<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Andrew Yates -->

# decode_total — the C-quote octal escape decode is total (discharged by `ay`)

> **SUPERSEDED — pending re-derivation.** The octal arm was rewritten (2026-07-13)
> to fix a real bug: git C-quotes non-ASCII as a run of adjacent `\NNN` UTF-8
> BYTES, so the decoder now accumulates the whole run into a `Vec<u8>` and decodes
> it with `String::from_utf8_lossy`, instead of turning each `\NNN` into its own
> `char` (which corrupted `café` → `cafÃ©`). The `.smt2` obligations below still
> model the OLD per-codepoint arm — and their `decode_octal_catches_u8_truncation`
> obligation actually argued *for* the buggy per-codepoint path. They must be
> re-authored for the new design; until then this bundle is stale. The new arm's
> totality is by construction: `u32::from_str_radix(_, 8)` is total, `& 0xFF` is a
> total u8 cast, and `String::from_utf8_lossy` never panics (invalid bytes → U+FFFD),
> so the run is always emitted and nothing is dropped.

Re-checkable certificate that the octal arm of git's C-quoted-path decoder — the
one variable-valued arm — never panics and never silently drops an escape: every
octal value it can produce is a valid Unicode scalar, so `char::from_u32` is
always `Some`.

Anchored to [`rust/PROOF_CARRYING_PERFORMANCE.md`](../../../../PROOF_CARRYING_PERFORMANCE.md).

**Discharged by `ay` (the Trust SAT/SMT solver) — Trust, not kani.** Run
`bash verify.sh` → exits 0 iff every obligation gets its expected verdict (or
`ay` is absent, in which case the bundle is **skipped, not failed**).

## Faithful source

`crates/orca-core/src/git_cquoted_path.rs:41-53` (the octal arm):

```rust
c if ('0'..='7').contains(&c) => {
    let mut octal = String::new();
    octal.push(c);
    while index + 1 < n - 1 && octal.len() < 3 && chars[index + 1].is_digit(8) { /* up to 3 digits */ }
    if let Some(decoded_ch) = u32::from_str_radix(&octal, 8).ok().and_then(char::from_u32) {
        decoded.push(decoded_ch);
    }
}
```

Each digit `di` is in `0..=7`, and the value is `v = ((d2*8 + d1)*8 + d0)`
(most-significant digit first), so `v` is in `0..=511`.

## What is proved

| File | Verdict | Obligation |
|---|---|---|
| `decode_octal_total.smt2` | **unsat** | for all `d0,d1,d2 <= 7`: `v <= 511` **and** `v < 0xD800` — so `v` is below the surrogate floor, `char::from_u32(v)` is **always Some**, and the escape is never dropped |
| `decode_octal_nonvacuity_sat.smt2` | **sat** | `v = 511` (`\777`) is reachable — the max octal value is real |
| `decode_octal_catches_u8_truncation_sat.smt2` | **sat** | `v > 255` is reachable, refuting any `octal as u8` per-byte path that would wrap `256..511` and corrupt `\400..\777` — pins the model **per value (0..=511)**, not per byte |

**Prove-and-catch:** the `unsat` totality theorem is paired with a `sat`
non-vacuity witness (`v = 511`) and a `sat` catch of the per-byte-truncation trap
(`v > 255`), per `assert_proves_and_catches`.

## Honest scope — what this does NOT prove

- This discharges **only the octal arm** — the sole arm whose pushed `char`
  depends on a parsed value. The named-escape arms (`\n`, `\t`, `\a`, …) and the
  literal `\\` / `\"` / fall-through arms push **constant or already-valid ASCII
  chars**, trivially valid.
- `u32::from_str_radix(&octal, 8)` on a 1-3-digit octal string is itself **total**
  (it returns `Err` rather than panicking on overflow), and the digits are
  pre-filtered by `is_digit(8)`; the value bound above is what makes the subsequent
  `char::from_u32` infallible.
