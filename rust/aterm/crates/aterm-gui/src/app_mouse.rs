// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! Pointer, selection, hover, and link handling: the mouse/cursor event handlers
//! plus the selection gesture state machine (click streaks, word/line/block
//! select, drag, copy), pane-under-pointer focus, pixel→cell mapping, and the
//! scroll snap. A verbatim inherent-impl split of `App`.

use std::time::Instant;

use aterm_core::selection::{SelectionSide, SelectionType};
use winit::event::{ElementState, MouseButton as WinitMouseButton, MouseScrollDelta};
use winit::window::CursorIcon;

use crate::app_render::{pixel_to_term_cell, strip_col_for_pixel};
use crate::input::{InputEvent, Source};
use crate::{
    App, GestureOrigin, MULTI_CLICK_MS, WindowId, control, is_safe_url, pane, plain_url_at,
    term_lock,
};

/// Map a winit mouse button to the engine's [`aterm_types::mouse::MouseButton`]
/// for an [`InputEvent::MouseButton`]. `None` for buttons the GUI does not report
/// (Back/Forward/Other), so the handler can early-return.
pub(crate) fn winit_mouse_button(b: WinitMouseButton) -> Option<aterm_types::mouse::MouseButton> {
    use aterm_types::mouse::MouseButton;
    match b {
        WinitMouseButton::Left => Some(MouseButton::Left),
        WinitMouseButton::Middle => Some(MouseButton::Middle),
        WinitMouseButton::Right => Some(MouseButton::Right),
        _ => None,
    }
}

impl App {
    /// The FOCUSED pane's top-left `(row_off, col_off)` cell offset in window
    /// `wid`'s grid. `(0, 0)` when the focused pane fills the window (no splits) — so
    /// subtracting it from a window mouse cell is a no-op on the single-pane path,
    /// keeping mouse handling byte-identical. Used to translate window mouse coords
    /// into the focused pane's local grid (its engine expects pane-local cells).
    pub(crate) fn focused_pane_origin(&self, wid: WindowId) -> (u16, u16) {
        let Some(ws) = self.windows.get(&wid) else {
            return (0, 0);
        };
        let tree = &ws.layouts[ws.tabs.active];
        // Fast path: a single-pane tab's focused pane fills the window at the
        // origin — no layout walk on the mouse-move hot path.
        if tree.len() == 1 {
            return (0, 0);
        }
        let focus = tree.focus();
        tree.compute_layout(ws.rows, ws.cols)
            .into_iter()
            .find(|r| r.session == focus)
            .map_or((0, 0), |r| (r.row_off, r.col_off))
    }

    /// Click-to-focus in window `wid`: if its last pointer position (window cell)
    /// lands on a pane OTHER than the focused one, move focus there (re-mirroring the
    /// control socket + renderer onto it) and re-derive the pane-local mouse cell.
    /// Returns `true` iff focus moved (the caller then swallows the press). A press
    /// in the already-focused pane, on a divider, or in a single-pane tab returns
    /// `false` (the press proceeds to the normal selection/tracking path).
    pub(crate) fn focus_pane_under_pointer(&mut self, wid: WindowId) -> bool {
        let Some(ws) = self.windows.get(&wid) else {
            return false;
        };
        let tree = &ws.layouts[ws.tabs.active];
        // Single-pane tab: there is nothing else to focus (the press proceeds to the
        // normal selection/tracking path), and no layout walk.
        if tree.len() == 1 {
            return false;
        }
        let (wr, wc) = ws.last_mouse_window_cell;
        let (rows, cols) = (ws.rows, ws.cols);
        let Some(hit) = tree.pane_at(wr, wc, rows, cols) else {
            return false; // divider / outside grid: nothing to focus
        };
        if hit == tree.focus() {
            return false; // already focused: proceed with the normal press
        }
        let moved = self.active_tree_mut(wid).is_some_and(|t| t.set_focus(hit));
        if !moved {
            return false;
        }
        // Re-derive the pane-local mouse cell for the newly-focused pane so any
        // follow-up gesture uses its grid; re-mirror term/master/socket onto it.
        let (ro, co) = self.focused_pane_origin(wid);
        if let Some(ws) = self.windows.get_mut(&wid) {
            ws.last_mouse_cell = (wr.saturating_sub(ro), wc.saturating_sub(co));
        }
        self.sync_window(wid);
        true
    }

    /// If window `wid`'s last pointer position lands on a pane DIVIDER, arm a
    /// divider-resize drag on it and return `true` (the caller then swallows the
    /// press — it neither focuses a pane nor starts a selection). Returns `false`
    /// for a press inside a pane / a single-pane (or zoomed) tab, so the normal
    /// press path proceeds. The armed [`pane::DividerHit`] is held in
    /// `ws.divider_drag` until release; `drag_divider` consumes it on each move.
    pub(crate) fn begin_divider_drag(&mut self, wid: WindowId) -> bool {
        let Some(ws) = self.windows.get(&wid) else {
            return false;
        };
        let tree = &ws.layouts[ws.tabs.active];
        if tree.len() == 1 {
            return false; // no divider to grab
        }
        let (wr, wc) = ws.last_mouse_window_cell;
        let (rows, cols) = (ws.rows, ws.cols);
        let Some(hit) = tree.divider_at(wr, wc, rows, cols) else {
            return false; // not on a divider
        };
        // Resize-cursor affordance for the drag: E-W for a vertical divider (columns
        // move), N-S for a horizontal one (rows move).
        let icon = match hit.dir {
            pane::SplitDir::Vertical => CursorIcon::ColResize,
            pane::SplitDir::Horizontal => CursorIcon::RowResize,
        };
        if let Some(ws) = self.windows.get_mut(&wid) {
            ws.divider_drag = Some(hit);
            if let Some(w) = &ws.os_window {
                w.set_cursor(icon);
            }
        }
        true
    }

    /// Mid-drag of a pane divider: map the current pointer (window cell) to the held
    /// divider's split ratio and apply it, then relay out (resize every pane's
    /// engine/PTY) and repaint. A no-op when no divider is being dragged. The ratio
    /// is clamped inside [`pane::PaneTree::set_divider_ratio`], so a drag past either
    /// edge just pins the boundary at the `[MIN_RATIO, MAX_RATIO]` floor/ceiling.
    pub(crate) fn drag_divider(&mut self, wid: WindowId) {
        let Some(ws) = self.windows.get(&wid) else {
            return;
        };
        let Some(hit) = ws.divider_drag.clone() else {
            return;
        };
        let (wr, wc) = ws.last_mouse_window_cell;
        let tree = &ws.layouts[ws.tabs.active];
        let Some(ratio) = tree.ratio_for_pointer(&hit, wr, wc) else {
            return;
        };
        let applied = self
            .active_tree_mut(wid)
            .is_some_and(|t| t.set_divider_ratio(&hit, ratio));
        if !applied {
            return;
        }
        // Resize every pane's engine/PTY to its new sub-rect, then repaint the frame.
        self.resize_panes(wid);
        if let Some(w) = self.windows.get(&wid).and_then(|ws| ws.os_window.as_ref()) {
            w.request_redraw();
        }
    }

    /// End any in-flight divider drag (left release). Returns whether a drag was
    /// active (the caller then swallows the release rather than completing a
    /// selection that never started).
    pub(crate) fn finish_divider_drag(&mut self, wid: WindowId) -> bool {
        match self.windows.get_mut(&wid) {
            Some(ws) if ws.divider_drag.is_some() => {
                ws.divider_drag = None;
                // Restore the default cursor (the hover state machine re-asserts the
                // link pointer on the next move if warranted).
                ws.hover_pointer = false;
                if let Some(w) = &ws.os_window {
                    w.set_cursor(CursorIcon::Default);
                }
                true
            }
            _ => false,
        }
    }

    /// Cmd-C: copy the selected text to the macOS system clipboard (`pbcopy`).
    /// Returns whether anything was copied; the selection is NOT cleared (so a
    /// highlight survives the copy, and repeated copies work).
    pub(crate) fn copy_selection(&self) -> bool {
        let Some(ws) = self.front() else {
            return false;
        };
        let Some(text) = term_lock(&ws.term).selection_to_string() else {
            return false;
        };
        !text.is_empty() && control::pbcopy(&text)
    }

    /// Clear any active selection (the standard "typing deselects" behavior)
    /// and repaint so the highlight disappears. No-op when nothing is selected.
    pub(crate) fn clear_selection(&mut self, wid: WindowId) {
        let Some(ws) = self.windows.get(&wid) else {
            return;
        };
        let cleared = {
            let mut term = term_lock(&ws.term);
            if term.text_selection().has_selection() {
                term.text_selection_mut().clear();
                true
            } else {
                false
            }
        };
        if cleared && let Some(w) = &ws.os_window {
            w.request_redraw();
        }
    }

    /// Map a pixel position to a 0-based (row, col) TERMINAL grid cell of window
    /// `wid`, clamped to the grid. Two insets are stripped first (see
    /// [`pixel_to_term_cell`]): the interior `pad` border around the whole window,
    /// then the `tab_strip_rows` pixel rows of the strip — so a click in the
    /// terminal region lands on the right terminal row, and a click in the strip/pad
    /// border clamps to terminal row 0 (the caller intercepts strip clicks via
    /// [`Self::strip_col_at`] BEFORE using this). With `pad == 0` && `tab_strip_rows
    /// == 0` this is byte-identical to the pre-strip mapping.
    pub(crate) fn pixel_to_cell(&self, wid: WindowId, x: f64, y: f64) -> (u16, u16) {
        let (cw, ch) = self.cell_size();
        let pad = self.backend.pad();
        let (rows, cols) = self
            .windows
            .get(&wid)
            .map_or((0, 0), |ws| (ws.rows, ws.cols));
        pixel_to_term_cell(x, y, cw, ch, rows, cols, self.tab_strip_rows, pad)
    }

    /// If pixel position `(x, y)` lands in window `wid`'s tab-strip region (the top
    /// `tab_strip_rows` pixel rows), return its strip COLUMN; otherwise `None` (the
    /// click is in the terminal region and maps to a cell as usual). Always `None`
    /// when the strip is disabled. Used by the mouse handlers to intercept strip
    /// clicks BEFORE the focused-pane cell mapping.
    pub(crate) fn strip_col_at(&self, wid: WindowId, x: f64, y: f64) -> Option<u16> {
        if !self.tab_strip_enabled() {
            return None;
        }
        let (cw, ch) = self.cell_size();
        let pad = self.backend.pad();
        let cols = self.windows.get(&wid).map_or(0, |ws| ws.cols);
        strip_col_for_pixel(x, y, cw, ch, cols, self.tab_strip_rows, pad)
    }

    /// `CursorMoved` -> remember the cell under the pointer; mid-drag, grow the
    /// text selection to that cell (and, when motion tracking is on, report the
    /// move to the app instead).
    /// Show the "pointer" cursor while Cmd-hovering a link, else the default. Only
    /// touches the OS cursor on a state CHANGE (not every mouse move). Updated on
    /// both pointer motion and Cmd press/release so the affordance tracks the key.
    pub(crate) fn update_hover_cursor(&mut self, wid: WindowId) {
        let super_held = self.windows.get(&wid).is_some_and(|ws| ws.mods.super_key());
        let over_link = super_held && self.link_under_pointer(wid).is_some();
        let Some(ws) = self.windows.get_mut(&wid) else {
            return;
        };
        if over_link != ws.hover_pointer {
            ws.hover_pointer = over_link;
            if let Some(w) = &ws.os_window {
                w.set_cursor(if over_link {
                    CursorIcon::Pointer
                } else {
                    CursorIcon::Default
                });
            }
        }
    }

    pub(crate) fn on_cursor_moved(&mut self, wid: WindowId, x: f64, y: f64) {
        // Remember the raw pixel position so a follow-up button press can tell
        // whether it landed in the tab strip (intercepted before cell mapping).
        if let Some(ws) = self.windows.get_mut(&wid) {
            ws.last_cursor_px = (x, y);
        }
        // While the pointer is over the tab strip, it is NOT over the terminal grid:
        // show the default cursor and do not report a mouse-move to any pane's app
        // (the strip is GUI chrome). A no-op when the strip is disabled.
        if self.strip_col_at(wid, x, y).is_some() {
            if let Some(ws) = self.windows.get_mut(&wid)
                && ws.hover_pointer
            {
                ws.hover_pointer = false;
                if let Some(w) = &ws.os_window {
                    w.set_cursor(CursorIcon::Default);
                }
            }
            return;
        }
        let (row, col) = self.pixel_to_cell(wid, x, y);
        // The FOCUSED-PANE-LOCAL cell (window cell minus the focused pane's offset)
        // so the focused pane's selection/tracking math sees its own grid. With no
        // splits the offset is (0,0) → byte-identical.
        let (ro, co) = self.focused_pane_origin(wid);
        if let Some(ws) = self.windows.get_mut(&wid) {
            // Remember the raw WINDOW cell (for click-to-focus hit-testing) AND the
            // pane-local cell (for PTY mouse reports).
            ws.last_mouse_window_cell = (row, col);
            ws.last_mouse_cell = (row.saturating_sub(ro), col.saturating_sub(co));
        }
        // SPLIT-PANE DIVIDER DRAG: while a divider is held, motion resizes the split
        // (relayout + repaint) and short-circuits the selection / mouse-report path —
        // the drag is GUI chrome, not terminal input. A no-op when none is held.
        if self
            .windows
            .get(&wid)
            .is_some_and(|ws| ws.divider_drag.is_some())
        {
            self.drag_divider(wid);
            return;
        }
        self.update_hover_cursor(wid);
        // Which half of the cell the pointer is in: the right half includes
        // the hovered cell, the left half stops before it. Remembered so a
        // shift-click press (which has no pixel position of its own) can
        // anchor by the half that was pressed. Subtract the `pad` inset first so
        // the half-split lines up with the (padded) cell, matching `pixel_to_cell`.
        let (cw, ch) = self.cell_size();
        let cw = cw.max(1);
        let ch = ch.max(1);
        let gx = (x - self.backend.pad() as f64).max(0.0) as usize;
        let side = if (gx % cw) * 2 >= cw {
            SelectionSide::Right
        } else {
            SelectionSide::Left
        };
        // Sub-cell pixel offset of the pointer inside its cell, measured from the
        // real winit cursor (pad- and strip-stripped) so a DEC 1016 (SGR-pixel)
        // report carries a GENUINE sub-cell coordinate, not a cell-origin one. The
        // strip occupies the top rows, so subtract its pixel height from `y` before
        // taking the per-cell remainder (matches `pixel_to_term_cell`). Ignored by
        // every cell-coordinate encoding — see [`crate::input::PixelOffset`].
        let strip_px = self.tab_strip_rows as usize * ch;
        let gy = (y - self.backend.pad() as f64).max(0.0) as usize;
        let gy = gy.saturating_sub(strip_px);
        let px_off = crate::input::PixelOffset {
            x: (gx % cw) as u16,
            y: (gy % ch) as u16,
        };
        // Phase 0.5: the cell-half (`side`) is GUI-derived (it needs the pixel x),
        // then handed to the seam as DATA. The seam runs the `self.selecting` local
        // drag and the tracking-ON motion report under ONE mode read. `buttons == 3`
        // is the no-button hover code (kills c: a controller drag arrives as
        // `MouseMove { buttons != 3 }` in a batch). The seam also updates
        // last_mouse_cell/last_mouse_side, so both sources keep that state in sync.
        let mods = self.mouse_modifiers(wid);
        // The X10 button code of the held button (Left=0/Middle=1/Right=2), or `3`
        // (no button held) for a true hover. `encode_mouse_motion` ORs in the 32
        // motion bit, so a drag in 1002/1003 reports the held button correctly and
        // a button-less hover still reports 3 (which 1002 drops, as it should).
        let buttons = self
            .windows
            .get(&wid)
            .and_then(|ws| ws.held_mouse_button)
            .map_or(3u8, |b| b.code());
        // Remember the sub-cell offset so a follow-up button press / wheel notch
        // (winit delivers no pixel position on those) reports the same pixel the
        // pointer last hovered, under DEC 1016.
        if let Some(ws) = self.windows.get_mut(&wid) {
            ws.last_mouse_px_off = px_off;
        }
        // SELECTION AUTOSCROLL: a drag dragged past the top/bottom grid edge scrolls
        // the scrollback so the selection extends into off-screen content. Done
        // BEFORE the MouseMove drag below so `drag_selection` (which maps the row
        // through the now-updated `display_offset`) grows the selection to the
        // freshly-revealed edge row. A no-op when no selection drag is active or the
        // pointer is inside the grid. `row`/`col` are already clamped to the grid by
        // `pixel_to_cell`, so the edge row is 0 (top) or rows-1 (bottom).
        self.selection_autoscroll(wid, y);
        self.input(
            wid,
            InputEvent::MouseMove {
                buttons,
                row,
                col,
                mods,
                side,
                px_off,
            },
            Source::Human,
        );
    }

    /// Mid-drag: grow the selection to the hovered viewport cell — by cell for
    /// simple/block drags, by whole words/lines when the drag began as a
    /// double/triple click (the gesture origin stays fully selected whichever
    /// direction the drag goes).
    pub(crate) fn drag_selection(&mut self, wid: WindowId, row: u16, col: u16) {
        let Some(fws) = self.windows.get_mut(&wid) else {
            return;
        };
        let sel_row = {
            let mut term = term_lock(&fws.term);
            let sel_row = i32::from(row) - term.grid().display_offset() as i32;
            match fws.gesture {
                None => {
                    term.text_selection_mut()
                        .update_selection(sel_row, col, fws.last_mouse_side);
                }
                // Triple-click drag: whole rows from the origin line to the
                // hovered line. Rebuilt from the origin each move so the
                // anchor sides stay inclusive in either drag direction.
                Some(g) if g.kind == SelectionType::Lines => {
                    let max_col = term.cols().saturating_sub(1);
                    let sel = term.text_selection_mut();
                    if sel_row < g.row {
                        sel.start_selection(
                            g.row,
                            max_col,
                            SelectionSide::Right,
                            SelectionType::Lines,
                        );
                        sel.update_selection(sel_row, 0, SelectionSide::Left);
                    } else {
                        sel.start_selection(g.row, 0, SelectionSide::Left, SelectionType::Lines);
                        sel.update_selection(sel_row, max_col, SelectionSide::Right);
                    }
                }
                // Double-click drag: snap the moving end to the hovered word
                // (or bare cell on whitespace); the origin word stays fully
                // selected by anchoring at its far boundary.
                Some(g) => {
                    let (ws, we) = control::word_cols(&term, sel_row, col).unwrap_or((col, col));
                    let sel = term.text_selection_mut();
                    if (sel_row, col) < (g.row, g.start_col) {
                        sel.start_selection(
                            g.row,
                            g.end_col,
                            SelectionSide::Right,
                            SelectionType::Semantic,
                        );
                        sel.update_selection(sel_row, ws, SelectionSide::Left);
                    } else {
                        sel.start_selection(
                            g.row,
                            g.start_col,
                            SelectionSide::Left,
                            SelectionType::Semantic,
                        );
                        sel.update_selection(sel_row, we, SelectionSide::Right);
                    }
                }
            }
            sel_row
        };
        if (sel_row, col) != fws.sel_press_cell {
            fws.sel_dragged = true;
        }
        if let Some(w) = &fws.os_window {
            w.request_redraw();
        }
    }

    /// While a left-drag selection is in flight, AUTOSCROLL the scrollback when the
    /// pointer is dragged PAST the top/bottom viewport edge, so the selection can
    /// extend into content that is currently off-screen (the standard text-editor
    /// "drag to the edge to keep selecting" gesture). Returns `true` iff the viewport
    /// actually moved (the caller then re-grows the selection to the freshly-revealed
    /// edge row and repaints).
    ///
    /// A NO-OP unless a selection drag is active (`selecting`) — a plain hover past
    /// the edge never scrolls. The line count + direction come from the pure
    /// [`crate::app_render::selection_autoscroll_lines`] (so the edge math is
    /// unit-testable); `scroll_display` clamps at the history ends, so dragging past
    /// the oldest/newest line is harmless.
    pub(crate) fn selection_autoscroll(&mut self, wid: WindowId, y: f64) -> bool {
        let Some(ws) = self.windows.get(&wid) else {
            return false;
        };
        if !ws.selecting {
            return false;
        }
        let rows = ws.rows;
        let ch = self.cell_size().1.max(1);
        let pad = self.backend.pad();
        let strip_px = self.tab_strip_rows as usize * ch;
        let lines = crate::app_render::selection_autoscroll_lines(y, pad, strip_px, ch, rows);
        if lines == 0 {
            return false;
        }
        let Some(ws) = self.windows.get(&wid) else {
            return false;
        };
        let moved = {
            let mut term = term_lock(&ws.term);
            let before = term.grid().display_offset();
            term.scroll_display(lines);
            term.grid().display_offset() != before
        };
        if moved && let Some(w) = &ws.os_window {
            w.request_redraw();
        }
        moved
    }

    /// Left press with mouse tracking OFF — the selection-gesture dispatcher.
    ///
    /// Shift with an existing selection extends it to the pressed cell;
    /// otherwise the multi-click count picks the gesture: 1 starts a simple
    /// drag (rectangular block with alt/option held), 2 selects the word under
    /// the press, 3 selects the whole line. Word/line selections stay
    /// draggable until release (extending by whole words/lines).
    /// Shift-click: extend the existing selection (GUI affordance) and reset the
    /// multi-click streak (this press is not part of a double-click). The actual
    /// selection mutation reuses [`Self::extend_selection_to`]. Stays in the human
    /// handler — it is keyed on `self.mods`, which a controller never sets (the
    /// controller analogue is the `select extend` verb).
    pub(crate) fn shift_extend_press(&mut self, wid: WindowId) {
        let Some((row, col)) = self.windows.get(&wid).map(|ws| ws.last_mouse_cell) else {
            return;
        };
        let Some(term) = self.windows.get(&wid).map(|ws| ws.term.clone()) else {
            return;
        };
        let sel_row = i32::from(row) - term_lock(&term).grid().display_offset() as i32;
        let now = Instant::now();
        self.extend_selection_to(wid, sel_row, col);
        if let Some(ws) = self.windows.get_mut(&wid) {
            ws.last_press = Some((now, (sel_row, col)));
            ws.click_count = 1;
            if let Some(w) = &ws.os_window {
                w.request_redraw();
            }
        }
    }

    /// Advance the MULTI_CLICK_MS streak FSM and RETURN the resulting click_count
    /// (1 = single, 2 = double, 3 = triple; a fourth rapid click wraps to 1). The
    /// human handler owns this streak state (`last_press`/`click_count`); a
    /// controller passes an authoritative count without mutating it (A.2.2). The
    /// gesture DISPATCH on the returned count now lives in the seam
    /// (`seam_left_press`), shared by both sources.
    pub(crate) fn advance_click_streak(&mut self, wid: WindowId) -> u8 {
        let Some(ws) = self.windows.get_mut(&wid) else {
            return 1;
        };
        let (row, col) = ws.last_mouse_cell;
        let sel_row = i32::from(row) - term_lock(&ws.term).grid().display_offset() as i32;
        let now = Instant::now();
        ws.click_count = match ws.last_press {
            Some((t, cell))
                if cell == (sel_row, col)
                    && now.duration_since(t).as_millis() <= MULTI_CLICK_MS =>
            {
                ws.click_count % 3 + 1
            }
            _ => 1,
        };
        ws.last_press = Some((now, (sel_row, col)));
        ws.click_count
    }

    /// Shift-click: extend an EXISTING non-empty selection so the pressed cell
    /// becomes its new endpoint (side by cell half), then complete it again.
    /// Returns false (no-op) when there is nothing to extend.
    pub(crate) fn extend_selection_to(&mut self, wid: WindowId, sel_row: i32, col: u16) -> bool {
        let Some(ws) = self.windows.get(&wid) else {
            return false;
        };
        let mut term = term_lock(&ws.term);
        let sel = term.text_selection_mut();
        if !sel.has_selection() || sel.is_empty() {
            return false;
        }
        sel.extend_selection(sel_row, col, ws.last_mouse_side);
        sel.complete_selection();
        true
    }

    /// Double-click: word-select the pressed cell (builtin smart rules — URLs,
    /// paths, words; just the cell on whitespace), completed immediately, and
    /// arm the gesture so a drag before release extends by whole words.
    pub(crate) fn select_word_click(&mut self, wid: WindowId, sel_row: i32, col: u16) {
        let Some(term) = self.windows.get(&wid).map(|ws| ws.term.clone()) else {
            return;
        };
        let (start_col, end_col) = {
            let mut term = term_lock(&term);
            control::select_word(&mut term, sel_row, col)
        };
        if let Some(ws) = self.windows.get_mut(&wid) {
            ws.gesture = Some(GestureOrigin {
                row: sel_row,
                start_col,
                end_col,
                kind: SelectionType::Semantic,
            });
        }
        self.arm_gesture_drag(wid, sel_row, col);
    }

    /// Triple-click: select the full line under the press, completed
    /// immediately, and arm the gesture so a drag extends by whole lines.
    pub(crate) fn select_line_click(&mut self, wid: WindowId, sel_row: i32, col: u16) {
        let Some(term) = self.windows.get(&wid).map(|ws| ws.term.clone()) else {
            return;
        };
        let end_col = {
            let mut term = term_lock(&term);
            control::select_line(&mut term, sel_row);
            term.cols().saturating_sub(1)
        };
        if let Some(ws) = self.windows.get_mut(&wid) {
            ws.gesture = Some(GestureOrigin {
                row: sel_row,
                start_col: 0,
                end_col,
                kind: SelectionType::Lines,
            });
        }
        self.arm_gesture_drag(wid, sel_row, col);
    }

    /// Keep a completed double/triple-click selection draggable while the
    /// button stays down: `sel_dragged` is pre-set so the release completes
    /// the selection instead of treating it as a deselecting plain click.
    pub(crate) fn arm_gesture_drag(&mut self, wid: WindowId, sel_row: i32, col: u16) {
        let Some(ws) = self.windows.get_mut(&wid) else {
            return;
        };
        ws.selecting = true;
        ws.sel_dragged = true;
        ws.sel_press_cell = (sel_row, col);
        if let Some(w) = &ws.os_window {
            w.request_redraw();
        }
    }

    /// Single press with mouse tracking OFF: start a text selection of `kind`
    /// (`Simple`, or `Block` for alt-drag) at the cell under the pointer,
    /// mapped to live-screen selection coords (viewport row minus
    /// `display_offset`, so a scrolled-back press lands in scrollback).
    pub(crate) fn begin_selection(&mut self, wid: WindowId, kind: SelectionType) {
        let Some(ws) = self.windows.get_mut(&wid) else {
            return;
        };
        let (row, col) = ws.last_mouse_cell;
        let sel_row = {
            let mut term = term_lock(&ws.term);
            let sel_row = i32::from(row) - term.grid().display_offset() as i32;
            term.text_selection_mut()
                .start_selection(sel_row, col, SelectionSide::Left, kind);
            sel_row
        };
        ws.selecting = true;
        ws.sel_dragged = false;
        ws.sel_press_cell = (sel_row, col);
        ws.gesture = None;
        if let Some(w) = &ws.os_window {
            w.request_redraw();
        }
    }

    /// Left release ending a drag: complete the selection — unless the pointer
    /// never left the press cell, in which case a plain click deselects.
    ///
    /// COPY-ON-SELECT: when `copy_on_select` is enabled (config, default off) and
    /// the release actually COMPLETED a selection (a real drag, not a deselecting
    /// click), the selected text is copied to the system clipboard right here — no
    /// explicit Cmd-C needed. The highlight is left intact (`copy_selection` does
    /// not clear it), so Cmd-C still works on the same selection afterwards.
    ///
    /// Returns whether the copy-on-select path FIRED (an opted-in completed drag) —
    /// the auto-copy trigger, independent of whether `pbcopy` itself succeeded — so
    /// the firing CONDITION is unit-testable without touching the system clipboard.
    pub(crate) fn finish_selection(&mut self, wid: WindowId) -> bool {
        let Some(ws) = self.windows.get_mut(&wid) else {
            return false;
        };
        let completed = ws.sel_dragged;
        {
            let mut term = term_lock(&ws.term);
            let sel = term.text_selection_mut();
            if completed {
                sel.complete_selection();
            } else {
                sel.clear();
            }
        }
        ws.selecting = false;
        ws.gesture = None;
        if let Some(w) = &ws.os_window {
            w.request_redraw();
        }
        // A completed drag-select auto-copies when the user opted in. Done AFTER
        // the borrow above ends (it re-locks the term to stringify the selection)
        // and only for a real selection — a plain click that cleared never copies.
        let fired = completed && self.copy_on_select;
        if fired {
            self.copy_selection();
        }
        fired
    }

    /// The URL under the pointer, if any: an (authorized) OSC 8 hyperlink on the
    /// cell wins; else a plain-text `http(s)://` URL detected in the row. Used by
    /// Cmd-click (open) and Cmd-hover (pointer cursor).
    pub(crate) fn link_under_pointer(&self, wid: WindowId) -> Option<String> {
        let ws = self.windows.get(&wid)?;
        let (row, col) = ws.last_mouse_cell;
        let term = term_lock(&ws.term);
        term.hyperlink_at(row, col).map(str::to_owned).or_else(|| {
            plain_url_at(&term.render_row(row as usize), col as usize).map(|(u, _, _)| u)
        })
    }

    /// Cmd-click: if there is a link under the pointer with a safe scheme, open it
    /// via the OS and report `true`. The `is_safe_url` allowlist is the security
    /// boundary — a hostile program's link can never make `open` launch an app or
    /// touch the filesystem (covers both OSC 8 and auto-detected plain-text URLs).
    pub(crate) fn open_link_under_pointer(&self, wid: WindowId) -> bool {
        let Some(url) = self.link_under_pointer(wid) else {
            return false;
        };
        if !is_safe_url(&url) {
            return false;
        }
        #[cfg(target_os = "macos")]
        let _ = std::process::Command::new("/usr/bin/open")
            .arg(&url)
            .spawn();
        true
    }

    /// `MouseInput` -> when no app is tracking the mouse, left presses run the
    /// selection gestures (drag select, double-click word, triple-click line,
    /// shift-click extend, alt-drag block; a plain left click deselects); when
    /// tracking is on, encode the press/release for the cell under the pointer
    /// and write it to the PTY.
    pub(crate) fn on_mouse_input(
        &mut self,
        wid: WindowId,
        state: ElementState,
        button: WinitMouseButton,
    ) {
        // GUI-ONLY prefix (gesture-state owner = App; a controller can't trigger
        // these): Cmd-click link-open, shift-extend, and the MULTI_CLICK_MS streak
        // FSM that yields the authoritative `click_count`. These stay in the
        // handler; the seam consumes `click_count`/`side` as DATA.
        let pressed = state == ElementState::Pressed;
        let Some(mods_state) = self.windows.get(&wid).map(|ws| ws.mods) else {
            return;
        };
        // TAB STRIP: a left press in the strip region (top `tab_strip_rows` rows)
        // switches / closes / opens a tab and stops there — it never reaches the
        // terminal selection / pane-focus path. A no-op when the strip is disabled
        // or the press is in the terminal region.
        if pressed && button == WinitMouseButton::Left {
            let (px, py) = self
                .windows
                .get(&wid)
                .map_or((0.0, 0.0), |ws| ws.last_cursor_px);
            if let Some(col) = self.strip_col_at(wid, px, py) {
                self.handle_tab_strip_click(wid, col);
                return;
            }
        }
        // SPLIT-PANE DIVIDER DRAG: a left press ON a divider grabs it to resize the
        // split (and stops there — no focus change, no selection). Release ends the
        // drag. Checked BEFORE pane-focus so a press on the gap between panes resizes
        // rather than mis-focusing. A no-op on the single-pane path.
        if button == WinitMouseButton::Left {
            if pressed {
                if self.begin_divider_drag(wid) {
                    return;
                }
            } else if self.finish_divider_drag(wid) {
                if let Some(ws) = self.windows.get_mut(&wid) {
                    ws.held_mouse_button = None;
                }
                return;
            }
        }
        // SPLIT PANES: a left press in a DIFFERENT pane focuses it (and stops there
        // — it does not also start a selection in the old pane). A no-op on the
        // single-pane path (the hit-test always returns the only/focused pane).
        if pressed && button == WinitMouseButton::Left && self.focus_pane_under_pointer(wid) {
            return;
        }
        let mut click_count: u8 = 1;
        if button == WinitMouseButton::Left {
            let Some(term) = self.windows.get(&wid).map(|ws| ws.term.clone()) else {
                return;
            };
            let tracking = term_lock(&term).mouse_tracking_enabled();
            if pressed && !tracking {
                // Cmd-click an OSC 8 hyperlink opens it (safe schemes only) instead
                // of starting a selection. GUI-only — never reaches the seam.
                if mods_state.super_key() && self.open_link_under_pointer(wid) {
                    return;
                }
                // Shift-click extends an existing selection (GUI affordance keyed on
                // self.mods); it returns here without reaching the seam, like today.
                if mods_state.shift_key() {
                    self.shift_extend_press(wid);
                    return;
                }
                // Advance the streak and capture the count for the seam's gesture.
                click_count = self.advance_click_streak(wid);
            }
        }
        let Some(button) = winit_mouse_button(button) else {
            return;
        };
        // Track the held button so a subsequent motion report (tracking ON) carries
        // it instead of the hover code. Set on press, cleared on release; harmless
        // when tracking is OFF (the motion then takes the local selection path and
        // never reads this).
        if let Some(ws) = self.windows.get_mut(&wid) {
            ws.held_mouse_button = if pressed { Some(button) } else { None };
        }
        let (row, col) = self
            .windows
            .get(&wid)
            .map_or((0, 0), |ws| ws.last_mouse_cell);
        let mods = self.mouse_modifiers(wid);
        let side = self
            .windows
            .get(&wid)
            .map_or(SelectionSide::Left, |ws| ws.last_mouse_side);
        // The sub-cell pixel offset of the last pointer move (a press carries no
        // pixel of its own) so a DEC 1016 press/release lands on the genuine pixel.
        let px_off = self
            .windows
            .get(&wid)
            .map_or(crate::input::PixelOffset::CELL_ORIGIN, |ws| {
                ws.last_mouse_px_off
            });
        // Snapshot the block-select intent (held Alt/Option) HERE, at build time,
        // into event DATA — so the seam's selection-type decision is source-blind
        // (it reads `block`, never `self.mods`). A controller sends `block=1` for
        // the same effect; a human's later-released Alt can't retroactively change
        // this press's type.
        let block = mods_state.alt_key();
        // Phase 0.5: the seam reads mouse_tracking_enabled() ONCE under one lock
        // (closing the old two-lock window) and either emits the press/release
        // report (tracking ON, real `mods` — kills a) or runs the local selection
        // gesture (tracking OFF), dispatching on `click_count` (kills b) at `side`
        // (kills i) with type from `block`. Both sources share that machinery.
        self.input(
            wid,
            InputEvent::MouseButton {
                button,
                pressed,
                row,
                col,
                mods,
                click_count,
                side,
                block,
                px_off,
            },
            Source::Human,
        );
    }

    /// `MouseWheel` -> when an app is tracking the mouse, report wheel up/down at
    /// the cell under the pointer; otherwise scroll the scrollback viewport (the
    /// everyday "scroll up to see history" gesture).
    pub(crate) fn on_mouse_wheel(&mut self, wid: WindowId, delta: MouseScrollDelta) {
        // Lines to move per event: one line per LineDelta notch, or a fraction of
        // the cell height for trackpad PixelDelta (min 1 so a flick always moves).
        let (dir_up, lines) = match delta {
            MouseScrollDelta::LineDelta(x, y) => {
                // Ignore a predominantly-horizontal notch (a horizontal wheel or a
                // tilt-wheel): a horizontal gesture must NOT scroll the viewport
                // vertically. Without this, `y == 0.0` fell through to dir_up=false
                // + `.max(1)` and scrolled DOWN one line on every horizontal swipe.
                if y == 0.0 || y.abs() <= x.abs() {
                    return;
                }
                (y > 0.0, y.abs().round().max(1.0) as i32)
            }
            MouseScrollDelta::PixelDelta(p) => {
                // Same guard for trackpad pixel deltas: bail when the vertical
                // component is negligible or dominated by the horizontal one, so a
                // horizontal two-finger swipe is a no-op instead of a phantom
                // scroll-down. Vertical-dominant events keep the prior `.max(1)`
                // one-line-minimum behavior unchanged.
                if p.y.abs() < f64::EPSILON || p.y.abs() <= p.x.abs() {
                    return;
                }
                let ch = self.cell_size().1.max(1) as f64;
                (p.y > 0.0, (p.y.abs() / ch).round().max(1.0) as i32)
            }
        };
        let (row, col) = self
            .windows
            .get(&wid)
            .map_or((0, 0), |ws| ws.last_mouse_cell);
        let mods = self.mouse_modifiers(wid);
        let px_off = self
            .windows
            .get(&wid)
            .map_or(crate::input::PixelOffset::CELL_ORIGIN, |ws| {
                ws.last_mouse_px_off
            });
        // Phase 0.5: the seam decides tracking-ON (N reports / N lines — kills e)
        // vs tracking-OFF (scroll the viewport `lines`) under one mode read.
        self.input(
            wid,
            InputEvent::Wheel {
                dir_up,
                lines,
                row,
                col,
                mods,
                px_off,
            },
            Source::Human,
        );
    }

    /// Snap the viewport back to the live bottom (called on keyboard input, the
    /// standard "start typing and jump to the prompt" behavior).
    pub(crate) fn snap_to_bottom(&mut self, wid: WindowId) {
        let Some(ws) = self.windows.get(&wid) else {
            return;
        };
        let scrolled = {
            let mut term = term_lock(&ws.term);
            if term.grid().display_offset() != 0 {
                term.scroll_to_bottom();
                true
            } else {
                false
            }
        };
        if scrolled && let Some(w) = &ws.os_window {
            w.request_redraw();
        }
    }

    /// Edit ▸ Select All: select the entire visible screen as whole lines (a
    /// `Lines` selection from the top row to the bottom row, full width), then
    /// repaint so the highlight shows. Mirrors a triple-click line selection
    /// dragged top-to-bottom; the snap-to-bottom first makes 0..rows stable
    /// selection coordinates (matching `search_recompute`). Copy (Cmd-C) then
    /// works on the whole screen exactly as on a mouse selection.
    pub(crate) fn select_all(&mut self) {
        // A window-level command (menu Select All): targets the frontmost window.
        let Some(wid) = self.frontmost_window else {
            return;
        };
        self.snap_to_bottom(wid);
        let Some(ws) = self.front() else { return };
        let last = i32::from(ws.rows.saturating_sub(1));
        let max_col = ws.cols.saturating_sub(1);
        {
            let mut term = term_lock(&ws.term);
            let sel = term.text_selection_mut();
            sel.start_selection(0, 0, SelectionSide::Left, SelectionType::Lines);
            sel.update_selection(last, max_col, SelectionSide::Right);
            sel.expand_lines(max_col);
            sel.complete_selection();
        }
        if let Some(w) = &ws.os_window {
            w.request_redraw();
        }
    }
}
