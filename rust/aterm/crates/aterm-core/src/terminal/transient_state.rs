// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Transient terminal state cleared on reset (#4307).
//!
//! [`TransientState`] bundles scalar fields and small buffers that are always
//! cleared together during `reset_common_fields`. Grouping these reduces the
//! reset function's parameter count and ensures new resettable fields only
//! need to be added in one place.

use super::response_rate_limiter::ResponseRateLimiter;
use super::types::SgrStackEntry;
use aterm_types::{PipelineTimestamps, Rgb};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

// XTSAVE mode storage.
type XtsaveModesMap = HashMap<u16, bool>;

/// Default foreground/background — the VT/engine spec defaults (light grey on
/// black). These re-export the SINGLE source of truth in `aterm-types` so the
/// runtime terminal state and `TerminalConfig::default()` can never diverge (they
/// did historically: 229 here vs 255 in the config — a latent footgun the audit
/// flagged). See [`aterm_types::DEFAULT_FOREGROUND`].
pub(super) const DEFAULT_FOREGROUND: Rgb = aterm_types::DEFAULT_FOREGROUND;

/// See [`DEFAULT_FOREGROUND`]; the spec default background (black), single-sourced.
pub(super) const DEFAULT_BACKGROUND: Rgb = aterm_types::DEFAULT_BACKGROUND;

/// VT52 cursor addressing state.
///
/// VT52's direct cursor addressing (ESC Y row col) requires collecting
/// two parameter bytes after the ESC Y.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum Vt52CursorState {
    /// Not collecting cursor position.
    #[default]
    None,
    /// Waiting for row byte (first parameter after ESC Y).
    WaitingRow,
    /// Waiting for column byte (second parameter after ESC Y).
    WaitingCol(u8),
}

/// Grouped transient terminal state cleared on reset (#4307).
///
/// Bundles scalar fields and small buffers that are always cleared together
/// during `reset_common_fields`. Grouping these reduces the reset function's
/// parameter count and ensures new resettable fields only need to be added
/// in one place (this struct + its `reset()` method).
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent terminal flags, not a state machine"
)]
pub(super) struct TransientState {
    /// Response buffer for DSR/DA and other terminal responses.
    pub(super) response_buffer: Vec<u8>,
    /// Token-bucket rate limiter gating `send_response` (Part of #7874).
    ///
    /// Prevents response-amplification DoS: a malicious peer spamming
    /// DSR/DA/DECRQSS cannot force unlimited response generation even
    /// when the host drains the buffer in a tight loop.
    pub(super) response_rate_limiter: ResponseRateLimiter,
    /// Last graphic character received (for REP - CSI b). Stored RAW
    /// (pre-charset-translation): xterm CASE_REP re-translates it through
    /// the GL charset that is current at repeat time.
    pub(super) last_graphic_char: Option<char>,
    /// Current hyperlink (OSC 8).
    pub(super) current_hyperlink: Option<Arc<str>>,
    /// Current hyperlink ID (OSC 8 `id=` parameter).
    pub(super) current_hyperlink_id: Option<Arc<str>>,
    /// Current underline color (SGR 58).
    pub(super) current_underline_color: Option<u32>,
    /// VT52 cursor addressing state.
    pub(super) vt52_cursor_state: Vt52CursorState,
    /// Timestamp when synchronized output mode (2026) was enabled.
    pub(super) sync_start: Option<std::time::Instant>,
    /// Logical "now" for the current `process_at()` batch — the single
    /// timestamp every state-affecting time read in the pipeline observes.
    ///
    /// Captured once at the top of [`Terminal::process_at`] (the public
    /// [`Terminal::process`] passes `Instant::now()`) and read by the bell
    /// rate-limiter, `sync_start` arming, and the mode-2026 timeout check.
    /// Routing those through one injected instant — instead of each calling
    /// `Instant::now()` independently — makes `process_at` replayable: feeding
    /// the same `(bytes, instant)` schedule reproduces identical grid/cursor/
    /// mode state regardless of real wall-clock pacing. The value is always
    /// overwritten before any reader runs, so its initial/reset value is never
    /// observed.
    pub(super) process_now: std::time::Instant,
    /// Wall-clock epoch milliseconds for the current `process_at()` batch — the
    /// single wall reading every shell-integration command/output mark records
    /// (OSC 133/633 marks B/C/D). Captured alongside [`process_now`] so replay
    /// reproduces identical `command_*_time_ms` values from the recorded
    /// schedule instead of re-reading the host clock. `None` when the platform
    /// clock is unavailable. Always overwritten before any reader runs.
    pub(super) process_wall_ms: Option<u64>,
    /// SGR attribute stack for XTPUSHSGR/XTPOPSGR.
    pub(super) sgr_stack: VecDeque<SgrStackEntry>,
    /// Per-frame pipeline timing for keystroke-to-pixel decomposition (#5560).
    pub(super) pipeline_timestamps: PipelineTimestamps,
    /// Whether the last combining character added was a ZWJ (U+200D).
    ///
    /// Used to fast-path `should_combine_with_previous_zwj` — the full grid
    /// lookup is only needed when this is true, which is <0.1% of characters.
    pub(super) last_combining_was_zwj: bool,
    /// Cached flag: true when `current_hyperlink.is_some() || current_underline_color.is_some()`.
    /// Avoids 2 per-character Option checks in `write_char_core`.
    pub(super) has_transient_extras: bool,
    /// Set by the RIS handler to signal that the parser should be reset after
    /// the current `advance_fast` call completes (#7153). The parser cannot be
    /// reset from inside its own dispatch loop.
    pub(super) pending_parser_reset: bool,
    /// XTSAVE (CSI ? Ps s) saved DEC private mode values.
    ///
    /// Maps mode number to its saved boolean state. Restored by XTRESTORE
    /// (CSI ? Ps r). Cleared on terminal reset. Part of #7318.
    pub(super) xtsave_modes: XtsaveModesMap,
    /// Whether the most recent OSC was terminated by BEL (0x07) rather than ST.
    ///
    /// Used by OSC 52 clipboard query responses to echo the same terminator
    /// for compatibility with programs that only recognize BEL-terminated
    /// responses (#7548).
    pub(super) last_osc_bel_terminated: bool,
}

impl TransientState {
    pub(super) fn new() -> Self {
        Self {
            response_buffer: Vec::new(),
            response_rate_limiter: ResponseRateLimiter::new(),
            last_graphic_char: None,
            current_hyperlink: None,
            current_hyperlink_id: None,
            current_underline_color: None,
            vt52_cursor_state: Vt52CursorState::None,
            sync_start: None,
            // Placeholders; overwritten at the top of every process_at() before
            // any reader runs. CLOCK-EXEMPT: seed only, never observed as state.
            process_now: std::time::Instant::now(),
            process_wall_ms: None,
            sgr_stack: VecDeque::new(),
            pipeline_timestamps: PipelineTimestamps::default(),
            last_combining_was_zwj: false,
            has_transient_extras: false,
            pending_parser_reset: false,
            xtsave_modes: XtsaveModesMap::default(),
            last_osc_bel_terminated: false,
        }
    }

    /// Recompute the cached `has_transient_extras` flag.
    #[inline]
    pub(super) fn update_has_transient_extras(&mut self) {
        self.has_transient_extras =
            self.current_hyperlink.is_some() || self.current_underline_color.is_some();
    }

    /// Clear all transient state (called during terminal reset).
    pub(super) fn reset(&mut self) {
        self.response_buffer.clear();
        self.last_graphic_char = None;
        self.current_hyperlink = None;
        self.current_hyperlink_id = None;
        self.current_underline_color = None;
        self.vt52_cursor_state = Vt52CursorState::default();
        self.sync_start = None;
        self.sgr_stack.clear();
        self.pipeline_timestamps = PipelineTimestamps::default();
        self.last_combining_was_zwj = false;
        self.has_transient_extras = false;
        self.pending_parser_reset = false;
        self.xtsave_modes.clear();
        self.last_osc_bel_terminated = false;
    }
}
