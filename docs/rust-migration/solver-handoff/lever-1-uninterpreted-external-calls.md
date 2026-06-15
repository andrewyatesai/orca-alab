# #1 LEVER — model external/contract-free calls as uninterpreted (83% of the gap)

_2026-06-15, build #71. Precise root-cause of the residual cross-crate gap._

## The number

After the `&dyn Trait`→OpaqueConst lever (build #71), the cross-crate unknown count is **889**.
Breaking those down by *mechanism* (the `outcome.reason` of each unknown):

| mechanism | count | share |
|---|---|---|
| **native lower-fail: `Call`** | **740** | **83%** |
| native Unsupported (other) | 99 | 11% |
| lower-fail: Drop | 16 | |
| lower-fail: other (unsupported/Rust/Field/SetDiscriminant) | 34 | |

**One mechanism is 83% of the entire remaining gap across all four crates.**

## What it is

The survey runs `trust-full-verifier`, which lowers a function **and its local callees** into a
typed-TrustIr `NativeVerificationBundle`. `trust-ir-bridge::resolve_call` (lower.rs ~1011-1049)
resolves a `Terminator::Call` only if the callee is *present in the bundle*; otherwise it **fails
closed** — `"Call target … is not present in the TrustIr module"` → `"unsupported operation:
Call"` → the WHOLE function's obligations degrade to Unknown (lower.rs:2849-2851: "single-function
lowering fails closed for calls whose callee is not present").

So any function that calls an **external / std** function it can't inline (`Formatter::write_str`,
`debug_struct().field().finish()`, `str::eq`, allocator hooks, …) loses *all* its obligations —
including ones that don't depend on the call at all (e.g. the enum-discriminant `unreachable`,
which DOES prove on its own: a minimal `#[derive(Debug)] enum Color{...}` proves its unreachable
via the level-1 path in <60s, but the full-verifier rejects the function for the `Formatter`
calls before it gets there).

## The fix (sound, but owner-architecture)

Model an unresolvable call to a **contract-free** callee as an *uninterpreted function* instead
of failing closed:
- return value → fresh opaque value (`OpaqueConst`-style),
- `&mut` args (and reachable globals) → **havoc** (could be anything after the call),
- **no** precondition obligation (the callee has no `#[trust::requires]`).

This is the standard modular-verification treatment and is sound IFF the havoc over-approximates
everything the call could change. The **havoc of `&mut` args is the soundness crux** — omit it
and the verifier assumes a mutated value is unchanged → false-PROVE. That is exactly why the
owner currently fails closed: a *correct* uninterpreted-call model needs the native verifier's
memory/aliasing semantics, which is core-architecture territory.

For callees **with** a `#[trust::requires]`, the call must instead emit the precondition as a
caller obligation (already the contract path) — not havoc.

## Why this is not an autonomous change

It is architecturally central (the `NativeVerificationBundle` lowering) and soundness-critical
(the `&mut`/global havoc). A wrong havoc is a false-PROVE — the one hard line. The falsification
gate (`scripts/trust_falsification_gate.sh`) would not necessarily catch a call-aliasing
unsoundness (its mutants are bounds/overflow). So this needs the owner's design of the
uninterpreted-call + havoc semantics, then validation with new aliasing mutants.

## Smaller, possibly-safe sub-levers (for a future pass)

- `Drop` (16): a `Drop` terminator on a contract-free type is a call to `drop_in_place`; same
  uninterpreted treatment, but Drop glue often has no return/`&mut`-escape, so a narrower
  "Drop of a no-op-Drop type → skip" may be soundly bounded.
- The 99 "native Unsupported (other)" — worth re-bucketing after the Call lever; some may be
  the same `&dyn`/cast family already handled.
