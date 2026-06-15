// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Parser action dispatch for `TerminalHandler`.
//!
//! This module implements the **parser actions layer** of the terminal handler
//! concern separation (#2157). It receives parsed escape sequences from the
//! VT parser via the `ActionSink` trait and dispatches them to typed handler
//! methods. This layer depends only on `parser` (for the trait) and
//! `charset` (for character translation).
//!
//! ## Concern layers
//!
//! - **Parser actions** (this file): `ActionSink` dispatch from parser events
//! - **State transitions** (`handler_state.rs`): grid/mode mutations from typed operations
//! - **Side-effects**: callbacks and external service activation (inline in handler files)

use crate::grid::{CellFlags, Color, PackedColor, Style, StyleId};
use crate::parser::ActionSink;
use aterm_provenance::{Provenance, Pty};
use aterm_types::charset::{GlMapping, SingleShift};

use super::{TerminalHandler, Vt52CursorState};

impl ActionSink for TerminalHandler<'_> {
    fn print(&mut self, c: char) {
        // Handle VT52 cursor addressing state
        match self.transient.vt52_cursor_state {
            Vt52CursorState::WaitingRow => {
                // First byte after ESC Y - row (encoded as row + 32)
                let row = (c as u8).saturating_sub(32);
                self.transient.vt52_cursor_state = Vt52CursorState::WaitingCol(row);
                return;
            }
            Vt52CursorState::WaitingCol(row) => {
                // Second byte after ESC Y - column (encoded as col + 32)
                let col = (c as u8).saturating_sub(32);
                self.grid.set_cursor(u16::from(row), u16::from(col));
                self.transient.vt52_cursor_state = Vt52CursorState::None;
                return;
            }
            Vt52CursorState::None => {}
        }

        self.write_char(c);
    }

    /// FAST PATH: Print a run of ASCII bytes without per-character overhead.
    ///
    /// This is called by the parser for runs of printable ASCII (0x20-0x7E).
    /// Uses three tiers of optimization:
    ///
    /// 1. Ultra-fast: Default style, autowrap, no insert mode → `write_ascii_blast`
    /// 2. Fast: Styled but no RGB/hyperlinks/insert, autowrap → `write_ascii_run_styled`
    /// 3. Fallback: Per-character `write_char` for complex cases
    fn print_ascii_bulk(&mut self, data: &Provenance<[u8], Pty>) {
        let data = data.as_ref();
        // Blockers that require per-character processing
        if self.transient.vt52_cursor_state != Vt52CursorState::None {
            // VT52 cursor addressing consumes characters specially
            for &byte in data {
                self.print(byte as char);
            }
            return;
        }

        // Per-character fallback: only for conditions that truly require it.
        // Charset translation, insert mode, and no-autowrap need per-char processing
        // because they change behavior at each character position.
        if !self.charset.is_ascii_passthrough() || self.modes.insert_mode || !self.modes.auto_wrap {
            for &byte in data {
                self.write_char(byte as char);
            }
            return;
        }

        // Capture text for CopyToClipboard mode (OSC 1337) before fast-path write.
        // The fast paths bypass write_char, so we must capture here.
        if let Some(state) = self.clipboard.copy_state.as_mut() {
            for &byte in data {
                state.push(byte as char);
            }
        }

        // Check if style needs CellExtras overflow (RGB, hyperlinks, etc.).
        // Both flags cached at mutation time — no per-bulk-call overhead.
        if self.style.has_style_extras() || self.transient.has_transient_extras {
            self.write_ascii_bulk_with_extras(data);
        } else {
            self.write_ascii_bulk_fast(data);
        }
    }

    /// FAST PATH: Print a run of decoded non-ASCII characters.
    ///
    /// Called by the parser for consecutive multi-byte UTF-8 sequences.
    /// Amortizes per-character overhead (charset, clipboard, style checks)
    /// over the entire run. Falls back to per-character for complex cases.
    fn print_unicode_bulk(&mut self, chars: &Provenance<[char], Pty>) {
        let chars = chars.as_ref();
        // VT52 cursor addressing consumes characters specially
        if self.transient.vt52_cursor_state != Vt52CursorState::None {
            for &c in chars {
                self.print(c);
            }
            return;
        }

        self.write_unicode_bulk(chars);
    }

    /// Execute C0 and C1 control characters.
    ///
    /// Handles single-byte control codes that don't require parameters:
    ///
    /// **C0 codes (0x00-0x1F):**
    /// - **0x07** (BEL): Ring bell (triggers callback)
    /// - **0x08** (BS): Backspace with reverse wraparound support
    /// - **0x09** (HT): Horizontal tab
    /// - **0x0A-0x0C** (LF/VT/FF): Line feed (with optional CR in LNM mode)
    /// - **0x0D** (CR): Carriage return
    /// - **0x0E** (SO): Shift Out - select G1 character set
    /// - **0x0F** (SI): Shift In - select G0 character set
    /// - **0x18/0x1A** (CAN/SUB): Cancel/abort current sequence
    ///
    /// **C1 codes (0x80-0x9F):**
    /// - **0x84** (IND): Index - same as ESC D
    /// - **0x85** (NEL): Next line - same as ESC E
    /// - **0x88** (HTS): Tab set - same as ESC H
    /// - **0x8D** (RI): Reverse index - same as ESC M
    /// - **0x8E/0x8F** (SS2/SS3): Single shift - same as ESC N/O
    ///
    /// See `docs/ESCAPE_SEQUENCE_MATRIX.md` for complete control code coverage.
    fn execute(&mut self, byte: u8) {
        // Per VT220 spec: a control character arriving mid-sequence cancels
        // any in-progress ESC Y cursor addressing (VT52 mode).
        if self.transient.vt52_cursor_state != Vt52CursorState::None {
            self.transient.vt52_cursor_state = Vt52CursorState::None;
        }

        // Per VT220 spec: SS2/SS3 single-shift is cleared on any control
        // character, not just on the next graphic character.
        self.charset.clear_single_shift();

        match byte {
            // C0 control codes (0x00-0x1F)
            0x07 => self.handle_bell(),
            0x08 => {
                // BS (Backspace)
                // Per VT510: when DECLRMM is active, the "left margin" for BS
                // is the DECLRMM left margin, and reverse wraparound wraps to
                // the right margin (not last column).
                let left_bound = if self.modes.left_right_margin_mode {
                    self.grid.horizontal_margins().left
                } else {
                    0
                };
                if self.grid.cursor_col() <= left_bound && self.modes.reverse_wraparound {
                    let row = self.grid.cursor_row();
                    let top = self.grid.scroll_region().top;
                    let min_row = if row >= top { top } else { 0 };
                    if row > min_row {
                        let wrap_col = if self.modes.left_right_margin_mode {
                            self.grid.horizontal_margins().right
                        } else {
                            self.grid.cols().saturating_sub(1)
                        };
                        self.grid.set_cursor(row - 1, wrap_col);
                    }
                } else if self.modes.grapheme_cluster_mode {
                    // Mode 2027: respect grapheme cluster boundaries
                    self.cursor_state().cursor_backward_graphemes(1);
                } else {
                    self.grid
                        .cursor_backward_margin(1, self.modes.left_right_margin_mode);
                }
            }
            0x09 => {
                // HT (Horizontal Tab)
                // Capture tab for CopyToClipboard (OSC 1337)
                if let Some(state) = self.clipboard.copy_state.as_mut() {
                    state.push('\t');
                }
                self.grid.tab_margin(self.modes.left_right_margin_mode);
            }
            0x0A..=0x0C => {
                // LF, VT, FF
                // Capture newline for CopyToClipboard (OSC 1337)
                if let Some(state) = self.clipboard.copy_state.as_mut() {
                    state.push('\n');
                }
                // In new line mode (LNM), LF also performs CR
                if self.modes.new_line_mode {
                    self.grid
                        .carriage_return_margin(self.modes.left_right_margin_mode);
                }
                // Per VT510: when DECLRMM is active, LF at the scroll boundary
                // scrolls only within horizontal margins (#7407).
                // Adjusts kitty graphics placements on scroll (#7687).
                self.line_feed_with_kitty_adjust(self.modes.left_right_margin_mode);
            }
            0x0D => {
                // CR (Carriage Return)
                // Capture CR for CopyToClipboard (OSC 1337)
                if let Some(state) = self.clipboard.copy_state.as_mut() {
                    state.push('\r');
                }
                self.grid
                    .carriage_return_margin(self.modes.left_right_margin_mode);
            }
            0x0E => {
                // SO (Shift Out) - invoke G1 into GL
                self.charset.gl = GlMapping::G1;
            }
            0x0F => {
                // SI (Shift In) - invoke G0 into GL
                self.charset.gl = GlMapping::G0;
            }

            // C1 control codes (0x80-0x9F)
            // These are 8-bit equivalents of ESC + character sequences
            0x84 => {
                // IND (Index) - same as ESC D
                // Move cursor down, scroll if at bottom of scroll region
                // Capture newline for CopyToClipboard (matches ESC D path)
                if let Some(state) = self.clipboard.copy_state.as_mut() {
                    state.push('\n');
                }
                // Per VT510: when DECLRMM is active, IND at the scroll boundary
                // scrolls only within horizontal margins (#7407).
                // Adjusts kitty graphics placements on scroll (#7687).
                self.line_feed_with_kitty_adjust(self.modes.left_right_margin_mode);
            }
            0x85 => {
                // NEL (Next Line) - same as ESC E
                // Move cursor to start of next line, scroll if needed
                // Capture newline for CopyToClipboard (matches ESC E path)
                if let Some(state) = self.clipboard.copy_state.as_mut() {
                    state.push('\n');
                }
                self.grid
                    .carriage_return_margin(self.modes.left_right_margin_mode);
                // Per VT510: when DECLRMM is active, NEL at the scroll boundary
                // scrolls only within horizontal margins (#7407).
                // Adjusts kitty graphics placements on scroll (#7687).
                self.line_feed_with_kitty_adjust(self.modes.left_right_margin_mode);
            }
            0x88 => {
                // HTS (Horizontal Tab Set) - same as ESC H
                // Set a tab stop at current column
                self.grid.set_tab_stop();
            }
            0x8D => {
                // RI (Reverse Index) - same as ESC M
                // Move cursor up, scroll down if at top of scroll region
                // Per VT510: when DECLRMM is active, RI at the scroll boundary
                // scrolls only within horizontal margins (#7407).
                self.grid
                    .reverse_line_feed_margined(self.modes.left_right_margin_mode);
            }
            0x8E => {
                // SS2 (Single Shift 2) - same as ESC N
                // Use G2 for next character only
                self.charset.single_shift = SingleShift::Ss2;
            }
            0x8F => {
                // SS3 (Single Shift 3) - same as ESC O
                // Use G3 for next character only
                self.charset.single_shift = SingleShift::Ss3;
            }

            _ => {}
        }
    }

    /// Dispatch CSI (Control Sequence Introducer) escape sequences.
    fn csi_dispatch(
        &mut self,
        params: &Provenance<[u16], Pty>,
        intermediates: &Provenance<[u8], Pty>,
        final_byte: u8,
    ) {
        let params = params.as_ref();
        let intermediates = intermediates.as_ref();
        // VT52 mode does not recognize CSI sequences — ESC [ is not a valid
        // VT52 escape. Silently ignore any CSI that arrives while in VT52 mode.
        if self.modes.vt52_mode {
            return;
        }
        // Mint a response capability for this dispatch frame. The token is
        // zero-sized and exists only for the duration of this CSI sequence;
        // downstream handlers that may push to the PTY response buffer must
        // receive `&cap` explicitly. See `response_capability.rs` (CF-003).
        //
        // #7994 note: engine consultation for response_capability is
        // performed at the `send_response` sink rather than at mint time —
        // denying the mint would suppress all CSI handling (cursor moves,
        // SGR, etc.), not just the response emission. See
        // `response_capability::mint_for_dispatch_with_engine` for the
        // engine-consulting variant, reserved for contexts that want to
        // short-circuit the whole dispatch. The default dispatch path
        // mints unconditionally and the engine gates individual response
        // sites (see `handler::TerminalHandler::send_response`).
        let cap = super::super::response_capability::ResponseCapability::mint_for_dispatch();
        // Fast path: no intermediates (vast majority of CSI sequences).
        // Handles SGR, cursor moves, erase, scroll, insert/delete inline —
        // avoids function call chain through csi_dispatch_with_intermediates.
        if intermediates.is_empty() {
            self.csi_dispatch_no_intermediates(&cap, params, final_byte);
            return;
        }
        // Slow path: sequences with intermediates (DEC private, CSI > etc.)
        // Per VT spec, sequences with unrecognized intermediates must be silently
        // ignored — they must NOT fall through to standard CSI handlers, which
        // would misinterpret e.g. CSI # h as CSI h (ANSI set mode).
        let _ = self.csi_dispatch_with_intermediates(&cap, params, intermediates, final_byte);
    }

    /// Handle CSI sequences with subparameter information.
    ///
    /// This is called when the parser detects colon-separated subparameters
    /// (e.g., `ESC[4:3m` for curly underline). The `subparam_mask` indicates
    /// which params were preceded by a colon.
    fn csi_dispatch_with_subparams(
        &mut self,
        params: &Provenance<[u16], Pty>,
        intermediates: &Provenance<[u8], Pty>,
        final_byte: u8,
        subparam_mask: u16,
    ) {
        if self.modes.vt52_mode {
            return;
        }
        // For SGR (Select Graphic Rendition), handle subparameters specially
        if final_byte == b'm' && intermediates.as_ref().is_empty() {
            self.sgr_style()
                .handle_sgr_with_subparams(params.as_ref(), subparam_mask);
            return;
        }

        // For all other sequences, fall back to normal dispatch
        self.csi_dispatch(params, intermediates, final_byte);
    }

    /// Dispatch ESC (Escape) sequences.
    fn esc_dispatch(&mut self, intermediates: &Provenance<[u8], Pty>, final_byte: u8) {
        let cap = super::super::response_capability::ResponseCapability::mint_for_dispatch();
        self.esc_dispatch_core(&cap, intermediates.as_ref(), final_byte);
    }

    /// Dispatch OSC (Operating System Command) escape sequences.
    fn osc_dispatch(&mut self, params: &Provenance<[&[u8]], Pty>) {
        // VT52 mode has no OSC sequences — silently ignore.
        if self.modes.vt52_mode {
            return;
        }
        self.transient.last_osc_bel_terminated = false;
        let cap = super::super::response_capability::ResponseCapability::mint_for_dispatch();
        self.osc_dispatch_inner(&cap, params.as_ref());
    }

    /// Dispatch OSC with terminator info for response echo (#7548).
    fn osc_dispatch_with_terminator(
        &mut self,
        params: &Provenance<[&[u8]], Pty>,
        bel_terminated: bool,
    ) {
        // VT52 mode has no OSC sequences — silently ignore.
        if self.modes.vt52_mode {
            return;
        }
        self.transient.last_osc_bel_terminated = bel_terminated;
        let cap = super::super::response_capability::ResponseCapability::mint_for_dispatch();
        self.osc_dispatch_inner(&cap, params.as_ref());
    }

    /// Begin processing a DCS (Device Control String) sequence.
    fn dcs_hook(
        &mut self,
        params: &Provenance<[u16], Pty>,
        intermediates: &Provenance<[u8], Pty>,
        final_byte: u8,
    ) {
        // VT52 mode has no DCS sequences — silently ignore.
        if self.modes.vt52_mode {
            return;
        }
        self.dcs_hook_inner(params.as_ref(), intermediates.as_ref(), final_byte);
    }

    /// Accumulate data bytes for the current DCS sequence.
    fn dcs_put(&mut self, byte: u8) {
        self.dcs_put_inner(byte);
    }

    /// Finalize a DCS sequence after receiving the String Terminator (ST).
    ///
    /// DCS unhook may produce responses for DECRQSS/XTGETTCAP; mint a
    /// capability here so downstream handlers can thread it.
    fn dcs_unhook(&mut self) {
        let cap = super::super::response_capability::ResponseCapability::mint_for_dispatch();
        self.dcs_unhook_inner(&cap);
    }

    fn apc_start(&mut self) {
        // VT52 mode has no APC sequences — silently ignore.
        if self.modes.vt52_mode {
            return;
        }
        // Release global budget from any abandoned prior DCS sequence.
        // Without this, an incomplete DCS (no ST) followed by APC leaks
        // its sequence_bytes permanently, eventually exhausting
        // MAX_DCS_GLOBAL_BUDGET and silently dropping all DCS (#7269).
        self.dcs.total_bytes = self.dcs.total_bytes.saturating_sub(self.dcs.sequence_bytes);
        self.dcs.sequence_bytes = 0;
        // Abort an abandoned Sixel decoder before clearing dcs_type.
        // Uses abort() instead of unhook() to avoid a transient 64MB
        // allocation for a copy that's immediately dropped. (#7453)
        #[cfg(feature = "sixel")]
        if matches!(self.dcs.dcs_type, super::super::DcsType::Sixel) {
            self.sixel.decoder.abort();
        }
        self.dcs.dcs_type = super::super::DcsType::None;
        self.dcs.data.clear(); // Reuse dcs_data buffer for APC
    }

    fn apc_put(&mut self, byte: u8) {
        // Accumulate APC data bytes
        // Limit to prevent DoS (same as OSC limit).
        // Track against global DCS budget so APC memory is visible
        // to the budget system (shares the dcs.data buffer).
        if self.dcs.total_bytes >= super::super::MAX_DCS_GLOBAL_BUDGET {
            return;
        }
        // Always count bytes against the budget, even when the data vec
        // is capped. Otherwise APC flooding past the cap goes untracked
        // and the budget system cannot throttle it.
        self.dcs.total_bytes += 1;
        self.dcs.sequence_bytes += 1;
        // Allow up to 4MB per APC sequence for Kitty graphics (#7688).
        // The global DCS budget (10MB) still caps total memory.
        if self.dcs.data.len() < 4 * 1024 * 1024 {
            self.dcs.data.push(byte);
        }
    }

    fn apc_end(&mut self) {
        // Process the complete APC sequence.
        // Kitty graphics (APC 'G') support was removed with the graphics
        // subsystem; APC payloads are accumulated, budgeted, and discarded.
        // Release APC bytes from the global DCS budget.
        self.dcs.total_bytes = self.dcs.total_bytes.saturating_sub(self.dcs.sequence_bytes);
        self.dcs.sequence_bytes = 0;
        // Clear the buffer and reclaim memory from large APC payloads
        // (same policy as DCS unhook and OSC dispatch — see #7272).
        self.dcs.data.clear();
        if self.dcs.data.capacity() > 4096 {
            self.dcs.data.shrink_to(128);
        }
    }
}

/// CSI dispatch fast paths extracted from ActionSink::csi_dispatch.
impl TerminalHandler<'_> {
    /// Fast-path CSI dispatch for sequences without intermediates.
    ///
    /// Single match on `final_byte` covers SGR, cursor moves, erase, scroll,
    /// and insert/delete — the top ~15 CSI sequences by frequency. Avoids the
    /// previous 3-function call chain for non-SGR sequences.
    #[inline]
    fn csi_dispatch_no_intermediates(
        &mut self,
        cap: &super::super::response_capability::ResponseCapability,
        params: &[u16],
        final_byte: u8,
    ) {
        match final_byte {
            b'm' => self.csi_dispatch_sgr_fast(params),
            // Top 5 cursor ops — inlined to avoid csi_dispatch_standard_core call
            b'A' => {
                let n = params.first().copied().unwrap_or(1).max(1);
                self.grid.cursor_up(n);
            }
            b'B' => {
                let n = params.first().copied().unwrap_or(1).max(1);
                self.grid.cursor_down(n);
            }
            b'C' => {
                let n = params.first().copied().unwrap_or(1).max(1);
                if self.modes.grapheme_cluster_mode {
                    self.cursor_state().cursor_forward_graphemes(n);
                } else {
                    self.grid
                        .cursor_forward_margin(n, self.modes.left_right_margin_mode);
                }
            }
            b'D' => {
                let n = params.first().copied().unwrap_or(1).max(1);
                if self.modes.grapheme_cluster_mode {
                    self.cursor_state().cursor_backward_graphemes(n);
                } else {
                    self.grid
                        .cursor_backward_margin(n, self.modes.left_right_margin_mode);
                }
            }
            b'H' | b'f' => {
                let row = params.first().copied().unwrap_or(1).saturating_sub(1);
                let col = params.get(1).copied().unwrap_or(1).saturating_sub(1);
                let (actual_row, actual_col) = if self.modes.origin_mode {
                    let region = self.grid.scroll_region();
                    let r = region.top.saturating_add(row).min(region.bottom);
                    if self.modes.left_right_margin_mode {
                        let margins = self.grid.horizontal_margins();
                        let c = margins.left.saturating_add(col).min(margins.right);
                        (r, c)
                    } else {
                        (r, col)
                    }
                } else {
                    (row, col)
                };
                self.grid.set_cursor(actual_row, actual_col);
            }
            // Remaining standard ops — delegate to avoid bloating this function
            _ => {
                let _ = self.csi_dispatch_standard_core(cap, params, final_byte);
            }
        }
    }

    /// SGR fast-path dispatch (extracted from csi_dispatch for clarity).
    #[inline]
    #[allow(
        clippy::too_many_lines,
        reason = "SGR dispatch table with many attribute codes"
    )]
    fn csi_dispatch_sgr_fast(&mut self, params: &[u16]) {
        // Ultra-fast: SGR 0 (reset) and bare CSI m
        // Both empty params and explicit 0 are SGR reset — use reset_sgr()
        // to preserve DECSCA protection attribute (#7321).
        // Must also clear underline color to match the CSI 0 m path (#7254).
        if params.is_empty() || (params.len() == 1 && params[0] == 0) {
            self.style.reset_sgr();
            self.transient.current_underline_color = None;
            self.transient.update_has_transient_extras();
            *self.current_style_id = StyleId::DEFAULT;
            // Reset BCE cursor template when SGR is fully default (#7522).
            self.grid
                .set_cursor_template(crate::grid::Cell::EMPTY, None);
            return;
        }
        // Single-param basic colors — ANSI 8/16 and default fg/bg reset
        if params.len() == 1 {
            let p = params[0];
            match p {
                30..=37 | 90..=97 => {
                    let index =
                        crate::terminal::sgr_color_u8(if p >= 90 { p - 90 + 8 } else { p - 30 });
                    self.style.fg = PackedColor::indexed(index);
                    let probe = Style {
                        fg: Color::from_ansi_256(index),
                        bg: self.style.cached_bg_color(),
                        attrs: self.style.cached_attrs(),
                    };
                    if let Some(id) = self.grid.try_intern_style_l1(&probe) {
                        self.style.update_fg_cache_indexed(index);
                        *self.current_style_id = id;
                    } else if let Some(id) = self.grid.try_intern_style_l2_indexed(&probe, index) {
                        self.style.update_fg_cache_indexed(index);
                        *self.current_style_id = id;
                    } else {
                        let ext = self.style.build_extended_style_fg_changed();
                        *self.current_style_id = self.grid.intern_extended_style(ext);
                    }
                    return;
                }
                39 => {
                    self.style.fg = PackedColor::DEFAULT_FG;
                    self.style.update_cached_colors();
                    let style = self.style.build_style();
                    if let Some(id) = self.grid.try_intern_style_l1(&style) {
                        *self.current_style_id = id;
                    } else {
                        let ext = self.style.build_extended_style();
                        *self.current_style_id = self.grid.intern_extended_style(ext);
                    }
                    return;
                }
                40..=47 | 100..=107 => {
                    self.style.bg =
                        PackedColor::indexed(crate::terminal::sgr_color_u8(if p >= 100 {
                            p - 100 + 8
                        } else {
                            p - 40
                        }));
                    let ext = self.style.build_extended_style_bg_changed();
                    if let Some(id) = self.grid.try_intern_style_l1(&ext.style) {
                        *self.current_style_id = id;
                    } else {
                        *self.current_style_id = self.grid.intern_extended_style(ext);
                    }
                    // Update BCE cursor template for background change (#7522).
                    self.grid.set_cursor_template(
                        crate::grid::Cell::bce_blank(self.style.cached_colors()),
                        self.style.bce_bg_rgb(),
                    );
                    return;
                }
                49 => {
                    self.style.bg = PackedColor::DEFAULT_BG;
                    self.style.update_cached_colors();
                    let style = self.style.build_style();
                    if let Some(id) = self.grid.try_intern_style_l1(&style) {
                        *self.current_style_id = id;
                    } else {
                        let ext = self.style.build_extended_style();
                        *self.current_style_id = self.grid.intern_extended_style(ext);
                    }
                    // Reset BCE cursor template when bg returns to default (#7522).
                    self.grid.set_cursor_template(
                        crate::grid::Cell::bce_blank(self.style.cached_colors()),
                        self.style.bce_bg_rgb(),
                    );
                    return;
                }
                _ => {} // Non-color single params fall through to handle_sgr
            }
        }
        self.csi_dispatch_sgr_extended(params);
    }

    #[inline(never)]
    fn csi_dispatch_sgr_extended(&mut self, params: &[u16]) {
        // 5-param truecolor fg/bg — bat, delta, vim truecolor output
        if params.len() == 5 && params[1] == 2 {
            if params[0] == 38 {
                self.style.fg = PackedColor::rgb(
                    params[2].min(255) as u8,
                    params[3].min(255) as u8,
                    params[4].min(255) as u8,
                );
                let ext = self.style.build_extended_style_fg_changed();
                if let Some(id) = self.grid.try_intern_style_l1(&ext.style) {
                    *self.current_style_id = id;
                } else {
                    *self.current_style_id = self.grid.intern_extended_style(ext);
                }
                return;
            }
            if params[0] == 48 {
                self.style.bg = PackedColor::rgb(
                    params[2].min(255) as u8,
                    params[3].min(255) as u8,
                    params[4].min(255) as u8,
                );
                let ext = self.style.build_extended_style_bg_changed();
                if let Some(id) = self.grid.try_intern_style_l1(&ext.style) {
                    *self.current_style_id = id;
                } else {
                    *self.current_style_id = self.grid.intern_extended_style(ext);
                }
                // Update BCE cursor template for truecolor bg change (#7522).
                self.grid.set_cursor_template(
                    crate::grid::Cell::bce_blank(self.style.cached_colors()),
                    self.style.bce_bg_rgb(),
                );
                return;
            }
        }
        // 3-param 256-color fg/bg
        if params.len() == 3 && params[1] == 5 {
            let index = crate::terminal::sgr_color_u8(params[2]);
            if params[0] == 38 {
                self.style.fg = PackedColor::indexed(index);
                let probe = Style {
                    fg: Color::from_ansi_256(index),
                    bg: self.style.cached_bg_color(),
                    attrs: self.style.cached_attrs(),
                };
                if let Some(id) = self.grid.try_intern_style_l1(&probe) {
                    self.style.update_fg_cache_indexed(index);
                    *self.current_style_id = id;
                } else if let Some(id) = self.grid.try_intern_style_l2_indexed(&probe, index) {
                    self.style.update_fg_cache_indexed(index);
                    *self.current_style_id = id;
                } else {
                    let ext = self.style.build_extended_style_fg_changed();
                    *self.current_style_id = self.grid.intern_extended_style(ext);
                }
                return;
            }
            if params[0] == 48 {
                self.style.bg = PackedColor::indexed(index);
                let ext = self.style.build_extended_style_bg_changed();
                if let Some(id) = self.grid.try_intern_style_l1(&ext.style) {
                    *self.current_style_id = id;
                } else {
                    *self.current_style_id = self.grid.intern_extended_style(ext);
                }
                // Update BCE cursor template for 256-color bg change (#7522).
                self.grid.set_cursor_template(
                    crate::grid::Cell::bce_blank(self.style.cached_colors()),
                    self.style.bce_bg_rgb(),
                );
                return;
            }
        }
        self.sgr_style().handle_sgr(params);
    }
}

/// Bulk ASCII write helpers extracted from `print_ascii_bulk`.
impl TerminalHandler<'_> {
    /// Fast-path bulk ASCII writer for data that passed all precondition checks.
    ///
    /// Selects between three strategies:
    /// - **Cell-run path**: same byte repeated N times uses `write_cell_run`
    ///   (memset-like fill, avoids per-cell branch overhead)
    /// - **Blast path**: default style (no colors, no flags) uses `write_ascii_blast`
    /// - **Styled path**: non-default style uses `write_ascii_run_styled`
    ///
    /// All paths update `last_graphic_char` for the REP (repeat) sequence.
    fn write_ascii_bulk_fast(&mut self, data: &[u8]) {
        let flags = if self.style.protected {
            self.style.flags.union(CellFlags::PROTECTED)
        } else {
            self.style.flags
        };
        let is_default = self.style.is_default();
        let colors = self.style.cached_colors();

        // Real terminal output is dominated by short fragments between
        // color/reset/newline boundaries. For those chunks, the 4+-run scan
        // below is overhead with little upside, so write them directly.
        if data.len() <= 64 {
            if is_default {
                let written = self.grid.write_ascii_blast(data);
                if written > 0 {
                    if let Some(&last) = data.get(written.saturating_sub(1)) {
                        self.transient.last_graphic_char = Some(last as char);
                    }
                }
            } else {
                let mut last_byte: Option<u8> = None;
                self.grid
                    .write_ascii_run_styled_packed(data, colors, flags, &mut last_byte);
                if let Some(b) = last_byte {
                    self.transient.last_graphic_char = Some(b as char);
                }
            }
            return;
        }

        // Single-pass scan: find runs of 4+ identical bytes AND mixed segments
        // in one traversal. Previous two-pass approach (scan_identical_run then
        // scan_mixed_segment) re-scanned the same bytes for diverse content.
        let mut pos = 0;
        while pos < data.len() {
            let byte = data[pos];
            let mut run_end = pos + 1;
            while run_end < data.len() && data[run_end] == byte {
                run_end += 1;
            }

            if run_end - pos >= 4 {
                let run_len = run_end - pos;
                let mut last_byte: Option<u8> = None;
                if is_default {
                    self.grid.write_cell_run(
                        byte,
                        run_len,
                        crate::grid::PackedColors::DEFAULT,
                        CellFlags::empty(),
                        &mut last_byte,
                    );
                } else {
                    self.grid
                        .write_cell_run(byte, run_len, colors, flags, &mut last_byte);
                }
                if let Some(b) = last_byte {
                    self.transient.last_graphic_char = Some(b as char);
                }
                pos = run_end;
                continue;
            }

            // Mixed segment: accumulate until we hit a 4+ run.
            let seg_start = pos;
            pos = run_end;
            while pos < data.len() {
                let b = data[pos];
                let mut r = pos + 1;
                while r < data.len() && data[r] == b {
                    r += 1;
                }
                if r - pos >= 4 {
                    break;
                }
                pos = r;
            }
            let segment = &data[seg_start..pos];

            if is_default {
                let written = self.grid.write_ascii_blast(segment);
                if written > 0 {
                    if let Some(&last) = segment.get(written.saturating_sub(1)) {
                        self.transient.last_graphic_char = Some(last as char);
                    }
                }
            } else {
                let mut last_byte: Option<u8> = None;
                self.grid
                    .write_ascii_run_styled_packed(segment, colors, flags, &mut last_byte);
                if let Some(b) = last_byte {
                    self.transient.last_graphic_char = Some(b as char);
                }
            }
        }
    }

    /// Bulk ASCII writer for styles that need `CellExtras` overflow.
    ///
    /// Handles RGB colors, hyperlinks, underline colors, and extended flags
    /// in bulk instead of falling back to per-character processing. Writes
    /// cells via `write_ascii_run_with_extras` which does bulk cell writes
    /// followed by batch extras application — 4-5x faster than per-char.
    fn write_ascii_bulk_with_extras(&mut self, data: &[u8]) {
        // Use pre-computed packed colors from CurrentStyle.
        let colors = self.style.cached_colors();
        let flags = if self.style.protected {
            self.style.flags.union(CellFlags::PROTECTED)
        } else {
            self.style.flags
        };

        let fg_rgb = if self.style.fg.is_rgb() {
            let (r, g, b) = self.style.fg.rgb_components();
            Some([r, g, b])
        } else {
            None
        };
        let bg_rgb = if self.style.bg.is_rgb() {
            let (r, g, b) = self.style.bg.rgb_components();
            Some([r, g, b])
        } else {
            None
        };
        let extended_flags_bits = if self.style.flags.has_extended_flags() {
            self.style.flags.extended_flags().bits()
        } else {
            0
        };

        let mut last_byte: Option<u8> = None;
        self.grid.write_ascii_run_with_extras(
            data,
            colors,
            flags,
            fg_rgb,
            bg_rgb,
            self.transient.current_underline_color,
            extended_flags_bits,
            self.transient.current_hyperlink.as_ref(),
            self.transient.current_hyperlink_id.as_ref(),
            &mut last_byte,
        );

        if let Some(b) = last_byte {
            self.transient.last_graphic_char = Some(b as char);
        }
    }
}
