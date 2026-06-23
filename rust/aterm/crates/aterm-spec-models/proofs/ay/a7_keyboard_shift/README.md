<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The aterm Authors -->

# A7 — legacy keyboard shift: effective + total into printable ASCII (discharged by `ay`)

Re-checkable certificate for initiative **A7**: the legacy (non-Kitty) keyboard
**Shift** map is **effective** — holding Shift changes *every* shiftable key — and
**total into printable ASCII**. This is the property the **"Shift doesn't work"**
regression (`a2742d7`) violated: the legacy encoder applied Shift with
`c.to_ascii_uppercase()`, which is the **identity on every non-letter**, so
`Shift+2` emitted `'2'` instead of `'@'` and the entire digit/symbol row was
unreachable.

**Discharged by `ay` (the Trust SAT/SMT solver) on hand-encoded SMT-LIB2 — Trust,
not kani.** Run `bash verify.sh` → exits 0 iff all four obligations get their
expected verdict.

## Faithful source

`crates/aterm-types/src/keyboard/encode.rs` and `…/encode_legacy.rs`.

- `encode.rs:390-420` `shifted_character(c, SHIFT)`: the **single** US-QWERTY shift
  map — `'a'..='z' => to_ascii_uppercase`, `'1'=>'!' '2'=>'@' … '/'=>'?'`. Both the
  legacy and the Kitty `REPORT_ALTERNATE_KEYS` paths use it after `a2742d7`.
- `encode_legacy.rs:43` bare-Shift branch: `super::shifted_character(c, modifiers).unwrap_or(c)`
  — the byte emitted for `Shift+c` in legacy mode. Pre-`a2742d7` this read
  `if SHIFT { c.to_ascii_uppercase() } else { c }`, the modeled **bug** (`Upper`).
- `encode_legacy.rs:21-30` Alt+Shift branch routes through the same map (ESC + glyph).

## What is proved

| File | Verdict | Obligation |
|---|---|---|
| `shift_is_effective.smt2` | **unsat** | for every shiftable key `c`, `ShiftSpec(c) != c` — Shift never returns the unshifted byte (**the property the bug broke**; needs no knowledge of the exact glyph) |
| `shift_glyph_is_printable.smt2` | **unsat** | for every shiftable key `c`, `0x20 <= ShiftSpec(c) <= 0x7e` — the shifted glyph is always one printable ASCII byte (totality / well-formed single-byte output) |
| `catches_uppercase_bug_sat.smt2` | **sat** | the buggy `to_ascii_uppercase` map disagrees with the spec on some shiftable key (`Upper(c) != ShiftSpec(c)`, e.g. `c='2'`: `Upper='2'`, `Spec='@'`) — **the historical bug is caught; the spec is non-vacuous** |
| `shift_effective_nonvacuity_sat.smt2` | **sat** | a shiftable key is genuinely moved (`ShiftSpec(c) != c` is satisfiable) — the effectiveness `unsat` is not vacuous over an empty domain |

**Prove-and-catch non-vacuity** (per `assert_proves_and_catches`): the two `unsat`
lemmas are paired with `sat` controls. `shift_effective_nonvacuity_sat` witnesses
that the shiftable domain is non-empty and the map really moves a key (so the
effectiveness lemma is not vacuously true), and `catches_uppercase_bug_sat`
catches the *actual historical defect* — `to_ascii_uppercase` is provably not the
shift map — certifying the spec would have **rejected** the buggy encoder.

## Honest property statement

For **every** shiftable key (the lower-case letter range and the US-QWERTY symbol
row), holding Shift produces a **different, printable** byte than the unshifted
key. The pre-`a2742d7` `to_ascii_uppercase` legacy branch is **caught** as a
non-conforming map (it is the identity on the symbol row, violating effectiveness
on 21 keys). Widths are byte-faithful: a key/glyph is an 8-bit BV, the
letter-range uppercase is `bvsub c 0x20`, and the symbol table is a pure-BV
`ite`-chain (no array theory), so every obligation stays in `QF_BV` and solves in
milliseconds.

This is the SMT twin of the `clean` algebraic proof
`crates/aterm-spec-models/proofs/clean/keyboard_shift.lean` (which exhibits the
buggy-vs-fixed divergence on `'2'` by ground `rfl`) and the always-on Rust
refinement test `crates/aterm-types/src/keyboard/encode_tests.rs`
(`shifted_character_refines_independent_spec`, `legacy_shift_changes_every_shiftable_key`,
`uppercase_only_shift_is_caught_by_the_spec`), which runs in plain `cargo test`
with no Trust toolchain present.

## Honest scope — what this does NOT prove

- **US-QWERTY only.** `ShiftSpec` is the US-ANSI layout map, the same hardcoded
  table the engine ships (the engine has no live OS keyboard layout). On non-US
  layouts (AZERTY, Dvorak, …) the shifted glyph of a symbol key differs, and a
  fully correct fix would require either a layout-aware engine or feeding the
  GUI's resolved `text` for bare printable keys in legacy mode — blocked today by
  the mode-blind convergence-seam design. This bundle proves the **US** map is
  effective and total; it does **not** claim layout independence.
- **Single-byte legacy branch only.** This models the bare-Shift (and, by the same
  map, Alt+Shift) character branch of `encode_character_legacy`. The Ctrl branch
  (checked first, `encode_legacy.rs:10`) and the Kitty CSI-u path (`encode.rs:64`,
  which already used `shifted_character`) are out of scope here; Ctrl precedence is
  unaffected because it short-circuits before the Shift branch.
- **Map fidelity, not layout truth.** The proof certifies the encoder's map equals
  the modeled US-QWERTY table and that the table is effective/printable. That the
  modeled table is itself the *correct* US-ANSI layout is established by inspection
  against the physical layout (and the always-on Rust test's independent table),
  not by an external layout oracle.
