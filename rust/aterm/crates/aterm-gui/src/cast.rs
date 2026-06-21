// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! Per-session **asciicast v2** recording of the child's program output
//! (design Addendum A.5.1 / B.7).
//!
//! [`CastRecorder`] accumulates coalesced PTY-output bursts as asciicast v2
//! events — a JSON header line then one `[t, "o", "<data>"]` record per burst
//! plus `[t, "r", "<cols>x<rows>"]` on resize — and serializes them with
//! [`CastRecorder::to_asciicast`] into a string `asciinema play`/`agg` accept.
//!
//! ## Why a real serializer, not a byte tee (A.5.1)
//! 1. **JSON-escape every chunk** (`"`, `\`, control bytes, `\n \r \t \uXXXX`)
//!    and UTF-8-lossy decode — PTY output is binary and routinely splits a
//!    multibyte sequence across reads; raw bytes in a JSON string are invalid
//!    JSON and kill the whole replay.
//! 2. **Output only** — the recorder is fed `buf[..r]` (genuine program output)
//!    at the reader-thread tap; the reader's `take_response()` query replies are
//!    the terminal's OWN bytes and MUST NOT appear as `"o"` events. Enforcing
//!    that is the *caller's* contract (this module never sees responses).
//! 3. **Monotonic non-decreasing timestamps** from one epoch captured at
//!    recorder construction; inter-event deltas are clamped to ≥ 0.
//! 4. **Bounded** — a byte budget with drop-oldest, so an idle terminal costs
//!    nothing and a flood cannot balloon RAM.
//!
//! The recorder does no fs/socket/lock work; the GUI hands bursts to it
//! lock-free off a dedicated writer thread (mirroring the OSC52 clipboard
//! thread), so the reader's hot path is never serialized under `term_lock`.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Default byte budget for the retained event payloads: a flood cannot balloon
/// RAM past this, and an idle terminal costs nothing (no events ⇒ no bytes).
pub const DEFAULT_BUDGET_BYTES: usize = 4 * 1024 * 1024;

/// One recorded asciicast v2 event: program output (`"o"`) or a resize (`"r"`).
enum Event {
    /// `[t, "o", "<json-escaped(bytes)>"]` — a coalesced output burst, stored as
    /// the RAW bytes (not a lossy-decoded `String`): non-UTF-8 and multibyte
    /// sequences split across reads survive verbatim, escaped only at render time.
    Output { t: Duration, data: Vec<u8> },
    /// `[t, "r", "<cols>x<rows>"]` — a geometry change the program observed.
    Resize { t: Duration, cols: u16, rows: u16 },
}

impl Event {
    /// The byte cost charged against the budget: the retained payload length.
    fn cost(&self) -> usize {
        match self {
            Event::Output { data, .. } => data.len(),
            // A resize payload is tiny + fixed-ish; charge its rendered length.
            Event::Resize { .. } => 16,
        }
    }
}

/// Accumulates program-output bursts as asciicast v2 and serializes them.
///
/// Construct with the child's initial grid size; feed coalesced output bursts
/// with [`record_output`](Self::record_output) and geometry changes with
/// [`record_resize`](Self::record_resize); render with
/// [`to_asciicast`](Self::to_asciicast).
pub struct CastRecorder {
    /// asciicast v2 header width (cols), snapshotted at construction.
    width: u16,
    /// asciicast v2 header height (rows), snapshotted at construction.
    height: u16,
    /// Recorded events, oldest first; drop-oldest under [`budget`](Self::budget).
    events: std::collections::VecDeque<Event>,
    /// Sum of `Event::cost()` over `events` (kept in step with the deque).
    used: usize,
    /// The retained-payload byte budget; drop-oldest when `used` would exceed it.
    budget: usize,
    /// The last emitted timestamp, so we clamp deltas to be non-decreasing even
    /// if a caller hands a `t` that went backwards.
    last_t: Duration,
    /// The monotonic epoch this recorder's timeline is relative to, captured at
    /// construction. [`now`](Self::now) reads it so the output-burst tap (reader
    /// thread) and the resize tap (main thread) share ONE timeline per session.
    epoch: Instant,
    /// A trailing INCOMPLETE multibyte UTF-8 lead carried from the previous burst:
    /// PTY reads routinely split a multibyte sequence across the 64 KiB boundary,
    /// so we hold the dangling lead bytes here and prepend them to the next burst,
    /// reassembling the character losslessly instead of emitting a U+FFFD that the
    /// continuation in the next read would never repair. Always ≤ 3 bytes.
    pending: Vec<u8>,
}

impl CastRecorder {
    /// A recorder for a `cols`×`rows` grid with the default 4 MiB budget.
    pub fn new(cols: u16, rows: u16) -> Self {
        Self::with_budget(cols, rows, DEFAULT_BUDGET_BYTES)
    }

    /// A recorder with an explicit retained-payload byte budget (≥ 1).
    pub fn with_budget(cols: u16, rows: u16, budget: usize) -> Self {
        Self {
            width: cols,
            height: rows,
            events: std::collections::VecDeque::new(),
            used: 0,
            budget: budget.max(1),
            last_t: Duration::ZERO,
            epoch: Instant::now(),
            pending: Vec::new(),
        }
    }

    /// The current relative timestamp on this recorder's timeline (elapsed since
    /// its construction epoch). Both taps call this so a resize event recorded on
    /// the main thread and an output event recorded on the reader thread share
    /// one consistent, monotonic-able timeline.
    pub fn now(&self) -> Duration {
        self.epoch.elapsed()
    }

    /// Clamp `t` to be non-decreasing w.r.t. the last emitted timestamp.
    fn monotonic(&mut self, t: Duration) -> Duration {
        let t = t.max(self.last_t);
        self.last_t = t;
        t
    }

    /// Push `ev`, then drop oldest events until the budget holds. We never drop
    /// the event just pushed (a single burst over budget is truncated only by
    /// the caller's read size, not here), so the most recent activity survives.
    fn push(&mut self, ev: Event) {
        self.used += ev.cost();
        self.events.push_back(ev);
        while self.used > self.budget && self.events.len() > 1 {
            if let Some(old) = self.events.pop_front() {
                self.used = self.used.saturating_sub(old.cost());
            }
        }
    }

    /// Record a coalesced output burst at relative time `t` (since the epoch the
    /// caller captured at recorder start). `bytes` is genuine PROGRAM OUTPUT —
    /// never the terminal's own query replies (`take_response()`).
    pub fn record_output(&mut self, t: Duration, bytes: &[u8]) {
        let t = self.monotonic(t);
        // Reassemble across reads: prepend any incomplete lead carried from the
        // previous burst, then peel off a NEW incomplete trailing lead to carry
        // forward. What remains is byte-exact, completable program output.
        let mut buf = std::mem::take(&mut self.pending);
        buf.extend_from_slice(bytes);
        let tail = incomplete_tail_len(&buf);
        let split = buf.len() - tail;
        self.pending = buf[split..].to_vec();
        buf.truncate(split);
        if buf.is_empty() {
            return; // the whole burst was an incomplete lead; carried, nothing complete yet
        }
        self.push(Event::Output { t, data: buf });
    }

    /// Record a geometry change (`[t, "r", "<cols>x<rows>"]`) at relative `t`.
    pub fn record_resize(&mut self, t: Duration, cols: u16, rows: u16) {
        let t = self.monotonic(t);
        self.push(Event::Resize { t, cols, rows });
    }

    /// Number of recorded events (output + resize), for tests/introspection.
    pub fn event_count(&self) -> usize {
        self.events.len()
    }

    /// Serialize to asciicast v2 text: a header line then one event line each.
    /// The result is newline-terminated and ready to write to `screen.cast` /
    /// hand to `asciinema play`.
    ///
    /// A `cast` snapshot reflects output up to the last COMPLETE-character
    /// boundary: a trailing incomplete multibyte lead is parked in `pending`
    /// (reassembled into the next burst) and is deliberately NOT emitted here, so
    /// `to_asciicast` is idempotent and never invents a phantom U+FFFD. A live
    /// consumer that needs every byte the instant it lands uses the byte-exact
    /// `subscribe … bytes` channel instead.
    pub fn to_asciicast(&self) -> String {
        // Header: a small fixed-shape object; width/height are plain integers so
        // no escaping is needed.
        let mut out = format!(
            "{{\"version\": 2, \"width\": {}, \"height\": {}}}\n",
            self.width, self.height
        );
        for ev in &self.events {
            match ev {
                Event::Output { t, data } => {
                    out.push_str(&format!("[{}, \"o\", \"{}\"]\n", fmt_t(*t), json_escape_bytes(data)));
                }
                Event::Resize { t, cols, rows } => {
                    out.push_str(&format!("[{}, \"r\", \"{cols}x{rows}\"]\n", fmt_t(*t)));
                }
            }
        }
        out
    }
}

/// Format a [`Duration`] as the asciicast `f64` seconds field (microsecond
/// precision; always a decimal point so it parses as a JSON number/float).
fn fmt_t(t: Duration) -> String {
    format!("{:.6}", t.as_secs_f64())
}


/// The number of trailing bytes of `bytes` that form an INCOMPLETE (but so-far
/// valid) UTF-8 multibyte lead — i.e. a sequence the next read could complete.
/// Returns 0 when the buffer ends on a complete character or on a genuinely
/// invalid byte (which is NOT carried — it is rendered as U+FFFD where it sits).
/// Scans back over continuation bytes (≤ 3) to find the lead, then compares the
/// bytes-seen against the bytes-needed for that lead's length.
fn incomplete_tail_len(bytes: &[u8]) -> usize {
    let n = bytes.len();
    let max_back = 3.min(n);
    for back in 1..=max_back {
        let b = bytes[n - back];
        let needed = if b >> 5 == 0b110 {
            2
        } else if b >> 4 == 0b1110 {
            3
        } else if b >> 3 == 0b11110 {
            4
        } else if b >> 6 == 0b10 {
            // A continuation byte; the lead is further back — keep scanning.
            continue;
        } else {
            // ASCII (complete) or an invalid lead — nothing to carry.
            return 0;
        };
        // `back` continuation+lead bytes seen; if fewer than the sequence needs,
        // the tail is incomplete and must be carried.
        return if back < needed { back } else { 0 };
    }
    0
}

/// JSON-escape RAW bytes as a string body: decode the longest valid UTF-8 runs and
/// escape them with [`json_escape`], emitting exactly one U+FFFD per genuinely
/// invalid byte (incomplete trailing leads are carried by the caller, never reach
/// here). This is the lossless render of the raw `Output` payload.
fn json_escape_bytes(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() + 2);
    let mut i = 0;
    while i < bytes.len() {
        match std::str::from_utf8(&bytes[i..]) {
            Ok(s) => {
                out.push_str(&crate::control::json_escape(s));
                break;
            }
            Err(e) => {
                let valid = e.valid_up_to();
                if valid > 0 {
                    // SAFETY: bytes[i..i+valid] is valid UTF-8 by `valid_up_to`.
                    let s = std::str::from_utf8(&bytes[i..i + valid]).unwrap();
                    out.push_str(&crate::control::json_escape(s));
                }
                out.push('\u{fffd}');
                match e.error_len() {
                    Some(len) => i += valid + len,
                    // No `error_len` => an unexpected end; the caller carries
                    // genuine incomplete leads, so treat any residue as consumed.
                    None => break,
                }
            }
        }
    }
    out
}

// ===========================================================================
// ByteFanout — the LIVE, byte-lossless, every-frame output channel (Item 2)
// ===========================================================================
//
// `CastRecorder` is a pull SNAPSHOT (the `cast` verb). `ByteFanout` is its PUSH
// twin: the reader thread `tee`s every program-output burst (the SAME
// `Arc<[u8]>` it already builds, one extra refcount — no third copy) to each
// live subscriber, who drains a byte-exact, every-frame queue. Where `subscribe
// screen/cells` coalesces (latest grid per wake), the `bytes` stream loses
// NOTHING: the queue accumulates every burst between wakes; only a flood past
// the per-subscriber budget drops oldest, surfaced as a counted GAP. The
// producer NEVER blocks (push + drop-oldest under a leaf mutex), mirroring the
// subscribe registry's never-block guarantee.

/// One subscriber's bounded, byte-budget, drop-oldest queue of output bursts.
#[derive(Default)]
struct ByteQueue {
    /// Bursts in arrival order; each is the reader thread's shared `Arc<[u8]>`.
    bursts: VecDeque<Arc<[u8]>>,
    /// Sum of `bursts` byte lengths, kept in step for the budget check.
    used: usize,
    /// Bytes dropped (oldest-first) since the last `drain`, surfaced as a GAP.
    dropped: u64,
}

/// One registered byte subscriber: a stable id + its queue.
struct ByteSlot {
    id: u64,
    queue: Mutex<ByteQueue>,
}

/// The per-session live byte fan-out. Held in `SessionCtx` (so a `subscribe …
/// bytes` connection can register) and cloned into the reader thread (which
/// `tee`s every burst). Cheap when no one is subscribed (an empty slot vec).
pub struct ByteFanout {
    slots: Mutex<Vec<Arc<ByteSlot>>>,
    next_id: AtomicU64,
    /// Per-subscriber retained-byte budget; drop-oldest beyond it.
    budget: usize,
}

impl Default for ByteFanout {
    fn default() -> Self {
        Self::with_budget(DEFAULT_BUDGET_BYTES)
    }
}

impl ByteFanout {
    /// A fan-out with the default 4 MiB per-subscriber budget.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A fan-out with an explicit per-subscriber retained-byte budget (≥ 1).
    #[must_use]
    pub fn with_budget(budget: usize) -> Self {
        Self {
            slots: Mutex::new(Vec::new()),
            next_id: AtomicU64::new(0),
            budget: budget.max(1),
        }
    }

    /// Push `burst` (one `Arc` refcount bump) into every live subscriber's queue,
    /// dropping each queue's OLDEST bursts past the budget and counting the dropped
    /// bytes. NON-BLOCKING and infallible: a slow/stalled subscriber can never
    /// block or backpressure the producing reader thread.
    pub fn tee(&self, burst: &Arc<[u8]>) {
        let slots = self.slots.lock().unwrap_or_else(|p| p.into_inner());
        for slot in slots.iter() {
            let mut q = slot.queue.lock().unwrap_or_else(|p| p.into_inner());
            q.bursts.push_back(burst.clone());
            q.used += burst.len();
            while q.used > self.budget && q.bursts.len() > 1 {
                if let Some(old) = q.bursts.pop_front() {
                    q.used = q.used.saturating_sub(old.len());
                    q.dropped += old.len() as u64;
                }
            }
        }
    }

    /// Register a new live subscriber, returning an RAII [`ByteSubscription`] that
    /// drains its queue and deregisters on drop.
    #[must_use]
    pub fn subscribe(self: &Arc<Self>) -> ByteSubscription {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let slot = Arc::new(ByteSlot { id, queue: Mutex::new(ByteQueue::default()) });
        self.slots
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(slot.clone());
        ByteSubscription { fanout: self.clone(), slot, id }
    }

    fn deregister(&self, id: u64) {
        self.slots
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .retain(|s| s.id != id);
    }

    /// Number of live subscribers (test/introspection).
    #[must_use]
    #[allow(dead_code)]
    pub fn subscriber_count(&self) -> usize {
        self.slots.lock().unwrap_or_else(|p| p.into_inner()).len()
    }
}

/// The consumer end of a byte subscription. `drain` returns every burst queued
/// since the last call (byte-exact, every-frame) plus the dropped-byte count;
/// dropping it deregisters so the producer stops teeing to a dead subscriber.
pub struct ByteSubscription {
    fanout: Arc<ByteFanout>,
    slot: Arc<ByteSlot>,
    id: u64,
}

impl ByteSubscription {
    /// Take ALL queued bursts (in arrival order) and the dropped-byte count since
    /// the previous drain, resetting both. Loss-free between drains up to budget.
    #[must_use]
    pub fn drain(&self) -> (Vec<Arc<[u8]>>, u64) {
        let mut q = self.slot.queue.lock().unwrap_or_else(|p| p.into_inner());
        let bursts: Vec<Arc<[u8]>> = q.bursts.drain(..).collect();
        q.used = 0;
        let dropped = std::mem::take(&mut q.dropped);
        (bursts, dropped)
    }
}

impl Drop for ByteSubscription {
    fn drop(&mut self) {
        self.fanout.deregister(self.id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A header line that `asciinema` would accept: valid JSON object with
    /// `version`/`width`/`height`.
    #[test]
    fn header_is_valid_v2_json() {
        let rec = CastRecorder::new(120, 40);
        let cast = rec.to_asciicast();
        let header = cast.lines().next().unwrap();
        // Hand-parse the three fields we promise (no serde dep in-tree).
        assert!(header.starts_with('{') && header.ends_with('}'));
        assert!(header.contains("\"version\": 2"));
        assert!(header.contains("\"width\": 120"));
        assert!(header.contains("\"height\": 40"));
        // An empty recording is JUST the header line (one line, idle costs none).
        assert_eq!(cast.lines().count(), 1);
        assert_eq!(rec.event_count(), 0);
    }

    /// Each event line is a `[f64, "o"|"r", string]` JSON array, output-only for
    /// `record_output`, with a correctly JSON-escaped payload.
    #[test]
    fn events_are_well_formed_arrays() {
        let mut rec = CastRecorder::new(80, 24);
        rec.record_output(Duration::from_millis(100), b"hello\n");
        rec.record_output(Duration::from_millis(250), b"a\tb\"c\\d\r");
        rec.record_resize(Duration::from_millis(300), 100, 30);

        let cast = rec.to_asciicast();
        let mut lines = cast.lines();
        let _header = lines.next().unwrap();

        // `parse_event` UNESCAPES the JSON payload as a real reader would, so the
        // expected values are the ORIGINAL (decoded) bytes — proving the on-wire
        // escaping round-trips back to exactly what was fed.
        let e0 = lines.next().unwrap();
        assert_eq!(parse_event(e0), (0.100, "o".to_string(), "hello\n".to_string()));

        let e1 = lines.next().unwrap();
        assert_eq!(
            parse_event(e1),
            (0.250, "o".to_string(), "a\tb\"c\\d\r".to_string())
        );
        // And the RAW line really did escape them (no bare control byte / quote).
        assert!(e1.contains("\\t") && e1.contains("\\\"") && e1.contains("\\\\") && e1.contains("\\r"));
        assert!(!e1[1..e1.len() - 1].contains('\t'));

        let e2 = lines.next().unwrap();
        assert_eq!(parse_event(e2), (0.300, "r".to_string(), "100x30".to_string()));

        assert!(lines.next().is_none());
    }

    /// `take_response()` query replies must never reach the recorder — but the
    /// recorder also must never *invent* an event: only what is fed appears.
    #[test]
    fn only_fed_bursts_appear() {
        let mut rec = CastRecorder::new(80, 24);
        rec.record_output(Duration::from_millis(10), b"x");
        // No further calls => exactly one "o" event.
        let cast = rec.to_asciicast();
        let o_events = cast.lines().filter(|l| l.contains("\"o\"")).count();
        assert_eq!(o_events, 1);
    }

    /// Timestamps are monotonic non-decreasing even when a caller hands a `t`
    /// that went backwards (deltas clamp to ≥ 0).
    #[test]
    fn timestamps_are_monotonic() {
        let mut rec = CastRecorder::new(80, 24);
        rec.record_output(Duration::from_millis(500), b"a");
        rec.record_output(Duration::from_millis(200), b"b"); // backwards!
        rec.record_output(Duration::from_millis(700), b"c");

        let cast = rec.to_asciicast();
        let ts: Vec<f64> = cast
            .lines()
            .skip(1)
            .map(|l| parse_event(l).0)
            .collect();
        assert_eq!(ts.len(), 3);
        for w in ts.windows(2) {
            assert!(w[1] >= w[0], "ts not monotonic: {ts:?}");
        }
        // The backwards burst is clamped up to the previous timestamp, not down.
        assert_eq!(ts[0], 0.500);
        assert_eq!(ts[1], 0.500);
        assert_eq!(ts[2], 0.700);
    }

    /// Control bytes below 0x20 (other than \n\r\t) use the \u00XX form, so the
    /// payload is a legal JSON string body.
    #[test]
    fn control_bytes_use_unicode_escape() {
        let mut rec = CastRecorder::new(80, 24);
        // ESC (0x1b), BEL (0x07), NUL (0x00) — all illegal raw in a JSON string.
        rec.record_output(Duration::ZERO, b"\x1b[0m\x07\x00");
        let cast = rec.to_asciicast();
        let line = cast.lines().nth(1).unwrap();
        // The RAW wire line uses \u00XX for every C0 control — never a bare byte.
        assert!(line.contains("\\u001b") && line.contains("\\u0007") && line.contains("\\u0000"));
        // Strip the framing brackets; the inner JSON carries no bare control byte.
        assert!(!line.as_bytes().iter().any(|&b| b < 0x20));
        // And it decodes back to exactly the original control bytes.
        let payload = parse_event(line).2;
        assert_eq!(payload, "\u{1b}[0m\u{7}\u{0}");
    }

    /// Invalid UTF-8 (a split multibyte sequence) is replaced, never emitted raw,
    /// so the line stays valid JSON.
    #[test]
    fn invalid_utf8_is_lossy_replaced() {
        let mut rec = CastRecorder::new(80, 24);
        // Lone continuation byte 0x80 + a truncated 2-byte lead 0xc3.
        rec.record_output(Duration::ZERO, &[b'A', 0x80, b'B', 0xc3]);
        let cast = rec.to_asciicast();
        let payload = parse_event(cast.lines().nth(1).unwrap()).2;
        // U+FFFD REPLACEMENT CHARACTER passes through as itself (valid UTF-8).
        assert!(payload.starts_with('A'));
        assert!(payload.contains('\u{fffd}'));
        // Still no bare control bytes / unescaped quotes.
        assert!(!payload.contains('"') || payload.contains("\\\""));
    }

    /// Drop-oldest keeps the recording bounded under a flood; the newest event
    /// always survives and the header is unaffected.
    #[test]
    fn budget_drops_oldest() {
        // Tiny budget: each "o" payload below is ~4 bytes, so only a few fit.
        let mut rec = CastRecorder::with_budget(80, 24, 10);
        for i in 0..100u32 {
            rec.record_output(Duration::from_millis(i as u64), b"abcd");
        }
        // Bounded well under 100 events.
        assert!(rec.event_count() < 10, "unbounded: {}", rec.event_count());
        let cast = rec.to_asciicast();
        // Header still present and valid.
        assert!(cast.lines().next().unwrap().contains("\"version\": 2"));
        // Timestamps of the survivors are still monotonic.
        let ts: Vec<f64> = cast.lines().skip(1).map(|l| parse_event(l).0).collect();
        for w in ts.windows(2) {
            assert!(w[1] >= w[0]);
        }
    }

    /// A full round-trip: a header + several events parse as the asciicast v2
    /// shape `asciinema`-style parsing requires (header is a JSON object; each
    /// event line is a `[f64, string, string]` array).
    #[test]
    fn round_trip_parses_as_v2() {
        let mut rec = CastRecorder::new(90, 25);
        rec.record_output(Duration::from_millis(5), b"$ ls\r\n");
        rec.record_resize(Duration::from_millis(50), 90, 30);
        rec.record_output(Duration::from_millis(80), b"file1 file2\r\n");

        let cast = rec.to_asciicast();
        let mut lines = cast.lines();

        // Header is a JSON object with the required keys.
        let header = lines.next().unwrap();
        assert!(header.trim_start().starts_with('{'));
        assert!(header.contains("\"version\": 2"));

        // Every remaining line is a [f64, "code", "string"] array, in order.
        let mut count = 0;
        let mut prev = f64::NEG_INFINITY;
        for line in lines {
            let (t, code, _data) = parse_event(line);
            assert!(code == "o" || code == "r", "bad event code {code}");
            assert!(t >= prev, "non-monotonic across round-trip");
            prev = t;
            count += 1;
        }
        assert_eq!(count, 3);
    }

    /// ITEM 3: raw program-output bytes round-trip EXACTLY through the recorder —
    /// ESC/CSI/SGR, tab, quote, backslash, CR/LF all survive (escaped on the wire,
    /// decoded back to the identical bytes).
    #[test]
    fn raw_bytes_round_trip_exactly() {
        let mut rec = CastRecorder::new(80, 24);
        let raw: &[u8] = b"\x1b[31mred\x1b[0m\tx\"y\\z\r\n";
        rec.record_output(Duration::from_millis(5), raw);
        let cast = rec.to_asciicast();
        let payload = parse_event(cast.lines().nth(1).unwrap()).2;
        assert_eq!(payload.as_bytes(), raw, "raw bytes must round-trip exactly");
    }

    /// A multibyte sequence split across two reads (the 64 KiB-boundary case) is
    /// REASSEMBLED, not corrupted into U+FFFD.
    #[test]
    fn split_multibyte_across_bursts_reassembles() {
        let mut rec = CastRecorder::new(80, 24);
        rec.record_output(Duration::from_millis(1), &[0xE2, 0x82]); // first 2 of '€'
        assert_eq!(rec.event_count(), 0, "incomplete lead must be carried, not emitted");
        rec.record_output(Duration::from_millis(2), &[0xAC]); // final byte
        let cast = rec.to_asciicast();
        let payload = parse_event(cast.lines().nth(1).unwrap()).2;
        assert_eq!(payload, "€");
        assert!(!payload.contains('\u{fffd}'), "no replacement char: {payload:?}");
    }

    /// A genuinely invalid byte (a lone continuation mid-stream, not a trailing
    /// lead) is still rendered as exactly one U+FFFD in place.
    #[test]
    fn genuinely_invalid_byte_still_fffd() {
        let mut rec = CastRecorder::new(80, 24);
        rec.record_output(Duration::ZERO, &[b'A', 0x80, b'B']);
        let cast = rec.to_asciicast();
        let payload = parse_event(cast.lines().nth(1).unwrap()).2;
        assert_eq!(payload, "A\u{fffd}B");
    }

    /// A trailing incomplete lead is CARRIED (not replaced); the next burst that
    /// completes it produces the whole character.
    #[test]
    fn trailing_incomplete_lead_is_carried_not_replaced() {
        let mut rec = CastRecorder::new(80, 24);
        rec.record_output(Duration::ZERO, &[b'X', 0xC3]); // 'X' + lead of 'é'
        let payload = parse_event(rec.to_asciicast().lines().nth(1).unwrap()).2;
        assert_eq!(payload, "X");
        assert!(!payload.contains('\u{fffd}'));
        rec.record_output(Duration::from_millis(1), &[0xA9]); // 0xC3 0xA9 = 'é'
        let last = parse_event(rec.to_asciicast().lines().last().unwrap()).2;
        assert_eq!(last, "é");
    }

    /// The byte budget still bounds raw-byte storage (drop-oldest).
    #[test]
    fn budget_still_bounds_raw_bytes() {
        let mut rec = CastRecorder::with_budget(80, 24, 10);
        for i in 0..100u32 {
            rec.record_output(Duration::from_millis(u64::from(i)), b"abcd");
        }
        assert!(rec.event_count() < 10, "unbounded: {}", rec.event_count());
    }

    /// ITEM 2: `tee` delivers EVERY burst (incl. non-UTF-8) to ALL subscribers,
    /// byte-exact and every-frame (no coalescing).
    #[test]
    fn byte_fanout_tees_every_burst_to_all_subscribers() {
        let fan = Arc::new(ByteFanout::new());
        let a = fan.subscribe();
        let b = fan.subscribe();
        assert_eq!(fan.subscriber_count(), 2);
        fan.tee(&Arc::from(&b"\x1b[31m"[..]));
        fan.tee(&Arc::from(&[0x80u8, 0xff, 0x00][..])); // non-UTF-8 + NUL
        for sub in [&a, &b] {
            let (bursts, dropped) = sub.drain();
            assert_eq!(dropped, 0);
            assert_eq!(bursts.len(), 2, "every burst delivered, no coalesce");
            assert_eq!(&bursts[0][..], b"\x1b[31m");
            assert_eq!(&bursts[1][..], &[0x80, 0xff, 0x00]);
            // A second drain is empty (queue consumed).
            assert_eq!(sub.drain().0.len(), 0);
        }
    }

    /// A full queue drops OLDEST and counts the dropped bytes; the producer never
    /// blocks. The newest burst always survives.
    #[test]
    fn byte_fanout_full_queue_drops_oldest_and_counts() {
        let fan = Arc::new(ByteFanout::with_budget(10));
        let sub = fan.subscribe();
        for _ in 0..100 {
            fan.tee(&Arc::from(&b"abcd"[..])); // 4 bytes each, budget 10
        }
        let (bursts, dropped) = sub.drain();
        // Bounded well under 100 retained; the rest counted as dropped.
        assert!(bursts.len() <= 3, "queue bounded: {}", bursts.len());
        assert!(dropped >= 4 * (100 - bursts.len() as u64), "dropped counted: {dropped}");
        // The newest burst is retained.
        assert_eq!(&bursts.last().unwrap()[..], b"abcd");
    }

    /// Dropping a subscription deregisters it; `tee` then pays nothing for it.
    #[test]
    fn byte_subscription_drop_deregisters() {
        let fan = Arc::new(ByteFanout::new());
        {
            let _s = fan.subscribe();
            assert_eq!(fan.subscriber_count(), 1);
        }
        assert_eq!(fan.subscriber_count(), 0, "deregistered on drop");
        fan.tee(&Arc::from(&b"x"[..])); // safe no-op
    }

    // ---- test helper: a minimal asciicast-event parser ----

    /// Parse one event line `[<f64>, "<code>", "<json-string>"]` into
    /// `(t, code, unescaped_payload)`. Validates the array shape strictly enough
    /// to stand in for an `asciinema`-style reader in the round-trip tests.
    fn parse_event(line: &str) -> (f64, String, String) {
        let inner = line
            .strip_prefix('[')
            .and_then(|s| s.strip_suffix(']'))
            .unwrap_or_else(|| panic!("not a JSON array: {line}"));
        // t: up to the first comma.
        let (t_str, after_t) = inner.split_once(',').expect("missing comma after t");
        let t: f64 = t_str.trim().parse().expect("t is not an f64");
        // code: the next quoted string.
        let after_t = after_t.trim_start();
        let (code, after_code) = parse_json_string(after_t);
        let after_code = after_code.trim_start();
        let after_code = after_code.strip_prefix(',').expect("missing comma after code");
        // data: the final quoted string (rest of the array).
        let (data, tail) = parse_json_string(after_code.trim_start());
        assert!(tail.trim().is_empty(), "trailing junk after data: {tail:?}");
        (t, code, data)
    }

    /// Parse a leading JSON string literal, returning (unescaped, remainder).
    /// Recognizes the exact escapes [`json_escape`] produces.
    fn parse_json_string(s: &str) -> (String, &str) {
        let chars: Vec<char> = s.chars().collect();
        assert_eq!(chars.first().copied(), Some('"'), "expected a JSON string at {s:?}");
        let mut out = String::new();
        let mut ci = 1; // skip the opening quote
        while ci < chars.len() {
            match chars[ci] {
                '"' => {
                    // Closing quote; compute the byte remainder past it.
                    let consumed: usize = chars[..=ci].iter().map(|c| c.len_utf8()).sum();
                    return (out, &s[consumed..]);
                }
                '\\' => {
                    ci += 1;
                    match chars[ci] {
                        '"' => out.push('"'),
                        '\\' => out.push('\\'),
                        'n' => out.push('\n'),
                        'r' => out.push('\r'),
                        't' => out.push('\t'),
                        // control.rs json_escape emits short forms for 0x08/0x0C.
                        'b' => out.push('\u{8}'),
                        'f' => out.push('\u{c}'),
                        'u' => {
                            let hex: String = chars[ci + 1..ci + 5].iter().collect();
                            let cp = u32::from_str_radix(&hex, 16).expect("bad \\u");
                            out.push(char::from_u32(cp).expect("bad codepoint"));
                            ci += 4;
                        }
                        other => panic!("unknown escape \\{other}"),
                    }
                }
                other => out.push(other),
            }
            ci += 1;
        }
        panic!("unterminated JSON string: {s:?}");
    }
}
