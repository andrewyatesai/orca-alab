//! Incremental UTF-8 decoder that carries a partial multibyte character across
//! read boundaries. The daemon reads the socket and the PTY in fixed-size chunks;
//! a `from_utf8_lossy` per chunk turns any multibyte character split across a
//! boundary (CJK, emoji, box-drawing) into U+FFFD. That corrupts large pastes on
//! the inbound `write` path and desyncs the live output stream / checkpoint records
//! from the (raw-byte-fed, correct) engine grid. This decoder holds the trailing
//! incomplete bytes (at most 3) until the next chunk completes them, matching the
//! Node daemon's StringDecoder. Genuinely invalid byte sequences still become
//! U+FFFD, exactly like `from_utf8_lossy`.

#[derive(Default)]
pub struct Utf8StreamDecoder {
    /// Trailing bytes of an incomplete multibyte char from the previous chunk
    /// (< 4 bytes). Empty when the last chunk ended on a character boundary.
    tail: Vec<u8>,
}

impl Utf8StreamDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Decode `bytes` as a continuation of any carried tail. Returns the text for
    /// the complete-character prefix and stashes an incomplete trailing character
    /// for the next call.
    pub fn decode(&mut self, bytes: &[u8]) -> String {
        // Fast path: nothing carried and the whole chunk is valid UTF-8 ending on a
        // boundary — the common case for ASCII-heavy terminal traffic.
        if self.tail.is_empty() {
            if let Ok(s) = std::str::from_utf8(bytes) {
                return s.to_string();
            }
        }

        let mut combined: Vec<u8> = Vec::with_capacity(self.tail.len() + bytes.len());
        combined.extend_from_slice(&self.tail);
        combined.extend_from_slice(bytes);
        self.tail.clear();

        let mut out = String::with_capacity(combined.len());
        let mut rest: &[u8] = &combined;
        loop {
            match std::str::from_utf8(rest) {
                Ok(s) => {
                    out.push_str(s);
                    break;
                }
                Err(e) => {
                    let valid = e.valid_up_to();
                    // [..valid] is valid UTF-8 by definition of valid_up_to, so this
                    // never panics (the crate forbids unsafe, so no _unchecked).
                    if let Ok(s) = std::str::from_utf8(&rest[..valid]) {
                        out.push_str(s);
                    }
                    match e.error_len() {
                        // Incomplete trailing char (split across the boundary): carry
                        // the remainder for the next chunk instead of replacing it.
                        None => {
                            self.tail.extend_from_slice(&rest[valid..]);
                            break;
                        }
                        // Genuinely invalid sequence: emit U+FFFD, skip it, continue —
                        // identical to from_utf8_lossy, so no boundary-carry hides real
                        // corruption.
                        Some(bad) => {
                            out.push('\u{FFFD}');
                            rest = &rest[valid + bad..];
                        }
                    }
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_ascii_passes_through() {
        let mut d = Utf8StreamDecoder::new();
        assert_eq!(d.decode(b"hello world"), "hello world");
    }

    #[test]
    fn multibyte_split_across_two_chunks_is_not_corrupted() {
        // "日本" = E6 97 A5 E6 9C AC. Split mid-first-char and mid-second-char.
        let bytes = "日本".as_bytes();
        let mut d = Utf8StreamDecoder::new();
        let mut out = String::new();
        out.push_str(&d.decode(&bytes[..2])); // E6 97 (incomplete)
        out.push_str(&d.decode(&bytes[2..4])); // A5 E6 (completes first, starts second)
        out.push_str(&d.decode(&bytes[4..])); // 9C AC (completes second)
        assert_eq!(out, "日本");
    }

    #[test]
    fn emoji_split_across_chunks_is_not_corrupted() {
        // "🦀" = F0 9F A6 80 (4 bytes). Feed one byte at a time.
        let bytes = "🦀".as_bytes();
        let mut d = Utf8StreamDecoder::new();
        let mut out = String::new();
        for b in bytes {
            out.push_str(&d.decode(std::slice::from_ref(b)));
        }
        assert_eq!(out, "🦀");
    }

    #[test]
    fn genuinely_invalid_bytes_become_replacement_like_lossy() {
        let mut d = Utf8StreamDecoder::new();
        // 0xFF is never valid in UTF-8; surrounding ASCII must survive.
        assert_eq!(d.decode(b"a\xffb"), "a\u{FFFD}b");
    }

    #[test]
    fn incomplete_tail_at_end_then_completed_next_call() {
        let mut d = Utf8StreamDecoder::new();
        // First chunk ends with a lone lead byte of "é" (C3 A9).
        assert_eq!(d.decode(b"x\xc3"), "x");
        assert_eq!(d.decode(b"\xa9y"), "éy");
    }
}
