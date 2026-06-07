//! `encodeURIComponent` / `decodeURIComponent` equivalents shared by modules
//! that build or parse URL/id segments (hosted-remote URLs, browser search,
//! web-terminal surface ids). Kept here so callers don't reach across domains
//! for the primitive.

/// `encodeURIComponent`: keep unreserved `A-Za-z0-9-_.!~*'()`, percent-escape the rest.
pub fn encode_uri_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// `decodeURIComponent` for a single part; returns the original on a malformed
/// `%`-escape (mirroring the TS try/catch).
pub fn decode_uri_component(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 < bytes.len() {
                let hi = (bytes[i + 1] as char).to_digit(16);
                let lo = (bytes[i + 2] as char).to_digit(16);
                if let (Some(hi), Some(lo)) = (hi, lo) {
                    out.push((hi * 16 + lo) as u8);
                    i += 3;
                    continue;
                }
            }
            return s.to_string();
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_reserved_and_keeps_unreserved() {
        assert_eq!(encode_uri_component("a b/c?d"), "a%20b%2Fc%3Fd");
        assert_eq!(encode_uri_component("keep-_.!~*'()"), "keep-_.!~*'()");
        assert_eq!(encode_uri_component("café"), "caf%C3%A9");
    }

    #[test]
    fn round_trips_and_passes_through_malformed_escapes() {
        assert_eq!(decode_uri_component(&encode_uri_component("a b/c::d")), "a b/c::d");
        assert_eq!(decode_uri_component("caf%C3%A9"), "café");
        // Malformed escape → original string, like the TS try/catch.
        assert_eq!(decode_uri_component("%zz"), "%zz");
        assert_eq!(decode_uri_component("trailing%2"), "trailing%2");
    }
}
