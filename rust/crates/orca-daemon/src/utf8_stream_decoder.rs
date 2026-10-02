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

        // Append onto the carried tail itself (leaving `self.tail` empty): no
        // separately sized buffer, so no length sum to trust.
        let mut combined: Vec<u8> = std::mem::take(&mut self.tail);
        combined.extend_from_slice(bytes);

        // No up-front reservation sized from the input: the first `push_str` of
        // the valid prefix allocates exactly what it needs (the common split-char
        // case is that one push, then the carry), and only invalid sequences grow
        // it further. Output is identical; no input length becomes an allocation
        // request on its own.
        let mut out = String::new();
        let mut rest: &[u8] = &combined;
        loop {
            match std::str::from_utf8(rest) {
                Ok(s) => {
                    out.push_str(s);
                    break;
                }
                Err(e) => {
                    // `valid_up_to` is an in-bounds char boundary and `error_len`
                    // fits after it (the decoder's contract); checked splits rely on
                    // that only for the result, never for a panic. Were it ever
                    // violated, the remainder is lossy-decoded — the same output
                    // class as any other invalid input.
                    let Some((valid, after)) = rest.split_at_checked(e.valid_up_to()) else {
                        out.push_str(&String::from_utf8_lossy(rest));
                        break;
                    };
                    // `valid` is UTF-8 by definition of valid_up_to (the crate
                    // forbids unsafe, so no _unchecked).
                    if let Ok(s) = std::str::from_utf8(valid) {
                        out.push_str(s);
                    }
                    match e.error_len() {
                        // Incomplete trailing char (split across the boundary): carry
                        // the remainder for the next chunk instead of replacing it.
                        None => {
                            self.tail.extend_from_slice(after);
                            break;
                        }
                        // Genuinely invalid sequence: emit U+FFFD, skip it, continue —
                        // identical to from_utf8_lossy, so no boundary-carry hides real
                        // corruption.
                        Some(bad) => {
                            out.push('\u{FFFD}');
                            rest = after.get(bad..).unwrap_or_default();
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
    fn invalid_runs_match_from_utf8_lossy_including_after_a_carried_tail() {
        let cases: [&[u8]; 6] = [
            b"\xff\xfe",
            b"a\xe2\x82b\xf0\x9f\xa6c",
            b"\xc3\x28",
            b"\xed\xa0\x80x",
            b"ok\x80\x80\x80",
            b"\xf4\x90\x80\x80!",
        ];
        for case in cases {
            let mut d = Utf8StreamDecoder::new();
            assert_eq!(d.decode(case), String::from_utf8_lossy(case), "{case:?}");
            // The same bytes after a carried lead byte that they fail to complete.
            let mut d = Utf8StreamDecoder::new();
            let mut out = d.decode(b"\xe2");
            out.push_str(&d.decode(case));
            let mut joined = vec![0xe2u8];
            joined.extend_from_slice(case);
            assert_eq!(out, String::from_utf8_lossy(&joined), "carried + {case:?}");
        }
    }

    #[test]
    fn incomplete_tail_at_end_then_completed_next_call() {
        let mut d = Utf8StreamDecoder::new();
        // First chunk ends with a lone lead byte of "é" (C3 A9).
        assert_eq!(d.decode(b"x\xc3"), "x");
        assert_eq!(d.decode(b"\xa9y"), "éy");
    }
}
