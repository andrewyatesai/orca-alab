<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Andrew Yates -->

# decode_total — the C-quote octal escape decode is total (discharged by `ay`)

Re-checkable certificate that the octal arm of git's C-quoted-path decoder — the
one variable-valued arm — never panics and never silently drops an escape, and
that its byte output matches the TypeScript decoder it was ported from.

Git C-quotes non-ASCII text as a run of adjacent `\NNN` **UTF-8 bytes**, so the
arm accumulates the whole run into a `Vec<u8>` and decodes it once with
`String::from_utf8_lossy` (re-derived 2026-07-13; the earlier per-`\NNN`-to-`char`
arm corrupted `café` → `cafÃ©`). Each escape contributes exactly one byte via
`(u32::from_str_radix(octal, 8).unwrap() & 0xFF) as u8`.

Anchored to [`rust/PROOF_CARRYING_PERFORMANCE.md`](../../../../PROOF_CARRYING_PERFORMANCE.md).

**Discharged by `ay` (the Trust SAT/SMT solver) — Trust, not kani.** Run
`bash verify.sh` → exits 0 iff every obligation gets its expected verdict (or
`ay` is absent, in which case the bundle is **skipped, not failed**).

## Faithful source

`crates/orca-core/src/git_cquoted_path.rs:41-68` (the octal arm):

```rust
c if ('0'..='7').contains(&c) => {
    let mut bytes: Vec<u8> = Vec::new();
    loop {
        let mut octal = String::new();
        octal.push(chars[index]);                          // 1..=3 octal digits
        while index + 1 < n - 1 && octal.len() < 3 && chars[index + 1].is_digit(8) {
            index += 1; octal.push(chars[index]);
        }
        if let Ok(value) = u32::from_str_radix(&octal, 8) {
            bytes.push((value & 0xFF) as u8);              // one byte per escape
        }
        // continue only while another `\NNN` follows immediately …
    }
    decoded.push_str(&String::from_utf8_lossy(&bytes));
}
```

Each digit `di` is in `0..=7`, most-significant first, so `v = ((d2*8 + d1)*8 + d0)`
is in `0..=511`.

## What is proved

| File | Verdict | Obligation |
|---|---|---|
| `decode_octal_total.smt2` | **unsat** | for all `d0,d1,d2 <= 7`: `v <= 511` **and** `(v & 0xFF) <= 255` — the parse cannot overflow `u32` (so `if let Ok` always takes the Ok branch and the escape is never dropped) and the `(v & 0xFF) as u8` cast is total |
| `decode_octal_mask_matches_uint8.smt2` | **unsat** | for all `v` in `0..=511`: `(v & 0xFF) == (v mod 256)` — the emitted byte is bit-identical to the TS `parseInt(octal, 8) & 0xFF` `Uint8Array` wrap |
| `decode_octal_wrap_reachable_sat.smt2` | **sat** | `v > 255` is reachable and there `(v & 0xFF) == v - 256` — the syntactic overflow region `\400..\777` exists and the mask genuinely reduces it, so the two `unsat` theorems above are non-vacuous |

**Prove-and-catch:** the two `unsat` theorems (totality + TS-faithfulness) are
paired with a `sat` non-vacuity witness (`v = 511 = \777`, where the mask wraps to
`255`), per `assert_proves_and_catches`.

## Honest scope — what this does NOT prove

- This discharges **only the octal arm** — the sole arm whose pushed bytes depend
  on a parsed value. The named-escape arms (`\n`, `\t`, `\a`, …) and the literal
  `\\` / `\"` / fall-through arms push **constant or already-valid chars**,
  trivially valid.
- `u32::from_str_radix(&octal, 8)` on a 1-3-digit octal string is itself **total**
  (it returns `Err` rather than panicking on overflow), and the digits are
  pre-filtered by `is_digit(8)`; the `v <= 511` bound is what makes the `if let Ok`
  branch infallible so no escape is dropped.
- `String::from_utf8_lossy` is **total by the Rust std guarantee** (invalid byte
  runs decode to U+FFFD, never a panic); the SMT models the per-escape byte cast,
  not the std call. Real git output emits one UTF-8 byte per escape (`\0..\377`),
  so the `\400..\777` wrap is a defensive total-ization of the syntactic space.
