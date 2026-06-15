// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0

//! Hexadecimal encoding and decoding.

use std::fmt;

/// Error during hex decoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// An invalid hex character was encountered at the given position.
    InvalidByte(usize, u8),
    /// The input length is odd (hex requires pairs of characters).
    OddLength(usize),
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidByte(pos, byte) => {
                write!(f, "invalid hex byte 0x{byte:02X} at position {pos}")
            }
            Self::OddLength(len) => {
                write!(f, "odd hex input length: {len}")
            }
        }
    }
}

impl std::error::Error for DecodeError {}

/// Encode bytes to lowercase hexadecimal.
#[must_use]
pub fn encode(input: &[u8]) -> String {
    let mut out = String::with_capacity(input.len() * 2);
    for &byte in input {
        out.push(HEX_LOWER[(byte >> 4) as usize] as char);
        out.push(HEX_LOWER[(byte & 0x0F) as usize] as char);
    }
    out
}

/// Encode bytes to uppercase hexadecimal.
#[must_use]
pub fn encode_upper(input: &[u8]) -> String {
    let mut out = String::with_capacity(input.len() * 2);
    for &byte in input {
        out.push(HEX_UPPER[(byte >> 4) as usize] as char);
        out.push(HEX_UPPER[(byte & 0x0F) as usize] as char);
    }
    out
}

/// Decode a hexadecimal string to bytes.
///
/// Accepts both uppercase and lowercase hex digits.
///
/// # Errors
///
/// Returns [`DecodeError`] if the input has odd length or contains
/// non-hex characters.
pub fn decode(input: &str) -> Result<Vec<u8>, DecodeError> {
    let bytes = input.as_bytes();
    if !bytes.len().is_multiple_of(2) {
        return Err(DecodeError::OddLength(bytes.len()));
    }

    let mut out = Vec::with_capacity(bytes.len() / 2);
    for i in (0..bytes.len()).step_by(2) {
        let high = decode_nibble(bytes[i], i)?;
        let low = decode_nibble(bytes[i + 1], i + 1)?;
        out.push((high << 4) | low);
    }

    Ok(out)
}

const HEX_LOWER: &[u8; 16] = b"0123456789abcdef";
const HEX_UPPER: &[u8; 16] = b"0123456789ABCDEF";

#[inline]
fn decode_nibble(byte: u8, pos: usize) -> Result<u8, DecodeError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(DecodeError::InvalidByte(pos, byte)),
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzz_hex_roundtrip_and_decode_never_panics() {
        // `decode` runs on untrusted strings; it must NEVER panic — only Ok/Err —
        // and `encode` ∘ `decode` must round-trip every byte string.
        let mut state: u64 = 0x3C6E_F372_FE94_F82B;
        let mut next = move || {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (state >> 33) as u32
        };
        for _ in 0..50_000 {
            let len = (next() % 64) as usize;
            let bytes: Vec<u8> = (0..len).map(|_| (next() & 0xFF) as u8).collect();
            assert_eq!(decode(&encode(&bytes)).expect("valid hex must decode"), bytes);

            // Arbitrary string (odd lengths, non-hex chars): clean Result, no panic.
            let slen = (next() % 80) as usize;
            let s: String = (0..slen)
                .map(|_| {
                    const ALPH: &[u8] = b"0123456789abcdefABCDEFxyzXYZ !\n\t-_";
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
    fn test_encode_deadbeef() {
        assert_eq!(encode(&[0xDE, 0xAD, 0xBE, 0xEF]), "deadbeef");
    }

    #[test]
    fn test_encode_upper() {
        assert_eq!(encode_upper(&[0xDE, 0xAD, 0xBE, 0xEF]), "DEADBEEF");
    }

    #[test]
    fn test_encode_all_bytes() {
        let input: Vec<u8> = (0..=255).collect();
        let encoded = encode(&input);
        assert_eq!(encoded.len(), 512);
        assert!(encoded.starts_with("000102"));
        assert!(encoded.ends_with("fdfeff"));
    }

    #[test]
    fn test_decode_empty() {
        assert_eq!(decode("").unwrap(), b"");
    }

    #[test]
    fn test_decode_deadbeef() {
        assert_eq!(decode("deadbeef").unwrap(), &[0xDE, 0xAD, 0xBE, 0xEF]);
    }

    #[test]
    fn test_decode_uppercase() {
        assert_eq!(decode("DEADBEEF").unwrap(), &[0xDE, 0xAD, 0xBE, 0xEF]);
    }

    #[test]
    fn test_decode_mixed_case() {
        assert_eq!(decode("DeAdBeEf").unwrap(), &[0xDE, 0xAD, 0xBE, 0xEF]);
    }

    #[test]
    fn test_decode_odd_length() {
        let result = decode("abc");
        assert!(matches!(result, Err(DecodeError::OddLength(3))));
    }

    #[test]
    fn test_decode_invalid_char() {
        let result = decode("zz");
        assert!(result.is_err());
        if let Err(DecodeError::InvalidByte(pos, byte)) = result {
            assert_eq!(pos, 0);
            assert_eq!(byte, b'z');
        }
    }

    #[test]
    fn test_roundtrip() {
        for input in [
            b"".as_slice(),
            b"\x00",
            b"\xff",
            &(0..=255).collect::<Vec<u8>>(),
            b"Hello, world!",
        ] {
            let encoded = encode(input);
            let decoded = decode(&encoded).expect("roundtrip decode failed");
            assert_eq!(decoded, input);
        }
    }

    #[test]
    fn test_decode_error_display() {
        let err = DecodeError::InvalidByte(5, b'z');
        assert_eq!(err.to_string(), "invalid hex byte 0x7A at position 5");

        let err = DecodeError::OddLength(3);
        assert_eq!(err.to_string(), "odd hex input length: 3");
    }
}
