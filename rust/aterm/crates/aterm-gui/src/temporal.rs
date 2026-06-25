// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Andrew Yates

//! Per-session **temporal recorder** — the GUI-side capture half of the
//! hydratable temporal buffer (design `HIERARCHICAL_SESSIONS.md` Addendum B, B.9).
//!
//! [`TemporalRecorder`] folds the live session into the `aterm-buffer` event-log
//! spine: a [`Keyframe`](aterm_buffer::Op::Keyframe) (a serialized
//! [`TerminalCheckpoint`]) plus the stream of
//! [`RawIn`](aterm_buffer::Op::RawIn)/[`Reply`](aterm_buffer::Op::Reply)/
//! [`Resize`](aterm_buffer::Op::Resize) events that drive the engine. Replaying
//! `hydrate(keyframe) + process(RawIn…up to t)` reconstructs the engine state at
//! any `t` — the property `recording_model` (B.8.3) proves and
//! `conformance_recording` (B.8.4) binds to the real engine.
//!
//! ## Discipline (mirrors [`CastRecorder`](crate::cast::CastRecorder))
//! - **Handles, not payloads on the spine.** The spine carries `BlobId`/
//!   `KeyframeId` handles; the bulk bytes live here in bounded side stores so the
//!   in-RAM ring stays small.
//! - **Spill-not-forget.** [`EventLog::append_at`] hands back the evicted oldest
//!   event; we move it to the warm `spilled` tier rather than drop it (the
//!   B.8.2 `tier_residency_model` obligation). The warm tier is itself bounded by
//!   a byte budget; anything dropped past the budget is **counted**
//!   ([`dropped_events`](Self::dropped_events)), never silently lost — the cold
//!   (disk) drain that would make the budget unnecessary is the off-lock
//!   persistence task (a documented follow-up, not this headless unit).
//! - **No fs / no lock / no wall-clock-as-state.** Ticks come from one epoch
//!   captured at construction; the GUI feeds bursts off the reader hot path on a
//!   dedicated writer thread, exactly as the asciicast tap does.

use std::collections::VecDeque;
use std::time::Instant;

use aterm_buffer::{BlobId, Event, EventLog, KeyframeId, Op, Ticks};
use aterm_core::terminal::TerminalCheckpoint;

/// Default byte budget for retained blob payloads + warm-tier events. A flood
/// cannot balloon RAM past this; an idle session costs nothing.
pub const DEFAULT_BUDGET_BYTES: usize = 8 * 1024 * 1024;

/// A burst handed from the reader hot path to the temporal writer thread
/// (lock-free, FIFO — mirrors the asciicast `Vec<u8>` channel). Recording the
/// tick + spine append happens on the writer thread, never under `term_lock`.
pub enum TemporalMsg {
    /// Raw PTY input fed to `process()` (the engine-driving bytes). `Arc<[u8]>` so
    /// the reader's single per-burst heap copy is shared with the asciicast tap.
    RawIn(std::sync::Arc<[u8]>),
    /// Engine reply bytes emitted to the PTY peer (`take_response()`).
    Reply(Vec<u8>),
}

/// A retained blob payload (the bytes behind a `RawIn`/`Reply` handle). Held in a
/// FIFO `VecDeque` (oldest-first); the owning `BlobId` is the queue position, so it
/// is not stored here — handle→bytes resolution will reintroduce it with its reader.
struct Blob {
    bytes: Vec<u8>,
}

/// Per-session capture into the `aterm-buffer` temporal spine.
pub struct TemporalRecorder {
    /// The event-log spine (the one timeline). Bounded ring; eviction spills.
    log: EventLog,
    /// Bulk payloads for `RawIn`/`Reply`, keyed by `BlobId`, oldest first.
    blobs: VecDeque<Blob>,
    /// Keyframes (serialized checkpoints), keyed by `KeyframeId`, oldest first.
    keyframes: VecDeque<(KeyframeId, TerminalCheckpoint)>,
    /// Warm tier: events evicted from the live ring (spill-not-forget). In a full
    /// deployment an off-lock task drains these to the cold/disk tier.
    spilled: VecDeque<Event>,
    /// Monotone blob-id source.
    next_blob: u64,
    /// Monotone keyframe-id source.
    next_keyframe: u64,
    /// Retained payload bytes (blobs + a fixed charge per spilled event).
    used: usize,
    /// The retained byte budget; drop-oldest (counted) when exceeded.
    budget: usize,
    /// Count of warm-tier events dropped past the budget (NEVER silent — the
    /// design's "no silent caps" rule). Zero once the cold drain is wired.
    dropped_events: u64,
    /// The monotonic epoch this recorder's tick timeline is relative to.
    epoch: Instant,
}

impl TemporalRecorder {
    /// A recorder with the default budget.
    #[must_use]
    pub fn new() -> Self {
        Self::with_budget(DEFAULT_BUDGET_BYTES)
    }

    /// A recorder with an explicit retained-byte budget (>= 1).
    #[must_use]
    pub fn with_budget(budget: usize) -> Self {
        Self {
            log: EventLog::default(),
            blobs: VecDeque::new(),
            keyframes: VecDeque::new(),
            spilled: VecDeque::new(),
            next_blob: 0,
            next_keyframe: 0,
            used: 0,
            budget: budget.max(1),
            dropped_events: 0,
            epoch: Instant::now(), // CLOCK-EXEMPT: recorder timeline epoch, not engine state
        }
    }

    /// The current tick on this recorder's timeline (micros since the epoch).
    /// Both the reader-thread (RawIn/Reply) and main-thread (Resize/Keyframe)
    /// taps call this so one session shares a single monotone tick timeline.
    #[must_use]
    pub fn now(&self) -> Ticks {
        // CLOCK-EXEMPT: derives the recorded tick from the recorder epoch; this
        // is the value we RECORD, not engine state read during process().
        Ticks(u64::try_from(self.epoch.elapsed().as_micros()).unwrap_or(u64::MAX))
    }

    /// Append `op` at `ts`, moving any evicted event to the warm tier (spill-not-
    /// forget), then enforce the byte budget (drop-oldest warm-tier events, counted).
    fn append(&mut self, op: Op, ts: Ticks) {
        let (_seq, evicted) = self.log.append_at(op, ts);
        if let Some(ev) = evicted {
            // SPILL: tier the evicted event instead of dropping it (B.8.2).
            self.spilled.push_back(ev);
            self.used += SPILLED_EVENT_CHARGE;
        }
        self.enforce_budget();
    }

    /// Record a raw PTY-input burst fed to `process()` (the `RawIn` event). The
    /// bytes are the genuine engine-driving input — replay re-feeds exactly these.
    pub fn record_raw_in(&mut self, bytes: &[u8]) {
        let ts = self.now();
        let id = self.store_blob(bytes);
        self.append(Op::RawIn(id), ts);
    }

    /// Record an engine reply burst (`take_response()` -> PTY peer). Recorded for
    /// forked-timeline fidelity; NOT re-emitted on replay (the design's contract).
    pub fn record_reply(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let ts = self.now();
        let id = self.store_blob(bytes);
        self.append(Op::Reply(id), ts);
    }

    /// Record a geometry change (reflow is path-dependent, so resize is a
    /// first-class recorded event, never re-ordered — B.2.3).
    pub fn record_resize(&mut self, rows: u16, cols: u16) {
        let ts = self.now();
        self.append(Op::Resize { rows, cols }, ts);
    }

    /// Record a keyframe (a serialized [`TerminalCheckpoint`] taken at a
    /// parser-ground boundary, B.3.3). Replay seeds from the nearest keyframe
    /// `<= seq(t)` and folds `RawIn` forward.
    pub fn record_keyframe(&mut self, checkpoint: TerminalCheckpoint) {
        let ts = self.now();
        let id = KeyframeId(self.next_keyframe);
        self.next_keyframe += 1;
        // A keyframe is large; charge its grid bytes against the budget.
        self.used += checkpoint.grid.len() + checkpoint.alt_grid.as_ref().map_or(0, Vec::len);
        self.keyframes.push_back((id, checkpoint));
        self.append(Op::Keyframe(id), ts);
        self.enforce_budget();
    }

    /// Store `bytes` under a fresh `BlobId`, charging the budget.
    fn store_blob(&mut self, bytes: &[u8]) -> BlobId {
        let id = BlobId(self.next_blob);
        self.next_blob += 1;
        self.used += bytes.len();
        self.blobs.push_back(Blob {
            bytes: bytes.to_vec(),
        });
        id
    }

    /// Drop oldest retained payloads (blobs, then warm-tier events, then oldest
    /// keyframes) until the budget holds. Every dropped warm-tier event is
    /// COUNTED in `dropped_events` — the recording is bounded but never silently
    /// loses without saying so.
    fn enforce_budget(&mut self) {
        while self.used > self.budget {
            // Prefer dropping the oldest blob (largest, most reclaimable) first.
            if let Some(b) = self.blobs.pop_front() {
                self.used = self.used.saturating_sub(b.bytes.len());
                continue;
            }
            if let Some(_ev) = self.spilled.pop_front() {
                self.used = self.used.saturating_sub(SPILLED_EVENT_CHARGE);
                self.dropped_events += 1;
                continue;
            }
            if let Some((_id, kf)) = self.keyframes.pop_front() {
                let cost = kf.grid.len() + kf.alt_grid.as_ref().map_or(0, Vec::len);
                self.used = self.used.saturating_sub(cost);
                continue;
            }
            break; // nothing left to reclaim
        }
    }

    /// Total events ever appended to the spine (live + spilled + dropped).
    #[must_use]
    pub fn total_events(&self) -> u64 {
        self.log.total()
    }

    /// Live (un-evicted) event count on the spine.
    #[must_use]
    pub fn live_events(&self) -> usize {
        self.log.live().count()
    }

    /// Warm-tier (spilled-but-retained) event count.
    #[must_use]
    pub fn spilled_events(&self) -> usize {
        self.spilled.len()
    }

    /// Keyframes currently retained.
    #[must_use]
    pub fn keyframe_count(&self) -> usize {
        self.keyframes.len()
    }

    /// Warm-tier events dropped past the budget (cold drain not yet wired).
    #[must_use]
    pub fn dropped_events(&self) -> u64 {
        self.dropped_events
    }
}

impl Default for TemporalRecorder {
    fn default() -> Self {
        Self::new()
    }
}

/// Fixed budget charge per warm-tier event (the `Event` struct itself; its
/// payload, if any, is charged separately as a blob). Keeps `enforce_budget`
/// O(1) per step without measuring each enum variant.
const SPILLED_EVENT_CHARGE: usize = 64;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_raw_in_reply_resize_on_one_spine() {
        let mut r = TemporalRecorder::new();
        r.record_raw_in(b"ls -la\n");
        r.record_reply(b"\x1b[0n"); // a DSR reply
        r.record_resize(30, 100);
        // Empty reply is a no-op (no spurious event).
        r.record_reply(b"");

        assert_eq!(
            r.total_events(),
            3,
            "raw_in + reply + resize (empty reply skipped)"
        );
        assert_eq!(r.live_events(), 3);
        assert_eq!(r.spilled_events(), 0, "nothing evicted under capacity");
        assert_eq!(r.dropped_events(), 0);
    }

    #[test]
    fn keyframe_is_recorded_and_counted() {
        let mut t = aterm_core::terminal::Terminal::new(6, 20);
        t.process(b"seed content\r\n");
        assert!(t.parser_is_ground());
        let cp = t.checkpoint();

        let mut r = TemporalRecorder::new();
        r.record_keyframe(cp);
        r.record_raw_in(b"more");

        assert_eq!(r.keyframe_count(), 1);
        assert_eq!(r.total_events(), 2, "Keyframe event + RawIn event");
    }

    #[test]
    fn ticks_are_monotone_nondecreasing() {
        let r = TemporalRecorder::new();
        let a = r.now();
        let b = r.now();
        assert!(b >= a, "recorder ticks must be monotone non-decreasing");
    }

    #[test]
    fn budget_bounds_blobs_without_silent_event_loss_on_spine() {
        // Tiny budget: each blob is ~64 bytes, so blobs get reclaimed, but the
        // SPINE (total_events) keeps counting every append — bounding payload RAM
        // never rewrites history's length.
        let mut r = TemporalRecorder::with_budget(256);
        for _ in 0..1000 {
            r.record_raw_in(&[b'x'; 64]);
        }
        // The spine counted every event...
        assert_eq!(r.total_events(), 1000);
        // ...while retained payload bytes stayed bounded.
        assert!(
            r.used <= r.budget + 64,
            "payload bytes bounded: used={}",
            r.used
        );
        // Blobs were reclaimed under budget (far fewer than 1000 retained).
        assert!(r.blobs.len() < 1000);
    }
}
