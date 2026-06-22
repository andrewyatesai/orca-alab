// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! Real-time SUBSCRIBER REGISTRY + PUSH face (design P1.3) — the additive layer
//! that turns the poll-only control socket into a server-PUSH face, so one agent
//! watches its OWN and OTHER sessions LIVE without busy-polling.
//!
//! ## Shape
//!
//! A `subscribe` connection (already its own thread, see [`crate::control`])
//! registers a [`SubscriberHandle`] keyed by the process-local `u64` id of each
//! session it watches — the SAME id the GUI routes `Wake::Output { session }` with
//! (main.rs `Wake::Output`). The handle is a SINGLE-SLOT non-blocking notify: an
//! `mpsc::sync_channel(1)` whose `try_send` NEVER blocks. The producer side (the
//! GUI/reader thread, via the one `Wake::Output` hook) calls [`Subscribers::notify`],
//! which `try_send`s a unit to every subscriber of that session and IGNORES a full
//! channel or a hung-up receiver. The subscriber thread blocks on `recv`, then
//! reads the session's CURRENT state and emits deltas.
//!
//! ## Coalescing — backpressure-safe BY CONSTRUCTION
//!
//! The notify slot has capacity ONE and `try_send` drops on a full slot, so a
//! pending-but-unread notify simply stays pending: N producer wakes between two
//! subscriber reads collapse into AT MOST ONE pending notify. When the subscriber
//! finally wakes it reads the LATEST state (the current `content_seq` and grid),
//! not a queue of every intermediate frame. A slow subscriber
//! therefore gets COARSER / fewer deltas and CANNOT block, backpressure, or even
//! slow the producing session's reader thread — the producer's `try_send` is O(1)
//! and infallible by design (it discards rather than waits). If the subscriber's
//! own socket write blocks or fails, its thread drops the connection (and
//! deregisters) — the producer is never involved in that path.
//!
//! ## Discipline
//!
//! The registry lock is a leaf: `notify` takes it, `try_send`s, and releases —
//! it is NEVER held across a `Terminal` lock or a socket write. The subscriber
//! thread reads the registry only to (de)register itself; it resolves and reads
//! target terminals through the [`crate::session_store::Store`] with the same
//! clone-then-release discipline the rest of the control path uses.
//!
//! ## Push-only
//!
//! Once a connection issues `subscribe` and the verb authorizes, [`crate::control`]
//! FLIPS that connection to push mode: it stops reading requests and enters
//! [`push_loop`]. A subscribed connection is PUSH-ONLY for the rest of its life —
//! the client reads `DELTA`/`EVENT`/`GAP` frames and never sends another verb.

use std::collections::HashMap;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, LockResult, Mutex, MutexGuard};
use std::time::Duration;

use aterm_core::terminal::Terminal;

use crate::cast::{ByteFanout, ByteSubscription};
use crate::session_store::Store;

/// The streams a subscription may watch. A subscription requests a subset and only
/// emits frames for the requested streams. `screen`/`cursor` ride the `content_seq`
/// delta path; `events` rides the block-complete (OSC 133 D) signal.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Streams {
    /// Emit `DELTA <sid> seq=<n> screen <changed rows>` when content advances.
    pub screen: bool,
    /// Emit `DELTA <sid> seq=<n> cursor <row> <col> <style>` when content advances.
    pub cursor: bool,
    /// Emit `EVENT <sid> block-complete <id> exit=<code>` on OSC 133 D.
    pub events: bool,
    /// Emit `DELTA <sid> seq=<n> cells <nbytes>\n<styled-json>` when content
    /// advances — the LOSSLESS styled-screen frame (Item 1's payload) pushed live,
    /// so an outer agent sees the inner TUI's colour/attrs, not plaintext.
    pub cells: bool,
    /// Emit `BYTES <sid> <len>\n<raw bytes>` for EVERY program-output burst — the
    /// live, byte-lossless, every-frame channel (Item 2). Unlike screen/cells this
    /// never coalesces: the per-subscriber queue holds every burst between wakes.
    pub bytes: bool,
}

impl Streams {
    /// Parse a whitespace-or-comma separated stream list (`screen,cursor,events`).
    /// Returns `None` if EMPTY or any token is not a known stream — fail-closed so a
    /// typo does not silently subscribe to nothing.
    #[must_use]
    pub fn parse(s: &str) -> Option<Streams> {
        let mut out = Streams::default();
        let mut any = false;
        for tok in s.split([',', ' ', '\t']).filter(|t| !t.is_empty()) {
            any = true;
            match tok {
                "screen" => out.screen = true,
                "cursor" => out.cursor = true,
                "events" => out.events = true,
                "cells" => out.cells = true,
                "bytes" => out.bytes = true,
                _ => return None,
            }
        }
        if any { Some(out) } else { None }
    }

    /// Whether any `content_seq`-driven stream (screen/cursor/cells) is requested.
    #[must_use]
    fn wants_content(self) -> bool {
        self.screen || self.cursor || self.cells
    }
}

/// One subscriber's wake handle: a single-slot notify the producer `try_send`s into.
/// Keyed in [`Subscribers`] by every process-local session id this subscriber
/// watches, plus a unique `token` so a dropped subscriber removes EXACTLY its own
/// entries (never another connection's that happens to watch the same session).
struct SubscriberHandle {
    /// Unique per registration, for precise removal across all watched sessions.
    token: u64,
    /// Single-slot, non-blocking notify. A full slot means "already pending" — the
    /// producer drops the extra wake (coalescing); the subscriber will read the
    /// latest state on its next wake regardless.
    notify: SyncSender<()>,
}

/// The process-wide subscriber index. Keyed by the GUI's process-local `u64`
/// session id so the existing `Wake::Output { session }` fan-out can find every
/// subscriber of the session that just produced output in O(1).
#[derive(Default)]
pub struct SubscriberSet {
    /// session local id -> the handles watching it. A session with no subscribers
    /// has no entry (and `notify` is then a cheap miss).
    by_session: HashMap<u64, Vec<SubscriberHandle>>,
    /// Monotonic source of unique registration tokens.
    next_token: u64,
}

/// The subscriber registry plus a LOCK-FREE "is anyone subscribed?" flag. The
/// `Mutex<SubscriberSet>` is the source of truth; `any` mirrors `!by_session
/// .is_empty()` and is updated under the SAME lock acquisitions that mutate the map
/// (register / drop), with `Release`/`Acquire` so a producer that observes `true`
/// also sees the registration. It lets the producer's per-`Wake::Output` hook skip
/// the mutex entirely in the overwhelmingly common ZERO-subscriber case — a single
/// atomic load instead of an acquire/release of the lock. A stale `true` only costs
/// one redundant lock+miss; a stale `false` is possible only in the instant after a
/// register and is benign — the next output burst observes `true`, and the
/// subscriber's `recv_timeout` requeries, so no update is permanently lost.
pub struct SubscriberRegistry {
    inner: Mutex<SubscriberSet>,
    any: AtomicBool,
}

impl SubscriberRegistry {
    /// Lock the underlying set. Same shape as `Mutex::lock`, so every existing
    /// `registry.lock()` call site is unchanged.
    pub fn lock(&self) -> LockResult<MutexGuard<'_, SubscriberSet>> {
        self.inner.lock()
    }
    /// Lock-free fast-path: `true` iff at least one session has a subscriber. The
    /// producer checks this before taking the lock to `notify`. `Acquire` pairs with
    /// the `Release` store in [`Self::refresh_any`] — the textbook publish-a-flag
    /// idiom — so once the producer observes `true` it also sees the registration that
    /// set it. (A momentarily-stale `false` right after a register is benign: the next
    /// output burst observes `true`, and the subscriber's own `recv_timeout` requeries,
    /// so no update is permanently lost.)
    #[must_use]
    pub fn any(&self) -> bool {
        self.any.load(Ordering::Acquire)
    }
    /// Refresh the flag from the (locked) set's emptiness. Called by register/drop
    /// while they already hold the lock, so it can never disagree across a mutation.
    /// `Release` so the matching `Acquire` in [`Self::any`] sees the map mutation.
    fn refresh_any(&self, set: &SubscriberSet) {
        self.any
            .store(!set.by_session.is_empty(), Ordering::Release);
    }
}

/// Shared handle to the subscriber registry: held by `App` (the producer side,
/// for the one `Wake::Output` notify hook) and cloned into the control thread
/// (the consumer side, where a `subscribe` connection registers itself).
pub type Subscribers = Arc<SubscriberRegistry>;

/// A new, empty subscriber registry.
#[must_use]
pub fn new_registry() -> Subscribers {
    Arc::new(SubscriberRegistry {
        inner: Mutex::new(SubscriberSet::default()),
        any: AtomicBool::new(false),
    })
}

/// The consumer-side end of a registration: the subscriber thread `recv`s wakes
/// here, and on drop deregisters itself from every session it watched. RAII so a
/// subscriber that returns (write failure / client hangup) cannot leak an entry
/// that would make `notify` pay for a dead receiver forever.
pub struct Subscription {
    registry: Subscribers,
    /// The sessions this subscription registered under (for precise deregistration).
    sessions: Vec<u64>,
    /// This subscription's unique token (matches its handles in the registry).
    token: u64,
    /// The blocking wake end. `recv` parks until the producer notifies (or every
    /// sender is dropped, which only happens when the registry entry is removed —
    /// i.e. after our own `Drop`, so in practice `recv` returns on a real notify).
    rx: Receiver<()>,
}

impl Subscription {
    /// Block until the producer notifies this subscriber (output landed on one of
    /// the watched sessions), or until `timeout` elapses. Returns `true` on a wake,
    /// `false` on timeout. A spurious/coalesced wake is fine: the caller re-reads
    /// the latest state and emits a delta only if `content_seq` advanced.
    #[must_use]
    pub fn wait(&self, timeout: Duration) -> bool {
        matches!(self.rx.recv_timeout(timeout), Ok(()))
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        let mut g = self.registry.lock().unwrap_or_else(|p| p.into_inner());
        for sid in &self.sessions {
            if let Some(v) = g.by_session.get_mut(sid) {
                v.retain(|h| h.token != self.token);
                if v.is_empty() {
                    g.by_session.remove(sid);
                }
            }
        }
        self.registry.refresh_any(&g);
    }
}

impl SubscriberSet {
    /// Register a subscriber watching `sessions` (their process-local ids). Returns
    /// a [`Subscription`] whose `wait` blocks until a watched session produces
    /// output; dropping it deregisters from all watched sessions. The single-slot
    /// notify is created here and its receiver handed back inside the subscription.
    #[must_use]
    pub fn register(registry: &Subscribers, sessions: &[u64]) -> Subscription {
        // capacity 1 == single-slot: at most one pending notify (coalescing).
        let (tx, rx) = sync_channel::<()>(1);
        let mut g = registry.lock().unwrap_or_else(|p| p.into_inner());
        let token = g.next_token;
        g.next_token = g.next_token.wrapping_add(1);
        for &sid in sessions {
            g.by_session.entry(sid).or_default().push(SubscriberHandle {
                token,
                notify: tx.clone(),
            });
        }
        registry.refresh_any(&g);
        drop(g);
        Subscription {
            registry: registry.clone(),
            sessions: sessions.to_vec(),
            token,
            rx,
        }
    }

    /// Notify every subscriber of session `local_id` that it produced output.
    /// NON-BLOCKING and infallible by construction: a full single-slot channel
    /// (notify already pending) or a hung-up receiver (subscriber thread gone) is
    /// silently ignored, so the producer's reader/GUI thread is NEVER stalled by a
    /// slow or dead subscriber. This is the ONLY method the producer calls.
    pub fn notify(&self, local_id: u64) {
        let Some(handles) = self.by_session.get(&local_id) else {
            return; // no subscribers for this session: cheap miss
        };
        for h in handles {
            match h.notify.try_send(()) {
                // delivered, or already pending (coalesced), or receiver gone:
                // every outcome is a no-op for the producer.
                Ok(()) | Err(TrySendError::Full(())) | Err(TrySendError::Disconnected(())) => {}
            }
        }
    }

    /// Number of distinct sessions with at least one subscriber (test/introspection).
    #[must_use]
    #[allow(dead_code)]
    pub fn watched_sessions(&self) -> usize {
        self.by_session.len()
    }
}

/// One watched target inside a subscription: its process-local id and the live
/// `(term, sid_string, ctx)` handle resolved ONCE at subscribe time, plus the
/// per-target send cursors (`last_sent` content seq, last block id) so the
/// coalescing compare is O(1) on each wake.
///
/// The multiplex `<sid>` tag on every frame is the `sid_label` (the process-local
/// id as a string), matching the same `@<sel>` the client used; this lets one
/// connection watching N sessions demultiplex by the leading token.
struct Watch {
    /// The process-local id (the registry key + the `<sid>` frame tag).
    local_id: u64,
    /// The live engine handle (cloned out of the store, clone-then-release).
    term: Arc<Mutex<Terminal>>,
    /// The last `content_seq` we emitted a DELTA for. A wake with an unchanged
    /// seq emits NOTHING (a pure viewport scroll never bumps content_seq).
    last_sent_seq: u64,
    /// The id of the highest block we have already reported `block-complete` for,
    /// so re-scanning completed blocks on each wake never double-emits. `None`
    /// before any block has completed.
    last_block_id: Option<u64>,
    /// The live byte-stream subscription for the `bytes` stream (Item 2), or `None`
    /// when `bytes` was not requested. Drained every wake into `BYTES`/`GAP` frames.
    byte_sub: Option<ByteSubscription>,
    /// `every-frame` mode: re-emit the `cells` frame on EVERY wake even when
    /// `content_seq` is unchanged (animation fidelity), instead of only on advance.
    non_coalesced: bool,
}

/// The wire `<sid>` tag for a target: its process-local id. One connection watching
/// multiple sessions demultiplexes frames by this leading token.
fn sid_tag(local_id: u64) -> String {
    local_id.to_string()
}

/// Read the CURRENT screen as `(seq, rows)` where each row is the trimmed visible
/// text — via [`crate::control::visible_row`], the SAME single source the
/// `text`/`text --json` verbs use, so a pushed DELTA row is byte-identical to a
/// polled `text` row. Returns the `content_seq` ALONGSIDE the rows under ONE lock
/// so the seq matches the rows.
fn read_screen(term: &Arc<Mutex<Terminal>>) -> (u64, Vec<String>) {
    let t = crate::term_lock(term);
    let seq = t.content_seq();
    let rows = t.rows() as usize;
    let mut out = Vec::with_capacity(rows);
    for r in 0..rows {
        out.push(crate::control::visible_row(&t, r));
    }
    (seq, out)
}

/// Read the CURRENT cursor as `(row, col, style)` — the deterministic state the
/// `cursor` verb reports (no blink phase involved; that is a renderer concern).
fn read_cursor(term: &Arc<Mutex<Terminal>>) -> (u16, u16, &'static str) {
    let t = crate::term_lock(term);
    let c = t.cursor();
    (
        c.row,
        c.col,
        crate::control::cursor_style_name(t.cursor_style()),
    )
}

/// Format a full screen DELTA for `sid` at `seq`: a header line followed by one
/// row per screen line (CHANGED set is the whole screen here — a coalesced wake
/// re-reads the latest grid rather than a diff, the backpressure-safe choice). The
/// row count is on the header so a client can frame the body without guessing.
///
/// `DELTA <sid> seq=<n> screen <nrows>\n` then `<nrows>` trimmed rows.
fn frame_screen(sid: &str, seq: u64, rows: &[String]) -> String {
    let mut out = format!("DELTA {sid} seq={seq} screen {}\n", rows.len());
    for r in rows {
        out.push_str(r);
        out.push('\n');
    }
    out
}

/// Format a cursor DELTA for `sid` at `seq`:
/// `DELTA <sid> seq=<n> cursor <row> <col> <style>\n`.
fn frame_cursor(sid: &str, seq: u64, row: u16, col: u16, style: &str) -> String {
    format!("DELTA {sid} seq={seq} cursor {row} {col} {style}\n")
}

/// Format a styled CELLS DELTA — the lossless styled-screen frame pushed live.
/// LENGTH-PREFIXED so the (large, single-line) JSON body frames cleanly on the
/// line-based socket: `DELTA <sid> seq=<n> cells <nbytes>\n<json>\n`. The body is
/// exactly Item 1's `styled_frame_payload` (one physical line); the trailing `\n`
/// is a non-counted terminator the client discards after reading `<nbytes>`.
fn frame_cells(sid: &str, seq: u64, payload: &str) -> String {
    format!("DELTA {sid} seq={seq} cells {}\n{payload}\n", payload.len())
}

/// Read the styled-screen frame payload (Item 1) under one lock.
fn read_cells_payload(term: &Arc<Mutex<Terminal>>) -> String {
    let t = crate::term_lock(term);
    crate::control::styled_frame_payload(&t)
}

/// Drain the `bytes` stream into `BYTES`/`GAP` frames as RAW bytes (binary-safe —
/// no escaping, no UTF-8 decode). A counted `GAP <sid> bytes-dropped=<n>` precedes
/// the bursts whenever the per-subscriber queue overflowed since the last drain.
/// Canonical binary framing: `BYTES <sid> <len>\n<len bytes>\n`.
fn drain_bytes_frames(watch: &mut Watch) -> Vec<u8> {
    let sid = sid_tag(watch.local_id);
    let mut out: Vec<u8> = Vec::new();
    let Some(bs) = &watch.byte_sub else {
        return out;
    };
    let (bursts, dropped) = bs.drain();
    if dropped > 0 {
        out.extend_from_slice(format!("GAP {sid} bytes-dropped={dropped}\n").as_bytes());
    }
    for burst in bursts {
        out.extend_from_slice(format!("BYTES {sid} {}\n", burst.len()).as_bytes());
        out.extend_from_slice(&burst);
        out.push(b'\n');
    }
    out
}

/// Format a block-complete EVENT for `sid`:
/// `EVENT <sid> block-complete <id> exit=<code|->\n`.
fn frame_block_complete(sid: &str, id: u64, exit: Option<i32>) -> String {
    let exit = exit.map_or_else(|| "-".to_string(), |c| c.to_string());
    format!("EVENT {sid} block-complete {id} exit={exit}\n")
}

/// Format a resync GAP for `sid`: `GAP <sid> resync=<seq>\n`. Emitted when the
/// engine's `content_seq` moved BACKWARD relative to our last-sent (a reset /
/// alt-screen swap / engine rebuild made the prior delta cursor meaningless), so
/// the client knows to treat the next DELTA as a fresh full snapshot.
fn frame_gap(sid: &str, seq: u64) -> String {
    format!("GAP {sid} resync={seq}\n")
}

/// Scan the target's completed blocks and emit a `block-complete` EVENT for every
/// block whose id is strictly greater than `last_block_id`, advancing it. The scan
/// clones the small `(id, exit)` tuples OUT under the lock and releases BEFORE any
/// socket write (the lock is never held across a write). Returns the new
/// `last_block_id` watermark.
fn drain_block_events(
    term: &Arc<Mutex<Terminal>>,
    sid: &str,
    last_block_id: Option<u64>,
    out: &mut String,
) -> Option<u64> {
    let completed: Vec<(u64, Option<i32>)> = {
        let t = crate::term_lock(term);
        t.all_blocks()
            .filter(|b| b.is_complete())
            .map(|b| (b.id, b.exit_code))
            .collect()
    };
    let mut high = last_block_id;
    for (id, exit) in completed {
        let newer = high.is_none_or(|h| id > h);
        if newer {
            out.push_str(&frame_block_complete(sid, id, exit));
            high = Some(high.map_or(id, |h| h.max(id)));
        }
    }
    high
}

/// Build the frames a single wake produces for one watched target, mutating its
/// send cursors. Returns the (possibly empty) byte string to write to the
/// subscriber socket. PURE w.r.t. the socket — the caller does the write — so this
/// is unit-testable headlessly with no real connection.
///
/// COALESCING: a screen/cursor DELTA is emitted ONLY when `content_seq` ADVANCED
/// past `last_sent_seq`; an unchanged seq (e.g. a pure viewport scroll, which never
/// bumps `content_seq`) emits nothing. A wake always re-reads the LATEST state, so
/// N coalesced producer wakes collapse into ONE delta carrying the newest grid.
fn frames_for_watch(watch: &mut Watch, streams: Streams) -> String {
    let sid = sid_tag(watch.local_id);
    let mut out = String::new();

    if streams.wants_content() {
        let (seq, rows) = read_screen(&watch.term);
        let emit = |out: &mut String, watch: &Watch, seq: u64| {
            if streams.screen {
                out.push_str(&frame_screen(&sid, seq, &rows));
            }
            if streams.cursor {
                let (cr, cc, cs) = read_cursor(&watch.term);
                out.push_str(&frame_cursor(&sid, seq, cr, cc, cs));
            }
            if streams.cells {
                out.push_str(&frame_cells(&sid, seq, &read_cells_payload(&watch.term)));
            }
        };
        if seq < watch.last_sent_seq {
            // The engine's content seq moved BACKWARD (reset / engine rebuild):
            // the client's prior delta cursor is meaningless — signal a resync.
            out.push_str(&frame_gap(&sid, seq));
            watch.last_sent_seq = seq;
            emit(&mut out, watch, seq);
        } else if seq > watch.last_sent_seq {
            watch.last_sent_seq = seq;
            emit(&mut out, watch, seq);
        } else if watch.non_coalesced && streams.cells {
            // `every-frame` mode: re-emit the styled frame on an unchanged seq so a
            // fast-repainting TUI's transient states are observable (animation
            // fidelity). Only `cells` re-emits; screen/cursor stay coalesced.
            out.push_str(&frame_cells(&sid, seq, &read_cells_payload(&watch.term)));
        }
        // else seq == last_sent_seq and not every-frame: emit nothing.
    }

    if streams.events {
        watch.last_block_id = drain_block_events(&watch.term, &sid, watch.last_block_id, &mut out);
    }

    out
}

/// A resolved subscribe target tuple, cloned OUT of the store at subscribe time:
/// the process-local id, the live engine handle, and the session's live byte
/// fan-out (for the `bytes` stream — a `subscribe` registers on it lazily).
pub type ResolvedTarget = (u64, Arc<Mutex<Terminal>>, Arc<ByteFanout>);

/// The PUSH LOOP for a `subscribe` connection. The connection has already
/// AUTHORIZED every target via the control gate; here we just register for wakes,
/// emit an immediate catch-up (so a fresh subscriber sees the current screen) and
/// optionally honor `since=<seq>`, then block on the registry notify and push a
/// coalesced frame on each wake until the client disconnects (write fails) or the
/// loop is asked to stop.
///
/// PUSH-ONLY: once here, the connection never reads another request line. The
/// writer is the ONLY thing this loop touches on the socket. A write failure
/// (broken pipe / slow-then-dead client) ends the loop and drops the
/// [`Subscription`] (deregistering), so the producer never pays for a dead
/// subscriber.
///
/// `since` (optional, applied per target): the client's last-seen `content_seq`.
/// If the live content has advanced past it, the first wake's compare already
/// emits a catch-up DELTA; we seed each watch's `last_sent_seq` to `since` so the
/// immediate catch-up fires exactly when content moved past `since`.
pub fn push_loop<W: Write>(
    registry: &Subscribers,
    store: &Store,
    targets: &[ResolvedTarget],
    streams: Streams,
    since: Option<u64>,
    non_coalesced: bool,
    writer: &mut W,
) {
    let local_ids: Vec<u64> = targets.iter().map(|(id, _, _)| *id).collect();
    let sub = SubscriberSet::register(registry, &local_ids);

    // Build the per-target send cursors. `since` seeds `last_sent_seq` so the
    // IMMEDIATE catch-up below fires exactly when the live content advanced past
    // the client's last-seen seq; otherwise we start at 0 (a brand-new subscriber
    // gets a full snapshot on the first wake / immediate pass).
    let mut watches: Vec<Watch> = targets
        .iter()
        .map(|(id, term, fanout)| Watch {
            local_id: *id,
            term: term.clone(),
            last_sent_seq: since.unwrap_or(0),
            // Seed the block watermark to the CURRENT high so we only push blocks
            // that COMPLETE after subscription, never the historical backlog —
            // `events` is a live stream, not a replay (matches `since` for screen).
            last_block_id: initial_block_watermark(term, streams),
            // Register on the byte fan-out ONLY when `bytes` is requested, so an
            // idle/unsubscribed session pays nothing for the live byte channel.
            byte_sub: if streams.bytes {
                Some(fanout.subscribe())
            } else {
                None
            },
            non_coalesced,
        })
        .collect();

    // IMMEDIATE catch-up: emit the current state once so a fresh subscriber is not
    // blind until the next output burst. With `since`, this fires a DELTA only if
    // content already advanced past `since`; without it, it sends the full screen.
    // (The `bytes` stream has no backlog to replay — it is live from this point.)
    for w in &mut watches {
        let frame = frames_for_watch(w, streams);
        if !frame.is_empty() && writer.write_all(frame.as_bytes()).is_err() {
            return; // client already gone
        }
    }
    // Surface a client that closed DURING catch-up immediately (same as the loop's
    // post-write flush at the bottom). Ignoring this let a dead subscriber linger —
    // registered, never reaped — until the watched session next produced output (a
    // silent session = effectively never), wasting a registry slot + notify channel.
    if writer.flush().is_err() {
        return;
    }

    // The push loop proper. Block on a notify (bounded so a never-producing set of
    // sessions still lets the loop notice a dropped client on the next write), then
    // re-read the LATEST state of every watched target and push a coalesced frame.
    loop {
        // A bounded wait: on a real wake we push immediately; on a timeout we still
        // loop (a no-op pass that costs one cheap content_seq compare per target and
        // lets a half-closed socket surface via the next write). The producer never
        // waits on us regardless (single-slot notify), so this interval only bounds
        // OUR own liveness, not the producer's.
        let _woke = sub.wait(Duration::from_millis(250));

        // Re-resolve liveness: a target deregistered from the store (its pane
        // closed) is dropped from our watch set so we stop reading a dead engine.
        prune_closed(store, &mut watches);
        if watches.is_empty() {
            return; // every watched session closed
        }

        // Per watch: the UTF-8 text/cells frames, then the RAW binary byte frames.
        // Writing per-watch keeps each session's frames contiguous; the byte frames
        // are length-prefixed so a client demuxes text vs binary unambiguously.
        let mut wrote = false;
        for w in &mut watches {
            let text = frames_for_watch(w, streams);
            if !text.is_empty() {
                if writer.write_all(text.as_bytes()).is_err() {
                    return; // dead client: end loop, drop Subscription (deregister)
                }
                wrote = true;
            }
            if streams.bytes {
                let bytes = drain_bytes_frames(w);
                if !bytes.is_empty() {
                    if writer.write_all(&bytes).is_err() {
                        return;
                    }
                    wrote = true;
                }
            }
        }
        if wrote && writer.flush().is_err() {
            return;
        }
    }
}

/// The block-id watermark to start a fresh `events` subscription at: the current
/// highest completed block id (so only blocks completing AFTER subscription are
/// pushed). `None` when the `events` stream is not requested or no block has
/// completed yet.
fn initial_block_watermark(term: &Arc<Mutex<Terminal>>, streams: Streams) -> Option<u64> {
    if !streams.events {
        return None;
    }
    let t = crate::term_lock(term);
    t.all_blocks()
        .filter(|b| b.is_complete())
        .map(|b| b.id)
        .max()
}

/// Drop any watched target whose session has been DEREGISTERED from the store
/// (its pane closed). Keeps the watch set tracking only live engines. A closed
/// session simply stops producing frames; the registry notify for it is already a
/// cheap miss after deregistration.
fn prune_closed(store: &Store, watches: &mut Vec<Watch>) {
    let g = store.read().unwrap_or_else(|p| p.into_inner());
    watches.retain(|w| g.by_local(w.local_id).is_some());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// A subscriber registered for a session is woken by a notify on that session,
    /// and NOT by a notify on an unrelated session.
    #[test]
    fn notify_wakes_only_subscribed_sessions() {
        let reg = new_registry();
        let sub = SubscriberSet::register(&reg, &[7]);

        // A notify on an unrelated session does not wake us.
        reg.lock().unwrap().notify(99);
        assert!(
            !sub.wait(Duration::from_millis(20)),
            "unrelated notify must not wake"
        );

        // A notify on our session wakes us.
        reg.lock().unwrap().notify(7);
        assert!(sub.wait(Duration::from_millis(200)), "our notify must wake");
    }

    /// COALESCING: a flood of notifies between two `wait`s collapses to a single
    /// pending wake (single-slot). The producer's `notify` never blocks regardless
    /// of how far behind the subscriber is.
    #[test]
    fn notify_coalesces_and_never_blocks_producer() {
        let reg = new_registry();
        let sub = SubscriberSet::register(&reg, &[1]);

        // 1000 notifies with NO intervening read: every one is O(1) and non-blocking
        // even though the slot fills after the first. (If notify ever blocked, this
        // would deadlock on the same thread.)
        for _ in 0..1000 {
            reg.lock().unwrap().notify(1);
        }
        // Exactly one wake is pending (coalesced); the second wait times out.
        assert!(sub.wait(Duration::from_millis(200)), "first wake delivered");
        assert!(
            !sub.wait(Duration::from_millis(20)),
            "flood coalesced to one wake"
        );
    }

    /// A notify to a session with a DROPPED subscriber is a no-op and the registry
    /// self-cleans: the dropped subscription deregisters, so the producer pays
    /// nothing for a dead subscriber.
    #[test]
    fn dropped_subscriber_deregisters_and_notify_is_noop() {
        let reg = new_registry();
        {
            let _sub = SubscriberSet::register(&reg, &[5]);
            assert_eq!(reg.lock().unwrap().watched_sessions(), 1);
        } // _sub dropped here
        assert_eq!(
            reg.lock().unwrap().watched_sessions(),
            0,
            "deregistered on drop"
        );
        // Still a safe no-op.
        reg.lock().unwrap().notify(5);
    }

    /// A STALLED subscriber (never calls `wait`) cannot block or backpressure the
    /// producer: 100k notifies complete instantly while the slot stays full.
    #[test]
    fn stalled_subscriber_never_blocks_producer() {
        let reg = new_registry();
        let _sub = SubscriberSet::register(&reg, &[3]); // never wait()ed: wedged
        let start = std::time::Instant::now();
        for _ in 0..100_000 {
            reg.lock().unwrap().notify(3);
        }
        // If notify blocked on a full slot this would never finish; assert it is fast.
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "producer not blocked by stall"
        );
    }

    /// Multiplex: one subscription watching two sessions is woken by EITHER.
    #[test]
    fn one_subscription_watches_multiple_sessions() {
        let reg = new_registry();
        let sub = SubscriberSet::register(&reg, &[10, 20]);
        reg.lock().unwrap().notify(20);
        assert!(
            sub.wait(Duration::from_millis(200)),
            "woken by either watched session"
        );
    }

    /// Stream parsing: a subset of the known streams parses; an empty list or a
    /// typo fails closed (so a bad request never silently subscribes to nothing).
    #[test]
    fn streams_parse_subset_and_fail_closed() {
        assert_eq!(
            Streams::parse("screen"),
            Some(Streams {
                screen: true,
                ..Default::default()
            })
        );
        assert_eq!(
            Streams::parse("screen,cursor,events"),
            Some(Streams {
                screen: true,
                cursor: true,
                events: true,
                ..Default::default()
            }),
        );
        assert_eq!(
            Streams::parse("cursor screen"),
            Some(Streams {
                screen: true,
                cursor: true,
                ..Default::default()
            }),
        );
        assert_eq!(Streams::parse(""), None, "empty fails closed");
        assert_eq!(Streams::parse("bogus"), None, "unknown stream fails closed");
        assert_eq!(
            Streams::parse("screen,bogus"),
            None,
            "one bad token fails the whole list"
        );
    }

    /// CORE coalescing claim at the FRAME level: a screen DELTA is emitted only when
    /// the engine's `content_seq` ADVANCES; a wake with unchanged content (a pure
    /// viewport scroll never bumps `content_seq`) emits NOTHING. Each emitted frame
    /// is `<sid>`-tagged so a multiplexed client can demultiplex it.
    #[test]
    fn screen_delta_on_content_change_none_on_viewport_scroll() {
        let term = Arc::new(Mutex::new(Terminal::new(24, 80)));
        let streams = Streams {
            screen: true,
            ..Default::default()
        };
        let mut w = Watch {
            local_id: 4,
            term: term.clone(),
            last_sent_seq: 0,
            last_block_id: None,
            byte_sub: None,
            non_coalesced: false,
        };

        // First wake on a fresh engine: content_seq is already > 0 (the engine
        // initialized its grid), so an immediate catch-up DELTA is produced, tagged
        // with our sid (4).
        crate::term_lock(&term).process(b"hello");
        let f1 = frames_for_watch(&mut w, streams);
        assert!(
            f1.starts_with("DELTA 4 seq="),
            "sid-tagged screen delta: {f1:?}"
        );
        assert!(f1.contains("hello"), "delta carries the live screen text");
        let seq_after_write = w.last_sent_seq;
        assert!(seq_after_write > 0);

        // A wake with NO content change (we only move the viewport — a pure scroll
        // does not bump content_seq) emits NOTHING.
        crate::term_lock(&term).scroll_display(1);
        let f2 = frames_for_watch(&mut w, streams);
        assert!(f2.is_empty(), "viewport scroll produces no delta: {f2:?}");
        assert_eq!(
            w.last_sent_seq, seq_after_write,
            "seq unchanged by a scroll"
        );

        // A real content change DOES advance the seq and re-emits a delta.
        crate::term_lock(&term).process(b" world");
        let f3 = frames_for_watch(&mut w, streams);
        assert!(
            f3.starts_with("DELTA 4 seq="),
            "content change re-emits: {f3:?}"
        );
        assert!(
            w.last_sent_seq > seq_after_write,
            "seq advanced on real content"
        );
    }

    /// MULTIPLEX: two distinct watches produce frames tagged with their OWN sid, so
    /// a single connection watching both can demultiplex by the leading `<sid>`.
    #[test]
    fn multiplex_two_sids_tag_their_own_deltas() {
        let term_a = Arc::new(Mutex::new(Terminal::new(24, 80)));
        let term_b = Arc::new(Mutex::new(Terminal::new(24, 80)));
        crate::term_lock(&term_a).process(b"alpha");
        crate::term_lock(&term_b).process(b"bravo");
        let streams = Streams {
            screen: true,
            ..Default::default()
        };
        let mut wa = Watch {
            local_id: 1,
            term: term_a,
            last_sent_seq: 0,
            last_block_id: None,
            byte_sub: None,
            non_coalesced: false,
        };
        let mut wb = Watch {
            local_id: 2,
            term: term_b,
            last_sent_seq: 0,
            last_block_id: None,
            byte_sub: None,
            non_coalesced: false,
        };

        let fa = frames_for_watch(&mut wa, streams);
        let fb = frames_for_watch(&mut wb, streams);
        assert!(fa.starts_with("DELTA 1 "), "watch A tags sid 1: {fa:?}");
        assert!(fa.contains("alpha"));
        assert!(fb.starts_with("DELTA 2 "), "watch B tags sid 2: {fb:?}");
        assert!(fb.contains("bravo"));
    }

    /// A cursor DELTA carries the deterministic cursor state in the SAME wire shape
    /// the `cursor` verb reports, tagged with the sid.
    #[test]
    fn cursor_delta_reports_position_and_style() {
        let term = Arc::new(Mutex::new(Terminal::new(24, 80)));
        crate::term_lock(&term).process(b"abc");
        let streams = Streams {
            cursor: true,
            ..Default::default()
        };
        let mut w = Watch {
            local_id: 9,
            term,
            last_sent_seq: 0,
            last_block_id: None,
            byte_sub: None,
            non_coalesced: false,
        };
        let f = frames_for_watch(&mut w, streams);
        // "abc" advances the cursor to col 3 on row 0.
        assert!(f.contains("DELTA 9 seq="), "sid-tagged cursor delta: {f:?}");
        assert!(f.contains("cursor 0 3 "), "cursor row/col reported: {f:?}");
    }

    /// `since=<seq>` SEMANTICS at the frame level: seeding `last_sent_seq` to the
    /// CURRENT content seq means a fresh wake emits NOTHING (the client is already
    /// caught up); seeding it BELOW the current content seq emits an immediate
    /// catch-up DELTA.
    #[test]
    fn since_seeds_catch_up_only_when_content_advanced() {
        let term = Arc::new(Mutex::new(Terminal::new(24, 80)));
        crate::term_lock(&term).process(b"state");
        let cur = crate::term_lock(&term).content_seq();
        let streams = Streams {
            screen: true,
            ..Default::default()
        };

        // since == current seq: caught up, no catch-up frame.
        let mut caught_up = Watch {
            local_id: 1,
            term: term.clone(),
            last_sent_seq: cur,
            last_block_id: None,
            byte_sub: None,
            non_coalesced: false,
        };
        assert!(
            frames_for_watch(&mut caught_up, streams).is_empty(),
            "no frame when caught up"
        );

        // since below current seq: an immediate catch-up DELTA fires.
        let mut behind = Watch {
            local_id: 1,
            term,
            last_sent_seq: cur - 1,
            last_block_id: None,
            byte_sub: None,
            non_coalesced: false,
        };
        assert!(
            frames_for_watch(&mut behind, streams).starts_with("DELTA 1 "),
            "catch-up delta when content advanced past since",
        );
    }

    /// ITEM 2: the `cells` and `bytes` stream tokens parse (additively with the
    /// existing screen/cursor/events).
    #[test]
    fn streams_parse_accepts_cells_and_bytes() {
        assert_eq!(
            Streams::parse("cells"),
            Some(Streams {
                cells: true,
                ..Default::default()
            })
        );
        assert_eq!(
            Streams::parse("bytes"),
            Some(Streams {
                bytes: true,
                ..Default::default()
            })
        );
        assert_eq!(
            Streams::parse("cells,bytes,screen"),
            Some(Streams {
                cells: true,
                bytes: true,
                screen: true,
                ..Default::default()
            }),
        );
    }

    /// A `cells` DELTA carries the LOSSLESS styled-screen JSON payload (Item 1),
    /// length-prefixed, on content advance.
    #[test]
    fn cells_delta_carries_styled_payload() {
        let term = Arc::new(Mutex::new(Terminal::new(2, 4)));
        crate::term_lock(&term).process(b"\x1b[1mhi");
        let streams = Streams {
            cells: true,
            ..Default::default()
        };
        let mut w = Watch {
            local_id: 5,
            term,
            last_sent_seq: 0,
            last_block_id: None,
            byte_sub: None,
            non_coalesced: false,
        };
        let f = frames_for_watch(&mut w, streams);
        assert!(f.starts_with("DELTA 5 seq="), "sid-tagged cells delta: {f}");
        assert!(f.contains(" cells "), "is a cells frame: {f}");
        assert!(
            f.contains("\"rows\":[["),
            "carries the styled frame payload: {f}"
        );
        assert!(f.contains("\"bold\""), "carries resolved decorations: {f}");
        // The length prefix matches the JSON body byte length.
        let header = f.lines().next().unwrap();
        let nbytes: usize = header.rsplit(' ').next().unwrap().parse().unwrap();
        let body = &f[header.len() + 1..f.len() - 1]; // between header\n and trailing \n
        assert_eq!(body.len(), nbytes, "length prefix matches body: {f}");
    }

    /// The `bytes` stream drains EVERY burst byte-exactly (incl. non-UTF-8) as
    /// length-prefixed `BYTES` frames — live and every-frame, no coalescing.
    #[test]
    fn bytes_drain_is_byte_lossless_and_every_frame() {
        let fan = Arc::new(ByteFanout::new());
        let term = Arc::new(Mutex::new(Terminal::new(2, 4)));
        let bs = fan.subscribe();
        fan.tee(&Arc::from(&b"\x1b[31m"[..]));
        fan.tee(&Arc::from(&[0x80u8, 0x00][..])); // non-UTF-8 + NUL
        let mut w = Watch {
            local_id: 7,
            term,
            last_sent_seq: 0,
            last_block_id: None,
            byte_sub: Some(bs),
            non_coalesced: false,
        };
        let out = drain_bytes_frames(&mut w);
        let mut expected: Vec<u8> = Vec::new();
        expected.extend_from_slice(b"BYTES 7 5\n\x1b[31m\n");
        expected.extend_from_slice(b"BYTES 7 2\n\x80\x00\n");
        assert_eq!(out, expected, "byte-exact, every-frame, length-prefixed");
    }

    /// A queue overflow surfaces as a counted `GAP … bytes-dropped=` before the
    /// surviving (newest) bursts.
    #[test]
    fn bytes_gap_emitted_when_queue_overflowed() {
        let fan = Arc::new(ByteFanout::with_budget(4));
        let term = Arc::new(Mutex::new(Terminal::new(2, 4)));
        let bs = fan.subscribe();
        for _ in 0..10 {
            fan.tee(&Arc::from(&b"abcd"[..]));
        }
        let mut w = Watch {
            local_id: 7,
            term,
            last_sent_seq: 0,
            last_block_id: None,
            byte_sub: Some(bs),
            non_coalesced: false,
        };
        let out = String::from_utf8_lossy(&drain_bytes_frames(&mut w)).into_owned();
        assert!(
            out.starts_with("GAP 7 bytes-dropped="),
            "gap precedes bursts: {out}"
        );
        assert!(out.contains("BYTES 7 4\n"), "newest burst survives: {out}");
    }

    /// `every-frame` mode re-emits the `cells` frame even when `content_seq` is
    /// unchanged (animation fidelity); coalesced mode emits nothing on no change.
    #[test]
    fn every_frame_reemits_cells_on_unchanged_seq() {
        let term = Arc::new(Mutex::new(Terminal::new(2, 4)));
        crate::term_lock(&term).process(b"x");
        let streams = Streams {
            cells: true,
            ..Default::default()
        };
        // Coalesced: second call on unchanged seq emits nothing.
        let mut w = Watch {
            local_id: 1,
            term: term.clone(),
            last_sent_seq: 0,
            last_block_id: None,
            byte_sub: None,
            non_coalesced: false,
        };
        assert!(frames_for_watch(&mut w, streams).starts_with("DELTA 1 "));
        assert!(
            frames_for_watch(&mut w, streams).is_empty(),
            "coalesced: no re-emit on unchanged seq"
        );
        // every-frame: re-emits cells on unchanged seq.
        let mut w2 = Watch {
            local_id: 1,
            term,
            last_sent_seq: 0,
            last_block_id: None,
            byte_sub: None,
            non_coalesced: true,
        };
        assert!(frames_for_watch(&mut w2, streams).starts_with("DELTA 1 "));
        assert!(
            frames_for_watch(&mut w2, streams).contains(" cells "),
            "every-frame re-emits cells on unchanged seq",
        );
    }
}
