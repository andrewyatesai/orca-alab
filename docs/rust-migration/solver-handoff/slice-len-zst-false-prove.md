# SOUNDNESS BUG: `conjoin_slice_len_bounds` false-PROVEs overflow for ZST-element slices

> **STILL LIVE as of trust origin/main `3e89a47627` (build #46, 2026-06-14 10:19).** Re-confirmed after the
> owner's `model slice-length metadata bounded` trust-mc bump (`42677fe814`): `zst_slice_overflow(s: &[()])
> { s.len()+2 }` still PROVES. The gate fix below has not been integrated; the false-PROVE remains.

**Where:** `crates/trust-vcgen/src/generate.rs:3107` `fn conjoin_slice_len_bounds` (origin/main,
introduced with `b8c461932f "vcgen: prove i+=1 cannot overflow in slice-bounded loops (len <= isize::MAX)"`).
**Severity:** false-PROVE (the cardinal verifier error — a real overflow reported as proved-safe).
**Found by:** the Orca co-evolution loop (2026-06-14), while implementing the same slice-len lever
independently and applying the soundness check that the owner's version is missing.

## The bug

```rust
fn conjoin_slice_len_bounds(formula: Formula) -> Formula {
    // ... for every var ending in "__slice_len":
    bounds.push(Formula::Ge(var, 0));
    bounds.push(Formula::Le(var, isize::MAX));   // <-- UNCONDITIONAL: unsound for ZST elements
}
```

The bound is conjoined for **every** `__slice_len` term, with no element-type check. The doc comment
defends it as sound:

> "A slice's length can never exceed `isize::MAX`: its total byte size is bounded by `isize::MAX` and
> every element occupies >= 1 byte, so the element count is `<= isize::MAX` for any element type."

**The premise "every element occupies >= 1 byte" is false.** A zero-sized type (`()`, `[T; 0]`, an
empty struct, `PhantomData`) occupies 0 bytes. For a ZST element, `size_of::<T>() * len = 0 <= isize::MAX`
holds for **any** `len`, so a `&[()]` length is **not** bounded by `isize::MAX` — it can reach
`usize::MAX`. (`core::slice::from_raw_parts(NonNull::dangling().as_ptr(), usize::MAX)` is explicitly valid
for ZSTs; `[(); usize::MAX]` is a valid type.)

Consequently `s.len() + k` on a `&[()]` **can** overflow (at `len` near `usize::MAX`), and a sound
verifier must REFUTE it — but the unconditional `s__slice_len <= isize::MAX` bound excludes exactly the
models where the overflow occurs, so the obligation is **false-PROVED**.

## Reproduce

```rust
pub fn zst_slice_overflow(s: &[()]) -> usize { s.len() + 2 }   // PROVED on origin/main  <-- UNSOUND
pub fn nonzst_slice_safe(s: &[u8]) -> usize { s.len() + 2 }    // PROVED — genuinely safe (control)
```
`TRUST_VERIFY_SURVEY=1 trustc -Z trust-verify -Z trust-verify-level=1 ... zst_unsound_demo.rs`. The first
function being PROVED is the soundness break. (Probe: `orc/docs/.../solver-handoff/` companion, also
`/tmp/zst_unsound_demo.rs`.)

## The fix (validated)

Gate the bound on a **provably non-ZST** element type. This was implemented and validated end-to-end
against the same lever (build #44): with the gate, `&[char]/&[u8]/&[i64]` `len()+k` still PROVE while
`&[()]` correctly FAILS; the soundness-gate unit test + all 1952 trust-vcgen tests pass.

```rust
/// True only for element types provably non-zero-sized (size >= 1 byte). The
/// slice-len <= isize::MAX bound is sound ONLY for these; ZST/unknown elements
/// MUST be excluded or `slice.len() + k` false-PROVEs.
fn slice_elem_known_non_zst(ty: &Ty) -> bool {
    match ty {
        Ty::Bool | Ty::Int { .. } | Ty::Float { .. } | Ty::Ref { .. }
        | Ty::RawPtr { .. } | Ty::FnPtr { .. } => true,
        Ty::Bv(w) => *w > 0,
        Ty::Array { elem, len } => *len > 0 && slice_elem_known_non_zst(elem),
        _ => false, // Unit, Tuple, Adt (may be ZST), Slice, Closure, Dynamic, ... — exclude
    }
}
```

To apply it, `conjoin_slice_len_bounds` needs `func: &VerifiableFunction` so each `{place}__slice_len`
var can be mapped back to its slice's element type (e.g. scan the slice-typed locals and the
`PtrMetadata`/`slice_len_formula` sites, build the set of non-ZST `__slice_len` var names, and bound only
those). The lower bound `0 <= v` stays unconditional (always sound). **Completeness note:** any
`__slice_len` var whose element type can't be confirmed non-ZST keeps no upper bound — a false-FAIL, never
a false-PROVE — so the gate is fail-closed and the owner's non-ZST loop proofs are preserved as long as the
non-ZST set is built comprehensively over the same sites that produce the vars.

This is the identical soundness class to the LIA-vs-isize::MAX issue: a bound that is "obviously true" for
the common case but false at the type-system edge.
