// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0

//! Fast path scanning for the parser with explicit SIMD intrinsics.
//!
//! ## Performance
//!
//! The ground state fast path finds the next byte that requires state
//! machine processing. With explicit SIMD intrinsics:
//! - AVX2 (x86_64): Processes 32 bytes per iteration
//! - NEON (aarch64): Processes 16 bytes per iteration
//! - Scalar fallback: Uses LLVM auto-vectorization
//!
//! Explicit SIMD provides better throughput than auto-vectorization for
//! the predicate `byte < 0x20 || byte > 0x7E` because we can use optimized
//! SIMD comparisons with saturating arithmetic.
//!
//! ## Special Bytes
//!
//! Non-printable bytes that exit the printable-ASCII fast path
//! (`find_non_printable`):
//! - C0 controls: 0x00-0x1F (including ESC at 0x1B)
//! - DEL: 0x7F
//! - High bytes: >= 0x80 (includes C1 controls and bytes >= 0xA0)

// =============================================================================
// x86_64 SIMD (AVX2)
// =============================================================================

/// AVX2 implementation for x86_64.
/// Processes 32 bytes per iteration using 256-bit SIMD registers.
#[cfg(all(target_arch = "x86_64", not(kani)))]
mod x86_simd {
    use std::arch::x86_64::*;

    /// Check if AVX2 is available at runtime.
    #[inline]
    pub(crate) fn has_avx2() -> bool {
        std::arch::is_x86_feature_detected!("avx2")
    }

    /// Find first C0 control byte (< 0x20) using AVX2.
    #[target_feature(enable = "avx2")]
    #[inline]
    pub(crate) unsafe fn find_c0_control_avx2(input: &[u8]) -> Option<usize> {
        let len = input.len();
        if len == 0 {
            return None;
        }
        let ptr = input.as_ptr();
        let mut offset = 0usize;
        while offset + 32 <= len {
            // SAFETY: offset + 32 <= len; caller guarantees AVX2.
            let found = unsafe {
                let chunk = _mm256_loadu_si256(ptr.add(offset) as *const __m256i);
                let threshold = _mm256_set1_epi8(0x20i8);
                let bias = _mm256_set1_epi8(-128i8);
                let biased_chunk = _mm256_add_epi8(chunk, bias);
                let biased_threshold = _mm256_add_epi8(threshold, bias);
                let below = _mm256_cmpgt_epi8(biased_threshold, biased_chunk);
                _mm256_movemask_epi8(below) as u32
            };
            if found != 0 {
                return Some(offset + found.trailing_zeros() as usize);
            }
            offset += 32;
        }
        for (i, &byte) in input[offset..].iter().enumerate() {
            if byte < 0x20 {
                return Some(offset + i);
            }
        }
        None
    }

    /// Find first non-printable byte using AVX2.
    /// Returns None if all bytes are printable ASCII (0x20-0x7E).
    ///
    /// # Safety
    /// Caller must ensure AVX2 is available (use `has_avx2()` first).
    #[target_feature(enable = "avx2")]
    #[inline]
    pub(crate) unsafe fn find_non_printable_avx2(input: &[u8]) -> Option<usize> {
        let len = input.len();
        if len == 0 {
            return None;
        }

        let ptr = input.as_ptr();
        let mut offset = 0usize;

        // Process 32 bytes at a time
        while offset + 32 <= len {
            // SAFETY: We've checked that offset + 32 <= len, so reading 32 bytes
            // from ptr.add(offset) is valid. Caller guarantees AVX2 is available.
            let found = unsafe {
                let chunk = _mm256_loadu_si256(ptr.add(offset) as *const __m256i);

                // Check for bytes < 0x20 or > 0x7E
                // AVX2 doesn't have unsigned compare, so we use signed with bias
                //
                // Bias trick: subtract 0x80 from each byte to convert to signed range
                // Then printable range [0x20, 0x7E] becomes [-0x60, -0x02] in signed
                let bias = _mm256_set1_epi8(-128i8); // 0x80
                let biased = _mm256_add_epi8(chunk, bias);

                // Printable low (0x20 - 0x80 = -0x60 = -96 signed)
                let biased_low = _mm256_set1_epi8(-96i8);
                // Printable high (0x7E - 0x80 = -0x02 = -2 signed)
                let biased_high = _mm256_set1_epi8(-2i8);

                // Check if biased < biased_low (meaning original < 0x20)
                let too_low = _mm256_cmpgt_epi8(biased_low, biased);
                // Check if biased > biased_high (meaning original > 0x7E)
                let too_high = _mm256_cmpgt_epi8(biased, biased_high);

                // Combine: any byte outside [0x20, 0x7E]
                let outside = _mm256_or_si256(too_low, too_high);

                _mm256_movemask_epi8(outside) as u32
            };

            if found != 0 {
                return Some(offset + found.trailing_zeros() as usize);
            }

            offset += 32;
        }

        // Handle remaining bytes with scalar fallback
        for (i, &byte) in input[offset..].iter().enumerate() {
            if !(0x20..=0x7E).contains(&byte) {
                return Some(offset + i);
            }
        }

        None
    }
}

// =============================================================================
// aarch64 SIMD (NEON)
// =============================================================================

/// NEON implementation for aarch64.
/// Processes 16 bytes per iteration using 128-bit SIMD registers.
#[cfg(all(target_arch = "aarch64", not(kani)))]
mod arm_simd {
    use std::arch::aarch64::*;

    /// Find first C0 control byte (< 0x20) using NEON.
    #[inline]
    pub(crate) fn find_c0_control_neon(input: &[u8]) -> Option<usize> {
        let len = input.len();
        if len == 0 {
            return None;
        }
        let ptr = input.as_ptr();
        let mut offset = 0usize;
        while offset + 16 <= len {
            // SAFETY: offset + 16 <= len. NEON always available on aarch64.
            let lanes: (u64, u64) = unsafe {
                let chunk = vld1q_u8(ptr.add(offset));
                let threshold = vdupq_n_u8(0x20);
                let below = vcltq_u8(chunk, threshold);
                let below_u64 = vreinterpretq_u64_u8(below);
                (
                    vgetq_lane_u64::<0>(below_u64),
                    vgetq_lane_u64::<1>(below_u64),
                )
            };
            let (low64, high64) = lanes;
            if low64 != 0 {
                return Some(offset + (low64.trailing_zeros() as usize) / 8);
            }
            if high64 != 0 {
                return Some(offset + 8 + (high64.trailing_zeros() as usize) / 8);
            }
            offset += 16;
        }
        for (i, &byte) in input[offset..].iter().enumerate() {
            if byte < 0x20 {
                return Some(offset + i);
            }
        }
        None
    }

    /// Find first non-printable byte using NEON.
    /// Returns None if all bytes are printable ASCII (0x20-0x7E).
    ///
    /// NEON is always available on aarch64, so this is safe to call without
    /// runtime feature detection.
    #[inline]
    pub(crate) fn find_non_printable_neon(input: &[u8]) -> Option<usize> {
        let len = input.len();
        if len == 0 {
            return None;
        }

        let ptr = input.as_ptr();
        let mut offset = 0usize;

        // Process 16 bytes at a time
        while offset + 16 <= len {
            // SAFETY: We've checked that offset + 16 <= len, so reading 16 bytes
            // from ptr.add(offset) is valid. NEON is always available on aarch64.
            //
            // The unsafe block only performs SIMD intrinsics and extracts the
            // two u64 lane values. Position arithmetic is done in safe code.
            let lanes: (u64, u64) = unsafe {
                let chunk = vld1q_u8(ptr.add(offset));

                // Check for bytes < 0x20 or > 0x7E
                let low_bound = vdupq_n_u8(0x20);
                let high_bound = vdupq_n_u8(0x7E);
                let too_low = vcltq_u8(chunk, low_bound);
                let too_high = vcgtq_u8(chunk, high_bound);
                let outside = vorrq_u8(too_low, too_high);

                // Extract the two 64-bit halves of the comparison result.
                // Each byte in `outside` is 0xFF (non-printable) or 0x00 (printable).
                let outside_u64 = vreinterpretq_u64_u8(outside);
                (
                    vgetq_lane_u64::<0>(outside_u64),
                    vgetq_lane_u64::<1>(outside_u64),
                )
            };

            // Safe: use trailing_zeros on the u64 lanes to find the first
            // non-printable byte position — O(1) vs O(16) buffer scan.
            let (low64, high64) = lanes;
            if low64 != 0 {
                // First match is in bytes 0-7.
                // Each byte is 0xFF, so trailing_zeros gives bit position;
                // divide by 8 to get byte index.
                return Some(offset + (low64.trailing_zeros() as usize) / 8);
            }
            if high64 != 0 {
                // First match is in bytes 8-15.
                return Some(offset + 8 + (high64.trailing_zeros() as usize) / 8);
            }

            offset += 16;
        }

        // Handle remaining bytes with scalar fallback
        for (i, &byte) in input[offset..].iter().enumerate() {
            if !(0x20..=0x7E).contains(&byte) {
                return Some(offset + i);
            }
        }

        None
    }
}

// =============================================================================
// Public API with runtime dispatch
// =============================================================================

/// Find the first non-printable byte using the best available SIMD.
///
/// This function automatically selects the optimal implementation:
/// - AVX2 on x86_64 with AVX2 support
/// - NEON on aarch64
/// - Scalar fallback on other platforms
#[inline]
#[allow(unreachable_code)]
fn find_non_printable_simd(input: &[u8]) -> Option<usize> {
    #[cfg(all(target_arch = "x86_64", not(kani)))]
    {
        if x86_simd::has_avx2() {
            // SAFETY: We just checked that AVX2 is available
            return unsafe { x86_simd::find_non_printable_avx2(input) };
        }
    }

    #[cfg(all(target_arch = "aarch64", not(kani)))]
    {
        return arm_simd::find_non_printable_neon(input);
    }

    // Scalar fallback (used on x86_64 without AVX2, or other architectures)
    find_non_printable_scalar(input)
}

/// Scalar implementation (LLVM auto-vectorized).
/// Used as fallback when SIMD is not available.
#[cfg(not(kani))]
#[inline]
fn find_non_printable_scalar(input: &[u8]) -> Option<usize> {
    input.iter().position(|&b| !(0x20..=0x7E).contains(&b))
}

/// Kani-specific scalar implementation.
/// Uses an explicit indexed loop to prevent LLVM auto-vectorization with
/// NEON intrinsics (simd_reduce_max) that Kani cannot model.
#[cfg(kani)]
fn find_non_printable_scalar(input: &[u8]) -> Option<usize> {
    let mut i = 0;
    while i < input.len() {
        let b = input[i];
        if b < 0x20 || b > 0x7E {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Find the next byte that's not in the printable ASCII range.
///
/// This is the primary fast path function, using explicit SIMD when available:
/// - AVX2 on x86_64 (32 bytes/iteration)
/// - NEON on aarch64 (16 bytes/iteration)
/// - Scalar fallback with LLVM auto-vectorization
#[inline]
pub(super) fn find_non_printable(input: &[u8]) -> Option<usize> {
    find_non_printable_simd(input)
}

/// Count the number of printable ASCII bytes at the start of input.
///
/// Returns the length of the prefix that's all printable ASCII.
#[inline]
pub(crate) fn count_printable(input: &[u8]) -> usize {
    find_non_printable(input).unwrap_or(input.len())
}

/// Optimized batch print: returns slice of printable ASCII at start.
///
/// This is used by the fast path to avoid per-byte dispatch for
/// long runs of printable text.
#[inline]
pub(crate) fn take_printable(input: &[u8]) -> (&[u8], &[u8]) {
    let n = count_printable(input);
    input.split_at(n)
}

/// Find the first C0 control byte (< 0x20) in input.
///
/// Used by the OSC/DCS fast paths to bulk-skip data bytes.
/// In OscString state, bytes 0x20-0xFF are all OscPut (data);
/// only bytes < 0x20 need state machine handling (BEL terminator,
/// ESC for ST, CAN, SUB, etc.).
///
/// Returns `None` if the entire input is >= 0x20.
#[inline]
pub(crate) fn find_c0_control(input: &[u8]) -> Option<usize> {
    find_c0_control_simd(input)
}

#[inline]
#[allow(unreachable_code)]
fn find_c0_control_simd(input: &[u8]) -> Option<usize> {
    #[cfg(all(target_arch = "x86_64", not(kani)))]
    {
        if x86_simd::has_avx2() {
            // SAFETY: We just checked that AVX2 is available
            return unsafe { x86_simd::find_c0_control_avx2(input) };
        }
    }

    #[cfg(all(target_arch = "aarch64", not(kani)))]
    {
        return arm_simd::find_c0_control_neon(input);
    }

    find_c0_control_scalar(input)
}

#[cfg(not(kani))]
#[inline]
fn find_c0_control_scalar(input: &[u8]) -> Option<usize> {
    input.iter().position(|&b| b < 0x20)
}

#[cfg(kani)]
fn find_c0_control_scalar(input: &[u8]) -> Option<usize> {
    let mut i = 0;
    while i < input.len() {
        if input[i] < 0x20 {
            return Some(i);
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_count_printable() {
        assert_eq!(count_printable(b"hello\x1bworld"), 5);
        assert_eq!(count_printable(b"hello world"), 11);
        assert_eq!(count_printable(b"\x1bhello"), 0);
    }

    #[test]
    fn test_take_printable() {
        let (printable, rest) = take_printable(b"hello\x1bworld");
        assert_eq!(printable, b"hello");
        assert_eq!(rest, b"\x1bworld");
    }

    // SIMD-specific tests
    #[test]
    fn test_find_non_printable_simd_empty() {
        assert_eq!(find_non_printable_simd(b""), None);
    }

    #[test]
    fn test_find_non_printable_simd_pure_ascii() {
        let data = b"Hello, World! This is a test of the terminal parser.";
        assert_eq!(find_non_printable_simd(data), None);
    }

    #[test]
    fn test_find_non_printable_simd_escape_at_start() {
        assert_eq!(find_non_printable_simd(b"\x1bhello"), Some(0));
    }

    #[test]
    fn test_find_non_printable_simd_escape_at_end() {
        let mut data = vec![b'A'; 100];
        data[99] = 0x1B;
        assert_eq!(find_non_printable_simd(&data), Some(99));
    }

    #[test]
    fn test_find_non_printable_simd_escape_middle() {
        let mut data = vec![b'A'; 100];
        data[50] = 0x1B;
        assert_eq!(find_non_printable_simd(&data), Some(50));
    }

    #[test]
    fn test_find_non_printable_simd_large_input() {
        // Test with >32 bytes to ensure SIMD path is exercised
        let mut data = vec![b'A'; 1024];
        data[512] = 0x1B;
        assert_eq!(find_non_printable_simd(&data), Some(512));
    }

    #[test]
    fn test_find_non_printable_simd_boundary_values() {
        // Test at exact boundaries
        assert_eq!(find_non_printable_simd(b"\x1F"), Some(0)); // Just below 0x20
        assert_eq!(find_non_printable_simd(b"\x20"), None); // Exactly 0x20
        assert_eq!(find_non_printable_simd(b"\x7E"), None); // Exactly 0x7E
        assert_eq!(find_non_printable_simd(b"\x7F"), Some(0)); // Just above 0x7E
    }

    #[test]
    fn test_find_non_printable_simd_all_printable_varying_sizes() {
        // Test various sizes to exercise both SIMD and scalar paths
        for size in [1usize, 15, 16, 17, 31, 32, 33, 63, 64, 65, 127, 128, 129] {
            let data: Vec<u8> = (0..size)
                .map(|i| b'A' + u8::try_from(i % 26).unwrap())
                .collect();
            assert_eq!(
                find_non_printable_simd(&data),
                None,
                "Failed for size {}",
                size
            );
        }
    }

    #[test]
    fn test_find_non_printable_simd_high_bytes() {
        // Test high bytes (>= 0x80) which are non-printable for this fast path.
        // These include C1 controls and bytes >= 0xA0.
        assert_eq!(find_non_printable_simd(b"\x80"), Some(0)); // First C1 control
        assert_eq!(find_non_printable_simd(b"\x9B"), Some(0)); // CSI (C1 control)
        assert_eq!(find_non_printable_simd(b"\x9F"), Some(0)); // Last C1 control
        assert_eq!(find_non_printable_simd(b"\xA0"), Some(0)); // Non-breaking space (UTF-8)
        assert_eq!(find_non_printable_simd(b"\xFF"), Some(0)); // Maximum byte value

        // Test high bytes embedded in text
        let mut data = vec![b'A'; 100];
        data[50] = 0x80;
        assert_eq!(find_non_printable_simd(&data), Some(50));

        // Test high byte after SIMD boundary
        let mut data = vec![b'A'; 64];
        data[33] = 0xFF;
        assert_eq!(find_non_printable_simd(&data), Some(33));
    }

    #[test]
    fn test_find_non_printable_simd_scalar_equivalence() {
        // Verify SIMD and scalar implementations produce identical results
        // across various input patterns

        // Test all single-byte values
        for byte in 0u8..=255u8 {
            let input = [byte];
            let simd_result = find_non_printable_simd(&input);
            let scalar_result = find_non_printable_scalar(&input);
            assert_eq!(
                simd_result, scalar_result,
                "Mismatch for byte 0x{:02X}: SIMD={:?}, scalar={:?}",
                byte, simd_result, scalar_result
            );
        }

        // Test all boundary positions for various sizes
        for size in [16, 32, 48, 64, 100] {
            for pos in [0, 1, size / 2, size - 2, size - 1] {
                if pos < size {
                    let mut data = vec![b'A'; size];
                    data[pos] = 0x1B; // ESC
                    assert_eq!(
                        find_non_printable_simd(&data),
                        find_non_printable_scalar(&data),
                        "Mismatch for size {} pos {}",
                        size,
                        pos
                    );
                }
            }
        }
    }

    #[test]
    fn test_find_c0_control_empty() {
        assert_eq!(find_c0_control(b""), None);
    }

    #[test]
    fn test_find_c0_control_pure_data() {
        assert_eq!(find_c0_control(b"Hello, World!"), None);
        assert_eq!(find_c0_control(b"\x80\x90\xA0\xFF"), None);
        assert_eq!(find_c0_control(b"\x20\x7E\x7F\x80"), None);
    }

    #[test]
    fn test_find_c0_control_at_start() {
        assert_eq!(find_c0_control(b"\x07hello"), Some(0));
        assert_eq!(find_c0_control(b"\x1Bhello"), Some(0));
        assert_eq!(find_c0_control(b"\x00hello"), Some(0));
    }

    #[test]
    fn test_find_c0_control_embedded() {
        assert_eq!(find_c0_control(b"abc\x07def"), Some(3));
        assert_eq!(find_c0_control(b"abcdefghijklmnop\x1Bqrs"), Some(16));
    }

    #[test]
    fn test_find_c0_control_boundary() {
        assert_eq!(find_c0_control(b"\x1F"), Some(0));
        assert_eq!(find_c0_control(b"\x20"), None);
    }

    #[test]
    fn test_find_c0_control_varying_sizes() {
        for size in [1, 15, 16, 17, 31, 32, 33, 63, 64, 65, 128] {
            let data: Vec<u8> = (0..size).map(|i| 0x20 + (i as u8 % 0x60)).collect();
            assert_eq!(find_c0_control(&data), None, "Failed for size {size}");

            for pos in [0, size / 2, size - 1] {
                let mut data2 = data.clone();
                data2[pos] = 0x07;
                assert_eq!(
                    find_c0_control(&data2),
                    Some(pos),
                    "Failed for size {size} pos {pos}"
                );
            }
        }
    }

    #[test]
    fn test_find_c0_control_scalar_equivalence() {
        for byte in 0u8..=255 {
            let input = [byte];
            assert_eq!(
                find_c0_control(&input),
                find_c0_control_scalar(&input),
                "Mismatch for byte 0x{byte:02X}"
            );
        }
    }
}
