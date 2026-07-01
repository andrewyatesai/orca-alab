//! NDJSON line framing — the byte-budgeted, oversized-discarding line splitter
//! ported from `src/main/daemon/ndjson.ts` (`createNdjsonParser`).
//!
//! The daemon socket is local but persistent; a peer that never sends a newline
//! must not grow the parser buffer without bound. This splitter accumulates
//! chunks, splits on `\n`, and enforces a per-line UTF-8 byte budget: a line that
//! would exceed the budget is dropped (an [`NdjsonEvent::Oversized`] is emitted)
//! and bytes are discarded until the next newline resynchronizes the stream.
//!
//! It does NOT parse JSON — it yields complete line strings; the caller runs
//! `JSON.parse` (kept in TS to avoid marshalling parsed values across the FFI).
//! Buffer length is UTF-8 bytes throughout: Rust `String::len()` is the byte count,
//! matching the TS `Buffer.byteLength(segment, 'utf8')`.
//!
//! INVARIANT (proven by `test_buffer_never_exceeds_budget` + `proofs/ay`): after any
//! `feed`, the retained buffer is `<= max_line_bytes` — the OOM bound.

/// Default per-line cap (16 MiB), mirroring `NDJSON_MAX_LINE_BYTES` in the TS.
pub const NDJSON_MAX_LINE_BYTES: usize = 16 * 1024 * 1024;

/// An event produced while feeding chunks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NdjsonEvent {
    /// A complete, non-empty line (newline stripped). The caller JSON-parses it.
    Line(String),
    /// A line exceeded the budget and was dropped; `observed_bytes` is the size
    /// that tripped the limit (buffer + segment). Mirrors the TS error report.
    Oversized { observed_bytes: usize },
}

/// Stateful NDJSON line splitter with a per-line byte budget. Accumulates across
/// `feed` calls; a partial (newline-less) tail is retained for the next chunk.
#[derive(Debug)]
pub struct NdjsonSplitter {
    buffer: String,
    discarding_oversized: bool,
    max_line_bytes: usize,
}

impl NdjsonSplitter {
    /// `max_line_bytes` is clamped to at least 1 (matches the TS `Math.max(1, …)`).
    #[must_use]
    pub fn new(max_line_bytes: usize) -> Self {
        Self {
            buffer: String::new(),
            discarding_oversized: false,
            max_line_bytes: max_line_bytes.max(1),
        }
    }

    /// The active per-line byte budget.
    #[must_use]
    pub fn max_line_bytes(&self) -> usize {
        self.max_line_bytes
    }

    /// UTF-8 bytes currently retained (the partial line). Always `<= max_line_bytes`.
    #[must_use]
    pub fn buffered_bytes(&self) -> usize {
        self.buffer.len()
    }

    /// Feed a decoded chunk; append complete lines / oversized reports to `out`.
    pub fn feed(&mut self, chunk: &str, out: &mut Vec<NdjsonEvent>) {
        let mut remaining = chunk;
        while !remaining.is_empty() {
            // '\n' is ASCII, so its byte index is a valid char boundary for slicing.
            let (segment, rest, has_newline) = match remaining.find('\n') {
                Some(i) => (&remaining[..i], &remaining[i + 1..], true),
                None => (remaining, "", false),
            };
            remaining = rest;

            if self.discarding_oversized {
                if has_newline {
                    self.discarding_oversized = false;
                    self.buffer.clear();
                    continue;
                }
                return;
            }

            // UTF-8 byte lengths (String::len / str::len), matching Buffer.byteLength utf8.
            let next_line_bytes = self.buffer.len() + segment.len();
            if next_line_bytes > self.max_line_bytes {
                out.push(NdjsonEvent::Oversized { observed_bytes: next_line_bytes });
                self.buffer.clear();
                if !has_newline {
                    self.discarding_oversized = true;
                    return;
                }
                continue;
            }

            self.buffer.push_str(segment);
            if !has_newline {
                return;
            }

            let line = std::mem::take(&mut self.buffer);
            if line.is_empty() {
                continue;
            }
            out.push(NdjsonEvent::Line(line));
        }
    }

    /// Convenience: feed a chunk and return the events it produced.
    pub fn feed_collect(&mut self, chunk: &str) -> Vec<NdjsonEvent> {
        let mut out = Vec::new();
        self.feed(chunk, &mut out);
        out
    }

    /// Drop the partial line + oversized state (peer reset).
    pub fn reset(&mut self) {
        self.buffer.clear();
        self.discarding_oversized = false;
    }
}

/// Encode a JSON string as one NDJSON record (`{json}\n`), mirroring `encodeNdjson`.
#[must_use]
pub fn encode_ndjson_line(json: &str) -> String {
    let mut s = String::with_capacity(json.len() + 1);
    s.push_str(json);
    s.push('\n');
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(events: Vec<NdjsonEvent>) -> Vec<String> {
        events
            .into_iter()
            .filter_map(|e| match e {
                NdjsonEvent::Line(l) => Some(l),
                NdjsonEvent::Oversized { .. } => None,
            })
            .collect()
    }

    #[test]
    fn splits_complete_lines() {
        let mut p = NdjsonSplitter::new(NDJSON_MAX_LINE_BYTES);
        assert_eq!(lines(p.feed_collect("{\"a\":1}\n{\"b\":2}\n")), ["{\"a\":1}", "{\"b\":2}"]);
    }

    #[test]
    fn holds_a_partial_line_across_feeds() {
        let mut p = NdjsonSplitter::new(NDJSON_MAX_LINE_BYTES);
        assert!(p.feed_collect("{\"a\":").is_empty());
        assert_eq!(lines(p.feed_collect("1}\n")), ["{\"a\":1}"]);
    }

    #[test]
    fn skips_empty_lines() {
        let mut p = NdjsonSplitter::new(NDJSON_MAX_LINE_BYTES);
        assert_eq!(lines(p.feed_collect("\n\n{\"a\":1}\n\n")), ["{\"a\":1}"]);
    }

    #[test]
    fn counts_utf8_bytes_not_chars_for_the_budget() {
        // '€' is 3 UTF-8 bytes. Budget of 3 admits exactly one '€' line.
        let mut p = NdjsonSplitter::new(3);
        assert_eq!(lines(p.feed_collect("€\n")), ["€"]);
        // Two '€' before a newline = 6 bytes > 3 → oversized (dropped).
        let events = p.feed_collect("€€\n");
        assert!(matches!(events.as_slice(), [NdjsonEvent::Oversized { observed_bytes: 6 }]));
    }

    #[test]
    fn oversized_line_is_dropped_then_stream_resyncs_at_next_newline() {
        // Budget 8 admits the resync line {"ok":1} (8 bytes) but not the garbage.
        let mut p = NdjsonSplitter::new(8);
        // 14 bytes, no newline → trips the budget → oversized + discard mode.
        let e1 = p.feed_collect("toolongtoolong");
        assert!(matches!(e1.as_slice(), [NdjsonEvent::Oversized { observed_bytes: 14 }]));
        // Continued garbage is discarded until a newline; then the next line parses.
        assert!(p.feed_collect("more-garbage").is_empty());
        assert_eq!(lines(p.feed_collect("\n{\"ok\":1}\n")), ["{\"ok\":1}"]);
    }

    #[test]
    fn reset_drops_partial_and_discard_state() {
        let mut p = NdjsonSplitter::new(NDJSON_MAX_LINE_BYTES);
        let _ = p.feed_collect("{\"partial\":");
        p.reset();
        assert_eq!(p.buffered_bytes(), 0);
        assert_eq!(lines(p.feed_collect("{\"a\":1}\n")), ["{\"a\":1}"]);
    }

    #[test]
    fn test_buffer_never_exceeds_budget() {
        // The OOM invariant: no feed sequence ever grows the retained buffer past
        // the budget — the whole point of the splitter on an untrusted socket.
        let big = "x".repeat(100);
        for budget in [1usize, 3, 7, 64] {
            let mut p = NdjsonSplitter::new(budget);
            let chunks = ["a", "bb", "€", "cccccccc", "\n", "dd€ee", big.as_str()];
            for c in chunks {
                let _ = p.feed_collect(c);
                assert!(
                    p.buffered_bytes() <= budget,
                    "buffer {} exceeded budget {}",
                    p.buffered_bytes(),
                    budget
                );
            }
        }
    }

    #[test]
    fn encode_ndjson_line_appends_newline() {
        assert_eq!(encode_ndjson_line("{\"a\":1}"), "{\"a\":1}\n");
    }
}
