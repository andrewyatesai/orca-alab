//! Base64 (standard + url-safe), shared by the relay protocols. The pairing
//! deep link uses url-safe-no-pad; the E2EE channel uses standard base64 (the
//! `Buffer.toString('base64')` form the peers speak). `decode` accepts either.

const STANDARD: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const URL_SAFE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

pub(crate) fn encode_standard(input: &[u8]) -> String {
    encode(input, STANDARD, true)
}

pub(crate) fn encode_url_safe_no_pad(input: &[u8]) -> String {
    encode(input, URL_SAFE, false)
}

fn encode(input: &[u8], alphabet: &[u8; 64], pad: bool) -> String {
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b1 = chunk.get(1).copied();
        let b2 = chunk.get(2).copied();
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(b1.unwrap_or(0)) << 8)
            | u32::from(b2.unwrap_or(0));
        out.push(alphabet[((n >> 18) & 63) as usize] as char);
        out.push(alphabet[((n >> 12) & 63) as usize] as char);
        match b1 {
            Some(_) => out.push(alphabet[((n >> 6) & 63) as usize] as char),
            None if pad => out.push('='),
            None => {}
        }
        match b2 {
            Some(_) => out.push(alphabet[(n & 63) as usize] as char),
            None if pad => out.push('='),
            None => {}
        }
    }
    out
}

/// Decode standard or url-safe base64, returning `None` on a non-base64
/// character. Uses a bounded bit-accumulator (masks consumed high bits) so it
/// stays panic-free under `forbid(unsafe)` for any input length.
pub(crate) fn decode(input: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let mut acc: u32 = 0;
    let mut bits: u32 = 0;
    for ch in input.chars() {
        let mapped = match ch {
            '-' => '+',
            '_' => '/',
            '=' => break, // padding terminates the data
            other => other,
        };
        let sextet = value(mapped)?;
        acc = (acc << 6) | u32::from(sextet);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

fn value(ch: char) -> Option<u8> {
    match ch {
        'A'..='Z' => Some(ch as u8 - b'A'),
        'a'..='z' => Some(ch as u8 - b'a' + 26),
        '0'..='9' => Some(ch as u8 - b'0' + 52),
        '+' => Some(62),
        '/' => Some(63),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_round_trips_with_padding() {
        assert_eq!(encode_standard(b"hello"), "aGVsbG8=");
        assert_eq!(encode_standard(br#"{"hi":1}"#), "eyJoaSI6MX0=");
        assert_eq!(decode("aGVsbG8=").unwrap(), b"hello");
        assert_eq!(decode(&encode_standard(&[0u8, 255, 16, 200, 3])).unwrap(), [0u8, 255, 16, 200, 3]);
    }

    #[test]
    fn url_safe_has_no_pad_or_plus_slash() {
        let code = encode_url_safe_no_pad(&[0xfb, 0xff, 0xbf]);
        assert!(!code.contains('+') && !code.contains('/') && !code.contains('='));
        assert_eq!(decode(&code).unwrap(), [0xfb, 0xff, 0xbf]);
    }

    #[test]
    fn rejects_non_base64_characters() {
        assert_eq!(decode("not base64!"), None);
    }
}
