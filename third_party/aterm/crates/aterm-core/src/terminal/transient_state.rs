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

/// Default foreground color (light gray — matches xterm default).
pub(super) const DEFAULT_FOREGROUND: Rgb = Rgb {
    r: 229,
    g: 229,
    b: 229,
};

/// Default background color (black — matches xterm default).
pub(super) const DEFAULT_BACKGROUND: Rgb = Rgb { r: 0, g: 0, b: 0 };

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
