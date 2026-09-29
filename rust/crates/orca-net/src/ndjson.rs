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
//! INVARIANT (proven by `buffer_never_exceeds_budget_on_any_chunking` + `proofs/ay`):
//! after any `feed`, the retained buffer is `<= max_line_bytes` — the OOM bound.

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
            // Explicit compare, not `.max(1)`: Trust cannot model `Ord::max`'s body.
            max_line_bytes: if max_line_bytes == 0 { 1 } else { max_line_bytes },
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
            // `split_once` rather than `find` + index: it yields both halves without a
            // slice-bounds obligation on an index Trust models as unconstrained.
            let (segment, rest, has_newline) = match remaining.split_once('\n') {
                Some((segment, rest)) => (segment, rest, true),
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
            // Two live allocation lengths can never sum past usize::MAX, but `len()` is
            // opaque to Trust, so spell the non-overflow out.
            let next_line_bytes = self.buffer.len().saturating_add(segment.len());
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
    // Capacity is only a reservation, so capping the hint at the line budget leaves
    // the result identical while giving the up-front allocation a constant bound
    // (an unbounded `json.len()` reservation is an unprovable bulk allocation).
    let exact = json.len().saturating_add(1);
    let hint = if exact > NDJSON_MAX_LINE_BYTES { NDJSON_MAX_LINE_BYTES } else { exact };
    let mut s = String::with_capacity(hint);
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

    /// The OOM invariant, over every chunking of a small alphabet rather than one
    /// hand-picked sequence: no feed sequence ever grows the retained buffer past
    /// the budget — the whole point of the splitter on an untrusted socket.
    ///
    /// Complements `proofs/ay/oom_bound`, whose `oom_buffer_le_max` reasons about
    /// a single push in isolation under an assumed guard. What it cannot reach —
    /// that the guard is placed correctly relative to every other arm (oversized,
    /// discarding, the newline `mem::take`), across statement boundaries and
    /// across `feed` calls — is what this walks.
    #[test]
    fn buffer_never_exceeds_budget_on_any_chunking() {
        // "€" is 3 UTF-8 bytes: the budget counts bytes, not chars, so a multi-byte
        // segment can cross a budget the char count would not.
        const ALPHABET: [&str; 4] = ["a", "bb", "€", "\n"];
        let big = "x".repeat(100);

        let mut frontier: Vec<Vec<&str>> = vec![Vec::new()];
        let mut sequences: Vec<Vec<&str>> = vec![Vec::new()];
        for _ in 0..4 {
            let mut next = Vec::new();
            for seq in &frontier {
                for c in ALPHABET {
                    let mut s = seq.clone();
                    s.push(c);
                    next.push(s);
                }
            }
            sequences.extend(next.iter().cloned());
            frontier = next;
        }
        assert!(sequences.len() > 300, "enumeration too thin ({})", sequences.len());

        let mut saw_oversized = 0;
        let mut saw_line = 0;
        let mut saw_brim = 0;

        for budget in [1usize, 2, 3, 4, 7, 64] {
            for seq in &sequences {
                let mut p = NdjsonSplitter::new(budget);
                for c in seq.iter().copied().chain([big.as_str()]) {
                    for ev in p.feed_collect(c) {
                        match ev {
                            NdjsonEvent::Oversized { observed_bytes } => {
                                assert!(observed_bytes > budget, "reported a non-oversized line");
                                saw_oversized += 1;
                            }
                            // An emitted line is bounded too — it is the buffer at
                            // the moment of the `mem::take`.
                            NdjsonEvent::Line(l) => {
                                assert!(l.len() <= budget, "emitted a {}-byte line", l.len());
                                if l.len() == budget {
                                    saw_brim += 1;
                                }
                                saw_line += 1;
                            }
                        }
                    }
                    assert!(
                        p.buffered_bytes() <= budget,
                        "buffer {} exceeded budget {budget} after {seq:?} + {c:?}",
                        p.buffered_bytes()
                    );
                }
                // The trailing 100-byte chunk overflows every budget here, so the
                // splitter must end in the discarding state with nothing retained.
                assert_eq!(p.buffered_bytes(), 0, "retained bytes after an oversized tail");
            }
        }

        // Non-vacuity, the in-source form of the `oom_nonvacuity_sat` and
        // `oom_catches_unguarded_sat` controls: the budget is genuinely reached at
        // the brim (the bound is tight, not loose) and the guard genuinely fires
        // (it is load-bearing, not dead).
        assert!(saw_brim > 0, "no line ever filled the budget — the bound is loose");
        assert!(saw_oversized > 0, "the oversized guard never fired — it is not load-bearing");
        assert!(saw_line > 0, "no line was ever emitted");
    }

    #[test]
    fn encode_ndjson_line_appends_newline() {
        assert_eq!(encode_ndjson_line("{\"a\":1}"), "{\"a\":1}\n");
    }
}
