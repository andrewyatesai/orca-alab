// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Andrew Yates

//! **aterm-agent** — layer L2 of the RFC "The Reactive Surface": the *agent
//! interface*. Two responsibilities, both of which MUST live outside the engine
//! core (RFC R2):
//!
//! 1. **Turn-completion** ([`Turn`]). "An agent finished its turn" is the most
//!    semantic thing in the stack — it is `IdleFor(d) ∧ RowMatches(prompt-ready)`
//!    composed over the L0/L0.5 predicates, plus response-region extraction and
//!    the Claude-specific prompt-ready patterns. None of this belongs in the
//!    terminal; it lives here, two crates above `aterm-core`.
//!
//! 2. **The self-reflection feedback governor** ([`SelfGovernor`]). When the
//!    observer and the observed are the *same* session (R4 self-reflection),
//!    `await-idle` alone does **not** damp the loop — a self-write that produces
//!    output keeps `content_seq` advancing. The governor is the safety bound:
//!    self-writes are **off by default**, rate-limited by a token bucket, and a
//!    circuit-breaker trips on sustained self-induced churn. Its `FailClosed`
//!    invariant is model-checked by `self_governor_model` (`aterm-spec`) and
//!    bound to this code by [`tests`].
//!
//! > **Layering note (the critic's gap).** This L2 governor is *policy*. The
//! > *un-bypassable floor* — a hard per-session rate-limit on self-targeted input
//! > injection — lives at the control dispatch path (`aterm-gui::inject_floor`,
//! > applied in `control.rs`/`run_feed_bin`), because a raw control client can
//! > drive `@.` in a loop without ever linking this crate. (Cross-session
//! > self-amplification is separately bounded by the proxy's per-op edge tokens,
//! > whose `DeriveLoop` op is un-grantable by default — `ProxyEntry::token_for`
//! > returns `None` for it.) This crate is the rich policy on top of that floor,
//! > not a substitute for it.

use std::time::Duration;

use aterm_observe::row_matcher;

/// The self-reflection feedback governor (R4). A bounded state machine whose
/// `FailClosed` property — *a self-write is permitted only with a spare token, a
/// non-tripped breaker, and self-writes explicitly enabled* — is model-checked.
#[derive(Clone, Debug)]
pub struct SelfGovernor {
    /// Self-driving is OFF unless the operator explicitly enables it.
    self_write_enabled: bool,
    /// Token bucket: available write permits.
    tokens: u32,
    /// Bucket capacity (also the refill ceiling).
    capacity: u32,
    /// Permits restored per [`tick`](Self::tick).
    refill: u32,
    /// Accumulated self-induced output since the last decay.
    churn: u32,
    /// Trip threshold: churn above this trips the breaker.
    churn_trip: u32,
    /// Once tripped, all self-writes are refused until [`reset`](Self::reset).
    tripped: bool,
}

impl SelfGovernor {
    /// A governor with self-writes **disabled** (the default posture). Capacity
    /// `capacity` permits, refilling `refill` per tick, tripping the breaker once
    /// self-induced churn exceeds `churn_trip`.
    #[must_use]
    pub fn disabled(capacity: u32, refill: u32, churn_trip: u32) -> Self {
        Self {
            self_write_enabled: false,
            tokens: capacity,
            capacity,
            refill,
            churn: 0,
            churn_trip,
            tripped: false,
        }
    }

    /// Explicitly opt into self-driving (the operator's deliberate choice). Even
    /// then, every write still passes the token bucket and the breaker.
    pub fn enable_self_write(&mut self) {
        self.self_write_enabled = true;
    }

    /// May a self-write proceed *right now*? Consumes one token on success. This
    /// is the FailClosed gate: `false` unless self-writes are enabled **and** the
    /// breaker is not tripped **and** a token is available.
    #[must_use]
    pub fn allow_self_write(&mut self) -> bool {
        if !self.self_write_enabled || self.tripped || self.tokens == 0 {
            return false;
        }
        self.tokens -= 1;
        true
    }

    /// Record `amount` of self-induced output. Sustained churn trips the breaker
    /// (latching) — the storm backstop that `await-idle` alone cannot provide.
    pub fn note_self_output(&mut self, amount: u32) {
        self.churn = self.churn.saturating_add(amount);
        if self.churn > self.churn_trip {
            self.tripped = true;
        }
    }

    /// One governor tick: refill the bucket (capped) and decay the churn window.
    pub fn tick(&mut self) {
        self.tokens = (self.tokens + self.refill).min(self.capacity);
        self.churn = self.churn.saturating_sub(self.refill);
    }

    /// Whether the breaker has tripped (manual [`reset`](Self::reset) to recover).
    #[must_use]
    pub fn tripped(&self) -> bool {
        self.tripped
    }

    /// Operator recovery after a trip: clear the breaker and refill.
    pub fn reset(&mut self) {
        self.tripped = false;
        self.churn = 0;
        self.tokens = self.capacity;
    }
}

/// The Claude-prompt-ready signal: the bottom rows show the input box (`❯`) with
/// no in-flight spinner. These patterns are Claude-specific and live ONLY here.
#[must_use]
pub fn claude_prompt_ready_pattern() -> &'static str {
    // The input caret at the start of a row; tolerant of the box border glyphs.
    r"(^|\s)❯(\s|$)"
}

/// A driven turn: type a prompt, submit it, then block until the agent's turn
/// completes — the surface goes `idle for `[`idle`], then a best-effort
/// prompt-ready confirm — and read the settled surface. The [`ControlClient`]
/// abstracts the transport (a Unix socket today, an astream network dial under
/// L3); this composition is the same regardless, and is exactly what the
/// `aterm-drive` CLI runs (this run-loop over [`CtlClient`] + the core `await`).
pub struct Turn {
    /// Quiescence window that counts as "the agent stopped streaming".
    pub idle: Duration,
    /// Overall deadline before giving up.
    pub timeout: Duration,
    /// The prompt-ready regex (defaults to [`claude_prompt_ready_pattern`]).
    pub ready_pattern: String,
}

impl Default for Turn {
    fn default() -> Self {
        Self {
            idle: Duration::from_millis(600),
            timeout: Duration::from_secs(180),
            ready_pattern: claude_prompt_ready_pattern().to_string(),
        }
    }
}

/// The transport seam the agent layer drives. Implemented over `aterm-ctl`'s
/// verbs locally and (L3) over an astream network dial remotely — the [`Turn`]
/// composition is identical either way.
pub trait ControlClient {
    /// The transport's error type.
    type Error;
    /// Type bytes into the target's input (the `send` verb).
    fn send(&mut self, bytes: &[u8]) -> Result<(), Self::Error>;
    /// Submit with a real Enter keypress (the `key enter` verb — never a raw LF).
    fn key_enter(&mut self) -> Result<(), Self::Error>;
    /// Settle the surface (`await idle`), then best-effort confirm a prompt-ready
    /// row (`await match <ready>`), then return the settled surface text. Idle is
    /// the authoritative turn-complete signal; the ready match only sharpens it.
    /// `ready_pattern` empty = idle only.
    fn await_idle_and_ready(
        &mut self,
        idle: Duration,
        ready_pattern: &str,
        timeout: Duration,
    ) -> Result<String, Self::Error>;
}

/// Why a turn could not be driven. The `Display` messages are written for an AI
/// agent reading them in a tool result — each says what happened AND what to try
/// next, so the model can self-correct without external docs.
#[derive(Debug)]
pub enum TurnError<E> {
    /// The self-reflection governor refused the write (off / rate-limited /
    /// breaker tripped).
    Governed,
    /// The supplied `ready_pattern` did not compile as a regex.
    BadPattern(regex_error::Error),
    /// The transport failed.
    Transport(E),
}

impl<E: std::fmt::Display> std::fmt::Display for TurnError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TurnError::Governed => write!(
                f,
                "self-reflection governor refused the write. This session is \
                 driving ITSELF and the feedback floor tripped or self-writes are \
                 off. Fix: enable self-writes deliberately (SelfGovernor::\
                 enable_self_write) and pace the loop — act only on a settled turn, \
                 never on every output burst."
            ),
            TurnError::BadPattern(e) => write!(
                f,
                "the prompt-ready pattern is not a valid regex ({e}). Fix: pass a \
                 simple anchored pattern, e.g. '❯' for a Claude input box or \
                 '\\$ $' for a shell prompt."
            ),
            TurnError::Transport(e) => write!(
                f,
                "the control transport failed ({e}). Fix: check the target aterm is \
                 running and ATERM_CONTROL_SOCK points at its socket (the path it \
                 printed as 'control socket listening at ...')."
            ),
        }
    }
}

/// Re-export so callers can match on a compile failure without depending on
/// `regex` directly (it is validated through `aterm-observe`).
pub mod regex_error {
    pub use ::aterm_observe::regex_compile_error::Error;
}

impl Turn {
    /// Drive one turn through `client`, gated by `gov` (the self-reflection
    /// governor — pass a permissive one for cross-session driving). Returns the
    /// settled surface text on completion.
    ///
    /// # Errors
    /// - [`TurnError::Governed`] if the governor refuses the write.
    /// - [`TurnError::BadPattern`] if the ready pattern is invalid.
    /// - [`TurnError::Transport`] on a transport failure.
    pub fn run<C: ControlClient>(
        &self,
        client: &mut C,
        gov: &mut SelfGovernor,
        prompt: &[u8],
    ) -> Result<String, TurnError<C::Error>> {
        // Validate the predicate before touching the transport.
        row_matcher(&self.ready_pattern).map_err(TurnError::BadPattern)?;
        // The self-reflection floor: refuse if the governor says so.
        if !gov.allow_self_write() {
            return Err(TurnError::Governed);
        }
        client.send(prompt).map_err(TurnError::Transport)?;
        client.key_enter().map_err(TurnError::Transport)?;
        let screen = client
            .await_idle_and_ready(self.idle, &self.ready_pattern, self.timeout)
            .map_err(TurnError::Transport)?;
        // Account the response toward the churn breaker (self-reflection safety).
        gov.note_self_output(u32::try_from(screen.len()).unwrap_or(u32::MAX));
        Ok(screen)
    }
}

/// A concrete [`ControlClient`] that drives a target aterm by shelling out to the
/// std-only `aterm-ctl` core client — the agent layer reuses the exact verbs a
/// human would type, with zero protocol re-implementation. Composition (idle,
/// then a bounded prompt-ready confirm, then read) lives HERE in the sugar, so
/// the core `await` verb stays single-predicate.
pub struct CtlClient {
    ctl: std::path::PathBuf,
    socket: Option<String>,
}

impl CtlClient {
    /// Build a client. `ctl` is the path to `aterm-ctl`; `socket` is an explicit
    /// `--sock` path, or `None` to use `$ATERM_CONTROL_SOCK` / the default.
    pub fn new(ctl: impl Into<std::path::PathBuf>, socket: Option<String>) -> Self {
        Self {
            ctl: ctl.into(),
            socket,
        }
    }

    /// Run `aterm-ctl [--sock S] <args...>`, returning stdout or a trimmed stderr.
    pub fn run(&self, args: &[&str]) -> Result<String, String> {
        let mut cmd = std::process::Command::new(&self.ctl);
        if let Some(s) = &self.socket {
            cmd.arg("--sock").arg(s);
        }
        cmd.args(args);
        let out = cmd
            .output()
            .map_err(|e| format!("could not run {}: {e}", self.ctl.display()))?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).into_owned())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
        }
    }
}

impl ControlClient for CtlClient {
    type Error = String;
    fn send(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
        let s = String::from_utf8_lossy(bytes);
        self.run(&["send", s.as_ref()]).map(|_| ())
    }
    fn key_enter(&mut self) -> Result<(), Self::Error> {
        self.run(&["key", "enter"]).map(|_| ())
    }
    fn await_idle_and_ready(
        &mut self,
        idle: Duration,
        ready_pattern: &str,
        timeout: Duration,
    ) -> Result<String, Self::Error> {
        let idle_ms = idle.as_millis().to_string();
        let to_ms = timeout.as_millis().to_string();
        // (1) Wait for the surface to settle — the core single-predicate
        //     `await idle` verb (turn-complete for a streaming TUI like Claude,
        //     whose spinner keeps the screen changing until the turn ends).
        self.run(&["await", "idle", &idle_ms, "timeout", &to_ms])?;
        // (2) Best-effort, advisory confirm that a prompt-ready row is present
        //     (`await match`). The surface is ALREADY idle, so a matching row — if
        //     present — returns at ONCE (free for a ready Claude prompt); a SHORT
        //     250 ms bound means a non-matching pattern (e.g. the Claude `❯`
        //     default against a `$` shell) costs at most 250 ms, never the full
        //     timeout. Non-fatal: idle is the authoritative turn-complete signal;
        //     this only sharpens it. Skipped when no pattern is set.
        if !ready_pattern.is_empty() {
            let _ = self.run(&["await", "match", ready_pattern, "timeout", "250"]);
        }
        // (3) Read the settled surface.
        self.run(&["text"])
    }
}

/// The AI-oriented help for the `aterm-drive` tool — written so a model reading
/// `--help` in a tool result builds correct intuition for the CORE primitives
/// (the `await`/`send`/`key`/`text` verbs) and the drive loop, without external
/// docs. The core protocol stays terse; this is where the teaching lives.
pub const DRIVE_HELP: &str = "\
aterm-drive — drive an interactive agent (e.g. Claude Code) running inside aterm.

MENTAL MODEL
    A HOST aterm runs your target program as its child and exposes a Unix control
    socket. This tool reads the live screen and drives keystrokes over that socket
    via `aterm-ctl` — the same engine a human types into. The key primitive is
    `await`: block until the surface reaches a condition, so you never sleep-and-
    hope or scrape for a spinner.

USAGE
    aterm-drive [--socket PATH] [--idle MS] [--timeout MS] <command> [text...]

COMMANDS
    prompt <text...>   Type <text>, press Enter, then BLOCK until the agent's turn
                       settles (no screen change for --idle ms), and print the
                       settled screen. This is the one you want for a drive loop.
    read               Print the live screen (one row per line).
    await <cond>       Block until a condition, then print the kernel's verdict:
                         idle <ms>        surface unchanged for <ms> (turn done)
                         match <regex>    a visible row matches <regex>
                         seq              the next content change lands
                         block            a shell command completes (OSC-133)
    shot [path]        Save a pixel-true PNG of the terminal content view (the
                       rendered cells; OS chrome/titlebar are NOT captured).
    help               Show this text.

OPTIONS
    --socket PATH   The target aterm's control socket. Defaults to
                    $ATERM_CONTROL_SOCK, else the newest local instance.
    --idle MS       Quiescence window that counts as 'turn complete' (default 600).
                    Bigger = more certain the turn ended; smaller = snappier.
    --timeout MS    Give up after this long (default 180000).

WHICH `await` TO USE
    * Driving Claude / a TUI with an animated spinner → `prompt` (idle works: the
      spinner keeps the screen changing until the turn ends).
    * A command that pauses SILENTLY mid-run (e.g. `sleep`) → don't trust idle
      alone; use `await match <regex>` on a known output marker instead.
    * A plain shell command → `await block` (waits for the command to finish).

EXAMPLES
    # one driven turn against Claude Code:
    aterm-drive prompt 'Refactor utils.rs to drop the unwrap() calls.'
    # wait for a specific marker rather than idle:
    aterm-drive await match 'BUILD SUCCESSFUL'
    # capture the terminal content as rendered pixels:
    aterm-drive shot /tmp/screen.png

GOTCHA
    Submit with a real Enter keypress (this tool uses `key enter`), never a raw
    newline byte — a TUI line editor reads Enter as a keypress (CR), not LF.";

#[cfg(test)]
mod tests {
    use super::*;

    /// A mock transport recording the driven verbs and returning a canned screen.
    struct MockClient {
        sent: Vec<u8>,
        entered: u32,
        screen: String,
    }
    impl ControlClient for MockClient {
        type Error = std::convert::Infallible;
        fn send(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
            self.sent.extend_from_slice(bytes);
            Ok(())
        }
        fn key_enter(&mut self) -> Result<(), Self::Error> {
            self.entered += 1;
            Ok(())
        }
        fn await_idle_and_ready(
            &mut self,
            _idle: Duration,
            _ready: &str,
            _timeout: Duration,
        ) -> Result<String, Self::Error> {
            Ok(self.screen.clone())
        }
    }

    #[test]
    fn turn_sends_prompt_presses_enter_and_returns_settled_screen() {
        let mut client = MockClient {
            sent: Vec::new(),
            entered: 0,
            screen: "⏺ ANSWER: 391\n❯ ".to_string(),
        };
        // A permissive governor (cross-session driving): enabled, ample tokens.
        let mut gov = SelfGovernor::disabled(8, 1, 1_000_000);
        gov.enable_self_write();
        let turn = Turn::default();
        let out = turn.run(&mut client, &mut gov, b"what is 17*23?").unwrap();
        assert_eq!(client.sent, b"what is 17*23?");
        assert_eq!(
            client.entered, 1,
            "submitted with exactly one Enter keypress"
        );
        assert!(out.contains("ANSWER: 391"));
    }

    #[test]
    fn governor_is_off_by_default_fail_closed() {
        // The default posture refuses self-writes entirely (R4 safety).
        let mut gov = SelfGovernor::disabled(8, 1, 1000);
        assert!(!gov.allow_self_write(), "self-write off by default");
        gov.enable_self_write();
        assert!(gov.allow_self_write(), "enabled + has tokens -> allowed");
    }

    #[test]
    fn governor_rate_limits_and_breaker_latches_fail_closed() {
        let mut gov = SelfGovernor::disabled(2, 1, 10);
        gov.enable_self_write();
        assert!(gov.allow_self_write()); // token 2 -> 1
        assert!(gov.allow_self_write()); // token 1 -> 0
        assert!(!gov.allow_self_write(), "bucket empty -> refused");
        gov.tick(); // refill 1
        assert!(gov.allow_self_write());
        // Sustained self-output trips the breaker; thereafter ALL writes refused.
        gov.note_self_output(100);
        assert!(gov.tripped());
        gov.tick(); // even with tokens, a tripped breaker refuses
        assert!(
            !gov.allow_self_write(),
            "tripped breaker is fail-closed regardless of tokens"
        );
        gov.reset();
        assert!(gov.allow_self_write(), "operator reset recovers");
    }

    #[test]
    fn turn_is_governed_when_self_write_disabled() {
        let mut client = MockClient {
            sent: Vec::new(),
            entered: 0,
            screen: String::new(),
        };
        let mut gov = SelfGovernor::disabled(8, 1, 1000); // NOT enabled
        let turn = Turn::default();
        assert!(matches!(
            turn.run(&mut client, &mut gov, b"x"),
            Err(TurnError::Governed)
        ));
        assert!(client.sent.is_empty(), "no bytes sent when governed");
    }

    #[test]
    fn run_rejects_a_bad_ready_pattern_before_touching_the_transport() {
        let mut client = MockClient {
            sent: Vec::new(),
            entered: 0,
            screen: String::new(),
        };
        let mut gov = SelfGovernor::disabled(8, 1, 1000);
        gov.enable_self_write();
        let turn = Turn {
            ready_pattern: "(unclosed".to_string(),
            ..Turn::default()
        };
        assert!(matches!(
            turn.run(&mut client, &mut gov, b"x"),
            Err(TurnError::BadPattern(_))
        ));
        assert!(client.sent.is_empty(), "no bytes sent on a bad pattern");
    }
}
