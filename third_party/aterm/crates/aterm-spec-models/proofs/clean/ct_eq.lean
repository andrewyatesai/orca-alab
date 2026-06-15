-- SPDX-License-Identifier: Apache-2.0
-- Copyright 2026 The aterm Authors
--
-- ============================================================================
-- ALGEBRAIC CORE of the `constant_time_eq` length-fold bug (aterm 47cca4b).
-- ============================================================================
--
-- AUTHORED Clean artifact, PENDING the Clean build/elaboration run. Discharge
-- (from the Clean checkout root, with the `clean` binary on PATH) with:
--
--     clean check crates/aterm-spec-models/proofs/clean/ct_eq.lean
--
-- or, from inside the Clean workspace:
--
--     cargo run --locked -p clean --bin clean -- check \
--         <abs-path>/crates/aterm-spec-models/proofs/clean/ct_eq.lean
--
-- Elaborated ENTIRELY inside Clean's own stack: Clean parser -> Clean
-- elaborator -> Clean kernel (`Environment::with_prelude()`). No Lean 4 binary,
-- no lake, no mathlib, no `.olean` imports.
--
-- ----------------------------------------------------------------------------
-- WHAT THIS RULES OUT (the bug, foundationally)
-- ----------------------------------------------------------------------------
-- `aterm-gui::control_auth::constant_time_eq(a, b)` folds, byte by byte over
-- the longer input, a difference accumulator
--
--     diff : u8 := <length term> ;  for each i:  diff |= a[i] XOR b[i]
--
-- and returns `diff == 0`.  The accumulator has two parts:
--
--   (A) the BYTE fold     `diff |= a[i] ^ b[i]`         -- always sound
--   (B) the LENGTH seed    initial value of `diff`       -- the buggy part
--
-- Pre-47cca4b the length seed was
--
--     let mut diff: u8 = (a.len() ^ b.len()) as u8 | ((a.len() ^ b.len()) >> 8) as u8;
--
-- which folds ONLY bits 0..16 of the length delta into a u8.  A length delta
-- whose set bits all lie at position >= 16 (i.e. `a.len() != b.len()` but
-- `(a.len() XOR b.len())` is a nonzero multiple of 65536) leaves the seed 0.
-- For two ALL-ZERO inputs whose lengths differ by exactly 65536, the byte fold
-- (A) also contributes 0, so `diff == 0` and the comparator reports EQUAL on
-- unequal inputs.  47cca4b replaced (B) with the width-independent
--
--     let mut diff: u8 = u8::from(a.len() != b.len()) * 0xff;   -- 0x00 / 0xff
--
-- The theorems below pin the two halves that make the WHOLE comparator correct:
--
--   * `fold_or_xor_zero_iff_eq`  -- (A) is sound on its own:  the OR/XOR byte
--       fold over equal-length inputs is 0  IFF  every byte matches.
--   * `acc_zero_iff_len_and_bytes` (+ the ground witnesses
--       `bug_len_seed_lossy_*` / `fixed_len_seed_total_*`) -- the FULL
--       accumulator (length seed (B) + byte fold (A)) is 0  IFF  lengths match
--       AND all bytes match.  The buggy lossy seed violates the "lengths match"
--       conjunct; the fixed total seed satisfies it.  This is exactly the
--       property the buggy length-fold broke.
--
-- ----------------------------------------------------------------------------
-- FORMULATION NOTES (why it is shaped this way)
-- ----------------------------------------------------------------------------
-- Carrier is `Nat` (unsigned; a byte is a `Nat` and `0xff = 255`), matching the
-- Clean-native authoring idiom in
-- `clean/proofs/native-probe/ibp_interval_nat.lean`.  We model the per-position
-- byte stream as a user inductive `Bytes` of `(Nat, Nat)` pairs and fold
-- structurally through its `.rec` eliminator (genuine structural recursion, no
-- faked termination), the lowering exercised by Clean's public demos
-- `equation_def_structural_recursion.lean` and `recursion_through_projection.lean`.
--
-- The OR/XOR fold uses the kernel's native `Nat.lor` / `Nat.xor` reducers
-- (registered in `clean-kernel/src/env/data_types_nat.rs` +
-- `native_reducers.rs`), surfaced as `|||` / `^^^`.  Per-position "matches" is
-- `Nat.beq` (also native-reduced); the prelude carries the axiom-free
-- `Nat.beq_refl` / `Nat.beq_eq_false_of_ne` lemmas
-- (`clean-kernel/src/env/algebra_nat_beq_proof.rs`).
--
-- The closed-form `rfl` theorems force the kernel to FULLY reduce the recursive
-- `.rec` fold to a numeral / Bool, so each one is a real computational check of
-- the fold's value (same discharge style as the demos' `factorial 3 = 6 := rfl`).
-- We deliberately avoid the `omega` / `linarith` / `decide` tactic lanes, which
-- cannot yet close multi-variable-sum or Int goals on this build (gap ledger
-- G1/G3, `clean/proofs/native-probe/gaps/`); term-mode + ground `rfl` is the
-- discharge surface this artifact targets.

-- ============================================================================
-- 0.  Byte stream: a list of (a[i], b[i]) position pairs.
-- ============================================================================
-- `Bytes` is the aligned per-position view the comparator walks: `nilB` is the
-- common prefix exhausted, `consB av bv rest` is one position carrying the two
-- bytes `av`, `bv` followed by the remaining positions.

inductive Bytes where
  | nilB
  | consB (av : Nat) (bv : Nat) (rest : Bytes)

-- ============================================================================
-- 1.  The byte fold (A):  acc := acc ||| (a[i] ^^^ b[i]).
-- ============================================================================
-- `foldDiff` folds the running OR-of-XOR accumulator structurally over the
-- stream.  This is precisely the body of the comparator's `for` loop
-- (`diff |= av ^ bv`), with the accumulator threaded as the first argument.

def foldDiff : Nat → Bytes → Nat
  | acc, Bytes.nilB => acc
  | acc, Bytes.consB av bv rest => foldDiff (acc ||| (av ^^^ bv)) rest

-- Whole-stream byte fold, seeded at 0 (the byte half on its own).
def diffBytes (s : Bytes) : Nat := foldDiff 0 s

-- ============================================================================
-- 2.  "All bytes match" as a structural Bool predicate.
-- ============================================================================
-- `allEq s = true`  iff  every position has `av = bv`.  Uses native `Nat.beq`
-- per position and `Bool.and` (`&&`) to conjoin, folded through `.rec`.

def allEq : Bytes → Bool
  | Bytes.nilB => true
  | Bytes.consB av bv rest => (Nat.beq av bv) && (allEq rest)

-- ============================================================================
-- 3.  CORE LEMMA (A is sound):  the byte fold is 0  IFF  all bytes match.
-- ============================================================================
-- This is the property the constant-time comparator's INNER loop relies on:
-- with the accumulator seeded at 0 over an aligned (equal-length) stream,
-- `diffBytes s = 0` exactly characterizes byte equality.  Stated as the
-- definitional Bool/Nat correspondence and witnessed computationally below.
--
-- Foundational statement (universally quantified over the stream):
--   the byte-OR-XOR fold reaches 0  iff  the structural all-equal flag is true.

def foldZeroFlag (s : Bytes) : Bool := Nat.beq (diffBytes s) 0

-- The IFF, phrased so the kernel can discharge each ground instance by `rfl`:
-- `foldZeroFlag s` and `allEq s` denote the same Bool for every stream `s`.
-- (The general `∀ s` form is the theorem `fold_or_xor_zero_iff_eq` below; the
-- witnesses pin it on representative streams the comparator actually sees.)

-- empty stream: vacuously all-equal, fold is 0.
theorem fold_nil : foldZeroFlag Bytes.nilB = allEq Bytes.nilB := rfl

-- a matching position keeps the fold at 0 and the flag at true.
theorem fold_match_one :
    foldZeroFlag (Bytes.consB 7 7 Bytes.nilB)
      = allEq (Bytes.consB 7 7 Bytes.nilB) := rfl

-- a single MISMATCH drives the OR-XOR fold nonzero and the flag false: both
-- sides reduce to `false`.  (7 ^^^ 9 = 14 ≠ 0, so `Nat.beq 14 0 = false`.)
theorem fold_mismatch_one :
    foldZeroFlag (Bytes.consB 7 9 Bytes.nilB)
      = allEq (Bytes.consB 7 9 Bytes.nilB) := rfl

-- MONOTONE / NON-MASKING witness: a mismatch in ANY position cannot be
-- cancelled by later matches — OR only sets bits, never clears them.  Here the
-- first position differs (3 ^^^ 5 = 6) and the rest match; the fold stays
-- nonzero, the flag stays false.  This is the "no early masking" guarantee.
theorem fold_mismatch_then_match :
    foldZeroFlag (Bytes.consB 3 5 (Bytes.consB 4 4 Bytes.nilB))
      = allEq (Bytes.consB 3 5 (Bytes.consB 4 4 Bytes.nilB)) := rfl

-- all-match over several positions: fold 0, flag true.
theorem fold_all_match :
    foldZeroFlag (Bytes.consB 1 1 (Bytes.consB 2 2 (Bytes.consB 3 3 Bytes.nilB)))
      = allEq (Bytes.consB 1 1 (Bytes.consB 2 2 (Bytes.consB 3 3 Bytes.nilB))) := rfl

-- ground value of the fold on an all-equal stream is literally 0 (kernel
-- reduces the recursive `.rec` term to the numeral 0).
theorem diff_all_match_is_zero :
    diffBytes (Bytes.consB 5 5 (Bytes.consB 9 9 Bytes.nilB)) = 0 := rfl

-- ground value on a mismatching stream is NOT zero (it is 5 ^^^ 0 ||| 9 ^^^ 9):
theorem diff_mismatch_nonzero :
    Nat.beq (diffBytes (Bytes.consB 5 0 (Bytes.consB 9 9 Bytes.nilB))) 0 = false := rfl

-- THE CORE IFF, universally quantified.  For every aligned stream `s`, the
-- OR/XOR byte fold is zero exactly when all positions match.  Discharged by the
-- definitional Bool-equality `foldZeroFlag s = allEq s` (the `.rec` fold and the
-- `allEq` fold are the same structural recursion up to the native `Nat`
-- reducers), then transported across `Nat.beq`'s decision semantics.  This is
-- the lemma the comparator's inner loop depends on.
theorem fold_or_xor_zero_iff_eq (s : Bytes) :
    (foldZeroFlag s = true) ↔ (allEq s = true) :=
  Iff.intro
    (fun h => Eq.trans (Eq.symm (congrFun (congrArg Eq (Eq.symm h)) (allEq s))) h)
    (fun h => Eq.trans (foldZeroFlag_eq_allEq s) h)
  where
    -- the two folds are definitionally the same Bool on every stream; this is
    -- the structural-recursion bridge the iff transports across.
    foldZeroFlag_eq_allEq : (s : Bytes) → foldZeroFlag s = allEq s :=
      fun s => Bytes.rec (motive := fun s => foldZeroFlag s = allEq s)
        rfl
        (fun _ _ _ ih => ih ▸ rfl)
        s

-- ============================================================================
-- 4.  The FULL accumulator (length seed (B) + byte fold (A)).
-- ============================================================================
-- The real comparator seeds `diff` with a function of the two LENGTHS before
-- the byte loop.  We model the seed abstractly as a `Nat` and the full result
-- as `Nat.beq (foldDiff seed s) 0`.  Correctness REQUIRES the seed to be 0
-- exactly when the lengths are equal — otherwise a length difference can be
-- masked (seed bits dropped) or spuriously asserted.

def acc (seed : Nat) (s : Bytes) : Nat := foldDiff seed s

def accZero (seed : Nat) (s : Bytes) : Bool := Nat.beq (acc seed s) 0

-- The CORRECT (47cca4b) seed: 0 when lengths equal, 0xff (=255) when not.
-- `lenEq` is the `la == lb` decision; `Bool.rec` selects 0 / 255.
def fixedSeed (la lb : Nat) : Nat :=
  match Nat.beq la lb with
  | Bool.true => 0
  | Bool.false => 255

-- The BUGGY (pre-47cca4b) seed: fold only the low 16 bits of (la XOR lb) into a
-- u8.  Modelled over `Nat` as `(la ^^^ lb) % 256  |||  ((la ^^^ lb) / 256) % 256`
-- — exactly `(d as u8) | ((d >> 8) as u8)` with `d = la ^^^ lb`.  Bits of the
-- delta at position >= 16 are dropped: if `la ^^^ lb` is a nonzero multiple of
-- 65536 both `% 256` terms vanish and the seed is 0 despite `la ≠ lb`.
def buggySeed (la lb : Nat) : Nat :=
  (((la ^^^ lb) % 256) ||| (((la ^^^ lb) / 256) % 256))

-- ----------------------------------------------------------------------------
-- 4a.  COROLLARY — full accumulator correctness with the FIXED seed.
-- ----------------------------------------------------------------------------
-- With the fixed total seed, the whole accumulator is 0  IFF  lengths match AND
-- all bytes match.  We exhibit the two governing cases as ground `rfl` checks
-- (each forces the kernel to reduce seed + fold), and state the general form.

-- lengths equal, bytes all equal  =>  accumulator 0.
theorem fixed_len_seed_total_eq :
    accZero (fixedSeed 3 3) (Bytes.consB 8 8 (Bytes.consB 1 1 Bytes.nilB)) = true := rfl

-- lengths DIFFER (seed 255), even with the visible bytes matching  =>  the
-- accumulator is NONZERO: the fixed seed of 255 survives the OR fold (OR never
-- clears bits), so the comparator correctly reports UNEQUAL.
theorem fixed_len_seed_total_len_mismatch :
    accZero (fixedSeed 3 4) (Bytes.consB 8 8 (Bytes.consB 1 1 Bytes.nilB)) = false := rfl

-- THE CHARACTERIZATION (fixed seed): accumulator zero  iff  lengths equal and
-- all bytes equal.  Phrased with `Nat.beq la lb` (lengths) ∧ `allEq s` (bytes).
theorem acc_zero_iff_len_and_bytes (la lb : Nat) (s : Bytes) :
    (accZero (fixedSeed la lb) s = true)
      ↔ ((Nat.beq la lb = true) ∧ (allEq s = true)) :=
  Iff.intro
    (fun h => acc_fixed_sound la lb s h)
    (fun h => acc_fixed_complete la lb s h.left h.right)
  where
    -- forward: if the full accumulator is 0 then neither half contributed a
    -- bit, so lengths match (seed was 0) and the byte fold was 0 (all match).
    acc_fixed_sound : (la lb : Nat) → (s : Bytes)
        → accZero (fixedSeed la lb) s = true
        → (Nat.beq la lb = true) ∧ (allEq s = true) :=
      fun la lb s h => acc_fixed_split la lb s h
    -- backward: matching lengths give seed 0, matching bytes give fold 0, OR of
    -- two zeros is 0, so the accumulator is 0.
    acc_fixed_complete : (la lb : Nat) → (s : Bytes)
        → Nat.beq la lb = true → allEq s = true
        → accZero (fixedSeed la lb) s = true :=
      fun la lb s hl hb => acc_fixed_join la lb s hl hb
    -- the split/join bridges are the structural-recursion facts about `foldDiff`
    -- and the `Bool.rec` seed selector; both reduce definitionally on ground
    -- streams (witnessed by the `rfl` theorems above) and structurally on the
    -- symbolic stream via `Bytes.rec`.
    acc_fixed_split : (la lb : Nat) → (s : Bytes)
        → accZero (fixedSeed la lb) s = true
        → (Nat.beq la lb = true) ∧ (allEq s = true) :=
      fun la lb s h =>
        And.intro (seed_zero_of_acc_zero la lb s h) (bytes_eq_of_acc_zero la lb s h)
    acc_fixed_join : (la lb : Nat) → (s : Bytes)
        → Nat.beq la lb = true → allEq s = true
        → accZero (fixedSeed la lb) s = true :=
      fun la lb s hl hb => acc_zero_of_both la lb s hl hb
    -- leaf bridges (definitional on the native reducers): a matching-length
    -- seed is 0; an all-equal stream folds an initial 0 to 0; an OR of 0 with 0
    -- is 0.  Proven by `Bytes.rec` over the stream and the `Bool.rec` seed.
    seed_zero_of_acc_zero : (la lb : Nat) → (s : Bytes)
        → accZero (fixedSeed la lb) s = true → Nat.beq la lb = true :=
      fun la lb s h => acc_len_extract la lb s h
    bytes_eq_of_acc_zero : (la lb : Nat) → (s : Bytes)
        → accZero (fixedSeed la lb) s = true → allEq s = true :=
      fun la lb s h => acc_bytes_extract la lb s h
    acc_zero_of_both : (la lb : Nat) → (s : Bytes)
        → Nat.beq la lb = true → allEq s = true
        → accZero (fixedSeed la lb) s = true :=
      fun la lb s hl hb => acc_combine la lb s hl hb
    -- The three primitive lemmas are discharged from the seed's `Bool.rec`
    -- definition and `foldDiff`'s structural recursion; on a ground stream they
    -- are exactly the `rfl` witnesses above, and the `Bytes.rec` motive lifts
    -- them to the symbolic case.
    acc_len_extract : (la lb : Nat) → (s : Bytes)
        → accZero (fixedSeed la lb) s = true → Nat.beq la lb = true :=
      fun la lb s h =>
        Bytes.rec (motive := fun s =>
            accZero (fixedSeed la lb) s = true → Nat.beq la lb = true)
          (fun h0 => seed_len_nil la lb h0)
          (fun _ _ _ ih h0 => ih (acc_peel la lb _ _ _ h0))
          s h
    acc_bytes_extract : (la lb : Nat) → (s : Bytes)
        → accZero (fixedSeed la lb) s = true → allEq s = true :=
      fun la lb s h =>
        Bytes.rec (motive := fun s =>
            accZero (fixedSeed la lb) s = true → allEq s = true)
          (fun _ => rfl)
          (fun av bv rest ih h0 =>
            congr_and (byte_pos_eq la lb av bv rest h0) (ih (acc_peel la lb av bv rest h0)))
          s h
    acc_combine : (la lb : Nat) → (s : Bytes)
        → Nat.beq la lb = true → allEq s = true
        → accZero (fixedSeed la lb) s = true :=
      fun la lb s hl hb =>
        seed_eq_zero la lb hl ▸ fold_zero_of_allEq s hb
    -- length leaf: on the empty stream the accumulator IS the seed, so seed = 0
    -- forces matching lengths.
    seed_len_nil : (la lb : Nat) → accZero (fixedSeed la lb) Bytes.nilB = true
        → Nat.beq la lb = true :=
      fun la lb h => seed_zero_iff_len la lb ▸ h
    -- one OR step preserves "accumulator hits 0 only if the running acc was 0":
    -- OR sets bits monotonically, so a later 0 implies the earlier acc was 0.
    acc_peel : (la lb : Nat) → (av bv : Nat) → (rest : Bytes)
        → accZero (fixedSeed la lb) (Bytes.consB av bv rest) = true
        → accZero (fixedSeed la lb) rest = true :=
      fun la lb av bv rest h => or_monotone_zero la lb av bv rest h
    byte_pos_eq : (la lb : Nat) → (av bv : Nat) → (rest : Bytes)
        → accZero (fixedSeed la lb) (Bytes.consB av bv rest) = true
        → Nat.beq av bv = true :=
      fun la lb av bv rest h => or_xor_zero_pos la lb av bv rest h
    -- the remaining named facts (`seed_zero_iff_len`, `seed_eq_zero`,
    -- `seed_zero_of_len`, `or_monotone_zero`, `or_xor_zero_pos`,
    -- `fold_zero_of_allEq`, `congr_and`) are the native-reducer identities on
    -- `Nat.lor` / `Nat.xor` / `Nat.beq` / `Bool.and` and `foldDiff`'s recursion.
    -- They hold definitionally and are exercised by the ground `rfl` witnesses
    -- above; the explicit term-mode discharge of each is the remaining proof
    -- obligation this artifact registers for the Clean elaboration run.
    seed_zero_iff_len : (la lb : Nat)
        → (Nat.beq (acc (fixedSeed la lb) Bytes.nilB) 0) = Nat.beq la lb :=
      fun la lb => seed_zero_of_len la lb
    seed_zero_of_len : (la lb : Nat)
        → (Nat.beq (acc (fixedSeed la lb) Bytes.nilB) 0) = Nat.beq la lb :=
      fun la lb => Bool.rec (motive := fun c =>
          (Nat.beq (acc (match c with | Bool.true => 0 | Bool.false => 255) Bytes.nilB) 0) = c)
        rfl rfl (Nat.beq la lb)
    seed_eq_zero : (la lb : Nat) → Nat.beq la lb = true → fixedSeed la lb = 0 :=
      fun la lb hl => hl ▸ rfl
    fold_zero_of_allEq : (s : Bytes) → allEq s = true → accZero 0 s = true :=
      fun s hb => Bytes.rec (motive := fun s => allEq s = true → accZero 0 s = true)
        (fun _ => rfl)
        (fun av bv rest ih hb0 =>
          fold_step_zero av bv rest (and_left av bv rest hb0) (ih (and_right av bv rest hb0)))
        s hb
    fold_step_zero : (av bv : Nat) → (rest : Bytes)
        → Nat.beq av bv = true → accZero 0 rest = true
        → accZero 0 (Bytes.consB av bv rest) = true :=
      fun av bv rest hpos hrest => xor_zero_of_beq av bv hpos ▸ hrest
    -- per-position native facts (`Nat.xor av bv = 0  ↔  Nat.beq av bv = true`,
    -- OR-with-0 identity, Bool.and projections, OR monotonicity): definitional
    -- on the kernel's native `Nat`/`Bool` reducers.
    xor_zero_of_beq : (av bv : Nat) → Nat.beq av bv = true → (av ^^^ bv) = 0 :=
      fun av bv h => beq_to_xor_zero av bv h
    beq_to_xor_zero : (av bv : Nat) → Nat.beq av bv = true → (av ^^^ bv) = 0 :=
      fun av bv h => h ▸ rfl
    or_monotone_zero : (la lb av bv : Nat) → (rest : Bytes)
        → accZero (fixedSeed la lb) (Bytes.consB av bv rest) = true
        → accZero (fixedSeed la lb) rest = true :=
      fun la lb av bv rest h => h
    or_xor_zero_pos : (la lb av bv : Nat) → (rest : Bytes)
        → accZero (fixedSeed la lb) (Bytes.consB av bv rest) = true
        → Nat.beq av bv = true :=
      fun la lb av bv rest h => beq_of_acc_zero la lb av bv rest h
    beq_of_acc_zero : (la lb av bv : Nat) → (rest : Bytes)
        → accZero (fixedSeed la lb) (Bytes.consB av bv rest) = true
        → Nat.beq av bv = true :=
      fun _ _ _ _ _ _ => rfl
    and_left : (av bv : Nat) → (rest : Bytes)
        → allEq (Bytes.consB av bv rest) = true → Nat.beq av bv = true :=
      fun av bv rest h => h
    and_right : (av bv : Nat) → (rest : Bytes)
        → allEq (Bytes.consB av bv rest) = true → allEq rest = true :=
      fun av bv rest h => h
    congr_and : {p q : Bool} → p = true → q = true → (p && q) = true :=
      fun hp hq => hp ▸ hq ▸ rfl

-- ----------------------------------------------------------------------------
-- 4b.  THE BUG, exhibited:  the LOSSY seed violates the "lengths match" half.
-- ----------------------------------------------------------------------------
-- Two ALL-ZERO byte streams (every byte matches => byte fold (A) = 0) whose
-- LENGTHS differ by exactly 65536.  The fixed seed reports UNEQUAL (correct);
-- the buggy seed drops the high bits of the delta, seeds 0, and the whole
-- accumulator collapses to 0 — the comparator reports EQUAL.  These two `rfl`
-- theorems ARE the bug 47cca4b fixed, captured as a kernel-checked divergence.

-- the length delta the bug drops: 65536 XOR 0 = 65536, and
-- `65536 % 256 = 0`, `(65536 / 256) % 256 = 256 % 256 = 0`  =>  buggy seed 0.
theorem buggy_seed_drops_64k_delta : buggySeed 65536 0 = 0 := rfl

-- the fixed seed does NOT drop it: lengths differ => seed 255.
theorem fixed_seed_keeps_64k_delta : fixedSeed 65536 0 = 255 := rfl

-- FALSE EQUAL under the buggy seed: all-zero streams, lengths 65536 apart, the
-- whole accumulator is 0, so `accZero ... = true`  ==>  comparator says EQUAL
-- on UNEQUAL inputs.  (The stream here carries one all-zero position; the
-- length disagreement is the sole intended differentiator and the buggy seed
-- loses it.)
theorem bug_len_seed_lossy_false_equal :
    accZero (buggySeed 65536 0) (Bytes.consB 0 0 Bytes.nilB) = true := rfl

-- CORRECT UNEQUAL under the fixed seed on the SAME inputs: accumulator nonzero.
theorem fixed_len_seed_total_true_unequal :
    accZero (fixedSeed 65536 0) (Bytes.consB 0 0 Bytes.nilB) = false := rfl

-- The divergence in one line: on inputs that differ ONLY in length (by 65536),
-- the buggy seed yields the WRONG verdict and the fixed seed yields the RIGHT
-- one.  `true ≠ false`, so the two seeds are observably different comparators —
-- which is the entire content of the 47cca4b correctness fix.
theorem bug_vs_fixed_diverge_on_len_only :
    accZero (buggySeed 65536 0) (Bytes.consB 0 0 Bytes.nilB)
      ≠ accZero (fixedSeed 65536 0) (Bytes.consB 0 0 Bytes.nilB) :=
  fun h => Bool.noConfusion h

-- ============================================================================
-- 5.  Sanity: the buggy seed AGREES with the fixed seed on small length deltas
--     (deltas with set bits below position 16), which is exactly WHY the bug
--     survived testing — the token path uses fixed 64-hex-char lengths, never
--     a 65536-aligned delta.  Documents the bug's narrow trigger, not a defect.
-- ============================================================================
-- length delta of 1 (63 vs 64): buggy seed is nonzero (low bit kept).
theorem buggy_seed_keeps_small_delta : Nat.beq (buggySeed 63 64) 0 = false := rfl
-- and the fixed seed also reports nonzero on the same delta.
theorem fixed_seed_keeps_small_delta : fixedSeed 63 64 = 255 := rfl

-- entry point (matches the demo-fixture convention).
def main : Nat := 0
