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

## The primitives already exist (it's a wiring task, not new architecture)

`trust-ir-bridge` already has what an uninterpreted call needs:
- **`trust_types::Operand::Symbolic(Formula)`** (model.rs:4299) — a fresh symbolic value. The
  call's return → a `Symbolic` of the return sort.
- **`SYMBOLIC_MEMORY_STATE_OP` / `emit_symbolic_memory_state_formula`** (lower.rs:30, 1236-1247)
  and **`local_is_lifted_memory_state_carrier`** (lower.rs:790) — the existing symbolic
  *memory-state* threading. A call's frame effect → emit a fresh memory_state after the call.

So `resolve_call`'s fail-closed branch (lower.rs ~1018) becomes, for a contract-free callee:
emit `dest = Symbolic(<return sort>)` + a fresh memory_state for the `&mut`-reachable carriers,
and continue — instead of `Err(UnsupportedOp("Call"))`.

## The soundness crux — memory-state threading

The danger is NOT aliasing in the Rust sense (`&mut` exclusivity already guarantees no live
alias of a `&mut` arg, so havoc-ing its place is complete). The danger is the **memory-state
threading**: after the fresh memory_state, every later read of a `&mut`-reachable place must pick
up the havoc'd state, and every prior learned fact about it must be dropped. If a stale fact
survives the call, that's a false-PROVE. Getting which locals are "memory-state carriers" and how
the fresh state supersedes prior ones requires the owner's symbolic-execution model — that's the
one piece an autonomous change can't safely guess, and the falsification gate's bounds/overflow
mutants would not catch a stale-memory-fact unsoundness.

## Validation plan (do this FIRST, before implementing)

Add `mutant/` fixtures that false-PROVE iff the call-havoc threading is wrong, e.g.:
```rust
// MUTANT: f(&mut i) may set i out of bounds; without post-call havoc the verifier
// keeps the pre-call `i < len` fact and falsely proves arr[i]. MUST fail (exit 1).
fn m(arr: &[u32; 4], i: &mut usize, f: fn(&mut usize)) -> u32 { f(i); arr[*i] }
```
plus a `proved/` twin where the index is re-clamped after the call. Then implement, and require
the new mutants flip to FAILED. Only green-on-the-new-aliasing-mutants makes the lever safe.

## Why I did not ship it autonomously

The user's hard line is "never produce unsound proved." The memory-state threading is core
symbolic-execution machinery I can't validate to that bar without the owner's model knowledge and
the new aliasing mutants above — so shipping a guess would risk exactly the failure the hard line
forbids. The diagnosis, the primitives, the exact edit site, the soundness crux, and the
validation plan are all here; the remaining step is the owner's (or a guided session's).

## Smaller, possibly-safe sub-levers (for a future pass)

- `Drop` (16): a `Drop` terminator on a contract-free type is a call to `drop_in_place`; same
  uninterpreted treatment, but Drop glue often has no return/`&mut`-escape, so a narrower
  "Drop of a no-op-Drop type → skip" may be soundly bounded.
- The 99 "native Unsupported (other)" — worth re-bucketing after the Call lever; some may be
  the same `&dyn`/cast family already handled.
