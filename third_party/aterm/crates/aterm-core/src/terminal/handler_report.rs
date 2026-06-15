// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Device status reports, device attributes, soft reset, and SGR stack
//! operations for `TerminalHandler`.
//!
//! Extracted from `handler_state.rs` to keep module sizes under 500 lines.
//! These methods generate terminal responses or perform cross-cutting state
//! resets, rather than individual grid/mode mutations.

use super::handler::TerminalHandler;
use super::response_capability::ResponseCapability;
use super::{
    CursorStyle, MouseEncoding, MouseMode, SGR_STACK_MAX_DEPTH, SgrPushMask, SgrStackEntry,
    Vt52CursorState,
};

impl TerminalHandler<'_> {
    /// Handle DECSTR (Soft Terminal Reset).
    ///
    /// CSI ! p
    ///
    /// Soft reset performs a partial reset of the terminal, resetting:
    /// - Cursor visibility (DECTCEM) → visible
    /// - Cursor style (DECSCUSR) → default (blinking block)
    /// - Origin mode (DECOM) → disabled
    /// - Auto-wrap mode (DECAWM) → enabled (xterm behavior)
    /// - Insert mode (IRM) → disabled (replace mode)
    /// - Application cursor keys (DECCKM) → disabled
    /// - Application keypad mode (DECKPAM) → disabled
    /// - Reverse wraparound mode (DECSET 45) → disabled
    /// - Left/right margin mode (DECLRMM, DECSET 69) → disabled
    /// - Reverse video mode (DECSCNM) → disabled (per VT220 §4.5.113)
    /// - Synchronized output mode (2026) → disabled
    /// - New line mode (LNM) → disabled (per VT510 spec §4.5.113)
    /// - Focus reporting (mode 1004) → disabled (per xterm)
    /// - Bracketed paste mode (2004) → disabled (per xterm)
    /// - Mouse tracking mode (9/1000/1002/1003) → disabled (per xterm)
    /// - Mouse encoding (1005/1006/1015/1016) → X10 default (per xterm)
    /// - Text attributes (SGR) → default
    /// - Character sets (G0-G3, GL) → defaults
    /// - Scroll margins (DECSTBM) → full screen
    /// - Saved cursor states → cleared
    ///
    /// Unlike RIS (hard reset), DECSTR does NOT reset:
    /// - Alternate screen buffer (stays on current screen)
    /// - Screen content (not erased)
    /// - Tab stops
    /// - Color palette
    /// - Working directory
    #[cfg_attr(
        test,
        aterm_spec::refines(machine = "terminal_modes", action = "SoftReset")
    )]
    pub(super) fn handle_decstr(&mut self) {
        // Reset cursor visibility
        self.modes.cursor_visible = true;

        // Reset cursor style to default; fire callback if it changed
        let old_style = self.modes.cursor_style;
        self.modes.cursor_style = CursorStyle::default();
        if old_style != self.modes.cursor_style {
            if let Some(callback) = self.cursor_style_callback {
                callback(self.modes.cursor_style);
            }
        }

        // Reset origin mode
        self.modes.origin_mode = false;

        // Reset auto-wrap mode (xterm sets to true, VT510 says false - follow xterm)
        self.modes.auto_wrap = true;

        // Reset insert mode
        self.modes.insert_mode = false;

        // Reset application cursor keys
        self.modes.application_cursor_keys = false;

        // Reset application keypad mode (DECKPAM/DECKPNM)
        self.modes.application_keypad = false;

        // Reset cursor blink (mode 12) (#7284)
        self.modes.cursor_blink = false;

        // Reset VT52 mode (DECANM, mode 2) (#7284)
        self.modes.vt52_mode = false;

        // Reset 132-column mode flag (DECCOLM, mode 3) (#7284)
        self.modes.column_mode_132 = false;

        // Reset DECCOLM enable (mode 40) — per xterm, soft reset disables mode 40
        self.modes.deccolm_enable = false;

        // Reset DECNCSM (mode 95) — per xterm, soft reset clears DECNCSM
        self.modes.decncsm = false;

        // Reset reverse wraparound mode (DECSET 45)
        self.modes.reverse_wraparound = false;

        // Reset left/right margin mode (DECLRMM, DECSET 69)
        // Unconditionally clear both the mode flag and any configured horizontal
        // margins. If we only reset when DECLRMM is currently active, stored
        // margin values survive a disable-then-DECSTR sequence and resurrect
        // when DECLRMM is later re-enabled. Fixes #7664.
        self.modes.left_right_margin_mode = false;
        self.grid.reset_horizontal_margins();

        // Reset reverse video mode (DECSCNM) per VT220 spec §4.5.113
        if self.modes.reverse_video {
            self.modes.reverse_video = false;
            self.grid.damage_mut().mark_full();
        }

        // Reset synchronized output mode (per synchronized rendering spec)
        self.modes.synchronized_output = false;
        self.transient.sync_start = None;

        // Reset grapheme cluster mode (mode 2027)
        self.modes.grapheme_cluster_mode = false;

        // Reset sixel display mode (DECSDM, mode 80) — xterm resets on DECSTR (#7496)
        self.modes.sixel_display_mode = false;

        // Reset alternate scroll mode (mode 1007) — xterm resets this on DECSTR
        self.modes.alternate_scroll = false;

        // Reset new-line mode (LNM, ANSI mode 20) per VT510 spec §4.5.113
        self.modes.new_line_mode = false;

        // Reset focus reporting (mode 1004) — xterm resets on DECSTR
        self.modes.focus_reporting = false;

        // Reset bracketed paste mode (mode 2004) — xterm resets on DECSTR.
        // Without this, a buggy app that enables bracketed paste and then
        // soft-resets leaves the shell receiving spurious paste brackets.
        self.modes.bracketed_paste = false;

        // Reset mouse tracking mode (modes 9/1000/1002/1003) — xterm resets on DECSTR.
        // Without this, mouse events keep being sent after soft reset.
        self.modes.mouse_mode = MouseMode::None;

        // Reset mouse encoding mode (modes 1005/1006/1015/1016) — xterm resets on DECSTR.
        self.modes.mouse_encoding = MouseEncoding::X10;

        // Reset BiDi modes to defaults (Terminal WG spec)
        self.modes.bidi_mode = crate::config::BiDiMode::Implicit;
        self.modes.bidi_direction = aterm_types::ParagraphDirection::Auto;
        self.modes.bidi_box_mirroring = false;
        self.modes.bidi_autodetection = false;
        // Terminal WG spec: default enabled
        self.modes.bidi_arrow_swap = true;
        // Invalidate BiDi cache since modes have changed
        self.invalidate_bidi_all();

        // Reset text attributes (SGR) to defaults
        self.style.reset_sgr();
        // DECSCA (character protection) is reset by DECSTR per VT510 spec.
        // reset_sgr() deliberately preserves `protected` (correct for SGR 0),
        // so we must clear it explicitly here.
        self.style.protected = false;
        self.sgr_style().update_style_id();
        self.transient.current_underline_color = None;
        // Clear active hyperlink — DECSTR resets all text attributes, and
        // leaving a stale hyperlink active after soft reset would cause
        // subsequent text to inherit an unexpected link (#7487).
        self.transient.current_hyperlink = None;
        self.transient.current_hyperlink_id = None;
        self.transient.update_has_transient_extras();

        // Clear last graphic character so REP (CSI b) does not repeat
        // stale characters after soft reset.
        self.transient.last_graphic_char = None;

        // Clear VT52 cursor addressing state — DECSTR resets vt52_mode above,
        // but if issued mid-sequence (e.g. while collecting ESC Y row/col), the
        // partial state must also be cleared.
        self.transient.vt52_cursor_state = Vt52CursorState::default();

        // Clear SGR stack — DECSTR resets all SGR state, so stale push entries
        // from before the reset should not be restorable via XTPOPSGR.
        self.transient.sgr_stack.clear();

        // Clear XTSAVE'd mode values — DECSTR resets all modes, so stale
        // saved values should not be restorable via XTRESTORE. Matches xterm
        // behavior. (#7450)
        self.transient.xtsave_modes.clear();

        // Reset xterm keyboard modifier/format options (XTMODKEYS/XTFMTKEYS).
        // xterm resets modifyOtherKeys on DECSTR.
        self.xterm_keyboard.reset();

        // Reset Kitty keyboard protocol state (#7477).
        // Per Kitty spec, soft reset clears the flags stack and disables
        // all enhancements. Without this, an app that pushes flags and then
        // soft-resets leaves stale keyboard encoding active.
        self.kitty_keyboard.reset();

        // Reset character set state
        self.charset.reset();

        // Reset scroll margins to full screen
        self.grid.reset_scroll_region();

        // Clear the DECSC saved cursor for the current screen only.
        // Per VT510 spec §4.5.113, DECSTR resets the saved cursor (DECSC),
        // but only for the active buffer — the inactive screen's save is
        // preserved. xterm likewise resets sc[SAVED_CURSOR] for the current
        // screen only.
        //
        // Mode 1049 cursor saves are NOT cleared: they are an xterm extension
        // separate from DECSC, and xterm does not touch them on soft reset.
        // Clearing them would lose the pre-alt-screen cursor position, so
        // a subsequent mode 1049 RESET would fail to restore the cursor (#7686).
        if self.modes.alternate_screen {
            self.cursor_save.alt = None;
        } else {
            self.cursor_save.main = None;
        }

        // Move cursor to home position (0, 0)
        self.grid.set_cursor(0, 0);
    }

    /// Handle DSR (Device Status Report).
    ///
    /// CSI Ps n
    /// - Ps = 5: Status report - responds with CSI 0 n (terminal OK)
    /// - Ps = 6: Cursor Position Report (CPR) - responds with CSI row ; col R
    pub(super) fn handle_dsr(&mut self, cap: &ResponseCapability, params: &[u16]) {
        let param = params.first().copied().unwrap_or(0);
        match param {
            5 => {
                // Device Status Report - report terminal OK
                self.send_response(cap, b"\x1b[0n");
            }
            6 => {
                // Cursor Position Report (CPR)
                // Reports cursor position as CSI row ; col R (1-indexed)
                // Per VT510: When origin mode (DECOM) is set, row is reported
                // relative to the scroll region top margin; when DECLRMM is also
                // active, column is reported relative to the left margin.
                let (row, col) = if self.modes.origin_mode {
                    let region = self.grid.scroll_region();
                    let r = self.grid.cursor_row().saturating_sub(region.top) + 1;
                    let c = if self.modes.left_right_margin_mode {
                        let margins = self.grid.horizontal_margins();
                        self.grid.cursor_col().saturating_sub(margins.left) + 1
                    } else {
                        self.grid.cursor_col() + 1
                    };
                    (r, c)
                } else {
                    (self.grid.cursor_row() + 1, self.grid.cursor_col() + 1)
                };
                let response = format!("\x1b[{row};{col}R");
                self.send_response(cap, response.as_bytes());
            }
            _ => {} // Unknown DSR parameter
        }
    }

    /// Handle DECDSR (DEC Device Status Report).
    ///
    /// CSI ? Ps n
    /// - Ps = 6: DECXCPR (Extended Cursor Position Report) - responds with
    ///   CSI ? row ; col ; page R (page always 1 for us)
    /// - Ps = 15: Printer status - responds with CSI ? 13 n (no printer)
    /// - Ps = 25: UDK status - responds with CSI ? 20 n (UDKs unlocked)
    /// - Ps = 26: Keyboard status - responds with CSI ? 27 ; 1 ; 0 ; 0 n
    ///   (North American, ready, no LK201 options)
    pub(super) fn handle_decdsr(&mut self, cap: &ResponseCapability, params: &[u16]) {
        let param = params.first().copied().unwrap_or(0);
        match param {
            6 => {
                // DECXCPR - Extended Cursor Position Report
                // Response: CSI ? row ; col ; page R (1-indexed, page always 1)
                // Per VT510: affected by origin mode (DECOM) and DECLRMM.
                let (row, col) = if self.modes.origin_mode {
                    let region = self.grid.scroll_region();
                    let r = self.grid.cursor_row().saturating_sub(region.top) + 1;
                    let c = if self.modes.left_right_margin_mode {
                        let margins = self.grid.horizontal_margins();
                        self.grid.cursor_col().saturating_sub(margins.left) + 1
                    } else {
                        self.grid.cursor_col() + 1
                    };
                    (r, c)
                } else {
                    (self.grid.cursor_row() + 1, self.grid.cursor_col() + 1)
                };
                let response = format!("\x1b[?{row};{col};1R");
                self.send_response(cap, response.as_bytes());
            }
            15 => {
                // Printer status: no printer connected
                self.send_response(cap, b"\x1b[?13n");
            }
            25 => {
                // UDK status: UDKs are unlocked
                self.send_response(cap, b"\x1b[?20n");
            }
            26 => {
                // Keyboard status: North American, ready, no LK201 options
                self.send_response(cap, b"\x1b[?27;1;0;0n");
            }
            _ => {} // Unknown DECDSR parameter
        }
    }

    /// Handle Primary Device Attributes (DA1).
    ///
    /// CSI Ps c (where Ps is 0 or omitted)
    ///
    /// Reports terminal type and capabilities. The conformance level is
    /// dynamic based on `modes.vt_level` (changed by DECSCL).
    ///
    /// Default response (VT420): CSI ? 64 ; 6 ; 22 ; 28 c
    /// - 64 = VT420 conformance level
    /// - 6 = Selective erase (DECSCA/DECSED/DECSEL)
    /// - 22 = ANSI color
    /// - 28 = Rectangular editing (DECFRA/DECCARA/DECCRA/DECSERA)
    pub(super) fn handle_primary_da(&mut self, cap: &ResponseCapability) {
        // Report conformance level matching the current VT level, plus capabilities.
        //
        // DA1 response format: CSI ? Pc ; Ps1 ; ... c
        // - Pc = Conformance level (61=VT100, 62=VT200, 63=VT300, 64=VT400, 65=VT500)
        // - 6 = Selective erase (DECSCA/DECSED/DECSEL)
        // - 22 = ANSI color (per VT525 DA1 spec)
        // - 28 = Rectangular editing (DECFRA/DECCARA/DECCRA/DECSERA)
        //
        // The level changes when DECSCL is used (#7562).
        // VT100 uses a different DA1 format: CSI ? 1 ; 2 c
        // (type 1 = VT100, option 2 = Advanced Video Option).
        // VT200+ uses the standard format: CSI ? Pc ; capabilities c
        if self.modes.vt_level == aterm_types::VtLevel::VT100 {
            self.send_response(cap, b"\x1b[?1;2c");
            return;
        }

        let level = match self.modes.vt_level {
            aterm_types::VtLevel::VT100 => unreachable!(),
            aterm_types::VtLevel::VT220 | aterm_types::VtLevel::VT240 => b"62" as &[u8],
            aterm_types::VtLevel::VT320
            | aterm_types::VtLevel::VT330
            | aterm_types::VtLevel::VT340 => b"63",
            aterm_types::VtLevel::VT420 => b"64",
            aterm_types::VtLevel::VT510
            | aterm_types::VtLevel::VT520
            | aterm_types::VtLevel::VT525 => b"65",
        };
        let mut response = Vec::with_capacity(20);
        response.extend_from_slice(b"\x1b[?");
        response.extend_from_slice(level);
        // Capability flags are level-dependent per VT510 spec:
        // - VT220+ (62+): 6 = selective erase, 22 = ANSI color
        // - VT420+ (64+): 28 = rectangular editing (DECFRA/DECCARA/DECCRA/DECSERA)
        // - Sixel (4): advertised when `sixel` feature is enabled (VT220+)
        match self.modes.vt_level {
            aterm_types::VtLevel::VT100 => unreachable!(),
            aterm_types::VtLevel::VT220
            | aterm_types::VtLevel::VT240
            | aterm_types::VtLevel::VT320
            | aterm_types::VtLevel::VT330
            | aterm_types::VtLevel::VT340 => {
                response.extend_from_slice(b";6;22");
                #[cfg(feature = "sixel")]
                response.extend_from_slice(b";4");
            }
            aterm_types::VtLevel::VT420
            | aterm_types::VtLevel::VT510
            | aterm_types::VtLevel::VT520
            | aterm_types::VtLevel::VT525 => {
                response.extend_from_slice(b";6;22;28");
                #[cfg(feature = "sixel")]
                response.extend_from_slice(b";4");
            }
        }
        response.push(b'c');
        self.send_response(cap, &response);
    }

    /// Handle Secondary Device Attributes (DA2).
    ///
    /// CSI > Ps c (where Ps is 0 or omitted)
    ///
    /// Reports terminal type, firmware version, and keyboard type.
    ///
    /// Response: CSI > Pp ; Pv ; Pc c
    /// - Pp = Terminal type (41 = VT420, consistent with DA1 conformance level 64)
    /// - Pv = Firmware version (we use 100 as a version number)
    /// - Pc = ROM cartridge registration number (0 = none)
    pub(super) fn handle_secondary_da(&mut self, cap: &ResponseCapability) {
        // Report terminal type matching the current VT level, version 1.0.0, no ROM cartridge.
        // Pp = DA2 parameter from VtLevel (e.g. 41 for VT420, 64 for VT520).
        // The level changes when DECSCL is used (#7562).
        let da2 = self.modes.vt_level.da2_param();
        let response = format!("\x1b[>{da2};100;0c");
        self.send_response(cap, response.as_bytes());
    }

    /// Handle Tertiary Device Attributes (DA3).
    ///
    /// CSI = Ps c (where Ps is 0 or omitted)
    ///
    /// Reports the terminal's unit ID (serial number). Per VT510/VT520 spec,
    /// the response is a DCS sequence with the unit identifier hex-encoded.
    ///
    /// Response: DCS ! | <hex-unit-id> ST
    /// - hex-unit-id = "30" (ASCII '0' = 0x30, meaning no unit ID assigned)
    pub(super) fn handle_tertiary_da(&mut self, cap: &ResponseCapability) {
        // Report no unit ID assigned (standard for software terminal emulators).
        // Per DEC spec, the unit ID is hex-encoded: ASCII '0' = 0x30.
        self.send_response(cap, b"\x1bP!|30\x1b\\");
    }

    /// Handle Kitty keyboard protocol query (CSI ? u).
    ///
    /// Responds with `CSI ? flags u` where flags is the current keyboard flags value.
    pub(super) fn handle_kitty_keyboard_query(&mut self, cap: &ResponseCapability) {
        let flags = self.kitty_keyboard.query_flags();
        // Format: CSI ? flags u
        let response = format!("\x1b[?{flags}u");
        self.send_response(cap, response.as_bytes());
    }

    /// Handle XTPUSHSGR - push SGR attributes onto stack (CSI # {).
    ///
    /// Saves the current SGR state (colors, flags, underline color) onto an internal
    /// stack for later restoration with XTPOPSGR. Stack is bounded to
    /// `SGR_STACK_MAX_DEPTH` (10) entries - oldest entry is discarded if full.
    ///
    /// Per xterm spec, `CSI # { Ps...` can selectively push only specific
    /// attributes. If no params (or param 0), all attributes are pushed.
    /// Recognized Ps values:
    /// - 1/2: bold/dim group
    /// - 3: italic
    /// - 4/21: underline group (all underline styles + underline color)
    /// - 5: blink
    /// - 7: inverse
    /// - 8: invisible
    /// - 9: strikethrough
    /// - 30/31/38/39: foreground color
    /// - 40/41/48/49: background color
    /// - 53: overline
    pub(super) fn handle_xtpushsgr(&mut self, params: &[u16]) {
        // Enforce stack depth limit (O(1) eviction via VecDeque)
        if self.transient.sgr_stack.len() >= SGR_STACK_MAX_DEPTH {
            self.transient.sgr_stack.pop_front();
        }

        // Determine which attributes to push
        let mask = if params.is_empty() || (params.len() == 1 && params[0] == 0) {
            SgrPushMask::ALL
        } else {
            SgrPushMask::from_params(params)
        };

        // Save current SGR state with the selective mask
        let entry = SgrStackEntry {
            style: *self.style,
            underline_color: self.transient.current_underline_color,
            mask,
        };
        self.transient.sgr_stack.push_back(entry);
    }

    /// Handle XTPOPSGR - pop and restore SGR attributes from stack (CSI # }).
    ///
    /// Restores the most recently pushed SGR state. If the stack is empty,
    /// this is a no-op (per xterm behavior). When selective push was used,
    /// only the masked attributes are restored.
    pub(super) fn handle_xtpopsgr(&mut self) {
        if let Some(entry) = self.transient.sgr_stack.pop_back() {
            if entry.mask.is_all() {
                // Full restore — fast path.
                // Preserve the current DECSCA protection attribute: XTPUSHSGR
                // is an SGR-only stack, and DECSCA is not an SGR attribute
                // (it is controlled by CSI Ps " q, not CSI m).
                let saved_protected = self.style.protected;
                *self.style = entry.style;
                self.style.protected = saved_protected;
                self.transient.current_underline_color = entry.underline_color;
            } else {
                // Selective restore — only restore masked attributes
                use crate::grid::CellFlags;

                if entry.mask.has_bold() {
                    // Bold/dim group: restore BOLD and DIM flags
                    let bold_dim = CellFlags::BOLD.union(CellFlags::DIM);
                    self.style.flags.remove(bold_dim);
                    self.style
                        .flags
                        .insert(CellFlags(entry.style.flags.0 & bold_dim.0));
                }
                if entry.mask.has_italic() {
                    self.style.flags.remove(CellFlags::ITALIC);
                    self.style
                        .flags
                        .insert(CellFlags(entry.style.flags.0 & CellFlags::ITALIC.0));
                }
                if entry.mask.has_underline() {
                    // Underline group: all underline style flags + underline color.
                    // ALL_UNDERLINES covers UNDERLINE, DOUBLE_UNDERLINE,
                    // CURLY_UNDERLINE, DOTTED_UNDERLINE, and DASHED_UNDERLINE.
                    let ul_flags = CellFlags::ALL_UNDERLINES;
                    self.style.flags.remove(ul_flags);
                    self.style
                        .flags
                        .insert(CellFlags(entry.style.flags.0 & ul_flags.0));
                    self.transient.current_underline_color = entry.underline_color;
                }
                if entry.mask.has_blink() {
                    self.style.flags.remove(CellFlags::BLINK);
                    self.style
                        .flags
                        .insert(CellFlags(entry.style.flags.0 & CellFlags::BLINK.0));
                }
                if entry.mask.has_inverse() {
                    self.style.flags.remove(CellFlags::INVERSE);
                    self.style
                        .flags
                        .insert(CellFlags(entry.style.flags.0 & CellFlags::INVERSE.0));
                }
                if entry.mask.has_invisible() {
                    self.style.flags.remove(CellFlags::HIDDEN);
                    self.style
                        .flags
                        .insert(CellFlags(entry.style.flags.0 & CellFlags::HIDDEN.0));
                }
                if entry.mask.has_strikethrough() {
                    self.style.flags.remove(CellFlags::STRIKETHROUGH);
                    self.style
                        .flags
                        .insert(CellFlags(entry.style.flags.0 & CellFlags::STRIKETHROUGH.0));
                }
                if entry.mask.has_foreground() {
                    self.style.fg = entry.style.fg;
                }
                if entry.mask.has_background() {
                    self.style.bg = entry.style.bg;
                }
                if entry.mask.has_overline() {
                    self.style.flags.remove(CellFlags::OVERLINE);
                    self.style
                        .flags
                        .insert(CellFlags(entry.style.flags.0 & CellFlags::OVERLINE.0));
                }
                // Recompute cached state after selective modification
                self.style.update_cached_colors();
            }
            self.transient.update_has_transient_extras();
            // Update the cached style ID
            self.sgr_style().update_style_id();
        }
        // Empty stack = no-op (xterm behavior)
    }
}
