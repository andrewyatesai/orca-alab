// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! DEC private mode handler for the terminal.
//!
//! This module contains handlers for DEC private mode escape sequences:
//! - DECSET/DECRST (CSI ? Ps h/l) - Enable/disable DEC private modes
//! - DECRQM (CSI ? Ps $ p) - Request DEC mode state
//! - Standard ANSI modes (CSI Ps h/l)
//!
//! Modes include: cursor keys, VT52 mode, column mode, reverse video,
//! origin mode, auto-wrap, cursor visibility, alternate screen buffer,
//! mouse tracking, bracketed paste, synchronized output, and more.
//!
//! Extracted from handler.rs as part of large files refactor.

use crate::grid::{CellFlags, Grid};

use super::handler::TerminalHandler;
use super::{MouseEncoding, MouseMode, SavedCursorState};

impl TerminalHandler<'_> {
    /// Handle DEC private mode set/reset.
    #[allow(
        clippy::too_many_lines,
        reason = "match on ~30 DEC private mode numbers per spec"
    )]
    pub(super) fn handle_dec_mode(&mut self, params: &[u16], set: bool) {
        for &param in params {
            match param {
                1 => {
                    if set {
                        self.enable_application_cursor_keys();
                    } else {
                        self.disable_application_cursor_keys();
                    }
                }
                2 => {
                    // DECANM — CSI ? 2 l enters VT52, CSI ? 2 h exits.
                    self.set_vt52_mode(!set);
                }
                3 => {
                    // DECCOLM — flag only, host resizes if desired.
                    // Per xterm spec, DECCOLM is only honored when mode 40
                    // (deccolm_enable) is set. Otherwise CSI ?3h/l is ignored.
                    if self.modes.deccolm_enable {
                        self.set_column_mode_132(set);
                    }
                }
                5 => {
                    // DECSCNM — reverse video, forces full redraw.
                    self.set_reverse_video(set);
                }
                6 => {
                    if set {
                        self.enable_origin_mode();
                    } else {
                        self.disable_origin_mode();
                    }
                }
                7 => {
                    if set {
                        self.enable_auto_wrap();
                    } else {
                        self.disable_auto_wrap();
                    }
                }
                12 => {
                    self.set_cursor_blink(set);
                }
                25 => {
                    if set {
                        self.show_cursor();
                    } else {
                        self.hide_cursor();
                    }
                }
                45 => {
                    self.set_reverse_wraparound(set);
                }
                66 => {
                    // DECNKM — Numeric Keypad Mode.
                    // CSI ? 66 h = application keypad (same as ESC =)
                    // CSI ? 66 l = numeric keypad (same as ESC >)
                    // Used by dialog(1), nano, and others as alternative to ESC =/>.
                    self.set_application_keypad(set);
                }
                69 => {
                    self.set_left_right_margin_mode(set);
                }
                40 => {
                    // Mode 40 — Enable 80/132 column switching.
                    // When reset, DECCOLM (mode 3) is ignored. Per xterm spec.
                    self.modes.deccolm_enable = set;
                }
                80 => {
                    // DECSDM — Sixel Display Mode.
                    // SET: Sixel output scrolls at bottom margin.
                    // RESET: Sixel output clips at bottom margin.
                    self.set_sixel_display_mode(set);
                }
                95 => {
                    // DECNCSM — No Clearing Screen on Column Change.
                    // When set, DECCOLM toggle does not clear screen/margins/cursor.
                    // Per xterm/VT510 spec.
                    self.modes.decncsm = set;
                }
                47 => {
                    // Mode 47: Switch buffer only. No save/restore cursor. No clear.
                    if set {
                        self.enter_alternate_screen_raw();
                    } else {
                        self.exit_alternate_screen_raw();
                    }
                }
                1047 => {
                    // Mode 1047: Switch buffer. Clear alt screen on exit. No cursor save/restore.
                    if set {
                        self.enter_alternate_screen_raw();
                    } else {
                        self.exit_alternate_screen_1047();
                    }
                }
                1049 => {
                    // Mode 1049: Switch buffer + save/restore cursor + clear alt on enter.
                    if set {
                        self.enter_alternate_screen();
                    } else {
                        self.exit_alternate_screen();
                    }
                }
                2004 => {
                    if set {
                        self.enable_bracketed_paste();
                    } else {
                        self.disable_bracketed_paste();
                    }
                }
                // Mouse tracking modes (mutually exclusive — reset to None on DECRST)
                9 => {
                    if set {
                        self.enable_mouse_x10_tracking();
                    } else {
                        self.disable_mouse_tracking();
                    }
                }
                1000 => {
                    if set {
                        self.enable_mouse_normal_tracking();
                    } else {
                        self.disable_mouse_tracking();
                    }
                }
                1002 => {
                    if set {
                        self.enable_mouse_button_event_tracking();
                    } else {
                        self.disable_mouse_tracking();
                    }
                }
                1003 => {
                    if set {
                        self.enable_mouse_any_event_tracking();
                    } else {
                        self.disable_mouse_tracking();
                    }
                }
                1004 => {
                    if set {
                        self.enable_focus_reporting();
                    } else {
                        self.disable_focus_reporting();
                    }
                }
                1007 => {
                    self.set_alternate_scroll(set);
                }
                // Mouse encoding modes (reset to X10 on DECRST)
                1005 => {
                    if set {
                        self.enable_utf8_mouse_encoding();
                    } else {
                        self.disable_utf8_mouse_encoding();
                    }
                }
                1006 => {
                    if set {
                        self.enable_sgr_mouse_encoding();
                    } else {
                        self.disable_sgr_mouse_encoding();
                    }
                }
                1015 => {
                    if set {
                        self.enable_urxvt_mouse_encoding();
                    } else {
                        self.disable_urxvt_mouse_encoding();
                    }
                }
                1016 => {
                    if set {
                        self.enable_sgr_pixel_mouse_encoding();
                    } else {
                        self.disable_sgr_pixel_mouse_encoding();
                    }
                }
                2026 => {
                    if set {
                        self.enable_synchronized_output();
                    } else {
                        self.disable_synchronized_output();
                    }
                }
                2027 => {
                    self.set_grapheme_cluster_mode(set);
                }
                // === BiDi DEC Private Modes (Terminal WG specification) ===
                // See: https://terminal-wg.pages.freedesktop.org/bidi/recommendation/escape-sequences.html
                1243 => {
                    self.set_bidi_arrow_swap(set);
                }
                2500 => {
                    self.set_bidi_box_mirroring(set);
                }
                2501 => {
                    self.set_bidi_autodetection(set);
                }
                // Mode 1048: xterm save/restore cursor (equivalent to DECSC/DECRC)
                1048 => {
                    if set {
                        self.cursor_state().save_cursor_state();
                    } else {
                        self.cursor_state().restore_cursor_state();
                    }
                }
                _ => {} // Unknown DEC mode
            }
        }
    }

    /// Enter alternate screen for mode 47/1047 — buffer swap only, no cursor
    /// save, no clear.
    fn enter_alternate_screen_raw(&mut self) {
        if self.modes.alternate_screen {
            return;
        }

        self.kitty_keyboard.switch_screen(true);
        // Clear hyperlink state — hyperlinks should not leak across screen
        // boundaries. A hyperlink opened on main should not apply to alt
        // screen text and vice versa. (#7414)
        self.transient.current_hyperlink = None;
        self.transient.current_hyperlink_id = None;
        self.transient.update_has_transient_extras();
        // Clear pending Sixel image — a Sixel rendered on main should not
        // leak into the alt screen context (#7484).
        #[cfg(feature = "sixel")]
        {
            self.sixel.pending_image = None;
        }
        // Per xterm there is one persistent alternate buffer: modes 47/1047
        // never clear it on entry (only 1049 does), so content from a
        // previous alt session must survive re-entry. Reuse the buffer kept
        // by the last exit, or allocate one lazily. Alt screen has no
        // scrollback per xterm spec — lines scrolled off the top of the
        // alternate buffer are discarded, not accumulated.
        let mut new_grid = self
            .alt_grid
            .take()
            .unwrap_or_else(|| Grid::with_scrollback(self.grid.rows(), self.grid.cols(), 0));
        // Per xterm: tab stops are global, shared between main and alt screens.
        // Copy main screen tab stops to the new alt screen (#7494).
        new_grid.restore_tab_stops(self.grid.tab_stops());
        // Per xterm the cursor is shared by both screen buffers: modes 47 and
        // 1047 swap the buffer without moving it (only 1048/1049 save/restore).
        let cursor = self.grid.cursor();
        new_grid.set_cursor(cursor.row, cursor.col);
        new_grid.set_pending_wrap(self.grid.pending_wrap());
        // Per xterm DECSTBM/DECSLRM margins live on the shared TScreen
        // (top_marg/bot_marg/lft_marg/rt_marg) — they persist across buffer
        // switches rather than belonging to either buffer.
        Self::copy_margins(self.grid, &mut new_grid);
        let old_grid = std::mem::replace(self.grid, new_grid);
        *self.alt_grid = Some(old_grid);
        self.modes.alternate_screen = true;
        // Invalidate selection — grid content changed completely.
        self.grid.force_selection_invalidation();
        if let Some(cb) = self.buffer_activation_callback {
            cb(true);
        }
    }

    /// Copy the scroll region and horizontal margins from one grid to
    /// another. Per xterm, DECSTBM/DECSLRM margins are fields of the shared
    /// `TScreen` (top_marg/bot_marg/lft_marg/rt_marg): switching between the
    /// main and alternate buffers (modes 47/1047/1049) does not reset them.
    fn copy_margins(from: &Grid, to: &mut Grid) {
        let region = from.scroll_region();
        to.set_scroll_region(region.top, region.bottom);
        let margins = from.horizontal_margins();
        to.set_horizontal_margins(margins.left, margins.right);
    }

    /// Exit alternate screen for mode 47 — buffer swap only, no cursor
    /// restore, no clear.
    fn exit_alternate_screen_raw(&mut self) {
        if !self.modes.alternate_screen {
            return;
        }

        self.kitty_keyboard.switch_screen(false);
        // Clear hyperlink state on screen exit (#7414).
        self.transient.current_hyperlink = None;
        self.transient.current_hyperlink_id = None;
        self.transient.update_has_transient_extras();
        // Clear pending Sixel image — a Sixel rendered on alt should not
        // leak back to the main screen context (#7484).
        #[cfg(feature = "sixel")]
        {
            self.sixel.pending_image = None;
        }
        // Per xterm: tab stops are global. Preserve alt screen tab stop
        // changes back to the main screen (#7494).
        let tab_stops = self.grid.tab_stops().to_vec();
        if let Some(mut main_grid) = self.alt_grid.take() {
            // Per xterm the cursor is shared by both screen buffers: modes 47
            // and 1047 exit performs no cursor restore — the cursor stays
            // where the alt screen left it (only 1048/1049 save/restore).
            let cursor = self.grid.cursor();
            main_grid.set_cursor(cursor.row, cursor.col);
            main_grid.set_pending_wrap(self.grid.pending_wrap());
            // Margins are shared TScreen state in xterm: whatever DECSTBM/
            // DECSLRM set while in the alt screen stays in force after exit.
            Self::copy_margins(self.grid, &mut main_grid);
            // Keep the alternate buffer: it is persistent in xterm and mode
            // 47 exit does not clear it — a later re-entry shows it again.
            let alt = std::mem::replace(self.grid, main_grid);
            *self.alt_grid = Some(alt);
        }
        self.grid.restore_tab_stops(&tab_stops);
        self.modes.alternate_screen = false;
        // Invalidate selection — grid content changed completely.
        self.grid.force_selection_invalidation();
        if let Some(cb) = self.buffer_activation_callback {
            cb(false);
        }
    }

    /// Exit alternate screen for mode 1047 — clear alt screen before switching
    /// back. No cursor restore.
    fn exit_alternate_screen_1047(&mut self) {
        if !self.modes.alternate_screen {
            return;
        }

        // Clear the alt screen (current grid) before switching back.
        // Set BCE template so erased cells inherit SGR background (#7522).
        self.grid.set_cursor_template(
            crate::grid::Cell::bce_blank(self.style.cached_colors()),
            self.style.bce_bg_rgb(),
        );
        self.grid.erase_screen();
        // Per xterm 1047 reset is otherwise identical to mode 47: a buffer
        // swap with no cursor save/restore.
        self.exit_alternate_screen_raw();
    }

    /// Enter alternate screen for mode 1049 — save cursor + clear alt screen
    /// on enter.
    #[cfg_attr(
        test,
        aterm_spec::refines(machine = "terminal_modes", action = "SetAlternateScreen")
    )]
    fn enter_alternate_screen(&mut self) {
        if self.modes.alternate_screen {
            return;
        }

        // Save main cursor state, swap to fresh alt grid.
        // xterm srm_OPT_ALTBUF_CURSOR SET does CursorSave(xw) into the SAME
        // per-buffer slot DECSC uses (screen->sc[whichBuf]) — 1049's save IS
        // the DECSC save, observable by a later bare DECRC on the main
        // screen restoring the position 1049 saved.
        self.cursor_save.main = Some(self.snapshot_cursor_state());
        self.kitty_keyboard.switch_screen(true);
        // Clear pending Sixel image to prevent in-progress images from
        // leaking into the alt screen context (#7469).
        #[cfg(feature = "sixel")]
        {
            self.sixel.pending_image = None;
        }
        // Clear hyperlink state — hyperlinks should not leak across screen
        // boundaries (#7451).
        self.transient.current_hyperlink = None;
        self.transient.current_hyperlink_id = None;
        self.transient.update_has_transient_extras();
        // Alt screen has no scrollback per xterm spec — lines scrolled off the top
        // of the alternate buffer are discarded, not accumulated.
        let mut new_grid = Grid::with_scrollback(self.grid.rows(), self.grid.cols(), 0);
        // Honor background-color-erase (BCE): xterm's ClearScreen on 1049-enter
        // fills the alt screen with the CURRENT SGR background, just like every
        // other clear. A fresh Grid is default-bg blank, so set the BCE template
        // and erase to apply the active background (#7522 parity with the
        // 1047-exit clear in exit_alternate_screen_1047).
        new_grid.set_cursor_template(
            crate::grid::Cell::bce_blank(self.style.cached_colors()),
            self.style.bce_bg_rgb(),
        );
        new_grid.erase_screen();
        // Per xterm: tab stops are global, shared between main and alt screens.
        // Copy main screen tab stops to the new alt screen (#7494).
        new_grid.restore_tab_stops(self.grid.tab_stops());
        // Per xterm the cursor is shared by both screen buffers and 1049 SET
        // is CursorSave + ToAlternate + ClearScreen — none of which moves the
        // cursor (srm_OPT_ALTBUF_CURSOR). Entering must NOT home it.
        let cursor = self.grid.cursor();
        new_grid.set_cursor(cursor.row, cursor.col);
        new_grid.set_pending_wrap(self.grid.pending_wrap());
        // Margins are shared TScreen state in xterm — they persist onto the
        // alternate screen.
        Self::copy_margins(self.grid, &mut new_grid);
        let old_grid = std::mem::replace(self.grid, new_grid);
        *self.alt_grid = Some(old_grid);
        self.modes.alternate_screen = true;
        // Invalidate selection — grid content changed completely.
        self.grid.force_selection_invalidation();
        if let Some(cb) = self.buffer_activation_callback {
            cb(true);
        }
    }

    /// Exit alternate screen for mode 1049 — restore cursor on exit.
    #[cfg_attr(
        test,
        aterm_spec::refines(machine = "terminal_modes", action = "ResetAlternateScreen")
    )]
    fn exit_alternate_screen(&mut self) {
        if !self.modes.alternate_screen {
            return;
        }

        // Swap back to the main grid. xterm 1049 RESET (FromAlternate +
        // CursorRestore) does NOT save the alt-screen cursor anywhere.
        self.kitty_keyboard.switch_screen(false);
        // Clear pending Sixel image to prevent in-progress images from
        // leaking back to the main screen context (#7469).
        #[cfg(feature = "sixel")]
        {
            self.sixel.pending_image = None;
        }
        // Clear hyperlink state on screen exit (#7451).
        self.transient.current_hyperlink = None;
        self.transient.current_hyperlink_id = None;
        self.transient.update_has_transient_extras();
        // Per xterm: tab stops are global. Preserve alt screen tab stop
        // changes back to the main screen (#7494).
        let tab_stops = self.grid.tab_stops().to_vec();
        if let Some(mut main_grid) = self.alt_grid.take() {
            // Margins are shared TScreen state in xterm: whatever DECSTBM/
            // DECSLRM set while in the alt screen stays in force after exit.
            Self::copy_margins(self.grid, &mut main_grid);
            *self.grid = main_grid;
        }
        self.grid.restore_tab_stops(&tab_stops);
        // Restore from the shared DECSC slot WITHOUT consuming it: xterm
        // CursorRestore leaves sc->saved set, so a later bare DECRC restores
        // the same state again.
        let saved = self.cursor_save.main;
        self.restore_cursor_snapshot(saved);
        self.modes.alternate_screen = false;
        // Invalidate selection — grid content changed completely.
        self.grid.force_selection_invalidation();
        if let Some(cb) = self.buffer_activation_callback {
            cb(false);
        }
    }

    #[cfg_attr(
        test,
        aterm_spec::refines(machine = "terminal_modes", action = "SetBracketedPaste")
    )]
    fn enable_bracketed_paste(&mut self) {
        self.modes.bracketed_paste = true;
    }

    #[cfg_attr(
        test,
        aterm_spec::refines(machine = "terminal_modes", action = "ResetBracketedPaste")
    )]
    fn disable_bracketed_paste(&mut self) {
        self.modes.bracketed_paste = false;
    }

    #[cfg_attr(
        test,
        aterm_spec::refines(machine = "terminal_modes", action = "SetMouseMode")
    )]
    fn enable_mouse_x10_tracking(&mut self) {
        self.modes.mouse_mode = MouseMode::X10;
    }

    #[cfg_attr(
        test,
        aterm_spec::refines(machine = "terminal_modes", action = "SetMouseMode")
    )]
    fn enable_mouse_normal_tracking(&mut self) {
        self.modes.mouse_mode = MouseMode::Normal;
    }

    #[cfg_attr(
        test,
        aterm_spec::refines(machine = "terminal_modes", action = "SetMouseMode")
    )]
    fn enable_mouse_button_event_tracking(&mut self) {
        self.modes.mouse_mode = MouseMode::ButtonEvent;
    }

    #[cfg_attr(
        test,
        aterm_spec::refines(machine = "terminal_modes", action = "SetMouseMode")
    )]
    fn enable_mouse_any_event_tracking(&mut self) {
        self.modes.mouse_mode = MouseMode::AnyEvent;
    }

    #[cfg_attr(
        test,
        aterm_spec::refines(machine = "terminal_modes", action = "SetMouseMode")
    )]
    fn disable_mouse_tracking(&mut self) {
        self.modes.mouse_mode = MouseMode::None;
    }

    #[cfg_attr(
        test,
        aterm_spec::refines(machine = "terminal_modes", action = "SetSgrMouseEncoding")
    )]
    fn enable_sgr_mouse_encoding(&mut self) {
        self.modes.mouse_encoding = MouseEncoding::Sgr;
    }

    #[cfg_attr(
        test,
        aterm_spec::refines(machine = "terminal_modes", action = "ResetSgrMouseEncoding")
    )]
    fn disable_sgr_mouse_encoding(&mut self) {
        if self.modes.mouse_encoding == MouseEncoding::Sgr {
            self.modes.mouse_encoding = MouseEncoding::X10;
        }
    }

    /// Capture current cursor position, style, and mode flags.
    fn snapshot_cursor_state(&self) -> SavedCursorState {
        SavedCursorState {
            cursor: self.grid.cursor(),
            style: *self.style,
            origin_mode: self.modes.origin_mode,
            auto_wrap: self.modes.auto_wrap,
            charset: *self.charset,
            pending_wrap: self.grid.pending_wrap(),
            underline_color: self.transient.current_underline_color,
        }
    }

    /// Restore cursor position, style, and mode flags from a snapshot (if any).
    ///
    /// When origin mode is restored as enabled, the cursor row is clamped to
    /// the current scroll region — matching the DECRC behavior in
    /// `restore_cursor_state`. Without this, a cursor saved outside the
    /// scroll region would be placed outside it on restore, violating VT510.
    fn restore_cursor_snapshot(&mut self, state: Option<SavedCursorState>) {
        if let Some(state) = state {
            // Restore modes first so origin_mode is known for clamping.
            // DECAWM is deliberately NOT restored — 1049 exit restores "as in
            // DECRC", and xterm's CursorRestoreFlags only applies DECSC_FLAGS
            // = (ATTRIBUTES|ORIGIN|PROTECTED) (cursor.c): WRAPAROUND is never
            // part of the saved-cursor state.
            self.modes.origin_mode = state.origin_mode;
            *self.style = state.style;
            *self.charset = state.charset;
            self.transient.current_underline_color = state.underline_color;
            // Refresh the cached extras flag — the restored underline_color
            // may differ from the pre-restore value (#7311).
            self.transient.update_has_transient_extras();
            // Update BCE cursor template from restored style's background color.
            // Without this, the first scroll after mode 1049 exit uses the alt
            // screen's cursor template, producing wrong-colored blank lines.
            self.style.update_cached_colors();
            self.grid.set_cursor_template(
                crate::grid::Cell::bce_blank(self.style.cached_colors()),
                self.style.bce_bg_rgb(),
            );

            // Clamp cursor to scroll region when origin mode is active.
            // Per VT510: when DECOM is active, cursor is clamped to the scroll
            // region vertically and to horizontal margins when DECLRMM is active.
            let (row, col) = if state.origin_mode {
                let region = self.grid.scroll_region();
                let clamped_row = state.cursor.row.clamp(region.top, region.bottom);
                let clamped_col = if self.modes.left_right_margin_mode {
                    let margins = self.grid.horizontal_margins();
                    state.cursor.col.clamp(margins.left, margins.right)
                } else {
                    state.cursor.col
                };
                (clamped_row, clamped_col)
            } else {
                (state.cursor.row, state.cursor.col)
            };
            self.grid.set_cursor(row, col);
            // Restore pending_wrap after set_cursor (which clears it) (#7283).
            // Only restore if cursor is still at the right edge — after terminal
            // resize the saved position may no longer be at the margin, making
            // a deferred wrap invalid (#7645).
            if state.pending_wrap {
                let max_col = self.grid.effective_cols_for_row(row).saturating_sub(1);
                self.grid.set_pending_wrap(col >= max_col);
            }
            // Update BCE cursor template from restored style so that the first
            // scroll after returning to the main screen uses the correct
            // background colors (#7658).
            self.style.update_cached_colors();
            self.grid.set_cursor_template(
                crate::grid::Cell::bce_blank(self.style.cached_colors()),
                self.style.bce_bg_rgb(),
            );
        }
    }

    /// Handle DECRQM - DEC Request Mode.
    ///
    /// CSI ? Ps $ p - Request DEC private mode state.
    /// Response: CSI ? Ps ; Pm $ y
    /// Where Pm is:
    ///   0 - Not recognized (mode not known)
    ///   1 - Set (mode is enabled)
    ///   2 - Reset (mode is disabled)
    ///   3 - Permanently set
    ///   4 - Permanently reset
    pub(super) fn handle_decrqm(
        &mut self,
        cap: &super::response_capability::ResponseCapability,
        params: &[u16],
    ) {
        let mode = params.first().copied().unwrap_or(0);

        // DECRQM state values: 1=set, 2=reset, 0=unknown
        #[inline]
        fn state(active: bool) -> u8 {
            if active { 1 } else { 2 }
        }

        let mode_state: u8 = match mode {
            1 => state(self.modes.application_cursor_keys),
            2 => state(!self.modes.vt52_mode), // Inverted: set=ANSI, reset=VT52
            3 => state(self.modes.column_mode_132),
            5 => state(self.modes.reverse_video),
            6 => state(self.modes.origin_mode),
            7 => state(self.modes.auto_wrap),
            12 => state(self.modes.cursor_blink),
            25 => state(self.modes.cursor_visible),
            40 => state(self.modes.deccolm_enable),
            45 => state(self.modes.reverse_wraparound),
            66 => state(self.modes.application_keypad),
            69 => state(self.modes.left_right_margin_mode),
            80 => state(self.modes.sixel_display_mode),
            95 => state(self.modes.decncsm),
            9 => state(self.modes.mouse_mode == MouseMode::X10),
            1000 => state(self.modes.mouse_mode == MouseMode::Normal),
            1002 => state(self.modes.mouse_mode == MouseMode::ButtonEvent),
            1003 => state(self.modes.mouse_mode == MouseMode::AnyEvent),
            1004 => state(self.modes.focus_reporting),
            1007 => state(self.modes.alternate_scroll),
            1005 => state(self.modes.mouse_encoding == MouseEncoding::Utf8),
            1006 => state(self.modes.mouse_encoding == MouseEncoding::Sgr),
            1015 => state(self.modes.mouse_encoding == MouseEncoding::Urxvt),
            1016 => state(self.modes.mouse_encoding == MouseEncoding::SgrPixel),
            47 | 1047 | 1049 => state(self.modes.alternate_screen),
            1048 => {
                // Mode 1048 is an action (save/restore cursor), not a tracked mode.
                // Report whether a cursor state has been saved for the current screen.
                let saved = if self.modes.alternate_screen {
                    self.cursor_save.alt.is_some()
                } else {
                    self.cursor_save.main.is_some()
                };
                state(saved)
            }
            1243 => state(self.modes.bidi_arrow_swap),
            2004 => state(self.modes.bracketed_paste),
            2026 => state(self.modes.synchronized_output),
            2027 => state(self.modes.grapheme_cluster_mode),
            // 2048 (in-band resize notifications) is unimplemented and must
            // fall through to Pm=0 (not recognized): kitty keyboard flags are
            // queried via CSI ? u, not DECRQM, and claiming 2048 is set/reset
            // without ever emitting resize reports breaks neovim 0.10+.
            2500 => state(self.modes.bidi_box_mirroring),
            2501 => state(self.modes.bidi_autodetection),
            // Recognized but permanently reset: modes with no effect in a modern
            // terminal emulator.  Pm=4 is more spec-correct than Pm=0 (unknown).
            4 | 8 => 4,   // DECSCLM (smooth scroll), DECARM (auto repeat) — OS-managed
            18 | 19 => 4, // DECPFF (print form feed), DECPEX (print extent) — no printer
            _ => 0,       // Unknown mode
        };

        // Send response: CSI ? <mode> ; <state> $ y
        let response = format!("\x1b[?{mode};{mode_state}$y");
        self.send_response(cap, response.as_bytes());
    }

    /// Handle ANSI DECRQM — Request Mode (non-private).
    ///
    /// CSI Ps $ p — Report current state of an ANSI (non-DEC-private) mode.
    /// Response: CSI Ps ; Pm $ y  (no `?` prefix — this is ANSI, not DEC)
    /// Where Pm is: 0=not recognized, 1=set, 2=reset, 3=perm set, 4=perm reset.
    pub(super) fn handle_ansi_decrqm(
        &mut self,
        cap: &super::response_capability::ResponseCapability,
        params: &[u16],
    ) {
        let mode = params.first().copied().unwrap_or(0);

        #[inline]
        fn state(active: bool) -> u8 {
            if active { 1 } else { 2 }
        }

        let mode_state: u8 = match mode {
            4 => state(self.modes.insert_mode),
            8 => {
                // BDSM — set means implicit BiDi (the default)
                state(self.modes.bidi_mode == crate::config::BiDiMode::Implicit)
            }
            20 => state(self.modes.new_line_mode),
            // SRM (Send/Receive Mode) — local echo control, always full-duplex
            // (permanently reset) in a modern terminal.  Pm=4 is more spec-correct
            // than Pm=0 (unknown).
            12 => 4,
            _ => 0, // Unknown ANSI mode
        };

        // Response format: CSI <mode> ; <state> $ y
        let response = format!("\x1b[{mode};{mode_state}$y");
        self.send_response(cap, response.as_bytes());
    }

    /// Handle ANSI mode set/reset.
    ///
    /// CSI Ps h - Set Mode
    /// CSI Ps l - Reset Mode
    ///
    /// Standard ANSI modes:
    /// - 4: Insert Mode (IRM) - when set, characters shift existing text right
    /// - 8: BDSM (BiDi Mode) - when set, implicit BiDi; when reset, explicit BiDi
    /// - 20: Line Feed/New Line Mode (LNM) - when set, LF also does CR
    ///
    /// See Terminal WG BiDi spec: <https://terminal-wg.pages.freedesktop.org/bidi/>
    pub(super) fn handle_ansi_mode(&mut self, params: &[u16], set: bool) {
        for &param in params {
            match param {
                4 => {
                    if set {
                        self.enable_insert_mode();
                    } else {
                        self.disable_insert_mode();
                    }
                }
                8 => {
                    // BDSM - Bidirectional Support Mode
                    self.set_bidi_support_mode(set);
                }
                20 => {
                    if set {
                        self.enable_new_line_mode();
                    } else {
                        self.disable_new_line_mode();
                    }
                }
                _ => {} // Unknown ANSI mode
            }
        }
    }

    /// Parse rectangular area coordinates (Pt;Pl;Pb;Pr) for VT420 rect ops.
    ///
    /// Handles 1-indexed to 0-indexed conversion, default-value resolution,
    /// and DECOM (origin mode) offset. Per DEC STD 070 Section 5.5.2,
    /// when DECOM is set, coordinates are relative to the scroll region
    /// and horizontal margins.
    ///
    /// Returns `Some((top, left, bottom, right))` with 0-indexed absolute
    /// coordinates, or `None` if the rectangle is invalid (top > bottom
    /// or left > right).
    fn parse_rect_coords(&self, params: &[u16]) -> Option<(u16, u16, u16, u16)> {
        let rows = self.grid.rows();
        let cols = self.grid.cols();

        // When DECOM is active, coordinates are relative to the scroll region
        // (and horizontal margins when DECLRMM is active).
        let (row_offset, row_limit, col_offset, col_limit) = if self.modes.origin_mode {
            let region = self.grid.scroll_region();
            let margins = self.grid.horizontal_margins();
            (
                region.top,
                region.bottom + 1,
                margins.left,
                margins.right + 1,
            )
        } else {
            (0, rows, 0, cols)
        };

        let row_extent = row_limit - row_offset;
        let col_extent = col_limit - col_offset;

        // Parse parameters (1-indexed, convert to 0-indexed within origin)
        let top = params
            .first()
            .copied()
            .unwrap_or(1)
            .max(1)
            .saturating_sub(1)
            .min(row_extent.saturating_sub(1))
            + row_offset;
        let left = params
            .get(1)
            .copied()
            .unwrap_or(1)
            .max(1)
            .saturating_sub(1)
            .min(col_extent.saturating_sub(1))
            + col_offset;
        let bottom = params
            .get(2)
            .copied()
            .map(|p| if p == 0 { row_extent } else { p })
            .unwrap_or(row_extent)
            .saturating_sub(1)
            .min(row_extent.saturating_sub(1))
            + row_offset;
        let right = params
            .get(3)
            .copied()
            .map(|p| if p == 0 { col_extent } else { p })
            .unwrap_or(col_extent)
            .saturating_sub(1)
            .min(col_extent.saturating_sub(1))
            + col_offset;

        if top > bottom || left > right {
            None
        } else {
            Some((top, left, bottom, right))
        }
    }

    /// Handle DECERA - Erase Rectangular Area (VT420+).
    ///
    /// CSI Pt ; Pl ; Pb ; Pr $ z
    ///
    /// Erases all characters in the rectangular area defined by:
    /// - Pt: top row (1-indexed, default: 1)
    /// - Pl: left column (1-indexed, default: 1)
    /// - Pb: bottom row (1-indexed, default: number of rows)
    /// - Pr: right column (1-indexed, default: number of columns)
    ///
    /// Per VT420 spec, parameter 0 is treated as the default value.
    /// When DECOM is active, coordinates are relative to scroll region.
    /// The erase fills cells with spaces using default attributes.
    pub(super) fn handle_decera(&mut self, params: &[u16]) {
        let Some((top, left, bottom, right)) = self.parse_rect_coords(params) else {
            return;
        };

        // Erase the rectangular area
        self.grid.erase_rect(top, left, bottom, right);
    }

    /// Handle DECCARA - Change Attributes in Rectangular Area (VT420+).
    ///
    /// CSI Pt ; Pl ; Pb ; Pr ; Ps... $ r
    ///
    /// Applies SGR attributes (Ps parameters) to all characters in the
    /// rectangular area defined by:
    /// - Pt: top row (1-indexed, default: 1)
    /// - Pl: left column (1-indexed, default: 1)
    /// - Pb: bottom row (1-indexed, default: number of rows)
    /// - Pr: right column (1-indexed, default: number of columns)
    /// - Ps...: SGR attribute parameters (from params[4..])
    ///
    /// Per VT420 spec, parameter 0 is treated as the default value.
    /// Only a subset of SGR attributes are supported: bold, dim, italic,
    /// underline, blink, inverse, hidden, strikethrough, and their resets.
    ///
    /// When DECSACE stream mode is active (`stream_attribute_extent`), the
    /// operation covers a contiguous character stream from (top,left) to
    /// (bottom,right) instead of a rectangle.
    pub(super) fn handle_deccara(&mut self, params: &[u16]) {
        let Some((top, left, bottom, right)) = self.parse_rect_coords(params) else {
            return;
        };

        // Parse SGR parameters from params[4..]
        let sgr_params = if params.len() > 4 {
            &params[4..]
        } else {
            &[0u16][..]
        };
        let (flags_to_set, flags_to_clear) = Self::sgr_params_to_flags(sgr_params);

        if self.modes.stream_attribute_extent {
            self.grid
                .change_attrs_stream(top, left, bottom, right, flags_to_set, flags_to_clear);
        } else {
            self.grid
                .change_attrs_rect(top, left, bottom, right, flags_to_set, flags_to_clear);
        }
    }

    /// Handle DECCRA - Copy Rectangular Area (VT420+).
    ///
    /// CSI Pts ; Pls ; Pbs ; Prs ; Pps ; Ptd ; Pld ; Ppd $ v
    ///
    /// Copies the rectangular area from source page to destination:
    /// - Pts: source top row (1-indexed, default: 1)
    /// - Pls: source left column (1-indexed, default: 1)
    /// - Pbs: source bottom row (1-indexed, default: number of rows)
    /// - Prs: source right column (1-indexed, default: number of columns)
    /// - Pps: source page (ignored - single page)
    /// - Ptd: destination top row (1-indexed, default: 1)
    /// - Pld: destination left column (1-indexed, default: 1)
    /// - Ppd: destination page (ignored - single page)
    ///
    /// Per VT420 spec, parameter 0 is treated as the default value.
    pub(super) fn handle_deccra(&mut self, params: &[u16]) {
        // Parse source rectangle (params[0..4]) with DECOM support
        let Some((src_top, src_left, src_bottom, src_right)) = self.parse_rect_coords(params)
        else {
            return;
        };
        // params[4] = source page (ignored)

        // Parse destination coordinates (params[5..6]) with DECOM offset
        let dst_params = if params.len() > 5 {
            &params[5..]
        } else {
            &[1u16][..]
        };
        // Reuse parse_rect_coords for destination: only top-left matters,
        // bottom-right are derived from source rectangle dimensions.
        let (row_offset, col_offset) = if self.modes.origin_mode {
            let region = self.grid.scroll_region();
            let margins = self.grid.horizontal_margins();
            (region.top, margins.left)
        } else {
            (0, 0)
        };
        let rows = self.grid.rows();
        let cols = self.grid.cols();
        let dst_top = dst_params
            .first()
            .copied()
            .unwrap_or(1)
            .max(1)
            .saturating_sub(1)
            .min(rows.saturating_sub(1).saturating_sub(row_offset))
            + row_offset;
        let dst_left = dst_params
            .get(1)
            .copied()
            .unwrap_or(1)
            .max(1)
            .saturating_sub(1)
            .min(cols.saturating_sub(1).saturating_sub(col_offset))
            + col_offset;
        // params[7] = destination page (ignored)

        self.grid
            .copy_rect(src_top, src_left, src_bottom, src_right, dst_top, dst_left);
    }

    /// Convert a slice of SGR parameter values into (flags_to_set, flags_to_clear).
    ///
    /// Processes the SGR subset relevant to DECCARA:
    /// bold, dim, italic, underline, blink, inverse, hidden, strikethrough,
    /// and their corresponding reset codes.
    fn sgr_params_to_flags(sgr_params: &[u16]) -> (CellFlags, CellFlags) {
        let mut flags_to_set = CellFlags::empty();
        let mut flags_to_clear = CellFlags::empty();

        for &param in sgr_params {
            match param {
                0 => {
                    // SGR 0 = reset all SGR attributes.
                    // Must NOT clear structural WIDE (bit 9) or WIDE_CONTINUATION (bit 10)
                    // flags — those track cell geometry, not visual attributes.
                    // VISUAL_FLAGS_MASK (0x3FFF) includes those bits; use the
                    // narrower SGR_ATTRIBUTE_MASK that excludes them.
                    flags_to_clear = CellFlags::from_bits(CellFlags::VISUAL_FLAGS_MASK & !0x0600);
                    flags_to_set = CellFlags::empty();
                }
                1 => flags_to_set = flags_to_set.union(CellFlags::BOLD),
                2 => flags_to_set = flags_to_set.union(CellFlags::DIM),
                3 => flags_to_set = flags_to_set.union(CellFlags::ITALIC),
                4 => {
                    // SGR 4 = single underline — must clear other underline
                    // styles first, matching apply_sgr_param behavior (#7464).
                    flags_to_clear = flags_to_clear.union(CellFlags::ALL_UNDERLINES);
                    flags_to_set = flags_to_set.union(CellFlags::UNDERLINE);
                }
                5 | 6 => flags_to_set = flags_to_set.union(CellFlags::BLINK),
                7 => flags_to_set = flags_to_set.union(CellFlags::INVERSE),
                8 => flags_to_set = flags_to_set.union(CellFlags::HIDDEN),
                9 => flags_to_set = flags_to_set.union(CellFlags::STRIKETHROUGH),
                22 => {
                    flags_to_clear = flags_to_clear.union(CellFlags::BOLD);
                    flags_to_clear = flags_to_clear.union(CellFlags::DIM);
                }
                23 => flags_to_clear = flags_to_clear.union(CellFlags::ITALIC),
                24 => {
                    // SGR 24 = remove underline — must clear ALL underline
                    // styles, matching apply_sgr_param behavior (#7464).
                    flags_to_clear = flags_to_clear.union(CellFlags::ALL_UNDERLINES);
                }
                25 => flags_to_clear = flags_to_clear.union(CellFlags::BLINK),
                27 => flags_to_clear = flags_to_clear.union(CellFlags::INVERSE),
                28 => flags_to_clear = flags_to_clear.union(CellFlags::HIDDEN),
                29 => flags_to_clear = flags_to_clear.union(CellFlags::STRIKETHROUGH),
                _ => {} // Other SGR codes are not applicable to DECCARA
            }
        }

        (flags_to_set, flags_to_clear)
    }

    /// Handle DECFRA - Fill Rectangular Area (VT420+).
    ///
    /// CSI Pch ; Pt ; Pl ; Pb ; Pr $ x
    ///
    /// Fills the rectangular area with character Pch:
    /// - Pch: character code to fill (default: none / no-op)
    /// - Pt: top row (1-indexed, default: 1)
    /// - Pl: left column (1-indexed, default: 1)
    /// - Pb: bottom row (1-indexed, default: number of rows)
    /// - Pr: right column (1-indexed, default: number of columns)
    ///
    /// Per VT420 spec, only printable characters (0x20..=0x7E and 0xA0..=0xFF)
    /// are accepted. Non-printable character codes are silently ignored.
    /// Parameter 0 is treated as the default value for coordinates.
    pub(super) fn handle_decfra(&mut self, params: &[u16]) {
        // First parameter is the character code
        let ch_code = params.first().copied().unwrap_or(0);

        // Per VT420 spec, only printable characters are accepted
        let printable = matches!(ch_code, 0x20..=0x7E | 0xA0..=0xFF);
        if !printable {
            return;
        }

        // Construct fill cell with current SGR attributes (per VT420 spec,
        // DECFRA fills with the specified character using current video attrs).
        // ch_code is in 0x20..=0x7E or 0xA0..=0xFF (validated above), fits in u16.
        let colors = self.style.cached_colors();
        let flags = if self.style.protected {
            self.style.flags.union(CellFlags::PROTECTED)
        } else {
            self.style.flags
        };
        let fill = crate::grid::Cell::from_raw_parts(ch_code, colors, flags);

        // Parse rectangle coordinates from params[1..5] with DECOM support.
        let rect_params = if params.len() > 1 { &params[1..] } else { &[] };
        let Some((top, left, bottom, right)) = self.parse_rect_coords(rect_params) else {
            return;
        };

        self.grid.fill_rect(fill, top, left, bottom, right);
    }

    /// Handle DECSERA - Selective Erase Rectangular Area (VT420+).
    ///
    /// CSI Pt ; Pl ; Pb ; Pr $ {
    ///
    /// Erases characters in the rectangular area that are NOT protected by DECSCA:
    /// - Pt: top row (1-indexed, default: 1)
    /// - Pl: left column (1-indexed, default: 1)
    /// - Pb: bottom row (1-indexed, default: number of rows)
    /// - Pr: right column (1-indexed, default: number of columns)
    ///
    /// Per VT420 spec, parameter 0 is treated as the default value.
    /// Protected cells (set via DECSCA) are preserved.
    pub(super) fn handle_decsera(&mut self, params: &[u16]) {
        let Some((top, left, bottom, right)) = self.parse_rect_coords(params) else {
            return;
        };

        self.grid.selective_erase_rect(top, left, bottom, right);
    }
}

include!("handler_dec_refinement.rs");
