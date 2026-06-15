// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0

//! `aterm-bits` — minimal `Pod`/`Zeroable` traits and byte-casting helpers.
//!
//! Zero-external-dependency replacement for the subset of [`bytemuck`] used by
//! aterm (issue #7894). Covers the GPU/FFI transmute paths in `aterm-gpu` and
//! `aterm-memory`:
//!
//! | bytemuck API                    | aterm-bits equivalent           |
//! |---------------------------------|---------------------------------|
//! | `bytemuck::Pod` (trait)         | [`Pod`]                         |
//! | `bytemuck::Zeroable` (trait)    | [`Zeroable`]                    |
//! | `bytemuck::cast_slice`          | [`cast_slice`]                  |
//! | `bytemuck::try_cast_slice`      | [`try_cast_slice`]              |
//! | `bytemuck::cast_slice_mut`      | [`cast_slice_mut`]              |
//! | `bytemuck::bytes_of`            | [`bytes_of`]                    |
//! | `bytemuck::bytes_of_mut`        | [`bytes_of_mut`]                |
//! | `bytemuck::from_bytes`          | [`from_bytes`]                  |
//! | `bytemuck::from_bytes_mut`      | [`from_bytes_mut`]              |
//! | `bytemuck::pod_read_unaligned`  | [`pod_read_unaligned`]          |
//!
//! # Deriving
//!
//! Unlike bytemuck this crate ships no proc-macro derive. All `Pod`/`Zeroable`
//! implementations are written manually as `unsafe impl` blocks at the call
//! site. This keeps the crate surface tiny and the safety contract explicit.
//!
//! # Safety model
//!
//! Both [`Pod`] and [`Zeroable`] are `unsafe trait`s. The implementor
//! guarantees:
//!
//! * `Zeroable`: the all-zero bit pattern is a valid value of the type.
//! * `Pod`: every bit pattern of `size_of::<Self>()` bytes is a valid value,
//!   the type is [`Copy`], contains no padding with uninitialized bytes, and
//!   contains no references, pointers, or `!Send`/`!Sync` handles.
//!
//! Implementors are responsible for verifying these properties; this crate
//! only provides the machinery to transmute once they hold.

#![forbid(unsafe_op_in_unsafe_fn)]
#![deny(missing_docs)]
#![deny(clippy::undocumented_unsafe_blocks)]
#![deny(rust_2018_idioms)]

use core::mem::{align_of, size_of, size_of_val};
use core::slice;

// ---------------------------------------------------------------------------
// Traits
// ---------------------------------------------------------------------------

/// Types whose all-zero bit pattern is a valid value.
///
/// # Safety
///
/// Implementors must guarantee that `[0u8; size_of::<Self>()]` interpreted
/// as `Self` is a sound, fully-initialized value. Enums must cover the 0
/// discriminant; structs must have every field implement `Zeroable`;
/// references, `NonZero*`, and any type with niche optimizations that
/// exclude 0 are not `Zeroable`.
pub unsafe trait Zeroable: Sized {
    /// Returns the all-zero bit pattern for this type.
    #[inline]
    #[must_use]
    fn zeroed() -> Self {
        // SAFETY: by the `Zeroable` trait contract, the all-zero bit pattern
        // is a valid value of `Self`.
        unsafe { core::mem::zeroed() }
    }
}

/// Plain-old-data: types for which every bit pattern of
/// `size_of::<Self>()` bytes is a valid value.
///
/// This enables sound transmutes between `&[Self]` and `&[u8]` (and vice
/// versa) and lets us read values directly from raw memory / network / GPU
/// buffers without going through a parser.
///
/// # Safety
///
/// Implementors must satisfy *every* bullet:
///
/// 1. The type is `Copy`.
/// 2. The type is `'static` — no lifetimes.
/// 3. The type contains no references, raw pointers, function pointers,
///    `NonZero*`, or any niche-optimized discriminants.
/// 4. The type has a well-defined layout (`#[repr(C)]`, `#[repr(transparent)]`,
///    or a primitive).
/// 5. There are **no padding bytes** with indeterminate contents. If the
///    struct has padding, the declared fields must cover every byte (use
///    explicit `_padding: [u8; N]` fields).
/// 6. Every field of the type is itself `Pod`.
pub unsafe trait Pod: Zeroable + Copy + 'static {}

// ---------------------------------------------------------------------------
// Primitive impls
// ---------------------------------------------------------------------------

macro_rules! impl_pod_zeroable_primitive {
    ($($t:ty),* $(,)?) => {
        $(
            // SAFETY: `$t` is a built-in primitive integer/float. All bit
            // patterns of the appropriate size are valid values of the type,
            // and the all-zero bit pattern corresponds to the value `0` /
            // `0.0` which is always valid.
            unsafe impl Zeroable for $t {}
            // SAFETY: see above — `$t` is `Copy + 'static`, has a canonical
            // layout, and every bit pattern is a valid value.
            unsafe impl Pod for $t {}
        )*
    };
}

impl_pod_zeroable_primitive!(u8, u16, u32, u64, u128, usize);
impl_pod_zeroable_primitive!(i8, i16, i32, i64, i128, isize);
impl_pod_zeroable_primitive!(f32, f64);

// SAFETY: `()` is a ZST; there are no bytes, so trivially every bit pattern
// (i.e. the empty one) is valid and all-zero is sound.
unsafe impl Zeroable for () {}
// SAFETY: see above.
unsafe impl Pod for () {}

// Arrays of Pod are Pod (arrays of Zeroable are Zeroable) for all lengths.
// SAFETY: `[T; N]` has layout `T` repeated `N` times with no extra padding
// (stable guarantee). If every `T` byte pattern is valid, so is every
// `[T; N]` byte pattern. Same argument for `Zeroable`.
unsafe impl<T: Zeroable, const N: usize> Zeroable for [T; N] {}
// SAFETY: see above; additionally `[T; N]: Copy` when `T: Copy`, and `[T; N]`
// is `'static` when `T: 'static`.
unsafe impl<T: Pod, const N: usize> Pod for [T; N] {}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Reason a byte cast failed at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PodCastError {
    /// The target alignment is stricter than the source alignment.
    TargetAlignmentGreaterAndInputNotAligned,
    /// The source byte length is not an integer multiple of the target size.
    OutputSliceWouldHaveSlop,
    /// The source slice is not large enough to cover one target value.
    SizeMismatch,
}

impl core::fmt::Display for PodCastError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let msg = match self {
            Self::TargetAlignmentGreaterAndInputNotAligned => {
                "target alignment is greater than the input slice's alignment"
            }
            Self::OutputSliceWouldHaveSlop => {
                "input byte length is not an integer multiple of the output element size"
            }
            Self::SizeMismatch => "input slice is smaller than one output value",
        };
        f.write_str(msg)
    }
}

impl core::error::Error for PodCastError {}

// ---------------------------------------------------------------------------
// Slice casting
// ---------------------------------------------------------------------------

/// Cast `&[A]` to `&[B]` where both are `Pod`.
///
/// # Panics
///
/// * If `size_of::<B>() == 0` but `size_of::<A>() != 0` (or vice versa).
/// * If the source byte length is not an integer multiple of
///   `size_of::<B>()`.
/// * If the source slice's starting address is not aligned to
///   `align_of::<B>()`.
///
/// Use [`try_cast_slice`] to handle these cases as errors instead.
#[inline]
#[must_use]
pub fn cast_slice<A: Pod, B: Pod>(src: &[A]) -> &[B] {
    match try_cast_slice(src) {
        Ok(out) => out,
        Err(err) => panic!("cast_slice failed: {err}"),
    }
}

/// Cast `&mut [A]` to `&mut [B]` where both are `Pod`.
///
/// # Panics
///
/// See [`cast_slice`].
#[inline]
#[must_use]
pub fn cast_slice_mut<A: Pod, B: Pod>(src: &mut [A]) -> &mut [B] {
    match try_cast_slice_mut(src) {
        Ok(out) => out,
        Err(err) => panic!("cast_slice_mut failed: {err}"),
    }
}

/// Fallible [`cast_slice`] — returns [`PodCastError`] instead of panicking.
#[inline]
pub fn try_cast_slice<A: Pod, B: Pod>(src: &[A]) -> Result<&[B], PodCastError> {
    let (ptr, out_len) = compute_cast::<A, B>(src.as_ptr().cast::<u8>(), size_of_val(src))?;
    // SAFETY: `compute_cast` verified:
    //   * `ptr` has alignment `>= align_of::<B>()` (same address as `src`).
    //   * `out_len * size_of::<B>() == size_of_val(src)` bytes of initialized
    //     memory are reachable from `ptr`.
    //   * Both `A` and `B` are `Pod`, so every bit pattern of
    //     `size_of::<B>() * out_len` bytes is a valid `[B; out_len]`.
    //   * The returned lifetime is tied to `&[A]`, so `B` cannot outlive it.
    // Therefore `from_raw_parts` produces a sound `&[B]`.
    Ok(unsafe { slice::from_raw_parts(ptr.cast::<B>(), out_len) })
}

/// Fallible [`cast_slice_mut`] — returns [`PodCastError`] instead of panicking.
#[inline]
pub fn try_cast_slice_mut<A: Pod, B: Pod>(src: &mut [A]) -> Result<&mut [B], PodCastError> {
    let byte_len = size_of_val(&*src);
    let (ptr, out_len) =
        compute_cast::<A, B>(src.as_mut_ptr().cast::<u8>().cast_const(), byte_len)?;
    // SAFETY: identical reasoning to `try_cast_slice`, plus: the source is
    // `&mut [A]` so we uniquely own the memory and can hand out `&mut [B]`.
    // The ptr was derived from a `*mut` so casting back to `*mut B` is fine.
    Ok(unsafe { slice::from_raw_parts_mut(ptr.cast::<B>().cast_mut(), out_len) })
}

/// Shared math for `try_cast_slice` / `try_cast_slice_mut`.
///
/// Returns `(ptr_cast_back_to_u8, output_element_count)` on success.
#[inline]
fn compute_cast<A, B>(ptr: *const u8, byte_len: usize) -> Result<(*const u8, usize), PodCastError> {
    let a_size = size_of::<A>();
    let b_size = size_of::<B>();

    // Handle the ZST corner cases cleanly — either both sides are ZST (OK,
    // empty slice) or neither is, otherwise the cast is nonsensical.
    if a_size == 0 || b_size == 0 {
        if a_size == b_size {
            // Empty slice of B — alignment still must be non-zero, which it
            // always is for valid types.
            return Ok((ptr, 0));
        }
        return Err(PodCastError::SizeMismatch);
    }

    // Alignment check — source pointer must already be aligned for B,
    // otherwise the cast produces an UB-adjacent misaligned reference.
    if !(ptr as usize).is_multiple_of(align_of::<B>()) {
        return Err(PodCastError::TargetAlignmentGreaterAndInputNotAligned);
    }

    // Size check — every byte must be covered by an integer number of Bs.
    if !byte_len.is_multiple_of(b_size) {
        return Err(PodCastError::OutputSliceWouldHaveSlop);
    }

    Ok((ptr, byte_len / b_size))
}

// ---------------------------------------------------------------------------
// Single-value views
// ---------------------------------------------------------------------------

/// View a single `Pod` value as its raw bytes.
#[inline]
#[must_use]
pub fn bytes_of<T: Pod>(value: &T) -> &[u8] {
    // SAFETY: `T: Pod` guarantees `size_of::<T>()` bytes of initialized
    // memory with a well-defined layout. The returned slice borrows from
    // `value`, so its lifetime is bounded correctly.
    unsafe { slice::from_raw_parts((value as *const T).cast::<u8>(), size_of::<T>()) }
}

/// View a single `Pod` value's raw bytes mutably.
#[inline]
#[must_use]
pub fn bytes_of_mut<T: Pod>(value: &mut T) -> &mut [u8] {
    // SAFETY: same argument as `bytes_of`. The unique `&mut T` means we can
    // produce a unique `&mut [u8]` over the same bytes — `Pod` types have
    // no invalid bit patterns, so arbitrary writes through the byte slice
    // cannot produce an invalid `T`.
    unsafe { slice::from_raw_parts_mut((value as *mut T).cast::<u8>(), size_of::<T>()) }
}

/// Interpret `bytes` as a single `Pod` value.
///
/// # Panics
///
/// Panics if `bytes.len() != size_of::<T>()` or if `bytes` is not aligned to
/// `align_of::<T>()`. Use [`try_from_bytes`] to handle these cases.
#[inline]
#[must_use]
pub fn from_bytes<T: Pod>(bytes: &[u8]) -> &T {
    match try_from_bytes(bytes) {
        Ok(v) => v,
        Err(err) => panic!("from_bytes failed: {err}"),
    }
}

/// Interpret `bytes` as a single `Pod` value mutably.
///
/// # Panics
///
/// See [`from_bytes`].
#[inline]
#[must_use]
pub fn from_bytes_mut<T: Pod>(bytes: &mut [u8]) -> &mut T {
    match try_from_bytes_mut(bytes) {
        Ok(v) => v,
        Err(err) => panic!("from_bytes_mut failed: {err}"),
    }
}

/// Fallible [`from_bytes`].
#[inline]
pub fn try_from_bytes<T: Pod>(bytes: &[u8]) -> Result<&T, PodCastError> {
    if bytes.len() != size_of::<T>() {
        return Err(PodCastError::SizeMismatch);
    }
    if !(bytes.as_ptr() as usize).is_multiple_of(align_of::<T>()) {
        return Err(PodCastError::TargetAlignmentGreaterAndInputNotAligned);
    }
    // SAFETY: length check above ensures `size_of::<T>()` initialized bytes
    // are reachable from the pointer; alignment check ensures the cast
    // yields a correctly aligned reference; `T: Pod` ensures any bit
    // pattern is a valid `T`.
    Ok(unsafe { &*bytes.as_ptr().cast::<T>() })
}

/// Fallible [`from_bytes_mut`].
#[inline]
pub fn try_from_bytes_mut<T: Pod>(bytes: &mut [u8]) -> Result<&mut T, PodCastError> {
    if bytes.len() != size_of::<T>() {
        return Err(PodCastError::SizeMismatch);
    }
    if !(bytes.as_ptr() as usize).is_multiple_of(align_of::<T>()) {
        return Err(PodCastError::TargetAlignmentGreaterAndInputNotAligned);
    }
    // SAFETY: same argument as `try_from_bytes`, plus `&mut [u8]` gives us
    // unique access so handing out `&mut T` is sound.
    Ok(unsafe { &mut *bytes.as_mut_ptr().cast::<T>() })
}

/// Read a `Pod` value from a potentially unaligned byte slice.
///
/// Unlike [`from_bytes`], this copies through `ptr::read_unaligned`, so no
/// alignment is required. The resulting value is owned.
///
/// # Panics
///
/// Panics if `bytes.len() < size_of::<T>()`.
#[inline]
#[must_use]
pub fn pod_read_unaligned<T: Pod>(bytes: &[u8]) -> T {
    assert!(
        bytes.len() >= size_of::<T>(),
        "pod_read_unaligned: slice shorter than size_of::<T>()",
    );
    // SAFETY: length check above ensures `size_of::<T>()` initialized bytes
    // are reachable. `ptr::read_unaligned` tolerates any alignment. `T: Pod`
    // means the resulting bit pattern is a valid `T`.
    unsafe { core::ptr::read_unaligned(bytes.as_ptr().cast::<T>()) }
}

// ---------------------------------------------------------------------------
// Kani proofs (non-trivial: verify cast_slice invariants & bytes-of roundtrip)
// ---------------------------------------------------------------------------

#[cfg(kani)]
mod kani_proofs {
    use super::*;

    /// Cast `&[u32]` to `&[u8]` and back: length × 4 must hold, and every
    /// source byte must be recoverable.
    #[kani::proof]
    fn cast_slice_u32_to_u8_preserves_bytes() {
        let src: [u32; 2] = [kani::any(), kani::any()];
        let bytes: &[u8] = cast_slice(&src[..]);
        kani::assert(bytes.len() == 8, "2×u32 must cast to 8 bytes");
        // The bytes must match the native-endian representation.
        for (i, word) in src.iter().enumerate() {
            let word_bytes = word.to_ne_bytes();
            for (j, expected) in word_bytes.iter().enumerate() {
                kani::assert(
                    bytes[i * 4 + j] == *expected,
                    "cast_slice must not permute bytes",
                );
            }
        }
    }

    /// `bytes_of` -> `pod_read_unaligned` must be a round-trip for any u64.
    #[kani::proof]
    fn bytes_of_pod_read_roundtrip_u64() {
        let original: u64 = kani::any();
        let bytes = bytes_of(&original);
        kani::assert(bytes.len() == 8, "u64 is 8 bytes");
        let recovered: u64 = pod_read_unaligned(bytes);
        kani::assert(
            recovered == original,
            "bytes_of -> pod_read_unaligned must round-trip",
        );
    }

    /// `try_cast_slice::<u8, u32>` must reject misaligned slices without UB.
    #[kani::proof]
    fn try_cast_slice_rejects_misaligned() {
        let buf: [u8; 16] = [0; 16];
        // Offset 1 is guaranteed misaligned for u32 (align=4).
        let slice = &buf[1..9];
        let result: Result<&[u32], _> = try_cast_slice(slice);
        kani::assert(
            matches!(
                result,
                Err(PodCastError::TargetAlignmentGreaterAndInputNotAligned)
            ),
            "misaligned cast must return alignment error, not UB",
        );
    }

    /// `try_cast_slice` must reject size mismatches without UB.
    #[kani::proof]
    fn try_cast_slice_rejects_slop() {
        // 6 bytes aligned to u32 — 6 % 4 = 2 leftover bytes.
        let buf: [u32; 2] = [0, 0];
        let bytes: &[u8] = cast_slice(&buf[..]);
        let truncated = &bytes[..6];
        let result: Result<&[u32], _> = try_cast_slice(truncated);
        kani::assert(
            matches!(result, Err(PodCastError::OutputSliceWouldHaveSlop)),
            "slop-bearing cast must return slop error, not UB",
        );
    }

    /// `Zeroable::zeroed()` must produce the all-zero bit pattern.
    #[kani::proof]
    fn zeroable_produces_all_zero_bytes() {
        let v = u64::zeroed();
        kani::assert(v == 0, "u64::zeroed must be 0");
        let a: [u32; 4] = <[u32; 4]>::zeroed();
        for w in a {
            kani::assert(w == 0, "[u32; 4]::zeroed must be all zeros");
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[repr(C)]
    #[derive(Copy, Clone, Debug, PartialEq)]
    struct Vec4 {
        x: f32,
        y: f32,
        z: f32,
        w: f32,
    }

    // SAFETY: Vec4 is `#[repr(C)]` with 4×f32 fields, no padding, no niches.
    unsafe impl Zeroable for Vec4 {}
    // SAFETY: see Zeroable impl; additionally `Copy + 'static`.
    unsafe impl Pod for Vec4 {}

    #[test]
    fn zeroable_primitives() {
        assert_eq!(u32::zeroed(), 0);
        assert_eq!(f32::zeroed(), 0.0);
        assert_eq!(<[u8; 3]>::zeroed(), [0, 0, 0]);
        assert_eq!(
            Vec4::zeroed(),
            Vec4 {
                x: 0.0,
                y: 0.0,
                z: 0.0,
                w: 0.0
            }
        );
    }

    #[test]
    fn cast_slice_u32_to_u8_native_endian() {
        let src: [u32; 2] = [0xDEAD_BEEF, 0x1234_5678];
        let bytes: &[u8] = cast_slice(&src);
        assert_eq!(bytes.len(), 8);
        let mut recovered = [0u32; 2];
        recovered[0] = u32::from_ne_bytes(bytes[0..4].try_into().unwrap());
        recovered[1] = u32::from_ne_bytes(bytes[4..8].try_into().unwrap());
        assert_eq!(recovered, src);
    }

    #[test]
    fn cast_slice_roundtrip_f32_u8() {
        let src: [f32; 3] = [1.0, -2.5, core::f32::consts::PI];
        let bytes: &[u8] = cast_slice(&src);
        assert_eq!(bytes.len(), 12);
        let back: &[f32] = try_cast_slice(bytes).expect("aligned buffer");
        assert_eq!(back, &src);
    }

    #[test]
    fn cast_slice_mut_modifies_source() {
        let mut src: [u32; 2] = [0, 0];
        {
            let bytes: &mut [u8] = cast_slice_mut(&mut src);
            bytes[0] = 0x01;
        }
        assert_eq!(src[0] & 0xff, 0x01);
    }

    #[test]
    fn try_cast_slice_misaligned_returns_err() {
        // Build an `[u8; 16]` (align 1) and slice at offset 1 — guaranteed
        // misaligned for u32 (align 4).
        let buf = [0u8; 16];
        let slice = &buf[1..9];
        let result: Result<&[u32], _> = try_cast_slice(slice);
        assert_eq!(
            result,
            Err(PodCastError::TargetAlignmentGreaterAndInputNotAligned)
        );
    }

    #[test]
    fn try_cast_slice_slop_returns_err() {
        let buf: [u32; 2] = [0, 0];
        let bytes: &[u8] = cast_slice(&buf);
        let truncated = &bytes[..6];
        let result: Result<&[u32], _> = try_cast_slice(truncated);
        assert_eq!(result, Err(PodCastError::OutputSliceWouldHaveSlop));
    }

    #[test]
    fn bytes_of_single_value() {
        let v: u32 = 0x0102_0304;
        let bytes = bytes_of(&v);
        assert_eq!(bytes.len(), 4);
        assert_eq!(u32::from_ne_bytes(bytes.try_into().unwrap()), v);
    }

    #[test]
    fn bytes_of_mut_single_value() {
        let mut v: u32 = 0;
        {
            let bytes = bytes_of_mut(&mut v);
            bytes.copy_from_slice(&0x0102_0304u32.to_ne_bytes());
        }
        assert_eq!(v, 0x0102_0304);
    }

    #[test]
    fn from_bytes_reads_vec4() {
        let original = Vec4 {
            x: 1.0,
            y: 2.0,
            z: 3.0,
            w: 4.0,
        };
        let bytes = bytes_of(&original);
        let recovered: &Vec4 = from_bytes(bytes);
        assert_eq!(*recovered, original);
    }

    #[test]
    fn from_bytes_mut_updates_vec4() {
        let mut v = Vec4 {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            w: 0.0,
        };
        {
            let bytes = bytes_of_mut(&mut v);
            let other = Vec4 {
                x: 9.0,
                y: 8.0,
                z: 7.0,
                w: 6.0,
            };
            bytes.copy_from_slice(bytes_of(&other));
        }
        assert_eq!(
            v,
            Vec4 {
                x: 9.0,
                y: 8.0,
                z: 7.0,
                w: 6.0
            }
        );
    }

    #[test]
    fn pod_read_unaligned_handles_offset() {
        // Build a buffer where the u32 we want starts at offset 1 — using
        // `from_bytes` here would panic for misalignment, but
        // `pod_read_unaligned` must succeed.
        let mut buf = [0u8; 5];
        buf[1..5].copy_from_slice(&0xCAFE_BABEu32.to_ne_bytes());
        let v: u32 = pod_read_unaligned(&buf[1..5]);
        assert_eq!(v, 0xCAFE_BABE);
    }

    #[test]
    #[should_panic(expected = "pod_read_unaligned: slice shorter")]
    fn pod_read_unaligned_panics_on_short_slice() {
        let buf = [0u8; 3];
        let _: u32 = pod_read_unaligned(&buf);
    }

    #[test]
    #[should_panic(expected = "cast_slice failed")]
    fn cast_slice_panics_on_slop() {
        let buf: [u32; 2] = [0, 0];
        let bytes: &[u8] = cast_slice(&buf);
        let truncated = &bytes[..6];
        let _: &[u32] = cast_slice(truncated);
    }

    #[test]
    fn empty_slice_cast() {
        let src: [u32; 0] = [];
        let bytes: &[u8] = cast_slice(&src);
        assert!(bytes.is_empty());
        let back: &[u32] = try_cast_slice(bytes).expect("empty slice");
        assert!(back.is_empty());
    }

    #[test]
    fn error_display_formats() {
        // Exercise the Display impl so it doesn't rot silently.
        let err = PodCastError::TargetAlignmentGreaterAndInputNotAligned;
        assert!(err.to_string().contains("alignment"));
        let err = PodCastError::OutputSliceWouldHaveSlop;
        assert!(err.to_string().contains("multiple"));
        let err = PodCastError::SizeMismatch;
        assert!(err.to_string().contains("smaller"));
    }
}
