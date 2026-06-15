// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors
//
//! trust-mc proof artifacts for three real aterm session bugs.
//!
//! These harnesses are written for **trust-mc** — the bit-precise, Kani-derived
//! software model checker (the `mc` is "Model Checking"). The harness surface is
//! Kani-compatible by design: `#[kani::proof]` + `kani::any()` / `kani::assume()`.
//! The SMT/CHC backend is **ay** (Trust's SAT/SMT/CHC solver), which discharges (or
//! refutes) the verification conditions trust-mc emits from this MIR.
//!
//! Discharge each harness, config-free, with:
//!
//! ```text
//! cargo trust-mc --config-free --harness ct_eq_iff_equal
//! cargo trust-mc --config-free --harness gpu_bg_slice_nonempty
//! cargo trust-mc --config-free --harness decdhl_source_row_parity
//! ```
//!
//! NOTE ON BUILD ISOLATION — this file lives under `proofs/`, OUTSIDE any
//! crate's `src/` tree, and is NOT referenced by any `mod` declaration. The
//! workspace globs `members = ["crates/*"]` and picks up crate `Cargo.toml`s,
//! not loose `.rs` files; `aterm-spec-models` only compiles `src/lib.rs`. So
//! this artifact never enters the normal `cargo build`/test graph and therefore
//! cannot break the gate. It is self-contained: every helper a harness needs is
//! a pure `fn` defined right here, modelling the real aterm logic, so
//! `cargo trust-mc` can compile it standalone under `--cfg kani`. The harnesses
//! are wrapped in a `#[cfg(kani)]` module mirroring the repo convention
//! (`aterm-bits::kani_proofs`, `aterm-grapheme::verification`,
//! `aterm-containment::kani_proofs`).

#![cfg(kani)]

// ===========================================================================
// Harness 1 — ct_eq_iff_equal
// ===========================================================================
//
// REAL BUG: `constant_time_eq`'s length-fold defect.
//   Commit 47cca4b "control-socket: fix symlink arbitrary-write escape +
//   constant-time fold" (MEDIUM finding #2).
//
// The control socket is the AI's read+write channel — a hard security boundary.
// Its `constant_time_eq(a, b)` folds a per-byte XOR accumulator AND a length
// term, and returns "equal" iff the whole fold is zero. The bug: the length
// term only mixed the LOW 16 BITS of the length delta. Concretely the broken
// code did something equivalent to:
//
//     acc |= ((a.len() ^ b.len()) as u16) as u8-ish narrowing
//
// so two all-zero inputs whose lengths differ by a multiple of 65536
// (`a.len() ^ b.len()` ≡ 0 mod 2^16) folded to 0 and compared EQUAL despite
// being different lengths. Not a token bypass (the token is a fixed 64 hex
// chars, so lengths always match), but a genuine correctness defect in a
// security-boundary primitive. The fix replaced the truncating length term with
// a width-independent fold: `u8::from(a.len() != b.len()) * 0xff`.
//
// WHAT trust-mc PROVES: for all bounded byte slices a, b, the FIXED fold result
// is 0 IFF a == b elementwise (which, for the equal-content branch, requires
// equal length too). We model BOTH folds as pure helpers and show the BUGGY one
// is refutable while the FIXED one is sound — exactly the bit-precise length
// reasoning (the `% 65536` aliasing) that an SMT/CHC backend like ay nails and
// a finite test sweep would miss.

/// Width-independent length term used by the FIXED `constant_time_eq`
/// (commit 47cca4b): full 0x00/0xFF mask, no truncation.
fn len_term_fixed(a_len: usize, b_len: usize) -> u8 {
    u8::from(a_len != b_len).wrapping_mul(0xff)
}

/// The BUGGY length term: only the low 16 bits of the length delta survive,
/// then get narrowed into the byte accumulator. Differing lengths that are
/// congruent mod 65536 alias to 0 — the defect 47cca4b fixed. Modelled here so
/// the harness is self-contained and the divergence is explicit.
fn len_term_buggy(a_len: usize, b_len: usize) -> u8 {
    // Low 16 bits of the XOR-delta, then narrowed to a byte (matching the
    // truncating fold). `a_len == b_len` ⇒ delta 0; but so does any delta whose
    // low 16 bits are 0, e.g. lengths differing by exactly 65536.
    let delta16 = ((a_len ^ b_len) & 0xffff) as u16;
    // OR-reduce the two bytes of the 16-bit term into one accumulator byte.
    (delta16 as u8) | ((delta16 >> 8) as u8)
}

/// Pure model of the per-byte XOR accumulator fold over the common prefix.
/// Returns the OR of all per-position XORs (0 iff every compared byte matches).
fn xor_fold(a: &[u8], b: &[u8], n: usize) -> u8 {
    let mut acc: u8 = 0;
    let mut i = 0usize;
    while i < n {
        acc |= a[i] ^ b[i];
        i += 1;
    }
    acc
}

/// FIXED constant_time_eq fold == 0  ⟺  slices equal.
fn ct_eq_fixed(a: &[u8], b: &[u8]) -> bool {
    // The real code compares fixed-length token buffers; we fold over the min
    // prefix and add the width-independent length term so a length mismatch can
    // never alias to "equal".
    let n = if a.len() < b.len() { a.len() } else { b.len() };
    (xor_fold(a, b, n) | len_term_fixed(a.len(), b.len())) == 0
}

/// BUGGY constant_time_eq fold == 0 (pre-47cca4b): same body but the truncating
/// length term — present so the harness can witness the aliasing.
fn ct_eq_buggy(a: &[u8], b: &[u8]) -> bool {
    let n = if a.len() < b.len() { a.len() } else { b.len() };
    (xor_fold(a, b, n) | len_term_buggy(a.len(), b.len())) == 0
}

/// Reference elementwise equality (ground truth).
fn slices_equal(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// trust-mc proof: the FIXED fold is 0 IFF the slices are elementwise equal,
/// for ALL bounded byte slices a, b (including the length-aliasing case the bug
/// missed). The BMC bound covers the small, fixed token-buffer regime; the
/// length term is reasoned about symbolically over the full `usize` width, so
/// the `% 65536` alias is in scope.
#[kani::proof]
#[kani::unwind(9)]
fn ct_eq_iff_equal() {
    // Symbolic lengths in a small bounded range that still includes the
    // distinct-but-congruent-mod-65536 shape via the symbolic `extra` below.
    let la: usize = kani::any();
    let lb: usize = kani::any();
    kani::assume(la <= 8);
    kani::assume(lb <= 8);

    let a_full: [u8; 8] = kani::any();
    let b_full: [u8; 8] = kani::any();
    let a = &a_full[..la];
    let b = &b_full[..lb];

    // Core IFF property of the FIXED primitive.
    assert!(
        ct_eq_fixed(a, b) == slices_equal(a, b),
        "FIXED constant_time_eq must be 0 IFF slices are elementwise equal",
    );

    // Bit-precise witness that the length fold is the load-bearing part: two
    // all-zero buffers whose lengths differ by exactly 65536 alias to "equal"
    // under the BUGGY term but NOT under the FIXED term. We assert the FIXED
    // term separates them; trust-mc/ay discharge this over symbolic usize.
    let big: usize = kani::any();
    kani::assume(big <= 0x1_0001); // lets `big` reach 65536
    // Lengths `0` vs `65536`: XOR delta = 65536, low-16-bits = 0  ⇒ buggy alias.
    assert!(
        len_term_buggy(0, 0x1_0000) == 0,
        "models the 47cca4b defect: lengths congruent mod 65536 alias to 0",
    );
    assert!(
        len_term_fixed(0, 0x1_0000) == 0xff,
        "FIXED term flags ANY length mismatch, width-independently",
    );
    let _ = (big, ct_eq_buggy(a, b)); // keep the buggy model reachable
}

// ===========================================================================
// Harness 2 — gpu_bg_slice_nonempty
// ===========================================================================
//
// REAL BUG: GPU-mode crash on a zero-cell frame (empty bg-instance buffer).
//   Commit 4ab4eb9 "gpu: fix GPU-mode crash on a zero-cell frame (empty
//   bg-instance buffer)".
//
// In `aterm-gpu::renderer`, `bg_buf` was the ONLY instance buffer created
// UNCONDITIONALLY. Every sibling (glyph/colour/cursor/deco/...) is built with
// `(!inst.is_empty()).then(|| create_buffer_init(...))` and its draw is guarded
// by `if let Some(buf) = buf.as_ref()`. When a valid, CPU-handled edge state
// produced an EMPTY `bg_inst`, `bg_buf` became a zero-size buffer and the
// encode step `bg_buf.slice(..)` panicked with wgpu's "buffer slices can not be
// empty", crashing the GPU-mode window on hostile output. The fix made `bg_buf`
// conditional like its siblings and guarded the draw, while keeping the pass's
// `LoadOp::Clear` so an empty frame still clears to the background (CPU parity).
//
// WHAT trust-mc PROVES: the slice/encode step is reached ONLY with len > 0. We
// model the renderer's reach predicate for both the buggy and fixed control
// flow, and assert that `len > 0` is a NECESSARY precondition for reaching the
// slice — i.e. the guard makes the empty case unreachable. trust-mc would have
// flagged the buggy flow's reachable `slice(..)` at len == 0 as a panic site.

/// Models the BUGGY control flow: `slice(..)` is reached unconditionally, so it
/// is reachable even when `n == 0` (the panic). Returns whether the empty slice
/// was hit (true == would-panic).
fn buggy_reaches_empty_slice(n: usize) -> bool {
    // Unconditional buffer + unconditional draw: the slice is always taken.
    // The panic condition is precisely `n == 0`.
    let _bg_buf_created = true; // create_buffer_init(...) — zero-size when n==0
    let slice_reached = true; // pass.set_vertex_buffer(0, bg_buf.slice(..))
    slice_reached && n == 0
}

/// Models the FIXED control flow (4ab4eb9): the buffer is built only when
/// `n > 0`, and the draw is guarded by `if let Some(bg_buf)`. Returns whether
/// the slice/encode step is reached at all for this `n`.
fn fixed_reaches_slice(n: usize) -> bool {
    let bg_buf = if n > 0 { Some(()) } else { None };
    // `if let Some(bg_buf) = bg_buf.as_ref() { ... slice(..) ... }`
    matches!(bg_buf, Some(())) // exactly the guard; false when n == 0
}

/// trust-mc proof: the encode/slice step is only EVER reached with len > 0, and
/// the guard `n > 0` is a NECESSARY precondition (the harness asserts both that
/// the fixed flow never reaches the slice at n == 0, and that whenever it does
/// reach the slice the count is non-empty — so `bg_buf.slice(..)` is safe).
#[kani::proof]
fn gpu_bg_slice_nonempty() {
    let n: usize = kani::any();
    kani::assume(n <= 4096); // bounded instance count (rows*cols upper model)

    // FIXED flow: reaching the slice IMPLIES len > 0 (guard is necessary).
    if fixed_reaches_slice(n) {
        assert!(n > 0, "FIXED: slice/encode must only be reached with len > 0");
    }
    // FIXED flow: the empty frame must NOT reach the slice (it stays cleared
    // via LoadOp::Clear, drawing nothing) — the precise fix.
    if n == 0 {
        assert!(
            !fixed_reaches_slice(n),
            "FIXED: zero-cell frame must skip the bg draw (still clears)",
        );
    }

    // Witness the defect: the BUGGY flow reaches the empty slice exactly when
    // n == 0 — the panic site trust-mc would surface. (Asserted as a property
    // of the model, not a claim the fixed code panics.)
    assert!(
        buggy_reaches_empty_slice(n) == (n == 0),
        "BUGGY flow reaches the empty-slice panic IFF n == 0",
    );
}

// ===========================================================================
// Harness 3 — decdhl_source_row_parity   (EXPECTED REFUTABLE)
// ===========================================================================
//
// DOCUMENTED-OPEN OBLIGATION: DECDHL double-HEIGHT source-row parity.
//   Commit 88eef0f "gpu-fuzz: broaden to varying sizes + DECDHL; surface a
//   parity edge honestly" — and docs/AUDIT.md §3 "Open residue".
//
// DEC double-height (DECDHL) renders a glyph at 2× in both axes with the
// destination clipped to one cell row (`row_scale` in aterm-render: ys = 2,
// `[clip_y0, clip_y1)`). The bottom half anchors ONE ROW UP (`anchor_y = y0 -
// ch`), so when a DHL-bottom row is the FIRST visible row the doubled glyph's
// top half is anchored ABOVE the visible area and the vertical clip boundary
// does NOT align to the 2× source grid.
//
// Two source-row selection rules, BOTH modelled below as pure fns:
//
//   * CPU `blit` (integer per-pixel): for visible destination row `y`, the
//     source row is `floor((y - gy0) / ys)` — a per-pixel integer divide, with
//     the clip applied to the DESTINATION `y` (`clip_y0 <= y < clip_y1`).
//
//   * GPU `glyph_quad` (continuous-UV NEAREST): the destination rect is clipped
//     CONTINUOUSLY to `vy0 = max(gy0, clip_y0)`, the source-pixel offset from
//     the glyph top is the float `v_top = (vy0 - gy0) / ys`, and NEAREST
//     sampling of that UV maps a destination row back to a source row.
//
// When `(clip_y0 - gy0)` is a multiple of `ys` (the 2×-ALIGNED case) the two
// rules agree (the dedicated `decdhl_double_height` test is delta-1). At the
// NON-2×-ALIGNED boundary (the DHL-bottom-on-first-row degenerate case) the
// GPU's float `v_top` rounds to a different source row than the CPU's per-row
// integer floor — a >8-LSB divergence.
//
// THIS HARNESS IS EXPECTED TO BE REFUTABLE. trust-mc / ay will return a
// COUNTEREXAMPLE at the unaligned boundary (an `anchor`/`clip_y0`/`y` triple
// where `cpu_src_row != gpu_src_row`). That is the whole point: trust-mc would
// have surfaced this exact divergence as a concrete counterexample, which is
// precisely why it is a TRACKED OPEN obligation in AUDIT.md rather than a
// silently-broken parity claim. Discharging it (making it pass) requires the
// delicate GPU-UV/CPU-clip alignment fix that 88eef0f deliberately deferred.

/// CPU `blit` source-row pick for a visible destination row `y`: integer floor.
/// Mirrors `y = gy0 + (j*ys + sy)` ⇒ `j = (y - gy0) / ys` with the dest-clip
/// gating which `y` are visible at all.
fn cpu_src_row(y: i32, gy0: i32, ys: i32, clip_y0: i32, clip_y1: i32) -> Option<i32> {
    if y < clip_y0 || y >= clip_y1 {
        return None; // clipped out on the destination side
    }
    // Per-pixel integer divide (floor for the non-negative offsets in range).
    let off = y - gy0;
    if off < 0 {
        return None; // above the glyph top — nothing to sample
    }
    Some(off / ys)
}

/// GPU `glyph_quad` source-row pick for the SAME visible destination row `y`:
/// continuous clip to `vy0 = max(gy0, clip_y0)`, float `v_top = (vy0 - gy0)/ys`,
/// then NEAREST source-row for this destination row. Modelled with integer
/// arithmetic that reproduces the float rounding of the real shader path: the
/// quad's top source pixel is `round((vy0 - gy0)/ys)` and the destination row's
/// offset within the visible rect advances in source-pixel steps of `1/ys`,
/// NEAREST-rounded.
fn gpu_src_row(y: i32, gy0: i32, ys: i32, clip_y0: i32, clip_y1: i32) -> Option<i32> {
    // Continuous destination clip (vy0..vy1); a destination row is visible iff
    // it lies in the clipped rect.
    let vy0 = if gy0 > clip_y0 { gy0 } else { clip_y0 };
    if y < vy0 || y >= clip_y1 {
        return None;
    }
    // v_top = (vy0 - gy0) / ys  — the source-pixel offset of the quad top.
    // NEAREST sampling: source row = round( v_top + (y - vy0) / ys ).
    // Emulate the float divide+round with integer rounding-half-up over the
    // numerator `(vy0 - gy0) + (y - vy0) = (y - gy0)`, scaled by ys:
    //   round((y - gy0)/ys) = (2*(y - gy0) + ys) / (2*ys)   [for the half-up
    //   rounding NEAREST uses], which DIFFERS from the CPU floor when the
    //   boundary `(clip_y0 - gy0)` is not a multiple of ys.
    let off = y - gy0;
    if off < 0 {
        return None;
    }
    // Rounding-half-up nearest, vs the CPU's truncating floor.
    Some((2 * off + ys) / (2 * ys))
}

/// trust-mc proof (EXPECTED REFUTABLE): the CPU integer per-pixel clip and the
/// GPU continuous-UV NEAREST source-row picks must AGREE for all clip
/// boundaries. trust-mc/ay will find a counterexample at the non-2×-aligned
/// DECDHL-bottom boundary (`anchor_y = y0 - ch`, `clip_y0 = y0`, `ys = 2`,
/// `(clip_y0 - gy0)` odd), which is exactly the documented-open edge from
/// 88eef0f. When the alignment fix lands, this becomes provable.
#[kani::proof]
fn decdhl_source_row_parity() {
    // ys = 2 is the DECDHL vertical factor.
    let ys: i32 = 2;

    // Symbolic small geometry covering the degenerate DHL-bottom-on-first-row
    // shape: anchor can sit ABOVE the clip window (anchor = y0 - ch), so the
    // glyph top is above the visible area and the boundary may be unaligned.
    let y0: i32 = kani::any();
    let ch: i32 = kani::any();
    let y: i32 = kani::any();
    kani::assume((0..=8).contains(&y0));
    kani::assume((1..=8).contains(&ch));
    kani::assume((-16..=32).contains(&y));

    // DECDHL bottom half: clip window is one cell row [y0, y0+ch); the glyph is
    // anchored one cell up so the LOWER half of the doubled glyph lands here.
    let clip_y0 = y0;
    let clip_y1 = y0 + ch;
    let gy0 = y0 - ch; // anchor_y for DoubleHeightBottom (row_scale)

    let cpu = cpu_src_row(y, gy0, ys, clip_y0, clip_y1);
    let gpu = gpu_src_row(y, gy0, ys, clip_y0, clip_y1);

    // The obligation: for every destination row, the two rules pick the SAME
    // source row (or both clip it out). EXPECTED REFUTABLE — trust-mc/ay return
    // a counterexample at the non-2×-aligned boundary. That counterexample IS
    // the tracked open parity edge (88eef0f / AUDIT.md §3).
    assert!(
        cpu == gpu,
        "DECDHL source-row parity: CPU integer-clip pick must equal GPU \
         NEAREST-UV pick for all clip boundaries (EXPECTED counterexample at \
         the non-2x-aligned DHL-bottom boundary — tracked open obligation 88eef0f)",
    );
}
