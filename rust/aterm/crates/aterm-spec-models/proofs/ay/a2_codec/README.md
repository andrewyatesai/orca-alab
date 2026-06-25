<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Andrew Yates -->

# A2 — base64/hex codec: decode never panics + encoder output is ASCII (discharged by `ay`)

Re-checkable certificate for initiative **A2**: `aterm-codec`'s base64 and hex
**decode** paths are **total** (return `Ok`/`Err`, never panic), and the base64
**encoder**'s output is **provably ASCII** — the load-bearing lemma that makes the
encoder's `unsafe { String::from_utf8_unchecked(out) }` (`base64.rs:148`) sound.

**Discharged by `ay` (the Trust SAT/SMT solver) on hand-encoded SMT-LIB2 — Trust,
not kani.** Run `bash verify.sh` → exits 0 iff all eight obligations get their
expected verdict.

## Faithful source

`crates/aterm-codec/src/base64.rs` and `crates/aterm-codec/src/hex.rs`.

- `base64.rs:219-220` `decode_byte`: `let val = table[byte as usize];` — `byte: u8`
  (0..=255), `table: &[u8; 256]` ⇒ the index is always `< 256`, lookup never panics.
- `base64.rs:116` encoder accumulator: `n = (u32::from(c0)<<16)|(u32::from(c1)<<8)|u32::from(c2)`
  with each `cN` a byte `<= 255` ⇒ max `n = 0xFF_FFFF`, the `u32` never overflows.
- `base64.rs:117-120,128-139` alphabet index: `alphabet[((n >> k) & 0x3F) as usize]`
  for `k ∈ {18,12,6,0}`, `alphabet: [u8; 64]` ⇒ the masked index is always `<= 63 < 64`.
- `base64.rs:9-10` `STANDARD_ALPHABET = b"A-Za-z0-9+/"` and the pad `b'='` (61) are
  **all ASCII (< 128)** ⇒ every emitted byte is ASCII ⇒ `from_utf8_unchecked` is sound.
- `hex.rs:82-89` `decode_nibble`: the `match` guards each subtraction (`byte - b'0'`,
  `byte - b'a' + 10`, `byte - b'A' + 10`), so no underflow; the `_` arm returns `Err`
  ⇒ total. Each arm's nibble is in `0..=15` (`< 16`).

## What is proved

| File | Verdict | Obligation |
|---|---|---|
| `decode_table_index_inbounds.smt2` | **unsat** | for all `byte: u8`, `(byte as usize) < 256` — the 256-entry decode table lookup never panics |
| `encoder_alphabet_index_inbounds.smt2` | **unsat** | for all `n: u32` and every shift `k < 32`, `((n >> k) & 0x3F) < 64` — the `[u8; 64]` alphabet lookup never panics |
| `encoder_accumulator_no_overflow.smt2` | **unsat** | `(c0<<16)\|(c1<<8)\|c2` with `cN <= 255` is `<= 0x00FF_FFFF` — the `u32` accumulator never overflows |
| `encoder_output_ascii.smt2` | **unsat** | every entry of the **concrete** 64-byte standard alphabet (and `b'='`) is `< 128` — **licenses `from_utf8_unchecked`** |
| `hex_nibble_no_underflow.smt2` | **unsat** | in each `match` arm the subtraction does not underflow **and** the nibble is `< 16` |
| `decode_total_witness_sat.smt2` | **sat** | non-vacuity/totality: the concrete decode table's **Err** path (`0xFF` sentinel) and **Ok** path (valid 0..63 index) are both reachable — decode is total, not a panic |
| `catches_false_alphabet_bound_sat.smt2` | **sat** | the masked index **63 is reachable**, refuting the false tighter bound `idx <= 62` — so `64` is the **least** upper bound and `< 64` is exact |
| `hex_total_witness_sat.smt2` | **sat** | a byte outside all three hex ranges hits the `_` arm ⇒ **Err** (total, not panic); the digit arm's nibble max `9` is attained |

**Prove-and-catch non-vacuity** (per `assert_proves_and_catches`): each `unsat`
bound is paired with `sat` controls — `decode_total_witness_sat` and
`hex_total_witness_sat` witness that both the `Ok` and `Err` branches are genuinely
reachable (so totality is not vacuous), and `catches_false_alphabet_bound_sat`
catches a deliberately-too-tight bound, certifying `< 64` is exact. The two
load-bearing `unsat` files were additionally mutation-probed: injecting a non-ASCII
alphabet byte flips `encoder_output_ascii` to `sat`, and forcing a contradictory
table entry flips the decode witness to `unsat` — confirming both encodings really
interpret the concrete constants rather than being trivially decided.

## Honest property statement

For **every** `u8` input byte the base64/hex decode lookups are in-bounds and the
hex nibble subtractions cannot underflow, so **decode is total — it returns `Ok` or
`Err` and never panics** on arbitrary (untrusted) input. For **every** `u32`
accumulator the base64 encoder's alphabet index is in-bounds and the accumulator
does not overflow, and **every byte the encoder emits is ASCII (`< 128`)** — which
is exactly the safety obligation of `unsafe { String::from_utf8_unchecked(out) }` at
`base64.rs:148`. These facts are width-faithful: `u8` is modeled as an 8-bit BV and
`u32` (including `u32::from` zero-extension and the `as usize` cast of a `u8`) as a
32-bit BV, matching the source exactly; no width was narrowed.

## Width

Native widths, no narrowing (the prompt's instruction for these divider-free
`QF_BV` problems): `u8` → `(_ BitVec 8)`, `u32` and `usize`-index casts → `(_ BitVec 32)`.
`u32::from(byte)` and `byte as usize` are modeled as **zero-extension** (a `u8`→wider
cast is zero-fill). The concrete alphabet and the 256-entry decode table are encoded
as pure-BV `ite`-chains (no array theory), so every obligation stays in `QF_BV` and
solves in milliseconds.

## Honest scope — what this does NOT prove

- **This does NOT license OSC-52 paste to skip its checked UTF-8 round-trip.** This
  bundle licenses the **encoder's** `from_utf8_unchecked` (its *output* is provably
  ASCII). It says **nothing** about the *decoder's* output: base64-/hex-decoded bytes
  are **arbitrary binary**, not guaranteed UTF-8. `aterm-core`'s OSC-52 handler keeps
  the mandatory **checked** `String::from_utf8(decoded)` at
  `crates/aterm-core/src/terminal/handler_osc.rs:521` (it bails with `return` on
  invalid UTF-8). **No codec proof here removes or weakens that check**, and none
  should — replacing it with the unchecked variant would be unsound.
- **No round-trip / bijection.** The all-lengths fact `decode(encode(x)) == x` is
  inductive over the chunk loop (the `chunks_exact(3)` / 4-byte-window iterations) and
  is **not** proved here — that is clean/`trust-vc` residue (a loop-invariant /
  inductive argument), not a flat SMT obligation. The shipped fuzz tests
  (`base64.rs:235`, `hex.rs:98`) exercise round-trip empirically; this bundle proves
  the per-step **totality and no-panic / no-overflow** facts those tests rely on.
- **Per-call totality, not full-function path coverage.** The `unsat` lemmas cover the
  arithmetic/indexing primitives (`decode_byte`, the encoder accumulator and index, the
  hex nibble arms). The surrounding `decode_with_table` / `encode_with_alphabet`
  length/padding bookkeeping (`base64.rs:99-145,156-213`) is straight-line slicing whose
  panic-freedom follows from these primitives plus `Vec::with_capacity` bounds; that
  composition is argued, not separately SMT-discharged here.
- **Standard alphabet modeled; URL-safe noted.** `encoder_output_ascii` encodes the
  concrete `STANDARD_ALPHABET`. The `URL_SAFE_ALPHABET` differs only at indices 62/63
  (`'-'`=45, `'_'`=95), both `< 128`, so the same ASCII conclusion holds for it; the
  file documents this but models the standard set that `encode()`/`encode_no_pad()` use.
