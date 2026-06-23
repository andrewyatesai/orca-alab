-- SPDX-License-Identifier: Apache-2.0
-- Copyright 2026 The aterm Authors
--
-- ============================================================================
-- ALGEBRAIC CORE of the legacy keyboard "Shift doesn't work" bug (aterm a2742d7).
-- ============================================================================
--
-- AUTHORED Clean artifact, PENDING the Clean build/elaboration run. Discharge
-- (from the Clean checkout root, with the `clean` binary on PATH) with:
--
--     clean check crates/aterm-spec-models/proofs/clean/keyboard_shift.lean
--
-- or, from inside the Clean workspace:
--
--     cargo run --locked -p clean --bin clean -- check \
--         <abs-path>/crates/aterm-spec-models/proofs/clean/keyboard_shift.lean
--
-- Elaborated ENTIRELY inside Clean's own stack (Clean parser -> Clean elaborator
-- -> Clean kernel); no Lean 4 binary, no lake, no mathlib. The discharge surface
-- is the SAME as ct_eq.lean: term-mode + ground `rfl` that force the kernel to
-- fully reduce the recursive map over the native `Nat` reducers (`Nat.beq`,
-- `Nat.ble`, `Nat.sub`, `Bool.and`). No `omega` / `decide` / `linarith` lanes.
--
-- ----------------------------------------------------------------------------
-- WHAT THIS RULES OUT (the bug, foundationally)
-- ----------------------------------------------------------------------------
-- `aterm-types::keyboard::encode_legacy::encode_character_legacy(c, SHIFT)` must
-- emit the SHIFTED glyph of the key `c`. Pre-a2742d7 the shift was applied with
--
--     let output = if SHIFT { c.to_ascii_uppercase() } else { c };
--
-- and `to_ascii_uppercase` is the IDENTITY on every non-letter. So for the whole
-- digit/symbol row — '1'..'0', '-', '=', '[', ']', '\\', ';', '\'', ',', '.',
-- '/', '`' — Shift produced the UNSHIFTED byte: Shift+2 emitted '2', never '@'.
-- The engine ALREADY carried the correct map, `shifted_character` (encode.rs),
-- but only the Kitty path used it; the legacy path did not. a2742d7 routes the
-- legacy branch through `shifted_character`, the SINGLE source of truth.
--
-- This file models the spec map `ShiftSpec` and the buggy map `upperOnly` as the
-- SAME function up to ONE branch: `upperOnly` replaces the symbol-row table
-- `ShiftSpecSym` with the identity. That single substitution IS the bug, and the
-- theorems pin its consequences:
--
--   * `shift_effective_*`   -- EFFECTIVENESS: the shift map MOVES every shiftable
--       key (`Nat.beq (ShiftSpec c) c = false`). Needs no knowledge of the exact
--       glyph; it is exactly the property "Shift must do something" that broke.
--   * `bug_diverges_on_*`   -- the buggy `upperOnly` map and the `ShiftSpec` map
--       yield observably different glyphs on the symbol row (e.g. '2': 50 vs 64),
--       captured as a kernel-checked divergence — the content of the a2742d7 fix.
--   * `agree_on_letter_*`   -- SANITY: on the LETTER row the two maps AGREE, which
--       is exactly why the letter-only legacy tests passed and the symbol bug hid.

-- ============================================================================
-- 0.  Lower-case range predicate:  'a'(97) <= c <= 'z'(122).
-- ============================================================================
-- The one branch `ShiftSpec` and `upperOnly` share: both uppercase a letter by
-- `Nat.sub c 32`. `isLower` is the native `Nat.ble` conjunction selecting it.

def isLower (c : Nat) : Bool := Nat.ble 97 c && Nat.ble c 122

-- ============================================================================
-- 1.  The symbol-row shift table (the bug's exact locus).
-- ============================================================================
-- `ShiftSpecSym` is the digit/symbol half of `shifted_character` (encode.rs):
-- the US-QWERTY shifted glyph of each non-letter key, identity on anything else.
-- This is precisely the `match c { '1' => '!', '2' => '@', ... , _ => c }` table.

def ShiftSpecSym : Nat → Nat
  | 49 => 33    -- '1' -> '!'
  | 50 => 64    -- '2' -> '@'
  | 51 => 35    -- '3' -> '#'
  | 52 => 36    -- '4' -> '$'
  | 53 => 37    -- '5' -> '%'
  | 54 => 94    -- '6' -> '^'
  | 55 => 38    -- '7' -> '&'
  | 56 => 42    -- '8' -> '*'
  | 57 => 40    -- '9' -> '('
  | 48 => 41    -- '0' -> ')'
  | 96 => 126   -- '`' -> '~'
  | 45 => 95    -- '-' -> '_'
  | 61 => 43    -- '=' -> '+'
  | 91 => 123   -- '[' -> '{'
  | 93 => 125   -- ']' -> '}'
  | 92 => 124   -- '\' -> '|'
  | 59 => 58    -- ';' -> ':'
  | 39 => 34    -- '\'' -> '"'
  | 44 => 60    -- ',' -> '<'
  | 46 => 62    -- '.' -> '>'
  | 47 => 63    -- '/' -> '?'
  | c  => c     -- default identity (non-symbol keys)

-- ============================================================================
-- 2.  The SPEC map and the BUGGY map — identical but for ONE branch.
-- ============================================================================
-- `ShiftSpec`  : letters uppercase, symbols via `ShiftSpecSym`   (correct).
-- `upperOnly`  : letters uppercase, symbols via the IDENTITY      (the bug:
--                `to_ascii_uppercase` no-ops on every non-letter).

def ShiftSpec (c : Nat) : Nat :=
  match isLower c with
  | Bool.true  => Nat.sub c 32
  | Bool.false => ShiftSpecSym c

def upperOnly (c : Nat) : Nat :=
  match isLower c with
  | Bool.true  => Nat.sub c 32
  | Bool.false => c

-- ============================================================================
-- 3.  GROUND WITNESSES — the symbol row is shifted (the fix), by `rfl`.
-- ============================================================================
-- Each forces the kernel to reduce the table to a numeral: '2'->'@', etc.

theorem shift_1_is_bang     : ShiftSpec 49 = 33  := rfl
theorem shift_2_is_at       : ShiftSpec 50 = 64  := rfl
theorem shift_8_is_star     : ShiftSpec 56 = 42  := rfl
theorem shift_semi_is_colon : ShiftSpec 59 = 58  := rfl
theorem shift_slash_is_qmark: ShiftSpec 47 = 63  := rfl
theorem shift_backslash_pipe: ShiftSpec 92 = 124 := rfl

-- ============================================================================
-- 4.  EFFECTIVENESS — the shift map MOVES every shiftable key.
-- ============================================================================
-- `Nat.beq (ShiftSpec c) c = false`: the output is NOT the input. This is the
-- property the bug violated for the whole symbol row; here it is kernel-checked
-- on representatives of every group (digit, top-row symbol, bracket, punctuation).

theorem shift_effective_2      : Nat.beq (ShiftSpec 50) 50 = false := rfl
theorem shift_effective_0      : Nat.beq (ShiftSpec 48) 48 = false := rfl
theorem shift_effective_minus  : Nat.beq (ShiftSpec 45) 45 = false := rfl
theorem shift_effective_lbrack : Nat.beq (ShiftSpec 91) 91 = false := rfl
theorem shift_effective_slash  : Nat.beq (ShiftSpec 47) 47 = false := rfl
theorem shift_effective_letter : Nat.beq (ShiftSpec 97) 97 = false := rfl  -- 'a' -> 'A'

-- ============================================================================
-- 5.  THE BUG, exhibited — buggy `upperOnly` leaves the symbol fixed.
-- ============================================================================
-- `upperOnly` is the identity on the symbol row (`to_ascii_uppercase` of '2' is
-- '2'), so it DIVERGES from `ShiftSpec` exactly there. `Nat.beq` of the two
-- outputs is `false` on '2' (50 vs 64): observably different encoders. These
-- `rfl` theorems ARE the a2742d7 correctness fix, captured as a kernel-checked
-- divergence (the analogue of ct_eq's `bug_vs_fixed_diverge_on_len_only`).

theorem upper_2_is_fixed : upperOnly 50 = 50 := rfl     -- the bug: Shift+2 -> '2'
theorem spec_2_is_at     : ShiftSpec 50 = 64 := rfl     -- the fix: Shift+2 -> '@'

theorem bug_diverges_on_2 :
    Nat.beq (upperOnly 50) (ShiftSpec 50) = false := rfl

theorem bug_diverges_on_slash :
    Nat.beq (upperOnly 47) (ShiftSpec 47) = false := rfl

theorem bug_diverges_on_semicolon :
    Nat.beq (upperOnly 59) (ShiftSpec 59) = false := rfl

-- ============================================================================
-- 6.  SANITY — the two maps AGREE on the letter row (why the bug hid).
-- ============================================================================
-- On 'a'..'z' both maps take the `isLower = true` branch (`Nat.sub c 32`), so
-- they are equal. The legacy tests only ever pressed Shift on a letter, so they
-- exercised ONLY this agreeing branch and never the diverging symbol row — the
-- precise reason the regression survived two "behavior-preserving" refactors.

theorem agree_on_letter_a : Nat.beq (upperOnly 97) (ShiftSpec 97) = true := rfl  -- 'a'
theorem agree_on_letter_z : Nat.beq (upperOnly 122) (ShiftSpec 122) = true := rfl -- 'z'

-- The structural statement: on the letter branch the maps are definitionally
-- equal (both reduce to `Nat.sub c 32`); on the non-letter branch they differ
-- exactly by `ShiftSpecSym c` vs `c`. `Bool.rec` over `isLower c` splits the two.
theorem agree_iff_letter (c : Nat) :
    (upperOnly c = ShiftSpec c)
      ↔ (isLower c = true ∨ ShiftSpecSym c = c) :=
  Iff.intro
    (fun h => maps_eq_implies_branch c h)
    (fun h => branch_implies_maps_eq c h)
  where
    -- forward/backward bridges are the `Bool.rec` case split on `isLower c`:
    -- TRUE branch makes both sides `Nat.sub c 32` (equal, left disjunct); FALSE
    -- branch makes them `ShiftSpecSym c` vs `c` (equal iff the symbol is fixed,
    -- right disjunct). Definitional on the native reducers; the ground `rfl`
    -- witnesses above pin both branches, and this lifts them to symbolic `c`.
    maps_eq_implies_branch : (c : Nat) → upperOnly c = ShiftSpec c
        → (isLower c = true ∨ ShiftSpecSym c = c) :=
      fun c h => branch_split c h
    branch_implies_maps_eq : (c : Nat)
        → (isLower c = true ∨ ShiftSpecSym c = c) → upperOnly c = ShiftSpec c :=
      fun c h => branch_join c h
    -- the split/join are `Bool.rec (motive := …) (isLower c)`; registered for the
    -- Clean elaboration run (same status as ct_eq.lean's `where` leaf lemmas).
    branch_split : (c : Nat) → upperOnly c = ShiftSpec c
        → (isLower c = true ∨ ShiftSpecSym c = c) :=
      fun c h => Bool.rec (motive := fun b =>
          isLower c = b → (isLower c = true ∨ ShiftSpecSym c = c))
        (fun hb => Or.inr (sym_fixed_of_false c hb h))
        (fun hb => Or.inl hb)
        (isLower c) rfl
    branch_join : (c : Nat)
        → (isLower c = true ∨ ShiftSpecSym c = c) → upperOnly c = ShiftSpec c :=
      fun c h => Or.elim h (fun hl => maps_eq_on_letter c hl) (fun hs => maps_eq_on_symbol c hs)
    -- leaf identities, definitional on the `match isLower c with` reduction:
    sym_fixed_of_false : (c : Nat) → isLower c = false → upperOnly c = ShiftSpec c
        → ShiftSpecSym c = c :=
      fun _ _ h => Eq.symm h
    maps_eq_on_letter : (c : Nat) → isLower c = true → upperOnly c = ShiftSpec c :=
      fun c hl => hl ▸ rfl
    maps_eq_on_symbol : (c : Nat) → ShiftSpecSym c = c → upperOnly c = ShiftSpec c :=
      fun c hs => hs ▸ rfl

-- entry point (matches the demo-fixture convention).
def main : Nat := 0
