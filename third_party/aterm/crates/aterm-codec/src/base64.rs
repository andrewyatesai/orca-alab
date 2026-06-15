// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0

//! Base64 encoding and decoding (RFC 4648).

use std::fmt;

/// Standard Base64 alphabet (A-Z, a-z, 0-9, +, /).
const STANDARD_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// URL-safe Base64 alphabet (A-Z, a-z, 0-9, -, _).
const URL_SAFE_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// Build a 256-byte decode lookup table from an alphabet.
/// Invalid characters map to 0xFF.
const fn build_decode_table(alphabet: &[u8; 64]) -> [u8; 256] {
    let mut table = [0xFFu8; 256];
    let mut i = 0;
    while i < 64 {
        table[alphabet[i] as usize] = i as u8;
        i += 1;
    }
    table
}

const STANDARD_DECODE: [u8; 256] = build_decode_table(STANDARD_ALPHABET);
const URL_SAFE_DECODE: [u8; 256] = build_decode_table(URL_SAFE_ALPHABET);

/// Error during Base64 decoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// An invalid character was encountered at the given position.
    InvalidByte(usize, u8),
    /// The input length is not valid for Base64 (not a multiple of 4 when padded).
    InvalidLength(usize),
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidByte(pos, byte) => {
                write!(f, "invalid base64 byte 0x{byte:02X} at position {pos}")
            }
            Self::InvalidLength(len) => {
                write!(f, "invalid base64 input length: {len}")
            }
        }
    }
}

impl std::error::Error for DecodeError {}

/// Encode bytes to standard Base64 with padding.
#[must_use]
pub fn encode(input: &[u8]) -> String {
    encode_with_alphabet(input, STANDARD_ALPHABET, true)
}

/// Encode bytes to URL-safe Base64 without padding.
#[must_use]
pub fn encode_url_safe_no_pad(input: &[u8]) -> String {
    encode_with_alphabet(input, URL_SAFE_ALPHABET, false)
}

/// Encode bytes to standard Base64 without padding.
#[must_use]
pub fn encode_no_pad(input: &[u8]) -> String {
    encode_with_alphabet(input, STANDARD_ALPHABET, false)
}

/// Decode standard Base64 (with or without padding).
///
/// # Errors
///
/// Returns [`DecodeError`] if the input contains invalid characters or has
/// an invalid length.
pub fn decode(input: &str) -> Result<Vec<u8>, DecodeError> {
    decode_with_table(input.as_bytes(), &STANDARD_DECODE)
}

/// Decode URL-safe Base64 (without padding).
///
/// # Errors
///
/// Returns [`DecodeError`] if the input contains invalid characters.
pub fn decode_url_safe_no_pad(input: &str) -> Result<Vec<u8>, DecodeError> {
    decode_with_table(input.as_bytes(), &URL_SAFE_DECODE)
}

// ── Internal ────────────────────────────────────────────────────────────────

fn encode_with_alphabet(input: &[u8], alphabet: &[u8; 64], pad: bool) -> String {
    if input.is_empty() {
        return String::new();
    }

    let full_chunks = input.len() / 3;
    let remainder = input.len() % 3;
    let encoded_len = if pad {
        (full_chunks + usize::from(remainder > 0)) * 4
    } else {
        full_chunks * 4
            + match remainder {
                1 => 2,
                2 => 3,
                _ => 0,
            }
    };

    let mut out = Vec::with_capacity(encoded_len);

    // Process full 3-byte chunks
    for chunk in input.chunks_exact(3) {
        let n = (u32::from(chunk[0]) << 16) | (u32::from(chunk[1]) << 8) | u32::from(chunk[2]);
        out.push(alphabet[((n >> 18) & 0x3F) as usize]);
        out.push(alphabet[((n >> 12) & 0x3F) as usize]);
        out.push(alphabet[((n >> 6) & 0x3F) as usize]);
        out.push(alphabet[(n & 0x3F) as usize]);
    }

    // Process remainder
    let rem = input.chunks_exact(3).remainder();
    match rem.len() {
        1 => {
            let n = u32::from(rem[0]) << 16;
            out.push(alphabet[((n >> 18) & 0x3F) as usize]);
            out.push(alphabet[((n >> 12) & 0x3F) as usize]);
            if pad {
                out.push(b'=');
                out.push(b'=');
            }
        }
        2 => {
            let n = (u32::from(rem[0]) << 16) | (u32::from(rem[1]) << 8);
            out.push(alphabet[((n >> 18) & 0x3F) as usize]);
            out.push(alphabet[((n >> 12) & 0x3F) as usize]);
            out.push(alphabet[((n >> 6) & 0x3F) as usize]);
            if pad {
                out.push(b'=');
            }
        }
        _ => {}
    }

    // SAFETY: all output bytes are ASCII from the alphabet or '='
    unsafe { String::from_utf8_unchecked(out) }
}

fn decode_with_table(input: &[u8], table: &[u8; 256]) -> Result<Vec<u8>, DecodeError> {
    if input.is_empty() {
        return Ok(Vec::new());
    }

    // Count and validate padding (0, 1, or 2 trailing '=' allowed).
    let pad_count = input.iter().rev().take_while(|&&b| b == b'=').count();
    if pad_count > 2 {
        return Err(DecodeError::InvalidLength(input.len()));
    }
    let input = &input[..input.len() - pad_count];

    let full_chunks = input.len() / 4;
    let remainder = input.len() % 4;

    // Remainder of 1 is never valid (would encode only 6 bits, less than a byte)
    if remainder == 1 {
        return Err(DecodeError::InvalidLength(input.len()));
    }

    let output_len = full_chunks * 3
        + match remainder {
            2 => 1,
            3 => 2,
            _ => 0,
        };
    let mut out = Vec::with_capacity(output_len);

    let mut i = 0;

    // Process full 4-byte chunks
    while i + 4 <= input.len() {
        let a = decode_byte(table, input[i], i)?;
        let b = decode_byte(table, input[i + 1], i + 1)?;
        let c = decode_byte(table, input[i + 2], i + 2)?;
        let d = decode_byte(table, input[i + 3], i + 3)?;

        let n = (u32::from(a) << 18) | (u32::from(b) << 12) | (u32::from(c) << 6) | u32::from(d);
        out.push((n >> 16) as u8);
        out.push((n >> 8) as u8);
        out.push(n as u8);
        i += 4;
    }

    // Process remainder
    let rem = &input[i..];
    match rem.len() {
        2 => {
            let a = decode_byte(table, rem[0], i)?;
            let b = decode_byte(table, rem[1], i + 1)?;
            let n = (u32::from(a) << 18) | (u32::from(b) << 12);
            out.push((n >> 16) as u8);
        }
        3 => {
            let a = decode_byte(table, rem[0], i)?;
            let b = decode_byte(table, rem[1], i + 1)?;
            let c = decode_byte(table, rem[2], i + 2)?;
            let n = (u32::from(a) << 18) | (u32::from(b) << 12) | (u32::from(c) << 6);
            out.push((n >> 16) as u8);
            out.push((n >> 8) as u8);
        }
        _ => {}
    }

    Ok(out)
}

#[inline]
fn decode_byte(table: &[u8; 256], byte: u8, pos: usize) -> Result<u8, DecodeError> {
    let val = table[byte as usize];
    if val == 0xFF {
        Err(DecodeError::InvalidByte(pos, byte))
    } else {
        Ok(val)
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzz_base64_roundtrip_and_decode_never_panics() {
        // base64 carries untrusted payloads (e.g. OSC 52 clipboard data from any
        // program), so `decode` must NEVER panic on arbitrary input — only
        // Ok/Err — and `encode` ∘ `decode` must round-trip every byte string.
        let mut state: u64 = 0xC2B2_AE3D_27D4_EB4F;
        let mut next = move || {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (state >> 33) as u32
        };
        for _ in 0..50_000 {
            // Round-trip arbitrary bytes.
            let len = (next() % 64) as usize;
            let bytes: Vec<u8> = (0..len).map(|_| (next() & 0xFF) as u8).collect();
            let encoded = encode(&bytes);
            assert_eq!(decode(&encoded).expect("valid base64 must decode"), bytes);

            // Arbitrary (likely-invalid) string: must return cleanly, never panic.
            let slen = (next() % 80) as usize;
            let s: String = (0..slen)
                .map(|_| {
                    const ALPH: &[u8] =
                        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/=-_ \n\t!";
                    ALPH[(next() as usize) % ALPH.len()] as char
                })
                .collect();
            let _ = decode(&s);
        }
    }

    #[test]
    fn test_encode_empty() {
        assert_eq!(encode(b""), "");
    }

    #[test]
    fn test_encode_hello_world() {
        assert_eq!(encode(b"Hello, world!"), "SGVsbG8sIHdvcmxkIQ==");
    }

    #[test]
    fn test_encode_padding_one() {
        // 1 byte remainder -> 2 padding chars
        assert_eq!(encode(b"f"), "Zg==");
    }

    #[test]
    fn test_encode_padding_two() {
        // 2 byte remainder -> 1 padding char
        assert_eq!(encode(b"fo"), "Zm8=");
    }

    #[test]
    fn test_encode_no_padding() {
        // 3 byte multiple -> no padding
        assert_eq!(encode(b"foo"), "Zm9v");
    }

    #[test]
    fn test_decode_empty() {
        assert_eq!(decode("").unwrap(), b"");
    }

    #[test]
    fn test_decode_hello_world() {
        assert_eq!(decode("SGVsbG8sIHdvcmxkIQ==").unwrap(), b"Hello, world!");
    }

    #[test]
    fn test_decode_without_padding() {
        // Should work without padding too
        assert_eq!(decode("SGVsbG8sIHdvcmxkIQ").unwrap(), b"Hello, world!");
    }

    #[test]
    fn test_decode_invalid_char() {
        let result = decode("SGV!bG8=");
        assert!(result.is_err());
        if let Err(DecodeError::InvalidByte(pos, byte)) = result {
            assert_eq!(pos, 3);
            assert_eq!(byte, b'!');
        }
    }

    #[test]
    fn test_decode_invalid_length() {
        // Single char is never valid
        let result = decode("A");
        assert!(result.is_err());
        assert!(matches!(result, Err(DecodeError::InvalidLength(1))));
    }

    #[test]
    fn test_roundtrip_standard() {
        for input in [
            b"".as_slice(),
            b"a",
            b"ab",
            b"abc",
            b"abcd",
            b"Hello, world!",
            &[0u8; 256],
            &(0..=255).collect::<Vec<u8>>(),
        ] {
            let encoded = encode(input);
            let decoded = decode(&encoded).expect("roundtrip decode failed");
            assert_eq!(decoded, input);
        }
    }

    #[test]
    fn test_roundtrip_url_safe() {
        for input in [
            b"".as_slice(),
            b"a",
            b"ab",
            b"abc",
            b"Hello, world!",
            &(0..=255).collect::<Vec<u8>>(),
        ] {
            let encoded = encode_url_safe_no_pad(input);
            let decoded = decode_url_safe_no_pad(&encoded).expect("roundtrip decode failed");
            assert_eq!(decoded, input);
        }
    }

    #[test]
    fn test_url_safe_alphabet() {
        // URL-safe should not contain + or /
        let encoded = encode_url_safe_no_pad(&[0xFF, 0xFF, 0xFF]);
        assert!(!encoded.contains('+'));
        assert!(!encoded.contains('/'));
    }

    #[test]
    fn test_encode_no_pad_function() {
        assert_eq!(encode_no_pad(b"f"), "Zg");
        assert_eq!(encode_no_pad(b"fo"), "Zm8");
        assert_eq!(encode_no_pad(b"foo"), "Zm9v");
    }

    #[test]
    fn test_rfc4648_vectors() {
        // Test vectors from RFC 4648 section 10
        assert_eq!(encode(b""), "");
        assert_eq!(encode(b"f"), "Zg==");
        assert_eq!(encode(b"fo"), "Zm8=");
        assert_eq!(encode(b"foo"), "Zm9v");
        assert_eq!(encode(b"foob"), "Zm9vYg==");
        assert_eq!(encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn test_decode_excess_padding_rejected() {
        assert!(decode("Zg=====").is_err());
        assert!(decode("Zg===").is_err());
        // Valid padding counts still work.
        assert_eq!(decode("Zg==").unwrap(), b"f");
        assert_eq!(decode("Zm8=").unwrap(), b"fo");
        assert_eq!(decode("Zm9v").unwrap(), b"foo");
    }

    #[test]
    fn test_decode_all_padding_rejected() {
        assert!(decode("====").is_err());
        assert!(decode("===").is_err());
    }

    #[test]
    fn test_decode_error_display() {
        let err = DecodeError::InvalidByte(3, 0xFF);
        assert_eq!(err.to_string(), "invalid base64 byte 0xFF at position 3");

        let err = DecodeError::InvalidLength(5);
        assert_eq!(err.to_string(), "invalid base64 input length: 5");
    }
}
