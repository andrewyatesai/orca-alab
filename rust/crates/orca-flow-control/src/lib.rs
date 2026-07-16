//! Producer-side PTY flow control — the pure decision core.
//!
//! Ported from `src/main/ipc/pty-producer-flow-control.ts`. Main tracks the
//! per-PTY renderer-pending char count; once it climbs past HIGH we pause the
//! actual PTY read (node-pty `pause()` → kernel backpressure → the flooding
//! shell blocks on write), and once it drains below LOW we resume. The wide
//! HIGH/LOW gap is deliberate hysteresis: a queue draining one flush-slice at a
//! time must not flap pause/resume every slice.
//!
//! This crate owns ONLY the scalar decision. Given `(pending_chars, now_ms)` it
//! returns [`FlowAction::Pause`] / [`FlowAction::Resume`] / [`FlowAction::None`];
//! the wall clock and the pause/resume transport stay with the caller. That keeps
//! the core deterministic and exhaustively testable.
//!
//! It is NOT a production cutover: `update` is called on every pending-data change
//! (per-chunk hot path in `pty.ts`), so a napi hop would regress exactly like the
//! rejected `pty:data`/napi-string cutovers — the decision is a trivial scalar
//! comparison, cheapest in-process. Instead this core is the machine-checkable,
//! ay-provable SPEC, proven equivalent to the TS production controller by the
//! shared `parity-corpus.txt` both run (P3 stage 2 — see `matches_shared_parity_corpus`).

#![forbid(unsafe_code)]

pub mod keep_tail;

use std::collections::HashMap;

/// Past this many renderer-pending chars, an unpaused PTY is paused. Mirrors
/// `PRODUCER_FLOW_HIGH_WATERMARK_CHARS` in the TS source.
pub const PRODUCER_FLOW_HIGH_WATERMARK_CHARS: u64 = 256 * 1024;
/// Below this many pending chars, a paused PTY is resumed. The gap to HIGH is
/// the anti-flap hysteresis band. Mirrors `PRODUCER_FLOW_LOW_WATERMARK_CHARS`.
pub const PRODUCER_FLOW_LOW_WATERMARK_CHARS: u64 = 32 * 1024;
/// The daemon auto-resumes a pause after its 5s lost-resume failsafe; if pending
/// is still above HIGH past this interval the pause must be re-asserted, or a
/// sustained flood runs unthrottled after the first failsafe fires. Mirrors
/// `PRODUCER_PAUSE_REASSERT_INTERVAL_MS`.
pub const PRODUCER_PAUSE_REASSERT_INTERVAL_MS: u64 = 5_000;

/// The transport action the caller must apply to the PTY read after an update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowAction {
    /// No transport call — either state is unchanged, or pending is moving
    /// inside the hysteresis band.
    None,
    /// Pause the PTY read: the first HIGH crossing, or a re-assert once the
    /// failsafe interval has elapsed while still flooding.
    Pause,
    /// Resume the PTY read: pending drained below LOW, or a teardown release.
    Resume,
}

/// Per-PTY producer flow-control decision machine. One instance owns every PTY
/// on a client; PTYs are tracked independently by id.
pub struct ProducerFlowController {
    high_watermark_chars: u64,
    low_watermark_chars: u64,
    reassert_interval_ms: u64,
    /// id → the monotonic-ms timestamp of the pause we last asserted. Absent
    /// means the PTY is not paused (mirrors the TS `pausedAtByPty` map, whose
    /// key presence is the paused flag).
    paused_at: HashMap<String, u64>,
}

impl ProducerFlowController {
    /// Construct with explicit watermarks (test overrides, alternate tunings).
    pub fn new(
        high_watermark_chars: u64,
        low_watermark_chars: u64,
        reassert_interval_ms: u64,
    ) -> Self {
        Self {
            high_watermark_chars,
            low_watermark_chars,
            reassert_interval_ms,
            paused_at: HashMap::new(),
        }
    }

    /// Construct with the production watermarks (the `PRODUCER_FLOW_*` constants).
    pub fn with_defaults() -> Self {
        Self::new(
            PRODUCER_FLOW_HIGH_WATERMARK_CHARS,
            PRODUCER_FLOW_LOW_WATERMARK_CHARS,
            PRODUCER_PAUSE_REASSERT_INTERVAL_MS,
        )
    }

    /// Report the current pending chars for a PTY at clock `now_ms`. Fires
    /// [`FlowAction::Pause`] exactly at the HIGH crossing (re-asserted only once
    /// the failsafe interval has elapsed), [`FlowAction::Resume`] exactly when
    /// pending drains below LOW, and [`FlowAction::None`] otherwise. The
    /// comparison operators match the TS source byte-for-byte: strictly `>` HIGH
    /// to pause, strictly `<` LOW to resume, `>=` interval to re-assert.
    pub fn update(&mut self, id: &str, pending_chars: u64, now_ms: u64) -> FlowAction {
        match self.paused_at.get(id).copied() {
            None => {
                if pending_chars > self.high_watermark_chars {
                    self.paused_at.insert(id.to_owned(), now_ms);
                    FlowAction::Pause
                } else {
                    FlowAction::None
                }
            }
            Some(paused_at) => {
                if pending_chars < self.low_watermark_chars {
                    self.paused_at.remove(id);
                    FlowAction::Resume
                } else if pending_chars > self.high_watermark_chars
                    // saturating_sub is a safety belt for a non-monotonic clock;
                    // for now_ms >= paused_at (the normal case) it is plain
                    // subtraction, matching the TS `Date.now() - pausedAt`.
                    && now_ms.saturating_sub(paused_at) >= self.reassert_interval_ms
                {
                    self.paused_at.insert(id.to_owned(), now_ms);
                    FlowAction::Pause
                } else {
                    FlowAction::None
                }
            }
        }
    }

    /// Resume a PTY if it was paused — for teardown paths (exit, kill) where the
    /// pending bookkeeping is dropped rather than drained. Returns
    /// [`FlowAction::Resume`] iff it had been paused, else [`FlowAction::None`].
    pub fn release(&mut self, id: &str) -> FlowAction {
        if self.paused_at.remove(id).is_some() {
            FlowAction::Resume
        } else {
            FlowAction::None
        }
    }

    /// Resume every paused PTY — for wholesale bookkeeping wipes (window
    /// destroyed), where a PTY left paused here would stay wedged forever.
    /// Returns the resumed ids (order unspecified) so the caller can drive the
    /// transport for each.
    pub fn release_all(&mut self) -> Vec<String> {
        let ids: Vec<String> = self.paused_at.keys().cloned().collect();
        self.paused_at.clear();
        ids
    }

    /// Whether the PTY is currently paused.
    pub fn is_paused(&self, id: &str) -> bool {
        self.paused_at.contains_key(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Compact tuning so the numbers read clearly: HIGH 100, LOW 10, reassert 5s.
    fn controller() -> ProducerFlowController {
        ProducerFlowController::new(100, 10, 5_000)
    }

    #[test]
    fn defaults_match_ts_constants() {
        assert_eq!(PRODUCER_FLOW_HIGH_WATERMARK_CHARS, 256 * 1024);
        assert_eq!(PRODUCER_FLOW_LOW_WATERMARK_CHARS, 32 * 1024);
        assert_eq!(PRODUCER_PAUSE_REASSERT_INTERVAL_MS, 5_000);
    }

    #[test]
    fn unpaused_below_high_is_noop() {
        let mut fc = controller();
        assert_eq!(fc.update("a", 0, 0), FlowAction::None);
        assert_eq!(fc.update("a", 99, 0), FlowAction::None);
        assert!(!fc.is_paused("a"));
    }

    #[test]
    fn exactly_at_high_does_not_pause() {
        // Strictly `>` HIGH — 100 is not above 100.
        let mut fc = controller();
        assert_eq!(fc.update("a", 100, 0), FlowAction::None);
        assert!(!fc.is_paused("a"));
    }

    #[test]
    fn pause_fires_once_at_high_crossing() {
        let mut fc = controller();
        assert_eq!(fc.update("a", 101, 0), FlowAction::Pause);
        assert!(fc.is_paused("a"));
        // Still flooding, within the reassert window → no repeat pause.
        assert_eq!(fc.update("a", 500, 1), FlowAction::None);
        assert_eq!(fc.update("a", 500, 4_999), FlowAction::None);
    }

    #[test]
    fn resume_fires_below_low() {
        let mut fc = controller();
        fc.update("a", 101, 0);
        // Inside the band (LOW..=HIGH) → hold the pause.
        assert_eq!(fc.update("a", 50, 10), FlowAction::None);
        assert_eq!(fc.update("a", 10, 10), FlowAction::None); // == LOW is not `< LOW`
        assert!(fc.is_paused("a"));
        // Below LOW → resume exactly once.
        assert_eq!(fc.update("a", 9, 20), FlowAction::Resume);
        assert!(!fc.is_paused("a"));
        // Already resumed; below HIGH → nothing.
        assert_eq!(fc.update("a", 9, 21), FlowAction::None);
    }

    #[test]
    fn hysteresis_band_never_flaps() {
        let mut fc = controller();
        fc.update("a", 200, 0); // paused
                                // Oscillating within (LOW, HIGH] must not emit a single action.
        for (n, t) in [(80u64, 1u64), (20, 2), (95, 3), (30, 4), (100, 5)] {
            assert_eq!(fc.update("a", n, t), FlowAction::None);
            assert!(fc.is_paused("a"));
        }
    }

    #[test]
    fn reassert_only_after_interval_and_still_flooding() {
        let mut fc = controller();
        assert_eq!(fc.update("a", 300, 1_000), FlowAction::Pause); // paused at t=1000
                                                                   // Before the interval elapses, even while flooding → no re-assert.
        assert_eq!(fc.update("a", 300, 5_999), FlowAction::None);
        // At exactly interval (>=) and still above HIGH → re-assert, clock resets.
        assert_eq!(fc.update("a", 300, 6_000), FlowAction::Pause);
        // Interval measured from the RE-assert, not the original pause.
        assert_eq!(fc.update("a", 300, 10_999), FlowAction::None);
        assert_eq!(fc.update("a", 300, 11_000), FlowAction::Pause);
    }

    #[test]
    fn reassert_requires_above_high_not_merely_in_band() {
        let mut fc = controller();
        fc.update("a", 300, 0); // paused
                                // Interval elapsed, but pending sits in the band (not above HIGH) → hold.
        assert_eq!(fc.update("a", 100, 10_000), FlowAction::None);
        assert!(fc.is_paused("a"));
    }

    #[test]
    fn release_resumes_iff_paused() {
        let mut fc = controller();
        assert_eq!(fc.release("a"), FlowAction::None); // never paused
        fc.update("a", 200, 0);
        assert_eq!(fc.release("a"), FlowAction::Resume);
        assert!(!fc.is_paused("a"));
        assert_eq!(fc.release("a"), FlowAction::None); // idempotent
    }

    #[test]
    fn release_all_resumes_every_paused_pty() {
        let mut fc = controller();
        fc.update("a", 200, 0);
        fc.update("b", 200, 0);
        fc.update("c", 50, 0); // never crossed HIGH → not paused
        let mut resumed = fc.release_all();
        resumed.sort();
        assert_eq!(resumed, vec!["a".to_string(), "b".to_string()]);
        assert!(!fc.is_paused("a"));
        assert!(!fc.is_paused("b"));
        assert!(fc.release_all().is_empty()); // idempotent
    }

    #[test]
    fn ptys_are_tracked_independently() {
        let mut fc = controller();
        assert_eq!(fc.update("a", 200, 0), FlowAction::Pause);
        // A second PTY under HIGH is untouched by the first's pause.
        assert_eq!(fc.update("b", 50, 0), FlowAction::None);
        assert!(fc.is_paused("a"));
        assert!(!fc.is_paused("b"));
        // Draining `a` leaves `b` alone.
        assert_eq!(fc.update("a", 0, 1), FlowAction::Resume);
        assert!(!fc.is_paused("a"));
        assert!(!fc.is_paused("b"));
    }

    // A full flood→drain→reflood lifecycle emits exactly the pause/resume edges,
    // never a spurious action in between — the property the daemon relies on.
    #[test]
    fn full_lifecycle_emits_only_edges() {
        let mut fc = controller();
        let mut actions = Vec::new();
        // ramp up
        for (n, t) in [(0u64, 0u64), (50, 1), (99, 2), (150, 3), (400, 4), (400, 5)] {
            let a = fc.update("a", n, t);
            if a != FlowAction::None {
                actions.push((t, a));
            }
        }
        // drain down
        for (n, t) in [(200u64, 6u64), (40, 7), (11, 8), (9, 9), (0, 10)] {
            let a = fc.update("a", n, t);
            if a != FlowAction::None {
                actions.push((t, a));
            }
        }
        // reflood
        let a = fc.update("a", 300, 11);
        if a != FlowAction::None {
            actions.push((11, a));
        }
        assert_eq!(
            actions,
            vec![
                (3, FlowAction::Pause),
                (9, FlowAction::Resume),
                (11, FlowAction::Pause)
            ]
        );
    }

    /// Run the SHARED parity corpus (`parity-corpus.txt`) — the exact same
    /// oracle the TS `PtyProducerFlowController` runs in its own test suite. If
    /// the Rust spec and the TS production path ever disagree on a step, one of
    /// the two tests fails. This is the cross-language differential parity
    /// certificate (P3 stage 2): production stays in TS (the flow-control
    /// `update` is per-chunk hot-path, so a napi hop would regress it like the
    /// rejected pty:data cutover), while this Rust core is the machine-checkable
    /// spec proven equivalent to it.
    #[test]
    fn matches_shared_parity_corpus() {
        let corpus = include_str!("../parity-corpus.txt");
        let mut fc = ProducerFlowController::new(100, 10, 5_000);
        for (idx, raw) in corpus.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (lhs, rhs) = line
                .split_once("=>")
                .unwrap_or_else(|| panic!("line {}: missing =>", idx + 1));
            let mut expected: Vec<String> = rhs.split_whitespace().map(str::to_string).collect();
            expected.sort();

            let toks: Vec<&str> = lhs.split_whitespace().collect();
            let mut got: Vec<String> = Vec::new();
            match toks.as_slice() {
                ["update", id, pending, now] => {
                    let action = fc.update(id, pending.parse().unwrap(), now.parse().unwrap());
                    match action {
                        FlowAction::Pause => got.push(format!("pause:{id}")),
                        FlowAction::Resume => got.push(format!("resume:{id}")),
                        FlowAction::None => {}
                    }
                }
                ["release", id] => {
                    if fc.release(id) == FlowAction::Resume {
                        got.push(format!("resume:{id}"));
                    }
                }
                ["releaseAll"] => {
                    for id in fc.release_all() {
                        got.push(format!("resume:{id}"));
                    }
                }
                other => panic!("line {}: unknown op {other:?}", idx + 1),
            }
            got.sort();
            assert_eq!(
                got,
                expected,
                "parity mismatch at line {}: `{line}`",
                idx + 1
            );
        }
    }
}
