# `Range::contains` validation guards: lever works + sound, but a ZERO gap-mover on orca-core

**Status (2026-06-14, build #55):** the contains lever is implemented, validated, and **sound**, but
its orca-core gap impact is **structurally zero** — confirmed, not a plumbing gap. Held uncommitted
pending the deeper capabilities below. This note records the full diagnosis.

## What the lever does (trust-mir-extract `rewrite_range_contains_calls`)

Rewrites `_r = RangeInclusive::contains(const_range, &x)` (and exclusive `Range`) into
`_t1 = x >= L; _t2 = x <= U; _r = _t1 & _t2; goto`, reading `L`/`U` from the promoted const's valtree
(ref-peeled — the const is `&Range<Idx>`). Fail-closed on any non-unique / non-constant / non-integer
operand, so it can never invent a bound.

**Validated on the exact orca form (build #55):**
- `validate_add`: `if !(0..=9999).contains(&year) || !(1..=12).contains(&month) { return None } Some(year+month)` → **PROVED** (lever fires on the negated-`||` chain; the Int/ADD lane recovers both bounds).
- Soundness gates, every build: `no_guard_add`, `no_guard_mul`, `wrong_var_mul` (guard binds the wrong var) → **all FAIL** ✓. The lever leaks no bound it didn't establish.

## Why it moves the orca-core gap by ZERO (structural)

Every contains-validated value in orca-core flows into a multiply whose **other** operand is unbounded:

`orca-core/src/hosted_review_queue.rs` (parse_iso8601-equivalent), after the
`!(0..=9999).contains(&year) || …` guard:
```rust
let days = days_from_civil(year, month, day);           // days: function return, UNBOUNDED to caller
Some((days * 86_400 + hour * 3_600 + minute * 60 + second) * 1_000 + millis)
//    ^^^^^^^^^^^^^^ unbounded `days` -> the whole sum's overflow obligation FAILS
//                                                                          ^^^^^^ loop accumulator
for (index, digit) in frac.chars().take(3).enumerate() {
    millis += i64::from(digit.to_digit(10)?) * 10i64.pow(2 - index as u32); // millis: loop accumulator, UNBOUNDED
}
```
- `tailnet_address.rs:21` `(64..=127).contains(&octets[1])` — pure boolean return, **no arithmetic obligation**.
- `git_cquoted_path.rs:41` `('0'..='7').contains(&c)` then octal `acc = acc*8 + …` — `acc` is a **loop accumulator**, unbounded.

So `hour`/`minute`/`second` being guard-bounded never matters: they are always summed/combined with an
unbounded `days` (function return) or `millis`/`acc` (loop accumulator). The lever is **1 of 4**
capabilities this code needs:
1. `#[trust::ensures]` postcondition on `days_from_civil` to bound `days`.
2. Call-site contract checking to *use* that postcondition (the caller-side dual of the landed
   `#[requires]`-as-assumption; today only the callee side exists).
3. The contains bounds on `hour`/`minute`/`second` (this lever).
4. Loop-invariant reasoning to bound `millis` / `acc`.

## The real (sound) false-FAIL, and its precise fix — held, owner-adjacent

Independently of orca, `(L..=U).contains(&x); x * <const>` is a genuine false-FAIL:

| function | result | why |
|---|---|---|
| `hand_const_mul`: `if h<0 \|\| h>23 {return} h*3600` | **PROVED** | hand-written guard splits into two single-fact switches; the BV mul dominating-guard lane already handles each |
| `const_mul`: `if !(0..=23).contains(&h) {return} h*3600` | **FAILS** (false-FAIL) | contains-rewrite emits a single-block `BitAnd` |

Root cause (pinned): `trust-vcgen/src/guards.rs::latest_same_block_bool_definition` resolves a bool local
only when its defining rvalue is a **comparison** (`Eq|Ne|Lt|Le|Gt|Ge`); for `dest = BitAnd(_t1,_t2)`
it returns `None`, so `guard_to_formula` yields an **opaque `Var(dest)`** instead of `And(Ge,Le)`. The
BV mul guard lane (`v2_bv_mul_dominating_guard_constraints` → `v2_linear_var_const_fact`, which has no
`And` arm) then never sees the bound. (The *Int* lane proves `validate_add` because it gets the full
definitional chain `dest=_t1&_t2, _t1=Ge, _t2=Le` from the block-definitions builder at `guards.rs:647`
— a path the BV mul lane doesn't use.)

**Precise fix (designed, NOT applied):** teach `latest_same_block_bool_definition` to unfold
`BitAnd`/`BitOr` of bools into `And`/`Or` of their recursively-resolved operands (semantically faithful:
`dest==true` ⟺ `_t1 && _t2`). Then `guard_to_formula` returns `And(Ge,Le)` and a one-line conjunct
flatten in `v2_bv_mul_dominating_guard_constraints` recovers both bounds.

**Why held (not applied autonomously):**
- It moves the **orca-core metric by zero** (structural reason above) — so it is not the authorized
  "gap-mover," and landing it is a capability decision for the owner.
- It edits **shared guard machinery** near the owner's documented revert (same function's precondition
  `And`-flatten was reverted for a days_from_civil 23h spin; gated on a per-obligation solve timeout).
  The owner's spin gate is now arguably met (execute_direct timeout + typed-CHC watchdog landed), and
  `var × const` cannot spin, but `var × var` bounded mul (`bounded_var_mul`) is a solver-strength wall —
  it returns **UNKNOWN and terminates** (watchdog confirmed, no hang), never proves.

## Bottom line

The lever is correct and sound and proves range-validated `+`/`-`. It is **not** an orca-core
gap-mover: orca-core's range-validated arithmetic is universally `bounded ⊕ unbounded`, so the frontier
is inter-procedural contracts (`ensures` + call-site checking) and loop invariants, not the contains
capability. The `var × const` false-FAIL fix is real and queued here for an owner-coordinated change.
