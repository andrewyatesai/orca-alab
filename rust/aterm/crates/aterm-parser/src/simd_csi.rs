// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0

//! SIMD-accelerated CSI parameter parsing.
//!
//! Parses the parameter portion of CSI sequences (digit runs separated by
//! semicolons/colons) using platform-specific SIMD intrinsics to quickly
//! locate delimiter boundaries. The actual digit-to-integer conversion uses
//! scalar arithmetic since parameter runs are typically 1-3 digits.
//!
//! ## Platform Support
//!
//! - **AVX2 (x86_64):** 32-byte chunk scanning with `_mm256_cmpgt_epi8` for
//!   boundary detection, `_mm256_movemask_epi8` for bitmask extraction.
//! - **NEON (aarch64):** 16-byte chunk scanning with `vcltq_u8`/`vcgtq_u8`
//!   for range checks, lane extraction for scalar reduction.
//! - **Scalar fallback:** Byte-by-byte loop, reference implementation for
//!   property tests.

/// Maximum number of parameters that `simd_parse_csi_params` will extract.
/// Tied to `crate::MAX_PARAMS` to prevent divergence.
const SIMD_MAX_PARAMS: usize = crate::MAX_PARAMS;

/// Result of SIMD CSI parameter parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CsiParamResult {
    /// Parsed parameter values. Unset slots are 0.
    pub(crate) params: [u16; SIMD_MAX_PARAMS],
    /// Number of valid parameters.
    pub(crate) count: usize,
    /// Number of input bytes consumed (up to but not including the final byte).
    pub(crate) bytes_consumed: usize,
    /// Whether any colon subparam separators were found.
    /// When true, the caller should fall back to the general parser which
    /// tracks subparam masks.
    pub(crate) has_subparams: bool,
}

/// Parse CSI parameter bytes using the best available SIMD.
///
/// Input: the byte slice starting after `ESC[` (or after a private marker).
/// Scans digits, semicolons, and colons. Stops at the first final byte
/// (0x40-0x7E), intermediate byte (0x20-0x2F), or invalid byte.
///
/// Returns `None` if the input is empty or starts with a non-param byte.
/// Returns `Some(result)` with parsed params and the number of bytes consumed.
///
/// Colon-separated subparams are detected but not fully parsed here (the
/// caller should fall back to the general path for subparam mask tracking).
#[inline]
#[allow(unreachable_code)]
pub(crate) fn simd_parse_csi_params(input: &[u8]) -> Option<CsiParamResult> {
    if input.is_empty() {
        return None;
    }

    // Check first byte is a valid param start (digit, semicolon, or colon)
    let first = input[0];
    if !first.is_ascii_digit() && first != b';' && first != b':' {
        return None;
    }

    #[cfg(all(target_arch = "x86_64", not(kani)))]
    {
        if std::arch::is_x86_feature_detected!("avx2") {
            // SAFETY: We just checked that AVX2 is available via runtime detection.
            return Some(unsafe { parse_csi_params_avx2(input) });
        }
    }

    #[cfg(all(target_arch = "aarch64", not(kani)))]
    {
        return Some(parse_csi_params_neon(input));
    }

    Some(parse_csi_params_scalar(input))
}

/// Scalar CSI parameter parser -- reference implementation.
///
/// Produces identical results to the SIMD paths for all inputs.
/// Used as fallback when SIMD is not available, and as the oracle
/// for property tests.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn parse_csi_params_scalar(input: &[u8]) -> CsiParamResult {
    let mut result = CsiParamResult {
        params: [0u16; SIMD_MAX_PARAMS],
        count: 0,
        bytes_consumed: 0,
        has_subparams: false,
    };

    let mut current: u32 = 0;
    let mut param_started = false;
    let limit = input.len().min(65);

    for (i, &b) in input[..limit].iter().enumerate() {
        if b.is_ascii_digit() {
            current = current
                .saturating_mul(10)
                .saturating_add(u32::from(b - b'0'));
            param_started = true;
        } else if b == b';' {
            if result.count < SIMD_MAX_PARAMS {
                let value = u16::try_from(current.min(u32::from(u16::MAX))).unwrap_or(u16::MAX);
                result.params[result.count] = value;
                result.count += 1;
            }
            current = 0;
            param_started = false;
        } else if b == b':' {
            result.has_subparams = true;
            if result.count < SIMD_MAX_PARAMS {
                let value = u16::try_from(current.min(u32::from(u16::MAX))).unwrap_or(u16::MAX);
                result.params[result.count] = value;
                result.count += 1;
            }
            current = 0;
            param_started = false;
        } else {
            if param_started && result.count < SIMD_MAX_PARAMS {
                let value = u16::try_from(current.min(u32::from(u16::MAX))).unwrap_or(u16::MAX);
                result.params[result.count] = value;
                result.count += 1;
            }
            result.bytes_consumed = i;
            return result;
        }
    }

    if param_started && result.count < SIMD_MAX_PARAMS {
        let value = u16::try_from(current.min(u32::from(u16::MAX))).unwrap_or(u16::MAX);
        result.params[result.count] = value;
        result.count += 1;
    }
    result.bytes_consumed = limit;
    result
}

// =============================================================================
// Shared scalar tail used by both AVX2 and NEON after their SIMD loops
// =============================================================================

/// Process remaining bytes after SIMD loop exhaustion.
/// Shared by AVX2 and NEON paths to avoid code duplication.
#[cfg(any(
    all(target_arch = "x86_64", not(kani)),
    all(target_arch = "aarch64", not(kani)),
))]
#[inline]
fn scalar_tail(
    input: &[u8],
    offset: &mut usize,
    len: usize,
    result: &mut CsiParamResult,
    current: &mut u32,
    param_started: &mut bool,
) {
    while *offset < len {
        let b = input[*offset];
        if b.is_ascii_digit() {
            *current = current
                .saturating_mul(10)
                .saturating_add(u32::from(b - b'0'));
            *param_started = true;
            *offset += 1;
        } else if b == b';' || b == b':' {
            if b == b':' {
                result.has_subparams = true;
            }
            if result.count < SIMD_MAX_PARAMS {
                result.params[result.count] =
                    u16::try_from((*current).min(u32::from(u16::MAX))).unwrap_or(u16::MAX);
                result.count += 1;
            }
            *current = 0;
            *param_started = false;
            *offset += 1;
        } else {
            if *param_started && result.count < SIMD_MAX_PARAMS {
                result.params[result.count] =
                    u16::try_from((*current).min(u32::from(u16::MAX))).unwrap_or(u16::MAX);
                result.count += 1;
            }
            result.bytes_consumed = *offset;
            return;
        }
    }

    if *param_started && result.count < SIMD_MAX_PARAMS {
        result.params[result.count] =
            u16::try_from((*current).min(u32::from(u16::MAX))).unwrap_or(u16::MAX);
        result.count += 1;
    }
    result.bytes_consumed = *offset;
}

// =============================================================================
// x86_64 AVX2 implementation
// =============================================================================

/// Parse CSI parameters using AVX2.
///
/// Scans 32 bytes at a time, using SIMD comparisons to identify the first
/// non-param byte (outside `[0x30, 0x3B]`). Once the boundary is found,
/// processes the digit/delimiter bytes with scalar arithmetic.
///
/// # Safety
/// Caller must ensure AVX2 is available via runtime feature detection.
#[cfg(all(target_arch = "x86_64", not(kani)))]
#[target_feature(enable = "avx2")]
unsafe fn parse_csi_params_avx2(input: &[u8]) -> CsiParamResult {
    use std::arch::x86_64::*;

    let mut result = CsiParamResult {
        params: [0u16; SIMD_MAX_PARAMS],
        count: 0,
        bytes_consumed: 0,
        has_subparams: false,
    };

    let len = input.len().min(65);
    let ptr = input.as_ptr();
    let mut offset = 0usize;
    let mut current: u32 = 0;
    let mut param_started = false;

    while offset + 32 <= len {
        // SAFETY: offset + 32 <= len guarantees the 32-byte load is in bounds.
        // Caller guarantees AVX2 is available.
        let (end_mask, colon_mask) = unsafe {
            let chunk = _mm256_loadu_si256(ptr.add(offset) as *const __m256i);

            // Classify: bytes in [0x30, 0x3B] are param bytes (digits/colon/semi).
            // Everything else is an "end" byte.
            let ascii_0 = _mm256_set1_epi8(0x30u8 as i8);
            let below = _mm256_cmpgt_epi8(ascii_0, chunk);
            let ascii_semi = _mm256_set1_epi8(0x3Bu8 as i8);
            let above = _mm256_cmpgt_epi8(chunk, ascii_semi);
            let end = _mm256_or_si256(below, above);
            let end_mask = _mm256_movemask_epi8(end) as u32;

            let colon_val = _mm256_set1_epi8(0x3Au8 as i8);
            let is_colon = _mm256_cmpeq_epi8(chunk, colon_val);
            let colon_mask = _mm256_movemask_epi8(is_colon) as u32;

            (end_mask, colon_mask)
        };

        if end_mask != 0 {
            let end_pos = end_mask.trailing_zeros() as usize;
            // Mask colon detection to only consider bytes before end_pos
            let param_colon_mask = colon_mask & ((1u32 << end_pos) - 1);
            if param_colon_mask != 0 {
                result.has_subparams = true;
            }
            for i in 0..end_pos {
                let b = input[offset + i];
                if b.is_ascii_digit() {
                    current = current
                        .saturating_mul(10)
                        .saturating_add(u32::from(b - b'0'));
                    param_started = true;
                } else {
                    if result.count < SIMD_MAX_PARAMS {
                        result.params[result.count] =
                            u16::try_from(current.min(u32::from(u16::MAX))).unwrap_or(u16::MAX);
                        result.count += 1;
                    }
                    current = 0;
                    param_started = false;
                }
            }
            if param_started && result.count < SIMD_MAX_PARAMS {
                result.params[result.count] =
                    u16::try_from(current.min(u32::from(u16::MAX))).unwrap_or(u16::MAX);
                result.count += 1;
            }
            result.bytes_consumed = offset + end_pos;
            return result;
        }

        // Entire 32-byte chunk is param bytes -- colon check is valid
        // since all 32 bytes are within the param region.
        if colon_mask != 0 {
            result.has_subparams = true;
        }
        for i in 0..32 {
            let b = input[offset + i];
            if b.is_ascii_digit() {
                current = current
                    .saturating_mul(10)
                    .saturating_add(u32::from(b - b'0'));
                param_started = true;
            } else {
                if result.count < SIMD_MAX_PARAMS {
                    result.params[result.count] =
                        u16::try_from(current.min(u32::from(u16::MAX))).unwrap_or(u16::MAX);
                    result.count += 1;
                }
                current = 0;
                param_started = false;
            }
        }
        offset += 32;
    }

    scalar_tail(
        input,
        &mut offset,
        len,
        &mut result,
        &mut current,
        &mut param_started,
    );
    result
}

// =============================================================================
// aarch64 NEON implementation
// =============================================================================

/// Parse CSI parameters using NEON.
///
/// Scans 16 bytes at a time, using NEON range comparisons to detect non-param
/// bytes. NEON is always available on aarch64.
#[cfg(all(target_arch = "aarch64", not(kani)))]
fn parse_csi_params_neon(input: &[u8]) -> CsiParamResult {
    use std::arch::aarch64::*;

    let mut result = CsiParamResult {
        params: [0u16; SIMD_MAX_PARAMS],
        count: 0,
        bytes_consumed: 0,
        has_subparams: false,
    };

    let len = input.len().min(65);
    let ptr = input.as_ptr();
    let mut offset = 0usize;
    let mut current: u32 = 0;
    let mut param_started = false;

    while offset + 16 <= len {
        // SAFETY: offset + 16 <= len guarantees the load is in bounds.
        // NEON is always available on aarch64.
        let (has_end, has_colon) = unsafe {
            let chunk = vld1q_u8(ptr.add(offset));

            let low = vdupq_n_u8(0x30);
            let high = vdupq_n_u8(0x3B);
            let below = vcltq_u8(chunk, low);
            let above = vcgtq_u8(chunk, high);
            let outside = vorrq_u8(below, above);

            let colon_val = vdupq_n_u8(0x3A);
            let is_colon = vceqq_u8(chunk, colon_val);

            let outside_u64 = vreinterpretq_u64_u8(outside);
            let has_end =
                vgetq_lane_u64::<0>(outside_u64) != 0 || vgetq_lane_u64::<1>(outside_u64) != 0;

            let colon_u64 = vreinterpretq_u64_u8(is_colon);
            let has_colon =
                vgetq_lane_u64::<0>(colon_u64) != 0 || vgetq_lane_u64::<1>(colon_u64) != 0;

            (has_end, has_colon)
        };

        if has_end {
            // Scan bytes to find end position, checking colons only before it
            for i in 0..16 {
                let idx = offset + i;
                let b = input[idx];
                if b.is_ascii_digit() {
                    current = current
                        .saturating_mul(10)
                        .saturating_add(u32::from(b - b'0'));
                    param_started = true;
                } else if b == b';' || b == b':' {
                    if b == b':' {
                        result.has_subparams = true;
                    }
                    if result.count < SIMD_MAX_PARAMS {
                        result.params[result.count] =
                            u16::try_from(current.min(u32::from(u16::MAX))).unwrap_or(u16::MAX);
                        result.count += 1;
                    }
                    current = 0;
                    param_started = false;
                } else {
                    if param_started && result.count < SIMD_MAX_PARAMS {
                        result.params[result.count] =
                            u16::try_from(current.min(u32::from(u16::MAX))).unwrap_or(u16::MAX);
                        result.count += 1;
                    }
                    result.bytes_consumed = idx;
                    return result;
                }
            }
            // Unreachable: has_end guarantees at least one non-param byte in the
            // 16-byte chunk, so the `else` branch above always returns.
            unreachable!("has_end was true but no end byte found in 16-byte scan");
        }

        // No end byte -- all 16 bytes are param bytes, colon check is valid.
        if has_colon {
            result.has_subparams = true;
        }
        for i in 0..16 {
            let b = input[offset + i];
            if b.is_ascii_digit() {
                current = current
                    .saturating_mul(10)
                    .saturating_add(u32::from(b - b'0'));
                param_started = true;
            } else {
                if result.count < SIMD_MAX_PARAMS {
                    result.params[result.count] =
                        u16::try_from(current.min(u32::from(u16::MAX))).unwrap_or(u16::MAX);
                    result.count += 1;
                }
                current = 0;
                param_started = false;
            }
        }
        offset += 16;
    }

    scalar_tail(
        input,
        &mut offset,
        len,
        &mut result,
        &mut current,
        &mut param_started,
    );
    result
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzz_simd_parse_csi_params_never_panics() {
        // The CSI param scanner runs on untrusted escape sequences emitted by any
        // program. It must NEVER panic on arbitrary bytes — in particular the
        // `unreachable!("no end byte found in 16-byte scan")` must hold for every
        // input (a SIMD-vs-scalar end-byte classification mismatch would reach it
        // and crash the terminal — a DoS). This deterministic fuzz sweeps 200k
        // pseudo-random byte sequences (incl. lengths that exercise the 16-byte
        // SIMD chunk boundary) and checks the result invariants.
        let mut state: u64 = 0xD1B5_4A32_D192_ED03;
        let mut next = move || {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (state >> 33) as u32
        };
        for _ in 0..200_000 {
            let len = (next() % 40) as usize; // spans the 16-byte SIMD boundary
            let input: Vec<u8> = (0..len).map(|_| (next() & 0xFF) as u8).collect();
            if let Some(result) = simd_parse_csi_params(&input) {
                // Sound result invariants — no over-read, no over-count.
                assert!(result.bytes_consumed <= input.len());
                assert!(result.count <= SIMD_MAX_PARAMS);
            }
        }
    }

    #[test]
    fn test_simd_csi_params_empty_input() {
        assert_eq!(simd_parse_csi_params(b""), None);
    }

    #[test]
    fn test_simd_csi_params_non_param_start() {
        assert_eq!(simd_parse_csi_params(b"m"), None);
        assert_eq!(simd_parse_csi_params(b"H"), None);
        assert_eq!(simd_parse_csi_params(b" "), None);
        assert_eq!(simd_parse_csi_params(b"\x00"), None);
        assert_eq!(simd_parse_csi_params(b"\xFF"), None);
    }

    #[test]
    fn test_simd_csi_params_single_digit() {
        let result = simd_parse_csi_params(b"5m").expect("should parse");
        assert_eq!(result.count, 1);
        assert_eq!(result.params[0], 5);
        assert_eq!(result.bytes_consumed, 1);
        assert!(!result.has_subparams);
    }

    #[test]
    fn test_simd_csi_params_two_digits() {
        let result = simd_parse_csi_params(b"31m").expect("should parse");
        assert_eq!(result.count, 1);
        assert_eq!(result.params[0], 31);
        assert_eq!(result.bytes_consumed, 2);
    }

    #[test]
    fn test_simd_csi_params_three_digits() {
        let result = simd_parse_csi_params(b"196m").expect("should parse");
        assert_eq!(result.count, 1);
        assert_eq!(result.params[0], 196);
        assert_eq!(result.bytes_consumed, 3);
    }

    #[test]
    fn test_simd_csi_params_five_digits() {
        let result = simd_parse_csi_params(b"65535m").expect("should parse");
        assert_eq!(result.count, 1);
        assert_eq!(result.params[0], 65535);
        assert_eq!(result.bytes_consumed, 5);
    }

    #[test]
    fn test_simd_csi_params_two_params_semicolon() {
        let result = simd_parse_csi_params(b"1;31m").expect("should parse");
        assert_eq!(result.count, 2);
        assert_eq!(result.params[0], 1);
        assert_eq!(result.params[1], 31);
        assert_eq!(result.bytes_consumed, 4);
        assert!(!result.has_subparams);
    }

    #[test]
    fn test_simd_csi_params_256_color() {
        let result = simd_parse_csi_params(b"38;5;196m").expect("should parse");
        assert_eq!(result.count, 3);
        assert_eq!(result.params[0], 38);
        assert_eq!(result.params[1], 5);
        assert_eq!(result.params[2], 196);
        assert_eq!(result.bytes_consumed, 8);
    }

    #[test]
    fn test_simd_csi_params_truecolor() {
        let result = simd_parse_csi_params(b"38;2;255;128;0m").expect("should parse");
        assert_eq!(result.count, 5);
        assert_eq!(result.params[0], 38);
        assert_eq!(result.params[1], 2);
        assert_eq!(result.params[2], 255);
        assert_eq!(result.params[3], 128);
        assert_eq!(result.params[4], 0);
        assert_eq!(result.bytes_consumed, 14);
    }

    #[test]
    fn test_simd_csi_params_cursor_position() {
        let result = simd_parse_csi_params(b"10;20H").expect("should parse");
        assert_eq!(result.count, 2);
        assert_eq!(result.params[0], 10);
        assert_eq!(result.params[1], 20);
        assert_eq!(result.bytes_consumed, 5);
    }

    #[test]
    fn test_simd_csi_params_zero_param() {
        let result = simd_parse_csi_params(b"0m").expect("should parse");
        assert_eq!(result.count, 1);
        assert_eq!(result.params[0], 0);
        assert_eq!(result.bytes_consumed, 1);
    }

    #[test]
    fn test_simd_csi_params_overflow_clamped_to_u16_max() {
        let result = simd_parse_csi_params(b"99999m").expect("should parse");
        assert_eq!(result.count, 1);
        assert_eq!(result.params[0], 65535);
        assert_eq!(result.bytes_consumed, 5);
    }

    #[test]
    fn test_simd_csi_params_max_params_16() {
        let input = b"1;2;3;4;5;6;7;8;9;10;11;12;13;14;15;16m";
        let result = simd_parse_csi_params(input).expect("should parse");
        assert_eq!(result.count, 16);
        assert_eq!(result.params[0], 1);
        assert_eq!(result.params[15], 16);
    }

    #[test]
    fn test_simd_csi_params_exceeds_max_params() {
        let input = b"1;2;3;4;5;6;7;8;9;10;11;12;13;14;15;16;17m";
        let result = simd_parse_csi_params(input).expect("should parse");
        assert_eq!(result.count, 16, "should cap at 16 params");
        assert_eq!(result.params[0], 1);
        assert_eq!(result.params[15], 16);
    }

    #[test]
    fn test_simd_csi_params_colon_subparams() {
        let result = simd_parse_csi_params(b"4:3m").expect("should parse");
        assert!(result.has_subparams, "should detect colon subparam");
        assert_eq!(result.count, 2);
        assert_eq!(result.params[0], 4);
        assert_eq!(result.params[1], 3);
        assert_eq!(result.bytes_consumed, 3);
    }

    #[test]
    fn test_simd_csi_params_mixed_semicolon_colon() {
        let result = simd_parse_csi_params(b"1;58:5:196m").expect("should parse");
        assert!(result.has_subparams);
        assert_eq!(result.params[0], 1);
        assert_eq!(result.params[1], 58);
        assert_eq!(result.params[2], 5);
        assert_eq!(result.params[3], 196);
        assert_eq!(result.count, 4);
    }

    #[test]
    fn test_simd_csi_params_semicolon_with_no_leading_digit() {
        let result = simd_parse_csi_params(b";31m").expect("should parse");
        assert_eq!(result.count, 2);
        assert_eq!(result.params[0], 0);
        assert_eq!(result.params[1], 31);
    }

    #[test]
    fn test_simd_csi_params_trailing_semicolon() {
        let result = simd_parse_csi_params(b"31;m").expect("should parse");
        assert_eq!(result.count, 1, "only param before semicolon is pushed");
        assert_eq!(result.params[0], 31);
    }

    #[test]
    fn test_simd_csi_params_intermediate_byte_stops_scan() {
        let result = simd_parse_csi_params(b"31 q").expect("should parse");
        assert_eq!(result.count, 1);
        assert_eq!(result.params[0], 31);
        assert_eq!(result.bytes_consumed, 2, "stops at the space");
    }

    #[test]
    fn test_simd_csi_params_all_final_bytes() {
        for final_byte in 0x40u8..=0x7E {
            let input = [b'5', final_byte];
            let result = simd_parse_csi_params(&input)
                .unwrap_or_else(|| panic!("should parse for final byte 0x{:02X}", final_byte));
            assert_eq!(result.count, 1);
            assert_eq!(result.params[0], 5);
            assert_eq!(result.bytes_consumed, 1);
        }
    }

    #[test]
    fn test_simd_csi_params_long_digit_run() {
        let input = b"99999999999999999m";
        let result = simd_parse_csi_params(input).expect("should parse");
        assert_eq!(result.count, 1);
        assert_eq!(result.params[0], 65535);
    }

    #[test]
    fn test_simd_csi_params_simd_scalar_equivalence() {
        let test_inputs: &[&[u8]] = &[
            b"0m",
            b"1m",
            b"31m",
            b"38;5;196m",
            b"38;2;255;128;64m",
            b"1;4;5;7m",
            b"0;0;0;0;0;0;0;0;0;0;0;0;0;0;0;0m",
            b"4:3m",
            b"1;58:5:196m",
            b"38:2:255:128:0m",
            b";m",
            b";;m",
            b";1;m",
            b"99999m",
            b"1;2;3;4;5;6;7;8;9;10;11;12;13;14;15;16;17;18m",
            b"10;20H",
            b"?1049h",
        ];

        for input in test_inputs {
            let simd_result = simd_parse_csi_params(input);
            let scalar_result = if input.is_empty()
                || (!input[0].is_ascii_digit() && input[0] != b';' && input[0] != b':')
            {
                None
            } else {
                Some(parse_csi_params_scalar(input))
            };

            assert_eq!(
                simd_result,
                scalar_result,
                "SIMD/scalar mismatch for input {:?}",
                std::str::from_utf8(input).unwrap_or("<non-utf8>")
            );
        }
    }

    #[test]
    fn test_simd_csi_params_simd_scalar_equivalence_exhaustive_short() {
        for b0 in 0x30u8..=0x3B {
            let input = [b0, b'm'];
            let simd_r = simd_parse_csi_params(&input);
            let scalar_r = Some(parse_csi_params_scalar(&input));
            assert_eq!(simd_r, scalar_r, "1-byte mismatch for {:?}", input);
        }

        for b0 in 0x30u8..=0x3B {
            for b1 in 0x30u8..=0x3B {
                let input = [b0, b1, b'm'];
                let simd_r = simd_parse_csi_params(&input);
                let scalar_r = Some(parse_csi_params_scalar(&input));
                assert_eq!(simd_r, scalar_r, "2-byte mismatch for {:?}", input);
            }
        }
    }

    #[test]
    fn test_simd_csi_params_at_65_byte_limit() {
        let mut input = Vec::new();
        for _ in 0..32 {
            input.extend_from_slice(b"1;");
        }
        input.extend_from_slice(b"2m");

        let simd_r = simd_parse_csi_params(&input).expect("should parse");
        let scalar_r = parse_csi_params_scalar(&input);
        assert_eq!(simd_r, scalar_r, "65-byte limit mismatch");
    }

    #[test]
    fn test_simd_csi_params_all_zeros() {
        let result = simd_parse_csi_params(b"0;0;0;0;0m").expect("should parse");
        assert_eq!(result.count, 5);
        for i in 0..5 {
            assert_eq!(result.params[i], 0, "param {} should be 0", i);
        }
    }

    #[test]
    fn test_simd_csi_params_invalid_byte_in_middle() {
        let result = simd_parse_csi_params(b"1;2\x7F3m").expect("should parse");
        assert_eq!(result.count, 2);
        assert_eq!(result.params[0], 1);
        assert_eq!(result.params[1], 2);
        assert_eq!(result.bytes_consumed, 3);
    }

    #[test]
    fn test_simd_csi_params_consecutive_semicolons() {
        let result = simd_parse_csi_params(b"1;;3m").expect("should parse");
        assert_eq!(result.count, 3);
        assert_eq!(result.params[0], 1);
        assert_eq!(result.params[1], 0);
        assert_eq!(result.params[2], 3);
    }

    #[test]
    fn test_simd_csi_params_varying_sizes_scalar_parity() {
        for num_params in 1..=16 {
            let mut input = Vec::new();
            for i in 0..num_params {
                if i > 0 {
                    input.push(b';');
                }
                let val = (i % 10) as u8;
                input.push(b'0' + val);
            }
            input.push(b'm');

            let simd_r = simd_parse_csi_params(&input).expect("should parse");
            let scalar_r = parse_csi_params_scalar(&input);
            assert_eq!(
                simd_r, scalar_r,
                "parity mismatch for {} params",
                num_params
            );
        }
    }
}
