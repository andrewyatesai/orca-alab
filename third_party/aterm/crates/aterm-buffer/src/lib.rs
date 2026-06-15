// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! aterm buffer-primitives kernel — the substrate of the 11-verb `BufferApi`
//! (ATERM_DESIGN §4), built bottom-up.
//!
//! This first slice lands the **CONSISTENCY core**: the single, bounded,
//! sequence-numbered event-log spine (§3.4) and the `apply`/`read_text`/
//! `resolve`/`snapshot` verbs over it. It deliberately stops short of the full
//! trait (read_image needs the Rasterizer, process needs world effects, spans &
//! transact are the next slices) so the freeze (§4, M1) lands piece by verified
//! piece rather than as a big bang.
//!
//! Invariant proven here AND model-checked by `aterm-spec-models/specs/` (Kernel.tla; Subscribe/Snapshot/Transact/Evict.tla cover poll, snapshot isolation, transact OCC, and ring eviction):
//! the event log is a **gap-free, strictly-monotonic spine** — every `apply`
//! yields exactly one new `Seq`, and `seq == log.len()` always (§4.3 clause 1).
//!
//! STATUS: per §0.1 — designed-for-verification; the Trust contracts and the
//! `aterm-buffer` TLA+ ledger (§6.2) are not yet green. This is tested, not proven.

#![forbid(unsafe_code)]

use std::num::NonZeroU64;

/// A monotonic position on the one event-log spine (§4.3 clause 1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Seq(pub u64);

/// The addressing root (§3.3): a Surface is the namespace every Addr lives in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SurfaceId(pub NonZeroU64);

/// A committed-line identity, stable across scroll/eviction (§3.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LineId(pub u64);

/// Where a byte/cell came from — carried on every read so prompt injection is
/// legible and never silently trusted (§4.3 clause 4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OriginTag {
    /// Authoritative output from the surface's owning Source (e.g. the PTY child).
    Source,
    /// An ephemeral overlay written by a non-owner (§3.2).
    Overlay,
    /// Engine-synthesized (banner, status).
    System,
}

/// Capability witnesses — passed BY REFERENCE; a verb with no matching cap is
/// unreachable, not "denied at runtime" (§4.3 clause 6, §5.4). These are
/// placeholder attenuations of a real capability; the sealed mint lands in
/// `aterm-cap` (§5.4).
#[derive(Clone, Copy, Debug)]
pub struct ReadCap;
#[derive(Clone, Copy, Debug)]
pub struct WriteCap;

/// A typed address rooted at the Surface (§3.3), minimal first slice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Addr {
    Surface(SurfaceId),
    Line(SurfaceId, LineId),
    Cell(SurfaceId, LineId, u32),
}

/// `resolve` is TOTAL — every address resolves to a first-class status, never a
/// silently-wrong cell (§4.3 clause 3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resolution {
    Resolved(LineId, u32),
    /// Below the scrollback horizon — gone, but never wrong.
    Evicted,
    /// Survived a width reflow; columns remapped.
    Reflowed,
    /// The live region it named was cleared/superseded.
    Invalidated,
}

/// A half-open line range `[start, end)` for reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Range {
    pub start: LineId,
    pub end: LineId,
}

/// The CLOSED edit algebra — the single `apply` verb's argument (§4.2 MUTATE).
/// Small and total so the screen↔logical addressing proof can't regress.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Edit {
    /// Append a new committed line of text.
    AppendLine(String),
    /// Replace the text of an existing committed line.
    SetLine(LineId, String),
    /// Clear a committed line to empty (kept, so its LineId survives — §3.3).
    ClearLine(LineId),
}

/// A content/structure predicate for `query` (§4.2 READ). search/grep/hit-test
/// all compose over this one fold. First slice: substring match.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Predicate {
    /// Lines whose text contains the needle.
    TextContains(String),
}

/// The outcome of a `transact` (§4.2 COMPOSE): an atomic, isolated apply-group
/// under optimistic concurrency control over a base snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TxnOutcome {
    /// All edits applied atomically; carries the resulting head Seq.
    Committed(Seq),
    /// The surface moved past the base snapshot — nothing applied; caller retries.
    Conflict,
}

/// One entry on the event-log spine: a coalesced high-level op, not a per-cell
/// delta (§3.4). Carries the monotone Seq it was assigned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub seq: Seq,
    pub op: Op,
}

/// High-level op summary recorded on the log (§3.4). Span mutations ride the
/// SAME spine as cell edits — there is no second timeline (§4.3 clause 1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Op {
    Append(LineId),
    Write(LineId),
    Clear(LineId),
    SpanDefine(SpanId),
    SpanRestyle(SpanId),
    SpanDrop(SpanId),
}

/// STRUCTURE axis (§4.2): one anchored typed-span primitive. `mark` = zero-width,
/// `region` = styled, `block` = provenance-typed (§5.7), `media` = pixel-backed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpanKind {
    Mark,
    Region,
    Block,
    Media,
}

/// A span's anchored extent over committed lines (half-open). A `Mark` is
/// zero-width (`start == end`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Extent {
    pub start: LineId,
    pub end: LineId,
}

/// Opaque, kind-specific span payload (a style id, a block label, a media handle).
/// First slice carries a small string; the typed variants land with the renderer.
pub type SpanPayload = String;

/// A first-class span id rooted at its Surface (§3.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SpanId(pub u64);

#[derive(Clone, Debug, PartialEq, Eq)]
struct Span {
    id: SpanId,
    surface: SurfaceId,
    extent: Extent,
    kind: SpanKind,
    payload: SpanPayload,
}

/// The public view of a resolved span (§4.2 `span_resolve`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedSpan {
    pub id: SpanId,
    pub extent: Extent,
    pub kind: SpanKind,
    pub payload: SpanPayload,
}

/// A subscription cursor — the synchronous read-face of the event log (§3.4).
/// The reader pulls new events since `at`; if it has fallen behind the ring's
/// horizon it gets a `Gap` and must re-pull via `read_text` to resync. A slow
/// subscriber NEVER blocks the writer.
#[derive(Clone, Copy, Debug)]
pub struct Cursor {
    at: Seq,
}

/// What a `poll` returns: the new events, or a gap signalling a required re-pull.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SubUpdate {
    Events(Vec<Event>),
    /// The cursor fell behind the live ring; re-pull state via `read_text`.
    Gap { resync_to: Seq },
}

/// Hard ceilings (§3.4): the log is bounded; old events behind the horizon are
/// reclaimed. `seq` keeps counting; the ring just forgets the oldest entries.
pub const MAX_LOG_EVENTS: usize = 1 << 16;

/// The bounded, append-only, sequence-numbered ring — THE only change timeline.
#[derive(Clone, Debug, Default)]
pub struct EventLog {
    ring: std::collections::VecDeque<Event>,
    /// Total events ever appended (monotone; the spine's logical length).
    total: u64,
}

impl EventLog {
    fn append(&mut self, op: Op) -> Seq {
        self.total += 1;
        let seq = Seq(self.total);
        self.ring.push_back(Event { seq, op });
        if self.ring.len() > MAX_LOG_EVENTS {
            self.ring.pop_front();
        }
        seq
    }
    /// The logical length of the spine (total events ever appended).
    pub fn total(&self) -> u64 {
        self.total
    }
    /// The current Seq (head of the spine), or `Seq(0)` before any event.
    pub fn head(&self) -> Seq {
        Seq(self.total)
    }
    /// Live (un-evicted) events, oldest first.
    pub fn live(&self) -> impl Iterator<Item = &Event> {
        self.ring.iter()
    }
}

/// A Surface: the addressing root holding committed lines + the event-log spine
/// (§3.1). First slice models a line as text; the cell/style model integrates
/// `aterm-grid` in the next slice.
#[derive(Clone, Debug)]
pub struct Surface {
    id: SurfaceId,
    lines: Vec<(LineId, String)>,
    next_line: u64,
    log: EventLog,
    /// Spans are a SEPARATE decoration stream, not per-cell fields (§4.2).
    spans: Vec<Span>,
    next_span: u64,
}

impl Surface {
    pub fn new(id: SurfaceId) -> Self {
        Surface {
            id,
            lines: Vec::new(),
            next_line: 0,
            log: EventLog::default(),
            spans: Vec::new(),
            next_span: 0,
        }
    }

    pub fn id(&self) -> SurfaceId {
        self.id
    }
    pub fn log(&self) -> &EventLog {
        &self.log
    }
    /// The current head of the spine (§4.3 clause 1).
    pub fn seq(&self) -> Seq {
        self.log.head()
    }

    fn line_index(&self, id: LineId) -> Option<usize> {
        self.lines.iter().position(|(l, _)| *l == id)
    }

    /// MUTATE — the one buffer-edit verb. Returns the monotone Seq it was
    /// assigned. Reversible (buffer-only). (§4.2)
    pub fn apply(&mut self, _c: &WriteCap, e: Edit) -> Seq {
        match e {
            Edit::AppendLine(text) => {
                let id = LineId(self.next_line);
                self.next_line += 1;
                self.lines.push((id, text));
                self.log.append(Op::Append(id))
            }
            Edit::SetLine(id, text) => {
                if let Some(i) = self.line_index(id) {
                    self.lines[i].1 = text;
                }
                self.log.append(Op::Write(id))
            }
            Edit::ClearLine(id) => {
                if let Some(i) = self.line_index(id) {
                    self.lines[i].1.clear();
                }
                self.log.append(Op::Clear(id))
            }
        }
    }

    /// READ — text projection over a line range, carrying origin + the reflected
    /// Seq (§4.2 READ, §4.3 clause 4). First slice tags all output `Source`.
    pub fn read_text(&self, _c: &ReadCap, r: Range) -> TextWithOrigin {
        let mut out = String::new();
        for (id, text) in &self.lines {
            if *id >= r.start && *id < r.end {
                out.push_str(text);
                out.push('\n');
            }
        }
        TextWithOrigin { text: out, origin: OriginTag::Source, seq: self.seq() }
    }

    /// READ — content/structure fold (§4.2). Returns the addresses of committed
    /// lines satisfying the predicate, over a line range. search/grep compose
    /// over this single verb (§4.4).
    pub fn query(&self, _c: &ReadCap, r: Range, p: &Predicate) -> Vec<Addr> {
        self.lines
            .iter()
            .filter(|(id, _)| *id >= r.start && *id < r.end)
            .filter(|(_, text)| match p {
                Predicate::TextContains(needle) => text.contains(needle.as_str()),
            })
            .map(|(id, _)| Addr::Line(self.id, *id))
            .collect()
    }

    /// ADDRESS — TOTAL resolution: every address maps to a first-class status
    /// (§4.3 clause 3). Never returns a silently-wrong cell.
    pub fn resolve(&self, a: Addr) -> Resolution {
        match a {
            Addr::Surface(s) | Addr::Line(s, _) | Addr::Cell(s, _, _) if s != self.id => {
                Resolution::Invalidated
            }
            Addr::Surface(_) => Resolution::Resolved(LineId(0), 0),
            Addr::Line(_, id) | Addr::Cell(_, id, _) => match self.line_index(id) {
                Some(_) => {
                    let col = if let Addr::Cell(_, _, c) = a { c } else { 0 };
                    Resolution::Resolved(id, col)
                }
                // A LineId below our first live line was evicted; above is not-yet.
                None if id.0 < self.first_line_id().map_or(0, |l| l.0) => Resolution::Evicted,
                None => Resolution::Invalidated,
            },
        }
    }

    fn first_line_id(&self) -> Option<LineId> {
        self.lines.first().map(|(l, _)| *l)
    }

    /// COMPOSE — O(1)-COW snapshot (§4.2). First slice is a cheap clone behind a
    /// SnapshotId; the structural-sharing COW lands with the grid integration.
    pub fn snapshot(&self, _c: &ReadCap) -> Snapshot {
        Snapshot { at: self.seq(), surface: self.clone() }
    }

    // ===== STRUCTURE ===== one anchored typed-span primitive (§4.2).

    /// Define a span; rides the spine like any mutation. Returns its stable id.
    pub fn span_define(
        &mut self,
        _c: &WriteCap,
        extent: Extent,
        kind: SpanKind,
        payload: SpanPayload,
    ) -> SpanId {
        let id = SpanId(self.next_span);
        self.next_span += 1;
        self.spans.push(Span { id, surface: self.id, extent, kind, payload });
        self.log.append(Op::SpanDefine(id));
        id
    }

    /// Resolve a span to its public view, or `None` if dropped/unknown.
    pub fn span_resolve(&self, _c: &ReadCap, id: SpanId) -> Option<ResolvedSpan> {
        self.spans.iter().find(|s| s.id == id).map(|s| ResolvedSpan {
            id: s.id,
            extent: s.extent,
            kind: s.kind,
            payload: s.payload.clone(),
        })
    }

    /// Query spans of a kind overlapping a line range (§4.2). The span/query fold
    /// search/hit-test/overlap all compose over this.
    pub fn span_query(&self, _c: &ReadCap, r: Range, kind: SpanKind) -> Vec<SpanId> {
        self.spans
            .iter()
            .filter(|s| s.kind == kind && Self::overlaps(s.extent, r))
            .map(|s| s.id)
            .collect()
    }

    fn overlaps(e: Extent, r: Range) -> bool {
        if e.start == e.end {
            // zero-width mark: overlaps iff its point falls in [r.start, r.end)
            e.start >= r.start && e.start < r.end
        } else {
            e.start < r.end && e.end > r.start
        }
    }

    /// Restyle a span in place (no id change); rides the spine.
    pub fn span_restyle(&mut self, _c: &WriteCap, id: SpanId, payload: SpanPayload) {
        if let Some(s) = self.spans.iter_mut().find(|s| s.id == id) {
            s.payload = payload;
            self.log.append(Op::SpanRestyle(id));
        }
    }

    /// Drop a span; rides the spine.
    pub fn span_drop(&mut self, _c: &WriteCap, id: SpanId) {
        if let Some(i) = self.spans.iter().position(|s| s.id == id) {
            self.spans.remove(i);
            self.log.append(Op::SpanDrop(id));
        }
    }

    // ===== READ (subscribe) ===== the event log's read-face (§3.4).

    /// Open a subscription cursor positioned at the current head: it will see
    /// events appended AFTER this point.
    pub fn subscribe(&self, _c: &ReadCap) -> Cursor {
        Cursor { at: self.seq() }
    }

    /// Pull what's new since the cursor. Returns the update and the advanced
    /// cursor. A cursor that fell behind the live ring's horizon gets a `Gap`
    /// (it must re-pull via `read_text`); the writer is never blocked.
    pub fn poll(&self, cursor: Cursor) -> (SubUpdate, Cursor) {
        let head = self.seq();
        // Did we fall behind the ring? The oldest still-live event has a seq; if
        // it is newer than cursor.at + 1, events in between were evicted.
        if self.log.live().next().is_some_and(|oldest| oldest.seq.0 > cursor.at.0 + 1) {
            return (SubUpdate::Gap { resync_to: head }, Cursor { at: head });
        }
        let events: Vec<Event> =
            self.log.live().filter(|e| e.seq.0 > cursor.at.0).cloned().collect();
        (SubUpdate::Events(events), Cursor { at: head })
    }

    /// COMPOSE — atomic, isolated apply-group under optimistic CC over a base
    /// snapshot seq (§4.2). Subsumes single-op CAS and frozen-world act: the
    /// editor undo unit, the multi-cursor atomic edit, and the harness scripted
    /// step are all `transact`. Commits iff the surface has not advanced past
    /// `base`; otherwise nothing is applied and the caller retries.
    pub fn transact(&mut self, c: &WriteCap, base: Seq, body: Vec<Edit>) -> TxnOutcome {
        if self.seq() != base {
            return TxnOutcome::Conflict;
        }
        for e in body {
            self.apply(c, e);
        }
        TxnOutcome::Committed(self.seq())
    }
}

/// A read result carrying provenance + the reflected Seq (§4.3 clause 4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextWithOrigin {
    pub text: String,
    pub origin: OriginTag,
    pub seq: Seq,
}

/// A seq-anchored COW prefix of a Surface (§4.2 COMPOSE).
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub at: Seq,
    surface: Surface,
}

impl Snapshot {
    /// Read the frozen world at the snapshot's seq.
    pub fn read_text(&self, c: &ReadCap, r: Range) -> TextWithOrigin {
        self.surface.read_text(c, r)
    }
    /// `branch` — a writable COW fork of the snapshot (§4.2). The fork is an
    /// independent Surface under a fresh id.
    pub fn branch(&self, new_id: SurfaceId) -> Surface {
        let mut s = self.surface.clone();
        s.id = new_id;
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sid(n: u64) -> SurfaceId {
        SurfaceId(NonZeroU64::new(n).unwrap())
    }

    /// THE kernel invariant (§4.3 clause 1), the executable twin of
    /// `Kernel.tla`'s `SeqIsLen` + `Monotonic`: every apply bumps seq by exactly
    /// one, the spine is gap-free, and seq == total events appended.
    #[test]
    fn event_log_is_gap_free_and_monotonic() {
        let mut s = Surface::new(sid(1));
        assert_eq!(s.seq(), Seq(0));
        let mut prev = 0u64;
        for i in 0..1000 {
            let before = s.seq().0;
            s.apply(&WriteCap, Edit::AppendLine(format!("line {i}")));
            let now = s.seq().0;
            assert_eq!(now, before + 1, "each apply yields exactly one Seq");
            assert!(now > prev, "seq is strictly monotonic");
            prev = now;
            // Monotonic + gap-free: seq equals total events ever appended.
            assert_eq!(s.seq().0, s.log().total());
        }
        assert_eq!(s.seq(), Seq(1000));
    }

    #[test]
    fn apply_read_round_trips() {
        let mut s = Surface::new(sid(1));
        s.apply(&WriteCap, Edit::AppendLine("hello".into()));
        s.apply(&WriteCap, Edit::AppendLine("world".into()));
        let got = s.read_text(&ReadCap, Range { start: LineId(0), end: LineId(2) });
        assert_eq!(got.text, "hello\nworld\n");
        assert_eq!(got.origin, OriginTag::Source);
        assert_eq!(got.seq, Seq(2));
    }

    #[test]
    fn resolve_is_total() {
        let mut s = Surface::new(sid(1));
        s.apply(&WriteCap, Edit::AppendLine("x".into()));
        // A live line resolves.
        assert!(matches!(s.resolve(Addr::Line(sid(1), LineId(0))), Resolution::Resolved(..)));
        // A wrong surface never silently succeeds.
        assert_eq!(s.resolve(Addr::Line(sid(2), LineId(0))), Resolution::Invalidated);
        // A not-yet line is a first-class status, never a wrong cell.
        assert!(matches!(
            s.resolve(Addr::Line(sid(1), LineId(99))),
            Resolution::Invalidated | Resolution::Evicted
        ));
    }

    #[test]
    fn snapshot_is_isolated_from_later_writes() {
        let mut s = Surface::new(sid(1));
        s.apply(&WriteCap, Edit::AppendLine("frozen".into()));
        let snap = s.snapshot(&ReadCap);
        s.apply(&WriteCap, Edit::AppendLine("after".into()));
        // The snapshot sees the frozen world; the live surface moved on.
        let snap_text = snap.read_text(&ReadCap, Range { start: LineId(0), end: LineId(9) }).text;
        let live_text = s.read_text(&ReadCap, Range { start: LineId(0), end: LineId(9) }).text;
        assert_eq!(snap_text, "frozen\n");
        assert_eq!(live_text, "frozen\nafter\n");
        assert_eq!(snap.at, Seq(1));
    }

    #[test]
    fn span_lifecycle_rides_the_one_spine() {
        let mut s = Surface::new(sid(1));
        for i in 0..5 {
            s.apply(&WriteCap, Edit::AppendLine(format!("l{i}")));
        }
        let before = s.seq().0;
        let id = s.span_define(
            &WriteCap,
            Extent { start: LineId(1), end: LineId(3) },
            SpanKind::Region,
            "bold".into(),
        );
        // STRUCTURE mutations ride the SAME spine (§4.3 clause 1).
        assert_eq!(s.seq().0, before + 1);

        let rs = s.span_resolve(&ReadCap, id).unwrap();
        assert_eq!(rs.kind, SpanKind::Region);
        assert_eq!(rs.payload, "bold");

        // query by kind + range overlap
        assert_eq!(
            s.span_query(&ReadCap, Range { start: LineId(0), end: LineId(2) }, SpanKind::Region),
            vec![id]
        );
        assert!(
            s.span_query(&ReadCap, Range { start: LineId(0), end: LineId(2) }, SpanKind::Block)
                .is_empty(),
            "wrong kind"
        );
        assert!(
            s.span_query(&ReadCap, Range { start: LineId(3), end: LineId(5) }, SpanKind::Region)
                .is_empty(),
            "non-overlapping range"
        );

        s.span_restyle(&WriteCap, id, "italic".into());
        assert_eq!(s.span_resolve(&ReadCap, id).unwrap().payload, "italic");

        s.span_drop(&WriteCap, id);
        assert!(s.span_resolve(&ReadCap, id).is_none());
    }

    #[test]
    fn subscribe_pulls_new_events_then_drains() {
        let mut s = Surface::new(sid(1));
        let cur = s.subscribe(&ReadCap); // positioned at head (seq 0)
        s.apply(&WriteCap, Edit::AppendLine("a".into()));
        s.apply(&WriteCap, Edit::AppendLine("b".into()));
        let (upd, cur) = s.poll(cur);
        match upd {
            SubUpdate::Events(ev) => {
                assert_eq!(ev.len(), 2);
                assert_eq!(ev[0].seq, Seq(1));
                assert_eq!(ev[1].seq, Seq(2));
            }
            other => panic!("expected events, got {other:?}"),
        }
        // caught up: empty, NOT a gap
        let (upd, _cur) = s.poll(cur);
        assert_eq!(upd, SubUpdate::Events(vec![]));
    }

    #[test]
    fn slow_subscriber_gets_a_gap_and_never_blocks() {
        let mut s = Surface::new(sid(1));
        let cur = s.subscribe(&ReadCap); // at seq 0
        // overflow the bounded ring so the oldest live event is past the cursor
        for i in 0..(MAX_LOG_EVENTS + 8) {
            s.apply(&WriteCap, Edit::AppendLine(format!("{i}")));
        }
        let (upd, _cur) = s.poll(cur);
        assert!(matches!(upd, SubUpdate::Gap { .. }), "fell behind horizon -> gap, not block");
    }

    #[test]
    fn query_folds_content_to_addresses() {
        let mut s = Surface::new(sid(1));
        s.apply(&WriteCap, Edit::AppendLine("error: boom".into()));
        s.apply(&WriteCap, Edit::AppendLine("all good".into()));
        s.apply(&WriteCap, Edit::AppendLine("error: again".into()));
        let hits = s.query(
            &ReadCap,
            Range { start: LineId(0), end: LineId(9) },
            &Predicate::TextContains("error".into()),
        );
        assert_eq!(hits, vec![Addr::Line(sid(1), LineId(0)), Addr::Line(sid(1), LineId(2))]);
    }

    #[test]
    fn transact_is_atomic_and_cc_guarded() {
        let mut s = Surface::new(sid(1));
        s.apply(&WriteCap, Edit::AppendLine("base".into()));
        let snap = s.snapshot(&ReadCap); // base = Seq(1)

        // up-to-date base: the whole group lands atomically
        let out = s.transact(
            &WriteCap,
            snap.at,
            vec![Edit::AppendLine("a".into()), Edit::AppendLine("b".into())],
        );
        assert_eq!(out, TxnOutcome::Committed(Seq(3)));
        assert_eq!(
            s.read_text(&ReadCap, Range { start: LineId(0), end: LineId(9) }).text,
            "base\na\nb\n"
        );

        // stale base: optimistic CC conflicts and applies NOTHING
        let out2 = s.transact(&WriteCap, snap.at, vec![Edit::AppendLine("z".into())]);
        assert_eq!(out2, TxnOutcome::Conflict);
        assert_eq!(s.seq(), Seq(3), "conflict left the surface untouched");
    }
}
