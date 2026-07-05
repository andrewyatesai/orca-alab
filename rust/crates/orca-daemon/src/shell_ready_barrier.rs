//! The shell-ready startup barrier — a faithful port of the Node daemon's
//! `shell-ready-marker-scanner.ts` + `session.ts` pre-ready stdin queue +
//! `post-ready-flush-gate.ts`, as one lock-free state machine.
//!
//! Why: startup commands (agent launches) must not be typed into the PTY until
//! the shell has sourced its rc files and drawn a prompt — the wrapper rcfiles
//! (generated client-side, see `docs/rust-migration/daemon-shell-launch.md`)
//! emit an OSC 777 `orca-shell-ready` marker at first prompt. Until then,
//! stdin writes queue; marker bytes (including a partial prefix straddling a
//! read boundary) are held out of the engine/records/stream.
//!
//! The barrier owns NO threads or timers: state transitions return the timer
//! the caller must schedule (`GateTimer`), and elapsed-timer callbacks are
//! generation-checked so a stale timer is a no-op. rpc.rs does the actual
//! `thread::spawn` + sleep wiring.

/// `SHELL_READY_MARKER_PREFIX` in shell-ready-marker-scanner.ts. The full
/// marker is this prefix followed by BEL (`\x07`).
pub const SHELL_READY_MARKER_PREFIX: &str = "\x1b]777;orca-shell-ready";

/// `SHELL_READY_TIMEOUT_MS` in session.ts — the default bound on waiting for a
/// marker that may never come (e.g. a wrapper-less shell).
pub const SHELL_READY_TIMEOUT_MS: u64 = 15_000;
/// `POST_READY_FLUSH_DELAY_MS`: short settle after prompt bytes so readline
/// has enabled raw mode before the flush (avoids a visible echo duplicate).
pub const POST_READY_FLUSH_DELAY_MS: u64 = 30;
/// `POST_READY_FLUSH_FALLBACK_MS`: wall-clock bound for marker-only cases
/// where no further prompt bytes arrive.
pub const POST_READY_FLUSH_FALLBACK_MS: u64 = 200;

// ── Marker scanner ──────────────────────────────────────────────────────────

/// Streaming scanner state (`ShellReadyScanState`): the current match depth
/// into the marker prefix and the held (withheld-from-output) prefix bytes.
#[derive(Default)]
struct MarkerScanState {
    match_pos: usize,
    held: String,
}

struct ScanOutcome {
    output: String,
    matched: bool,
    post_marker_bytes_observed: bool,
}

impl MarkerScanState {
    /// Port of `scanForShellReady`: strips a complete marker from the stream,
    /// holds a partial prefix across chunks, and releases false prefixes back
    /// into the output. The marker is pure ASCII, so per-`char` matching is
    /// equivalent to the TS per-code-unit loop.
    fn scan(&mut self, data: &str) -> ScanOutcome {
        let prefix = SHELL_READY_MARKER_PREFIX.as_bytes();
        let mut output = String::new();
        for (i, ch) in data.char_indices() {
            if self.match_pos < prefix.len() {
                if ch as u32 == prefix[self.match_pos] as u32 {
                    self.held.push(ch);
                    self.match_pos += 1;
                } else {
                    output.push_str(&self.held);
                    self.held.clear();
                    self.match_pos = 0;
                    if ch as u32 == prefix[0] as u32 {
                        self.held.push(ch);
                        self.match_pos = 1;
                    } else {
                        output.push(ch);
                    }
                }
            } else if ch == '\x07' {
                let remaining = &data[i + 1..];
                self.held.clear();
                self.match_pos = 0;
                output.push_str(remaining);
                return ScanOutcome {
                    output,
                    matched: true,
                    post_marker_bytes_observed: !remaining.is_empty(),
                };
            } else {
                // Full prefix but no BEL: a false marker — release it.
                output.push_str(&self.held);
                self.held.clear();
                self.match_pos = 0;
                if ch as u32 == prefix[0] as u32 {
                    self.held.push(ch);
                    self.match_pos = 1;
                } else {
                    output.push(ch);
                }
            }
        }
        ScanOutcome {
            output,
            matched: false,
            post_marker_bytes_observed: false,
        }
    }

    fn drain_held(&mut self) -> String {
        self.match_pos = 0;
        std::mem::take(&mut self.held)
    }
}

// ── Barrier ─────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShellReadyState {
    Pending,
    Ready,
    TimedOut,
    Unsupported,
}

impl ShellReadyState {
    /// The wire string (types.ts `ShellReadyState`).
    pub fn as_wire(&self) -> &'static str {
        match self {
            ShellReadyState::Pending => "pending",
            ShellReadyState::Ready => "ready",
            ShellReadyState::TimedOut => "timed_out",
            ShellReadyState::Unsupported => "unsupported",
        }
    }
}

/// A timer the CALLER must schedule after a transition. Each carries the
/// generation to pass back into the matching `on_*_elapsed` call.
#[derive(Debug, PartialEq, Eq)]
pub enum GateTimer {
    /// Sleep `POST_READY_FLUSH_DELAY_MS`, then `on_post_data_elapsed(gen)`.
    PostData(u64),
    /// Sleep `POST_READY_FLUSH_FALLBACK_MS`, then `on_fallback_elapsed(gen)`.
    Fallback(u64),
}

pub struct ShellReadyBarrier {
    state: ShellReadyState,
    scanner: Option<MarkerScanState>,
    queue: Vec<String>,
    // PostReadyFlushGate state: armed-but-no-prompt-bytes-yet, plus which
    // timer is outstanding. `generation` invalidates stale timers.
    awaiting_prompt_draw: bool,
    fallback_armed: bool,
    post_data_armed: bool,
    generation: u64,
}

impl ShellReadyBarrier {
    pub fn new_pending() -> Self {
        Self {
            state: ShellReadyState::Pending,
            scanner: Some(MarkerScanState::default()),
            queue: Vec::new(),
            awaiting_prompt_draw: false,
            fallback_armed: false,
            post_data_armed: false,
            generation: 0,
        }
    }

    pub fn state(&self) -> ShellReadyState {
        self.state
    }

    /// True while stdin writes must queue: the marker hasn't arrived yet, OR
    /// it has but the post-ready flush gate hasn't fired (writing directly
    /// then would let fresh input race ahead of the buffered startup command).
    pub fn should_queue(&self) -> bool {
        self.state == ShellReadyState::Pending
            || self.awaiting_prompt_draw
            || self.fallback_armed
            || self.post_data_armed
    }

    pub fn push_queued(&mut self, data: String) {
        self.queue.push(data);
    }

    pub fn drain_queue(&mut self) -> Vec<String> {
        std::mem::take(&mut self.queue)
    }

    /// True while pump output must go through `process_output` (marker bytes
    /// may need stripping). Once false, the caller feeds decoded chunks
    /// downstream unscanned and only reports them via `notify_output`.
    pub fn is_scanning(&self) -> bool {
        self.state == ShellReadyState::Pending && self.scanner.is_some()
    }

    /// Report a post-scan output chunk to the flush gate (the prompt-bytes
    /// signal that swaps the wall-clock fallback for the short settle).
    pub fn notify_output(&mut self) -> Option<GateTimer> {
        self.notify_data()
    }

    /// Feed one decoded PTY output chunk through the barrier. Returns the text
    /// to emit downstream (marker bytes stripped / partial prefix withheld)
    /// and a timer the caller must schedule, if any.
    pub fn process_output(&mut self, data: &str) -> (String, Option<GateTimer>) {
        if self.state == ShellReadyState::Pending {
            if let Some(scanner) = self.scanner.as_mut() {
                let outcome = scanner.scan(data);
                if outcome.matched {
                    let timer = self.transition_to_ready(outcome.post_marker_bytes_observed);
                    return (outcome.output, timer);
                }
                return (outcome.output, None);
            }
        }
        let timer = self.notify_data();
        (data.to_string(), timer)
    }

    fn transition_to_ready(&mut self, post_marker_bytes_observed: bool) -> Option<GateTimer> {
        self.state = ShellReadyState::Ready;
        self.scanner = None;
        if self.queue.is_empty() {
            return None;
        }
        // PostReadyFlushGate.arm(): with post-marker bytes already seen, go
        // straight to the short settle path; otherwise arm the wall-clock
        // fallback and wait for the next data chunk.
        self.awaiting_prompt_draw = true;
        if post_marker_bytes_observed {
            return self.notify_data();
        }
        self.fallback_armed = true;
        self.generation += 1;
        Some(GateTimer::Fallback(self.generation))
    }

    fn notify_data(&mut self) -> Option<GateTimer> {
        if !self.awaiting_prompt_draw {
            return None;
        }
        self.awaiting_prompt_draw = false;
        self.fallback_armed = false;
        self.generation += 1; // invalidates the outstanding fallback timer
        if !self.post_data_armed {
            self.post_data_armed = true;
            return Some(GateTimer::PostData(self.generation));
        }
        None
    }

    /// The `POST_READY_FLUSH_DELAY_MS` timer fired. True → the caller flushes
    /// the queue to the PTY (in the same registry-lock turn it checked this).
    pub fn on_post_data_elapsed(&mut self, generation: u64) -> bool {
        if self.post_data_armed && self.generation == generation {
            self.post_data_armed = false;
            return true;
        }
        false
    }

    /// The `POST_READY_FLUSH_FALLBACK_MS` timer fired. True → flush the queue.
    pub fn on_fallback_elapsed(&mut self, generation: u64) -> bool {
        if self.fallback_armed && self.generation == generation {
            self.fallback_armed = false;
            self.awaiting_prompt_draw = false;
            return true;
        }
        false
    }

    /// The shell-ready timeout fired while still pending → `timed_out`.
    /// Returns the held partial-marker bytes to release downstream; the caller
    /// must then flush the queue. `None` when readiness already resolved.
    pub fn on_ready_timeout_elapsed(&mut self) -> Option<String> {
        if self.state != ShellReadyState::Pending {
            return None;
        }
        self.state = ShellReadyState::TimedOut;
        self.generation += 1;
        Some(self.take_held())
    }

    /// Release held partial-marker bytes for a final (teardown) checkpoint —
    /// session.ts `prepareForFinalSnapshot`. State is left as-is; the scanner
    /// is gone so later output passes through unscanned.
    pub fn release_held_bytes(&mut self) -> String {
        self.take_held()
    }

    fn take_held(&mut self) -> String {
        let held = self
            .scanner
            .as_mut()
            .map(MarkerScanState::drain_held)
            .unwrap_or_default();
        self.scanner = None;
        held
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MARKER: &str = "\x1b]777;orca-shell-ready\x07";

    fn scan_all(state: &mut MarkerScanState, chunks: &[&str]) -> (String, bool, bool) {
        let mut out = String::new();
        let mut matched = false;
        let mut post = false;
        for c in chunks {
            let r = state.scan(c);
            out.push_str(&r.output);
            if r.matched {
                matched = true;
                post = r.post_marker_bytes_observed;
            }
        }
        (out, matched, post)
    }

    #[test]
    fn strips_a_whole_marker_and_reports_post_marker_bytes() {
        let mut s = MarkerScanState::default();
        let input = format!("before{MARKER}after");
        let (out, matched, post) = scan_all(&mut s, &[&input]);
        assert_eq!(out, "beforeafter");
        assert!(matched);
        assert!(post, "bytes after the marker in the same chunk");
    }

    #[test]
    fn marker_split_across_chunks_is_stripped() {
        let mut s = MarkerScanState::default();
        let (out, matched, post) = scan_all(&mut s, &["pre\x1b]777;orca-", "shell-ready", "\x07"]);
        assert_eq!(out, "pre");
        assert!(matched);
        assert!(!post, "marker-only tail chunk has no post-marker bytes");
    }

    #[test]
    fn false_prefix_is_released_back_into_the_output() {
        let mut s = MarkerScanState::default();
        let (out, matched, _) = scan_all(&mut s, &["\x1b]777;orca-NOPE"]);
        assert_eq!(out, "\x1b]777;orca-NOPE");
        assert!(!matched);
    }

    #[test]
    fn full_prefix_without_bel_is_released() {
        let mut s = MarkerScanState::default();
        let input = format!("{SHELL_READY_MARKER_PREFIX}X");
        let (out, matched, _) = scan_all(&mut s, &[&input]);
        assert_eq!(out, input);
        assert!(!matched);
    }

    #[test]
    fn partial_prefix_is_held_and_drainable() {
        let mut s = MarkerScanState::default();
        let r = s.scan("\x1b]777;orca-she");
        assert_eq!(r.output, "");
        assert_eq!(s.drain_held(), "\x1b]777;orca-she");
    }

    #[test]
    fn barrier_queues_until_marker_then_flushes_via_fallback() {
        let mut b = ShellReadyBarrier::new_pending();
        assert!(b.should_queue());
        b.push_queued("cmd\n".to_string());

        // Marker-only chunk → ready, fallback timer armed (no post bytes).
        let (out, timer) = b.process_output(MARKER);
        assert_eq!(out, "");
        assert_eq!(b.state(), ShellReadyState::Ready);
        let Some(GateTimer::Fallback(g)) = timer else {
            panic!("expected fallback timer, got {timer:?}");
        };
        assert!(b.should_queue(), "gate pending: writes must still queue");

        assert!(b.on_fallback_elapsed(g), "valid fallback fires the flush");
        assert!(!b.should_queue());
        assert_eq!(b.drain_queue(), vec!["cmd\n".to_string()]);
    }

    #[test]
    fn prompt_bytes_swap_the_fallback_for_the_short_settle() {
        let mut b = ShellReadyBarrier::new_pending();
        b.push_queued("cmd\n".to_string());
        let (_, timer) = b.process_output(MARKER);
        let Some(GateTimer::Fallback(stale)) = timer else {
            panic!("expected fallback");
        };
        // Prompt bytes arrive → 30ms post-data timer; the fallback is stale.
        let (out, timer) = b.process_output("$ ");
        assert_eq!(out, "$ ");
        let Some(GateTimer::PostData(g)) = timer else {
            panic!("expected post-data timer, got {timer:?}");
        };
        assert!(!b.on_fallback_elapsed(stale), "stale fallback is a no-op");
        assert!(b.should_queue(), "still queue until the settle fires");
        assert!(b.on_post_data_elapsed(g));
        assert!(!b.should_queue());
    }

    #[test]
    fn post_marker_bytes_go_straight_to_the_short_settle() {
        let mut b = ShellReadyBarrier::new_pending();
        b.push_queued("cmd\n".to_string());
        let input = format!("{MARKER}$ ");
        let (out, timer) = b.process_output(&input);
        assert_eq!(out, "$ ");
        assert!(matches!(timer, Some(GateTimer::PostData(_))));
    }

    #[test]
    fn empty_queue_skips_the_gate_entirely() {
        let mut b = ShellReadyBarrier::new_pending();
        let (_, timer) = b.process_output(MARKER);
        assert_eq!(timer, None);
        assert!(!b.should_queue());
    }

    #[test]
    fn timeout_releases_held_bytes_and_unblocks_writes() {
        let mut b = ShellReadyBarrier::new_pending();
        b.push_queued("cmd\n".to_string());
        let (out, _) = b.process_output("\x1b]777;orca-she");
        assert_eq!(out, "", "partial prefix is withheld");
        let held = b.on_ready_timeout_elapsed().expect("was pending");
        assert_eq!(held, "\x1b]777;orca-she");
        assert_eq!(b.state(), ShellReadyState::TimedOut);
        assert!(!b.should_queue());
        assert!(b.on_ready_timeout_elapsed().is_none(), "second fire no-ops");
    }

    #[test]
    fn teardown_release_keeps_pending_but_stops_scanning() {
        let mut b = ShellReadyBarrier::new_pending();
        let _ = b.process_output("\x1b]777;or");
        assert_eq!(b.release_held_bytes(), "\x1b]777;or");
        assert_eq!(b.state(), ShellReadyState::Pending);
        // Scanner gone: later output passes through verbatim.
        let (out, _) = b.process_output("\x1b]777;x");
        assert_eq!(out, "\x1b]777;x");
    }
}
