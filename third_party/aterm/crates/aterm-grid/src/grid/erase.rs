// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Grid erase operations.
//!
//! This module contains all erase-related operations for the terminal grid:
//! - Line erase (EL): erase parts of or entire lines
//! - Screen erase (ED): erase parts of or entire screen
//! - Scrollback erase: clear history buffer
//! - Selective erase (DECSEL/DECSED): erase only unprotected cells
//! - Screen alignment pattern (DECALN): fill screen with 'E' for testing
//!
//! All public erase methods clear the `pending_wrap` flag. This matches
//! xterm behavior where erase operations cancel the deferred wrap state
//! set by writing to the last column.

use super::{Cursor, Grid, ScrollRegion};
use crate::PageStore;
use crate::Row;

impl Grid {
    // ========================================================================
    // BCE RGB fill helpers (#7685)
    // ========================================================================

    /// Write BCE truecolor background into `RgbColorRing` for a column range.
    ///
    /// No-op when `cursor_template_bg_rgb` is `None` (indexed or default bg).
    /// Must be called after `extras.clear_range()` to repopulate the ring.
    pub(in crate::grid) fn fill_bce_rgb_range(&mut self, row: u16, col_start: u16, col_end: u16) {
        if let Some(bg_rgb) = self.storage.cursor_template_bg_rgb {
            let vis = self.storage.visible_rows;
            let cols = self.storage.cols;
            self.storage.extras.set_rgb_ring_range(
                row,
                col_start,
                col_end,
                None,
                Some(bg_rgb),
                vis,
                cols,
            );
        }
    }

    /// Write BCE truecolor background for an entire row.
    ///
    /// No-op when `cursor_template_bg_rgb` is `None`.
    pub(in crate::grid) fn fill_bce_rgb_row(&mut self, row: u16) {
        if self.storage.cursor_template_bg_rgb.is_some() {
            self.fill_bce_rgb_range(row, 0, self.storage.cols);
        }
    }

    /// Write BCE truecolor background for a range of full rows.
    ///
    /// No-op when `cursor_template_bg_rgb` is `None`.
    pub(in crate::grid) fn fill_bce_rgb_rows(&mut self, range: core::ops::Range<u16>) {
        if self.storage.cursor_template_bg_rgb.is_some() {
            for row in range {
                self.fill_bce_rgb_range(row, 0, self.storage.cols);
            }
        }
    }

    /// Write BCE truecolor background for a rectangular area.
    ///
    /// No-op when `cursor_template_bg_rgb` is `None`.
    pub(in crate::grid) fn fill_bce_rgb_rect(
        &mut self,
        rows: core::ops::Range<u16>,
        cols: core::ops::Range<u16>,
    ) {
        if self.storage.cursor_template_bg_rgb.is_some() {
            for row in rows {
                self.fill_bce_rgb_range(row, cols.start, cols.end);
            }
        }
    }

    // ========================================================================
    // Private helpers — parameterize regular vs selective erase
    // ========================================================================

    /// Erase from cursor to end of line.
    ///
    /// When DECLRMM horizontal margins are active and the cursor is within the
    /// margin region, erases to the right margin instead of the screen edge
    /// (#7644). When `selective`, only unprotected cells are cleared and extras
    /// are preserved. Non-selective erase uses the BCE cursor template for fill.
    fn erase_to_end_of_line_impl(&mut self, selective: bool) {
        self.erase_to_end_of_line_core(selective, true);
    }

    /// Core implementation for erase-to-end-of-line.
    ///
    /// `respect_margins`: when true, clamps to DECLRMM right margin (for EL).
    /// ED must pass false because ED is not affected by DECLRMM (#7644).
    fn erase_to_end_of_line_core(&mut self, selective: bool, respect_margins: bool) {
        let cursor_row = self.storage.cursor.row;
        let cursor_col = self.storage.cursor.col;
        let mut right_bound = self.storage.effective_cols_for_row(cursor_row);
        // Clamp to right margin when DECLRMM is active and cursor is within
        // the margin region, matching ECH behavior (#7644).
        if respect_margins && self.storage.has_horizontal_margins {
            let margins = self.storage.horizontal_margins();
            if cursor_col >= margins.left && cursor_col <= margins.right {
                right_bound = right_bound.min(margins.right + 1);
            }
        }
        let fill = self.storage.cursor_template;
        if cursor_col < right_bound {
            if let Some(row) = self.row_mut(cursor_row) {
                if selective {
                    row.selective_clear_range(cursor_col, right_bound);
                } else {
                    row.clear_range_with(cursor_col, right_bound, fill);
                }
            }
            if !selective {
                self.storage
                    .extras
                    .clear_range(cursor_row, cursor_col, right_bound);
                self.fill_bce_rgb_range(cursor_row, cursor_col, right_bound);
            }
            self.storage.damage.mark_row(cursor_row);
        }
    }

    /// Erase from start of line to cursor.
    ///
    /// When DECLRMM horizontal margins are active and the cursor is within the
    /// margin region, erases from the left margin instead of column 0 (#7644).
    /// When `selective`, only unprotected cells are cleared and extras are
    /// preserved. Non-selective erase uses the BCE cursor template for fill.
    fn erase_from_start_of_line_impl(&mut self, selective: bool) {
        self.erase_from_start_of_line_core(selective, true);
    }

    /// Core implementation for erase-from-start-of-line.
    ///
    /// `respect_margins`: when true, clamps to DECLRMM left margin (for EL).
    /// ED must pass false because ED is not affected by DECLRMM (#7644).
    fn erase_from_start_of_line_core(&mut self, selective: bool, respect_margins: bool) {
        let cursor_row = self.storage.cursor.row;
        let cursor_col = self.storage.cursor.col;
        let effective_cols = self.storage.effective_cols_for_row(cursor_row);
        let end = cursor_col.saturating_add(1).min(effective_cols);
        // Clamp to left margin when DECLRMM is active and cursor is within
        // the margin region (#7644).
        let mut start = 0u16;
        if respect_margins && self.storage.has_horizontal_margins {
            let margins = self.storage.horizontal_margins();
            if cursor_col >= margins.left && cursor_col <= margins.right {
                start = margins.left;
            }
        }
        let fill = self.storage.cursor_template;
        if end > start {
            if let Some(row) = self.row_mut(cursor_row) {
                if selective {
                    row.selective_clear_range(start, end);
                } else {
                    row.clear_range_with(start, end, fill);
                }
            }
            if !selective {
                self.storage.extras.clear_range(cursor_row, start, end);
                self.fill_bce_rgb_range(cursor_row, start, end);
            }
            self.storage.damage.mark_row(cursor_row);
        }
    }

    /// Erase entire line at cursor.
    ///
    /// When DECLRMM horizontal margins are active and the cursor is within the
    /// margin region, erases only from left margin to right margin (#7644).
    /// When `selective`, only unprotected cells are cleared and extras are
    /// preserved. Non-selective erase uses the BCE cursor template for fill.
    fn erase_line_impl(&mut self, selective: bool) {
        let cursor_row = self.storage.cursor.row;
        let cursor_col = self.storage.cursor.col;
        let fill = self.storage.cursor_template;
        // When DECLRMM is active and cursor is within margins, erase only
        // the margin region instead of the full line (#7644).
        if self.storage.has_horizontal_margins {
            let margins = self.storage.horizontal_margins();
            if cursor_col >= margins.left && cursor_col <= margins.right {
                let start = margins.left;
                let end = margins.right + 1;
                if let Some(row) = self.row_mut(cursor_row) {
                    if selective {
                        row.selective_clear_range(start, end);
                    } else {
                        row.clear_range_with(start, end, fill);
                    }
                }
                if !selective {
                    self.storage.extras.clear_range(cursor_row, start, end);
                    self.fill_bce_rgb_range(cursor_row, start, end);
                }
                self.storage.damage.mark_row(cursor_row);
                return;
            }
        }
        if let Some(row) = self.row_mut(cursor_row) {
            if selective {
                row.selective_clear();
            } else {
                // Use erase_with() to preserve DECDWL/DECDHL line attributes (#7497)
                // while applying BCE background (#7522).
                row.erase_with(fill);
            }
        }
        if !selective {
            self.storage.extras.clear_row(cursor_row);
            self.fill_bce_rgb_row(cursor_row);
        }
        self.storage.damage.mark_row(cursor_row);
    }

    /// Clear a range of full rows (used by screen-level erase).
    ///
    /// When `selective`, only unprotected cells are cleared and extras are preserved.
    /// Non-selective erase uses the BCE cursor template for fill.
    /// Uses batch `clear_rows()` on extras for O(E) instead of O(R * E).
    fn clear_rows(&mut self, range: core::ops::Range<u16>, selective: bool) {
        let fill = self.storage.cursor_template;
        for row in range.clone() {
            if let Some(r) = self.row_mut(row) {
                if selective {
                    r.selective_clear();
                } else {
                    // Use erase_with() to preserve DECDWL/DECDHL line attributes (#7497)
                    // while applying BCE background (#7522).
                    r.erase_with(fill);
                }
            }
        }
        if !selective {
            self.storage.extras.clear_rows(range.clone());
            self.fill_bce_rgb_rows(range);
        }
    }

    // ========================================================================
    // Public API — Line Erase (EL)
    // ========================================================================

    /// Erase from cursor to end of line.
    ///
    /// REQUIRES: self.storage.cursor.row < self.storage.visible_rows
    #[inline]
    pub fn erase_to_end_of_line(&mut self) {
        self.storage.clear_pending_wrap();
        self.erase_to_end_of_line_impl(false);
    }

    /// Erase from start of line to cursor.
    ///
    /// REQUIRES: self.storage.cursor.row < self.storage.visible_rows
    #[inline]
    pub fn erase_from_start_of_line(&mut self) {
        self.storage.clear_pending_wrap();
        self.erase_from_start_of_line_impl(false);
    }

    /// Erase entire line.
    ///
    /// REQUIRES: self.storage.cursor.row < self.storage.visible_rows
    #[doc(hidden)] // pub for crate benchmarks; not part of stable API
    #[inline]
    pub fn erase_line(&mut self) {
        self.storage.clear_pending_wrap();
        self.erase_line_impl(false);
    }

    // ========================================================================
    // Public API — Screen Erase (ED)
    // ========================================================================

    /// Erase from cursor to end of screen.
    ///
    /// REQUIRES: self.storage.cursor.row < self.storage.visible_rows
    #[doc(hidden)] // pub for crate benchmarks; not part of stable API
    #[inline]
    pub fn erase_to_end_of_screen(&mut self) {
        self.storage.clear_pending_wrap();
        // ED is NOT affected by DECLRMM horizontal margins per VT420 spec.
        // Use _core with respect_margins=false to bypass margin clamping.
        self.erase_to_end_of_line_core(false, false);
        let cursor_row = self.storage.cursor.row;
        let visible_rows = self.storage.visible_rows;
        self.clear_rows(cursor_row.saturating_add(1)..visible_rows, false);
        // Invalidate selection — erased content makes coordinates stale.
        self.storage.content_scroll_delta = i32::MAX;
        // cursor row already marked by erase_to_end_of_line_impl; mark remaining rows.
        self.storage
            .damage
            .mark_rows(cursor_row.saturating_add(1), visible_rows);
    }

    /// Erase from start of screen to cursor.
    ///
    /// REQUIRES: self.storage.cursor.row < self.storage.visible_rows
    pub fn erase_from_start_of_screen(&mut self) {
        self.storage.clear_pending_wrap();
        let cursor_row = self.storage.cursor.row;
        self.clear_rows(0..cursor_row, false);
        // ED is NOT affected by DECLRMM horizontal margins per VT420 spec.
        // Use _core with respect_margins=false to bypass margin clamping.
        self.erase_from_start_of_line_core(false, false);
        // Invalidate selection — erased content makes coordinates stale.
        self.storage.content_scroll_delta = i32::MAX;
        // cursor row already marked by erase_from_start_of_line_impl; mark rows above.
        self.storage.damage.mark_rows(0, cursor_row);
    }

    /// Erase entire screen.
    ///
    /// Uses the BCE cursor template for fill, so erased cells inherit the
    /// current SGR background color per VT420/xterm spec (#7522).
    /// Resets display_offset to 0 so the viewport snaps to the live terminal.
    pub fn erase_screen(&mut self) {
        self.storage.clear_pending_wrap();
        // Snap viewport to live terminal — ED 2 should show the freshly
        // erased screen, not stale scrollback position.
        self.storage.display_offset = 0;
        let fill = self.storage.cursor_template;
        for row in 0..self.storage.visible_rows {
            if let Some(r) = self.row_mut(row) {
                // Use erase_with() to preserve DECDWL/DECDHL line attributes (#7497)
                // while applying BCE background (#7522).
                r.erase_with(fill);
            }
        }
        self.storage.extras.clear();
        self.fill_bce_rgb_rows(0..self.storage.visible_rows);
        // Per-row HAS_STYLE_ID flags are cleared implicitly by `erase_with`
        // above, which resets row flags to LINE_ATTRIBUTES | DIRTY (#7872).
        // Note: any_double_width is NOT cleared because erase preserves
        // DECDWL/DECDHL line attributes per VT spec (#7497).
        // Invalidate selection — entire screen erased.
        self.storage.content_scroll_delta = i32::MAX;
        self.storage.damage.mark_full();
    }

    /// Clear all DEC line attributes (DECDWL/DECDHL) on every visible row.
    ///
    /// Used by RIS and DECCOLM toggle which must reset line attributes.
    /// Erase operations preserve line attributes per VT spec, so this
    /// must be called explicitly when a full reset is needed (#7497).
    pub fn clear_line_attributes(&mut self) {
        for row_idx in 0..self.storage.visible_rows {
            if let Some(r) = self.row_mut(row_idx) {
                r.set_line_size(super::LineSize::SingleWidth);
            }
        }
        self.storage.any_double_width = false;
    }

    // ========================================================================
    // Public API — Scrollback Erase
    // ========================================================================

    /// Erase scrollback.
    ///
    /// ENSURES: self.storage.display_offset == 0
    /// ENSURES: self.storage.total_lines == self.storage.visible_rows as usize
    pub fn erase_scrollback(&mut self) {
        let scrollback = self
            .storage
            .total_lines
            .saturating_sub(self.storage.visible_rows as usize);
        if scrollback == 0 {
            if let Some(scrollback) = self.storage.scrollback.as_mut()
                && let Err(e) = scrollback.clear()
            {
                aterm_log::warn!("scrollback clear failed: {e}");
            }
            self.storage.lazy_buffer.clear();
            self.storage.generations.evict_all();
            self.storage.ring_extras.clear();
            self.storage.display_offset = 0;
            self.storage.damage.mark_full();
            // Invalidate any selection anchored in scrollback (matches main path).
            self.storage.content_scroll_delta = i32::MAX;
            // Clear pending_wrap to match module contract (#7496).
            self.storage.clear_pending_wrap();
            debug_assert_eq!(self.storage.display_offset, 0);
            debug_assert_eq!(self.storage.total_lines, self.storage.visible_rows as usize);
            return;
        }

        // Preserve the live (display_offset = 0) visible rows and drop scrollback.
        debug_assert!(
            !self.storage.rows.is_empty(),
            "clear_scrollback: ring buffer has zero rows"
        );
        let live_top = (self.storage.ring_head + scrollback) % self.storage.rows.len();
        let mut new_pages = PageStore::new();
        let mut new_rows = Vec::with_capacity(self.storage.visible_rows as usize);
        for i in 0..self.storage.visible_rows {
            let idx = (live_top + i as usize) % self.storage.rows.len();
            // SAFETY: `new_rows` and `new_pages` are installed into `self`
            // together before the rebuilt rows can outlive this method.
            let mut row = unsafe { Row::new(self.storage.cols, &mut new_pages) };
            row.copy_from(&self.storage.rows[idx]);
            new_rows.push(row);
        }

        self.storage.rows = new_rows;
        self.storage.pages = new_pages;
        self.storage.total_lines = self.storage.visible_rows as usize;
        self.storage.ring_head = 0;
        self.storage.display_offset = 0;
        if let Some(scrollback) = self.storage.scrollback.as_mut()
            && let Err(e) = scrollback.clear()
        {
            aterm_log::warn!("scrollback clear failed: {e}");
        }
        self.storage.lazy_buffer.clear();
        self.storage.generations.evict_all();
        self.storage.ring_extras.clear();
        // Note: extras are keyed by visible row, so we don't need to clear them here
        // as scrollback rows don't have extras (they're saved as Line objects)
        self.storage.damage.mark_full();
        // Signal post_process to clear any selection anchored in scrollback.
        // i32::MAX is the sentinel that adjust_for_scroll interprets as "clear".
        self.storage.content_scroll_delta = i32::MAX;
        // Clear pending_wrap to match module contract (#7496).
        self.storage.clear_pending_wrap();
        debug_assert_eq!(self.storage.display_offset, 0);
        debug_assert_eq!(self.storage.total_lines, self.storage.visible_rows as usize);
    }

    // ========================================================================
    // Public API — Selective Erase (DECSED/DECSEL)
    // ========================================================================

    /// Selectively erase from cursor to end of line (DECSEL mode 0).
    ///
    /// Only erases cells that are NOT protected (DECSCA).
    ///
    /// REQUIRES: self.storage.cursor.row < self.storage.visible_rows
    pub fn selective_erase_to_end_of_line(&mut self) {
        self.storage.clear_pending_wrap();
        self.erase_to_end_of_line_impl(true);
    }

    /// Selectively erase from start of line to cursor (DECSEL mode 1).
    ///
    /// Only erases cells that are NOT protected (DECSCA).
    ///
    /// REQUIRES: self.storage.cursor.row < self.storage.visible_rows
    pub fn selective_erase_from_start_of_line(&mut self) {
        self.storage.clear_pending_wrap();
        self.erase_from_start_of_line_impl(true);
    }

    /// Selectively erase entire line (DECSEL mode 2).
    ///
    /// Only erases cells that are NOT protected (DECSCA).
    ///
    /// REQUIRES: self.storage.cursor.row < self.storage.visible_rows
    pub fn selective_erase_line(&mut self) {
        self.storage.clear_pending_wrap();
        self.erase_line_impl(true);
    }

    /// Selectively erase from cursor to end of screen (DECSED mode 0).
    ///
    /// Only erases cells that are NOT protected (DECSCA).
    ///
    /// REQUIRES: self.storage.cursor.row < self.storage.visible_rows
    pub fn selective_erase_to_end_of_screen(&mut self) {
        self.storage.clear_pending_wrap();
        // DECSED is NOT affected by DECLRMM horizontal margins per VT420 spec.
        self.erase_to_end_of_line_core(true, false);
        let cursor_row = self.storage.cursor.row;
        let visible_rows = self.storage.visible_rows;
        self.clear_rows(cursor_row.saturating_add(1)..visible_rows, true);
        // Invalidate selection — erased cells may be part of selection (#7499).
        self.storage.content_scroll_delta = i32::MAX;
        // cursor row already marked by erase_to_end_of_line_impl; mark remaining rows.
        self.storage
            .damage
            .mark_rows(cursor_row.saturating_add(1), visible_rows);
    }

    /// Selectively erase from start of screen to cursor (DECSED mode 1).
    ///
    /// Only erases cells that are NOT protected (DECSCA).
    ///
    /// REQUIRES: self.storage.cursor.row < self.storage.visible_rows
    pub fn selective_erase_from_start_of_screen(&mut self) {
        self.storage.clear_pending_wrap();
        let cursor_row = self.storage.cursor.row;
        self.clear_rows(0..cursor_row, true);
        // DECSED is NOT affected by DECLRMM horizontal margins per VT420 spec.
        self.erase_from_start_of_line_core(true, false);
        // Invalidate selection — erased cells may be part of selection (#7499).
        self.storage.content_scroll_delta = i32::MAX;
        // cursor row already marked by erase_from_start_of_line_impl; mark rows above.
        self.storage.damage.mark_rows(0, cursor_row);
    }

    /// Selectively erase entire screen (DECSED mode 2).
    ///
    /// Only erases cells that are NOT protected (DECSCA).
    pub fn selective_erase_screen(&mut self) {
        self.storage.clear_pending_wrap();
        self.clear_rows(0..self.storage.visible_rows, true);
        // Invalidate selection — erased cells may be part of selection (#7499).
        self.storage.content_scroll_delta = i32::MAX;
        self.storage.damage.mark_full();
    }

    /// Fill screen with 'E' for alignment test (DECALN - ESC # 8).
    ///
    /// This is used to test screen alignment by filling the entire screen
    /// with the character 'E'. It also resets scroll margins to full screen.
    ///
    /// ENSURES: self.storage.cursor.row == 0
    /// ENSURES: self.storage.cursor.col == 0
    /// ENSURES: self.storage.scroll_region.is_full(self.storage.visible_rows)
    pub fn screen_alignment_pattern(&mut self) {
        self.storage.clear_pending_wrap();
        // Reset scroll region to full screen (both vertical and horizontal).
        // Per xterm/VT420: DECALN resets DECSTBM and DECLRMM margins.
        self.storage.scroll_region = ScrollRegion::full(self.storage.visible_rows);
        self.storage.reset_horizontal_margins();

        let cols = self.storage.cols;

        // Fill all visible cells with 'E' using default attributes.
        // clear() resets each cell to Cell::EMPTY (space, default colors, no flags),
        // then write_char sets the character to 'E'. This ensures stale colors/flags
        // from previous content are not preserved.
        for row in 0..self.storage.visible_rows {
            if let Some(r) = self.row_mut(row) {
                r.clear();
                for col in 0..cols {
                    r.write_char(col, 'E');
                }
            }
        }

        // Clear overflow extras (RGB colors, hyperlinks, complex chars).
        // Without this, stale CellExtra entries from pre-DECALN content persist
        // as orphaned HashMap entries, leaking memory and risking stale data
        // if the same coordinates later need new extras.
        self.storage.extras.clear();

        // Clear the any_double_width optimization flag — r.clear() above reset
        // each row's LINE_ATTRIBUTES to single-width, so the grid-level flag
        // must also be cleared. Without this, cursor ops unnecessarily check
        // for double-width column clamping on every movement.
        self.storage.any_double_width = false;

        // Move cursor to home position
        self.storage.cursor = Cursor::default();

        // Invalidate selection — entire screen content was replaced (#7481).
        self.storage.content_scroll_delta = i32::MAX;
        self.storage.damage.mark_full();
        debug_assert_eq!(self.storage.cursor.row, 0);
        debug_assert_eq!(self.storage.cursor.col, 0);
        debug_assert!(
            self.storage
                .scroll_region
                .is_full(self.storage.visible_rows)
        );
    }

    /// Erase rectangular area (DECERA - VT420+).
    ///
    /// Erases all cells in the rectangular area defined by top-left (top, left)
    /// and bottom-right (bottom, right) coordinates. All coordinates are 0-indexed.
    ///
    /// Cells are cleared to spaces with default attributes.
    /// Does not affect cursor position.
    ///
    /// REQUIRES: self.storage.visible_rows > 0
    /// REQUIRES: self.storage.cols > 0
    pub fn erase_rect(&mut self, top: u16, left: u16, bottom: u16, right: u16) {
        self.storage.clear_pending_wrap();
        // Clamp to visible area
        let top = top.min(self.storage.visible_rows.saturating_sub(1));
        let bottom = bottom.min(self.storage.visible_rows.saturating_sub(1));
        let left = left.min(self.storage.cols.saturating_sub(1));
        let right = right.min(self.storage.cols.saturating_sub(1));

        // Validate rectangle
        if top > bottom || left > right {
            return;
        }

        let end_col = right.saturating_add(1);
        let fill = self.storage.cursor_template;

        // Erase each row in the rectangle with BCE fill (#7522).
        for row_idx in top..=bottom {
            if let Some(row) = self.row_mut(row_idx) {
                row.clear_range_with(left, end_col, fill);
            }
            self.storage.damage.mark_row(row_idx);
        }
        self.storage
            .extras
            .clear_rect(top..bottom.saturating_add(1), left..end_col);
        self.fill_bce_rgb_rect(top..bottom.saturating_add(1), left..end_col);
    }

    /// Fill rectangular area with a cell template (DECFRA - VT420+).
    ///
    /// Fills all cells in the rectangular area defined by top-left (top, left)
    /// and bottom-right (bottom, right) coordinates with the given cell template.
    /// The template carries both the fill character and current SGR attributes
    /// (colors + flags), per VT420 spec. All coordinates are 0-indexed.
    ///
    /// Does not affect cursor position.
    ///
    /// REQUIRES: self.storage.visible_rows > 0
    /// REQUIRES: self.storage.cols > 0
    pub fn fill_rect(&mut self, fill: super::Cell, top: u16, left: u16, bottom: u16, right: u16) {
        self.storage.clear_pending_wrap();
        // Clamp to visible area
        let top = top.min(self.storage.visible_rows.saturating_sub(1));
        let bottom = bottom.min(self.storage.visible_rows.saturating_sub(1));
        let left = left.min(self.storage.cols.saturating_sub(1));
        let right = right.min(self.storage.cols.saturating_sub(1));

        // Validate rectangle
        if top > bottom || left > right {
            return;
        }

        let end_col = right.saturating_add(1);

        // Fill each row in the rectangle using clear_range_with which handles
        // wide character fixup at range boundaries automatically.
        for row_idx in top..=bottom {
            if let Some(row) = self.row_mut(row_idx) {
                row.clear_range_with(left, end_col, fill);
            }
            self.storage.damage.mark_row(row_idx);
        }
        self.storage
            .extras
            .clear_rect(top..bottom.saturating_add(1), left..end_col);
    }

    /// Selectively erase rectangular area (DECSERA - VT420+).
    ///
    /// Erases characters in the rectangular area that are NOT protected by
    /// DECSCA. Protected cells are preserved. All coordinates are 0-indexed.
    ///
    /// Per VT520 (EK-VT520-RM): erased positions become spaces, but "DECSERA
    /// does not change: visual attributes set by the select graphic rendition
    /// (SGR) function; protection attributes set by DECSCA; line attributes."
    /// xterm matches (ScrnWipeRectangle only rewrites the character), so this
    /// wipes characters while keeping each cell's SGR flags and colors.
    ///
    /// Does not affect cursor position.
    ///
    /// REQUIRES: self.storage.visible_rows > 0
    /// REQUIRES: self.storage.cols > 0
    pub fn selective_erase_rect(&mut self, top: u16, left: u16, bottom: u16, right: u16) {
        self.storage.clear_pending_wrap();
        // Clamp to visible area
        let top = top.min(self.storage.visible_rows.saturating_sub(1));
        let bottom = bottom.min(self.storage.visible_rows.saturating_sub(1));
        let left = left.min(self.storage.cols.saturating_sub(1));
        let right = right.min(self.storage.cols.saturating_sub(1));

        // Validate rectangle
        if top > bottom || left > right {
            return;
        }

        let end_col = right.saturating_add(1);

        // Selectively erase each row in the rectangle, preserving the SGR
        // visual attributes of the wiped positions (VT520 DECSERA).
        for row_idx in top..=bottom {
            if let Some(row) = self.row_mut(row_idx) {
                row.selective_wipe_range(left, end_col);
            }
            self.storage.damage.mark_row(row_idx);
        }
        // Note: extras are NOT cleared for selective erase (protected cells
        // may have extras that must be preserved).
    }

    /// Change attributes in rectangular area (DECCARA - VT420+).
    ///
    /// Applies SGR attribute flags to all cells in the rectangular area defined
    /// by top-left (top, left) and bottom-right (bottom, right) coordinates.
    /// All coordinates are 0-indexed.
    ///
    /// `flags_to_set` are ORed into each cell's flags.
    /// `flags_to_clear` are ANDed out of each cell's flags.
    /// Does not affect cursor position.
    ///
    /// REQUIRES: self.storage.visible_rows > 0
    /// REQUIRES: self.storage.cols > 0
    pub fn change_attrs_rect(
        &mut self,
        top: u16,
        left: u16,
        bottom: u16,
        right: u16,
        flags_to_set: super::CellFlags,
        flags_to_clear: super::CellFlags,
    ) {
        self.storage.clear_pending_wrap();
        // Clamp to visible area
        let top = top.min(self.storage.visible_rows.saturating_sub(1));
        let bottom = bottom.min(self.storage.visible_rows.saturating_sub(1));
        let left = left.min(self.storage.cols.saturating_sub(1));
        let right = right.min(self.storage.cols.saturating_sub(1));

        // Validate rectangle
        if top > bottom || left > right {
            return;
        }

        let end_col = right.saturating_add(1);

        for row_idx in top..=bottom {
            if let Some(row) = self.row_mut(row_idx) {
                for col in left..end_col {
                    if let Some(cell) = row.get_mut(col) {
                        let mut flags = cell.flags();
                        // Clear first, then set (so set takes priority)
                        flags = flags.difference(flags_to_clear);
                        flags = flags.union(flags_to_set);
                        cell.set_flags(flags);
                    }
                }
            }
            self.storage.damage.mark_row(row_idx);
        }
    }

    /// Change attributes in a character stream (DECSACE stream mode).
    ///
    /// When DECSACE stream mode is active, DECCARA operates on a contiguous
    /// character stream from (top, left) to (bottom, right) rather than a
    /// rectangle. The first row starts at `left`, middle rows span the full
    /// width (0..cols-1), and the last row ends at `right`.
    ///
    /// All coordinates are 0-indexed.
    pub fn change_attrs_stream(
        &mut self,
        top: u16,
        left: u16,
        bottom: u16,
        right: u16,
        flags_to_set: super::CellFlags,
        flags_to_clear: super::CellFlags,
    ) {
        self.storage.clear_pending_wrap();
        // Clamp to visible area
        let top = top.min(self.storage.visible_rows.saturating_sub(1));
        let bottom = bottom.min(self.storage.visible_rows.saturating_sub(1));
        let left = left.min(self.storage.cols.saturating_sub(1));
        let right = right.min(self.storage.cols.saturating_sub(1));

        if top > bottom {
            return;
        }

        let max_col = self.storage.cols.saturating_sub(1);

        for row_idx in top..=bottom {
            // Stream extent: first row starts at `left`, last row ends at
            // `right`, middle rows span 0..max_col.
            let start = if row_idx == top { left } else { 0 };
            let end = if row_idx == bottom { right } else { max_col };

            if start > end {
                continue;
            }

            if let Some(row) = self.row_mut(row_idx) {
                for col in start..=end {
                    if let Some(cell) = row.get_mut(col) {
                        let mut flags = cell.flags();
                        flags = flags.difference(flags_to_clear);
                        flags = flags.union(flags_to_set);
                        cell.set_flags(flags);
                    }
                }
            }
            self.storage.damage.mark_row(row_idx);
        }
    }

    /// Copy rectangular area (DECCRA - VT420+).
    ///
    /// Copies cells from the source rectangle (src_top, src_left)-(src_bottom, src_right)
    /// to the destination starting at (dst_top, dst_left). All coordinates are 0-indexed.
    ///
    /// The copy is performed through a temporary buffer so overlapping source and
    /// destination rectangles are handled correctly.
    /// Does not affect cursor position.
    ///
    /// REQUIRES: self.storage.visible_rows > 0
    /// REQUIRES: self.storage.cols > 0
    pub fn copy_rect(
        &mut self,
        src_top: u16,
        src_left: u16,
        src_bottom: u16,
        src_right: u16,
        dst_top: u16,
        dst_left: u16,
    ) {
        self.storage.clear_pending_wrap();
        // Clamp source to visible area
        let max_row = self.storage.visible_rows.saturating_sub(1);
        let max_col = self.storage.cols.saturating_sub(1);
        let src_top = src_top.min(max_row);
        let src_bottom = src_bottom.min(max_row);
        let src_left = src_left.min(max_col);
        let src_right = src_right.min(max_col);

        // Validate source rectangle
        if src_top > src_bottom || src_left > src_right {
            return;
        }

        let rect_height = src_bottom - src_top + 1;
        let rect_width = src_right - src_left + 1;

        // Clamp destination start
        let dst_top = dst_top.min(max_row);
        let dst_left = dst_left.min(max_col);

        // Copy source cells into a temporary buffer (handles overlap).
        // Wide-char fixup at source boundaries (#7316): if the source rect
        // cuts a wide character pair, replace the orphaned half with a space.
        let mut buf: Vec<Vec<super::Cell>> = Vec::with_capacity(rect_height as usize);
        for row_offset in 0..rect_height {
            let src_row = src_top + row_offset;
            let mut row_cells = Vec::with_capacity(rect_width as usize);
            for col_offset in 0..rect_width {
                let src_col = src_left + col_offset;
                let cell = self
                    .cell(src_row, src_col)
                    .copied()
                    .unwrap_or(super::Cell::EMPTY);
                row_cells.push(cell);
            }
            // Left boundary: first cell is WIDE_CONTINUATION without its WIDE base
            if let Some(first) = row_cells.first()
                && first.flags().contains(super::CellFlags::WIDE_CONTINUATION)
            {
                row_cells[0] = super::Cell::EMPTY;
            }
            // Right boundary: last cell is WIDE without its continuation
            if let Some(last) = row_cells.last()
                && last.flags().contains(super::CellFlags::WIDE)
            {
                let last_idx = row_cells.len() - 1;
                row_cells[last_idx] = super::Cell::EMPTY;
            }
            buf.push(row_cells);
        }

        // Collect source extras into a temporary buffer (handles overlap).
        let mut extras_buf: Vec<(u16, u16, super::CellExtra)> = Vec::new();
        for row_offset in 0..rect_height {
            let src_row = src_top + row_offset;
            for col_offset in 0..rect_width {
                let src_col = src_left + col_offset;
                let coord = super::CellCoord {
                    row: src_row,
                    col: src_col,
                };
                if let Some(extra) = self.storage.extras.get(coord) {
                    extras_buf.push((row_offset, col_offset, extra.clone()));
                }
            }
        }

        // Write buffered cells to destination.
        // Destination wide-char fixup (#7316): before overwriting, clear orphaned
        // halves of existing wide characters at the destination boundaries.
        for (row_offset, row_cells) in buf.iter().enumerate() {
            #[allow(
                clippy::cast_possible_truncation,
                reason = "row_offset bounded by rect_height which is u16"
            )]
            let dst_row = dst_top.saturating_add(row_offset as u16);
            if dst_row > max_row {
                break;
            }
            let actual_width = row_cells.len().min((max_col - dst_left + 1) as usize);
            if actual_width > 0
                && let Some(row) = self.row_mut(dst_row)
            {
                row.fixup_wide_chars_in_range(dst_left, actual_width as u16);
            }
            for (col_offset, &cell) in row_cells.iter().enumerate() {
                #[allow(
                    clippy::cast_possible_truncation,
                    reason = "col_offset bounded by rect_width which is u16"
                )]
                let dst_col = dst_left.saturating_add(col_offset as u16);
                if dst_col > max_col {
                    break;
                }
                if let Some(row) = self.row_mut(dst_row) {
                    row.set(dst_col, cell);
                }
            }
            self.storage.damage.mark_row(dst_row);
        }

        // Clear destination extras in the target rectangle, then write source extras.
        let dst_bottom_eff = dst_top
            .saturating_add(rect_height.saturating_sub(1))
            .min(max_row);
        let dst_right_eff = dst_left
            .saturating_add(rect_width.saturating_sub(1))
            .min(max_col);
        self.storage.extras.clear_rect(
            dst_top..dst_bottom_eff.saturating_add(1),
            dst_left..dst_right_eff.saturating_add(1),
        );
        for (row_offset, col_offset, extra) in extras_buf {
            let dst_row = dst_top.saturating_add(row_offset);
            let dst_col = dst_left.saturating_add(col_offset);
            if dst_row <= max_row && dst_col <= max_col {
                let coord = super::CellCoord {
                    row: dst_row,
                    col: dst_col,
                };
                self.storage.extras.set(coord, extra);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Grid;
    use crate::cell_flags::CellFlags;

    /// Helper: write ASCII text at the current cursor position.
    fn write_text(grid: &mut Grid, text: &str) {
        for c in text.chars() {
            grid.write_char(c);
        }
    }

    /// Helper: read the character at a visible grid position.
    fn char_at(grid: &Grid, row: u16, col: u16) -> char {
        grid.cell(row, col).map_or(' ', |c| c.char())
    }

    /// Helper: check if cell at position is empty (space with default colors, no flags).
    fn is_empty_at(grid: &Grid, row: u16, col: u16) -> bool {
        grid.cell(row, col).is_none_or(|c| c.is_empty())
    }

    /// Helper: set the PROTECTED flag on a cell.
    fn protect_cell(grid: &mut Grid, row: u16, col: u16) {
        if let Some(cell) = grid.cell_mut(row, col) {
            let flags = cell.flags().union(CellFlags::PROTECTED);
            cell.set_flags(flags);
        }
    }

    /// Helper: fill a row with distinct characters 'A'+col_offset.
    fn fill_row(grid: &mut Grid, row: u16, cols: u16) {
        grid.move_cursor_to(row, 0);
        for col in 0..cols {
            let ch = (b'A' + (col % 26) as u8) as char;
            grid.write_char(ch);
        }
    }

    /// Helper: fill entire grid with distinct characters.
    fn fill_grid(grid: &mut Grid, rows: u16, cols: u16) {
        for row in 0..rows {
            fill_row(grid, row, cols);
        }
    }

    // =========================================================================
    // erase_to_end_of_line (EL 0)
    // =========================================================================

    #[test]
    fn test_el0_erases_from_cursor_to_end() {
        let mut grid = Grid::new(4, 10);
        fill_row(&mut grid, 0, 10);
        grid.move_cursor_to(0, 5);
        grid.erase_to_end_of_line();

        // Columns 0-4 should be preserved.
        for col in 0..5 {
            assert!(!is_empty_at(&grid, 0, col), "col {col} should be preserved");
        }
        // Columns 5-9 should be erased.
        for col in 5..10 {
            assert_eq!(char_at(&grid, 0, col), ' ', "col {col} should be erased");
        }
    }

    #[test]
    fn test_el0_cursor_at_column_zero_erases_entire_line() {
        let mut grid = Grid::new(4, 10);
        fill_row(&mut grid, 0, 10);
        grid.move_cursor_to(0, 0);
        grid.erase_to_end_of_line();

        for col in 0..10 {
            assert_eq!(char_at(&grid, 0, col), ' ', "col {col} should be erased");
        }
    }

    #[test]
    fn test_el0_cursor_at_last_column() {
        let mut grid = Grid::new(4, 10);
        fill_row(&mut grid, 0, 10);
        grid.move_cursor_to(0, 9);
        grid.erase_to_end_of_line();

        // Columns 0-8 preserved.
        for col in 0..9 {
            assert!(!is_empty_at(&grid, 0, col), "col {col} should be preserved");
        }
        // Column 9 erased.
        assert_eq!(char_at(&grid, 0, 9), ' ', "last col should be erased");
    }

    #[test]
    fn test_el0_clears_pending_wrap() {
        let mut grid = Grid::new(4, 5);
        write_text(&mut grid, "ABCD");
        grid.set_pending_wrap(true);
        assert!(grid.pending_wrap(), "pending_wrap should be set");
        grid.erase_to_end_of_line();
        assert!(
            !grid.pending_wrap(),
            "erase_to_end_of_line should clear pending_wrap"
        );
    }

    #[test]
    fn test_el0_does_not_affect_other_rows() {
        let mut grid = Grid::new(4, 10);
        fill_row(&mut grid, 0, 10);
        fill_row(&mut grid, 1, 10);
        grid.move_cursor_to(0, 5);
        grid.erase_to_end_of_line();

        // Row 1 should be completely untouched.
        for col in 0..10 {
            assert!(
                !is_empty_at(&grid, 1, col),
                "row 1 col {col} should be preserved"
            );
        }
    }

    // =========================================================================
    // erase_from_start_of_line (EL 1)
    // =========================================================================

    #[test]
    fn test_el1_erases_from_start_to_cursor() {
        let mut grid = Grid::new(4, 10);
        fill_row(&mut grid, 0, 10);
        grid.move_cursor_to(0, 4);
        grid.erase_from_start_of_line();

        // Columns 0-4 should be erased (inclusive of cursor).
        for col in 0..=4 {
            assert_eq!(char_at(&grid, 0, col), ' ', "col {col} should be erased");
        }
        // Columns 5-9 should be preserved.
        for col in 5..10 {
            assert!(!is_empty_at(&grid, 0, col), "col {col} should be preserved");
        }
    }

    #[test]
    fn test_el1_cursor_at_last_column_erases_entire_line() {
        let mut grid = Grid::new(4, 10);
        fill_row(&mut grid, 0, 10);
        grid.move_cursor_to(0, 9);
        grid.erase_from_start_of_line();

        for col in 0..10 {
            assert_eq!(char_at(&grid, 0, col), ' ', "col {col} should be erased");
        }
    }

    #[test]
    fn test_el1_cursor_at_column_zero() {
        let mut grid = Grid::new(4, 10);
        fill_row(&mut grid, 0, 10);
        grid.move_cursor_to(0, 0);
        grid.erase_from_start_of_line();

        // Only column 0 should be erased.
        assert_eq!(char_at(&grid, 0, 0), ' ', "col 0 should be erased");
        for col in 1..10 {
            assert!(!is_empty_at(&grid, 0, col), "col {col} should be preserved");
        }
    }

    #[test]
    fn test_el1_clears_pending_wrap() {
        let mut grid = Grid::new(4, 5);
        write_text(&mut grid, "ABCD");
        grid.set_pending_wrap(true);
        assert!(grid.pending_wrap());
        grid.erase_from_start_of_line();
        assert!(!grid.pending_wrap());
    }

    // =========================================================================
    // erase_line (EL 2)
    // =========================================================================

    #[test]
    fn test_el2_erases_entire_line() {
        let mut grid = Grid::new(4, 10);
        fill_row(&mut grid, 0, 10);
        grid.move_cursor_to(0, 5);
        grid.erase_line();

        for col in 0..10 {
            assert_eq!(char_at(&grid, 0, col), ' ', "col {col} should be erased");
        }
    }

    #[test]
    fn test_el2_does_not_affect_other_rows() {
        let mut grid = Grid::new(4, 10);
        fill_grid(&mut grid, 4, 10);
        grid.move_cursor_to(1, 3);
        grid.erase_line();

        // Row 0 and 2 should be intact.
        assert!(!is_empty_at(&grid, 0, 0));
        assert!(!is_empty_at(&grid, 2, 0));
        // Row 1 should be erased.
        for col in 0..10 {
            assert_eq!(
                char_at(&grid, 1, col),
                ' ',
                "row 1 col {col} should be erased"
            );
        }
    }

    #[test]
    fn test_el2_clears_pending_wrap() {
        let mut grid = Grid::new(4, 5);
        grid.set_pending_wrap(true);
        assert!(grid.pending_wrap());
        grid.erase_line();
        assert!(!grid.pending_wrap());
    }

    // =========================================================================
    // erase_to_end_of_screen (ED 0)
    // =========================================================================

    #[test]
    fn test_ed0_erases_from_cursor_to_end_of_screen() {
        let mut grid = Grid::new(4, 10);
        fill_grid(&mut grid, 4, 10);
        grid.move_cursor_to(1, 5);
        grid.erase_to_end_of_screen();

        // Row 0 should be completely preserved.
        for col in 0..10 {
            assert!(
                !is_empty_at(&grid, 0, col),
                "row 0 col {col} should be preserved"
            );
        }
        // Row 1, cols 0-4 preserved, cols 5-9 erased.
        for col in 0..5 {
            assert!(
                !is_empty_at(&grid, 1, col),
                "row 1 col {col} should be preserved"
            );
        }
        for col in 5..10 {
            assert_eq!(
                char_at(&grid, 1, col),
                ' ',
                "row 1 col {col} should be erased"
            );
        }
        // Rows 2-3 should be fully erased.
        for row in 2..4 {
            for col in 0..10 {
                assert_eq!(
                    char_at(&grid, row, col),
                    ' ',
                    "row {row} col {col} should be erased"
                );
            }
        }
    }

    #[test]
    fn test_ed0_cursor_at_top_left_erases_everything() {
        let mut grid = Grid::new(4, 10);
        fill_grid(&mut grid, 4, 10);
        grid.move_cursor_to(0, 0);
        grid.erase_to_end_of_screen();

        for row in 0..4 {
            for col in 0..10 {
                assert_eq!(
                    char_at(&grid, row, col),
                    ' ',
                    "({row},{col}) should be erased"
                );
            }
        }
    }

    #[test]
    fn test_ed0_cursor_at_last_row_last_col() {
        let mut grid = Grid::new(4, 10);
        fill_grid(&mut grid, 4, 10);
        grid.move_cursor_to(3, 9);
        grid.erase_to_end_of_screen();

        // Only cell (3,9) should be erased.
        assert!(!is_empty_at(&grid, 3, 8));
        assert_eq!(char_at(&grid, 3, 9), ' ');
    }

    #[test]
    fn test_ed0_clears_pending_wrap() {
        let mut grid = Grid::new(4, 5);
        grid.set_pending_wrap(true);
        assert!(grid.pending_wrap());
        grid.erase_to_end_of_screen();
        assert!(!grid.pending_wrap());
    }

    // =========================================================================
    // erase_from_start_of_screen (ED 1)
    // =========================================================================

    #[test]
    fn test_ed1_erases_from_start_of_screen_to_cursor() {
        let mut grid = Grid::new(4, 10);
        fill_grid(&mut grid, 4, 10);
        grid.move_cursor_to(2, 4);
        grid.erase_from_start_of_screen();

        // Rows 0-1 should be fully erased.
        for row in 0..2 {
            for col in 0..10 {
                assert_eq!(
                    char_at(&grid, row, col),
                    ' ',
                    "({row},{col}) should be erased"
                );
            }
        }
        // Row 2, cols 0-4 erased.
        for col in 0..=4 {
            assert_eq!(
                char_at(&grid, 2, col),
                ' ',
                "row 2 col {col} should be erased"
            );
        }
        // Row 2, cols 5-9 preserved.
        for col in 5..10 {
            assert!(
                !is_empty_at(&grid, 2, col),
                "row 2 col {col} should be preserved"
            );
        }
        // Row 3 should be preserved.
        for col in 0..10 {
            assert!(
                !is_empty_at(&grid, 3, col),
                "row 3 col {col} should be preserved"
            );
        }
    }

    #[test]
    fn test_ed1_cursor_at_bottom_right_erases_everything() {
        let mut grid = Grid::new(4, 10);
        fill_grid(&mut grid, 4, 10);
        grid.move_cursor_to(3, 9);
        grid.erase_from_start_of_screen();

        for row in 0..4 {
            for col in 0..10 {
                assert_eq!(
                    char_at(&grid, row, col),
                    ' ',
                    "({row},{col}) should be erased"
                );
            }
        }
    }

    #[test]
    fn test_ed1_clears_pending_wrap() {
        let mut grid = Grid::new(4, 5);
        grid.set_pending_wrap(true);
        assert!(grid.pending_wrap());
        grid.erase_from_start_of_screen();
        assert!(!grid.pending_wrap());
    }

    // =========================================================================
    // erase_screen (ED 2)
    // =========================================================================

    #[test]
    fn test_ed2_erases_entire_screen() {
        let mut grid = Grid::new(4, 10);
        fill_grid(&mut grid, 4, 10);
        grid.move_cursor_to(2, 5);
        grid.erase_screen();

        for row in 0..4 {
            for col in 0..10 {
                assert_eq!(
                    char_at(&grid, row, col),
                    ' ',
                    "({row},{col}) should be erased"
                );
            }
        }
    }

    #[test]
    fn test_ed2_resets_display_offset() {
        let mut grid = Grid::new(4, 10);
        // Artificially check that display_offset is 0 after erase_screen.
        grid.erase_screen();
        assert_eq!(grid.display_offset(), 0);
    }

    #[test]
    fn test_ed2_clears_pending_wrap() {
        let mut grid = Grid::new(4, 5);
        grid.set_pending_wrap(true);
        assert!(grid.pending_wrap());
        grid.erase_screen();
        assert!(!grid.pending_wrap());
    }

    // =========================================================================
    // erase_scrollback (ED 3)
    // =========================================================================

    #[test]
    fn test_erase_scrollback_no_scrollback() {
        let mut grid = Grid::new(4, 10);
        fill_grid(&mut grid, 4, 10);
        // No scrollback was generated, total_lines == visible_rows.
        grid.erase_scrollback();
        assert_eq!(grid.display_offset(), 0);
        assert_eq!(grid.total_lines(), 4);
    }

    #[test]
    fn test_erase_scrollback_clears_pending_wrap() {
        let mut grid = Grid::new(4, 5);
        grid.set_pending_wrap(true);
        assert!(grid.pending_wrap());
        grid.erase_scrollback();
        assert!(!grid.pending_wrap());
    }

    // =========================================================================
    // selective_erase_to_end_of_line (DECSEL 0)
    // =========================================================================

    #[test]
    fn test_sel0_erases_unprotected_from_cursor_to_end() {
        let mut grid = Grid::new(4, 10);
        fill_row(&mut grid, 0, 10);

        // Protect cells at columns 6 and 8.
        protect_cell(&mut grid, 0, 6);
        protect_cell(&mut grid, 0, 8);

        grid.move_cursor_to(0, 5);
        grid.selective_erase_to_end_of_line();

        // Cols 0-4 preserved (before cursor).
        for col in 0..5 {
            assert!(!is_empty_at(&grid, 0, col), "col {col} should be preserved");
        }
        // Col 5 erased (unprotected, at cursor).
        assert_eq!(char_at(&grid, 0, 5), ' ');
        // Col 6 protected -> preserved.
        assert_ne!(
            char_at(&grid, 0, 6),
            ' ',
            "col 6 is protected, should be preserved"
        );
        // Col 7 erased.
        assert_eq!(char_at(&grid, 0, 7), ' ');
        // Col 8 protected -> preserved.
        assert_ne!(
            char_at(&grid, 0, 8),
            ' ',
            "col 8 is protected, should be preserved"
        );
        // Col 9 erased.
        assert_eq!(char_at(&grid, 0, 9), ' ');
    }

    #[test]
    fn test_sel0_all_protected_nothing_erased() {
        let mut grid = Grid::new(4, 5);
        fill_row(&mut grid, 0, 5);
        for col in 0..5 {
            protect_cell(&mut grid, 0, col);
        }
        grid.move_cursor_to(0, 0);
        grid.selective_erase_to_end_of_line();

        // All protected, nothing erased.
        for col in 0..5 {
            assert_ne!(char_at(&grid, 0, col), ' ', "col {col} is protected");
        }
    }

    #[test]
    fn test_sel0_clears_pending_wrap() {
        let mut grid = Grid::new(4, 5);
        grid.set_pending_wrap(true);
        assert!(grid.pending_wrap());
        grid.selective_erase_to_end_of_line();
        assert!(!grid.pending_wrap());
    }

    // =========================================================================
    // selective_erase_from_start_of_line (DECSEL 1)
    // =========================================================================

    #[test]
    fn test_sel1_erases_unprotected_from_start_to_cursor() {
        let mut grid = Grid::new(4, 10);
        fill_row(&mut grid, 0, 10);

        // Protect cells at columns 1 and 3.
        protect_cell(&mut grid, 0, 1);
        protect_cell(&mut grid, 0, 3);

        grid.move_cursor_to(0, 5);
        grid.selective_erase_from_start_of_line();

        // Col 0 erased.
        assert_eq!(char_at(&grid, 0, 0), ' ');
        // Col 1 protected -> preserved.
        assert_ne!(char_at(&grid, 0, 1), ' ');
        // Col 2 erased.
        assert_eq!(char_at(&grid, 0, 2), ' ');
        // Col 3 protected -> preserved.
        assert_ne!(char_at(&grid, 0, 3), ' ');
        // Col 4-5 erased (cursor inclusive).
        assert_eq!(char_at(&grid, 0, 4), ' ');
        assert_eq!(char_at(&grid, 0, 5), ' ');
        // Cols 6-9 preserved (after cursor).
        for col in 6..10 {
            assert!(!is_empty_at(&grid, 0, col), "col {col} should be preserved");
        }
    }

    #[test]
    fn test_sel1_clears_pending_wrap() {
        let mut grid = Grid::new(4, 5);
        grid.set_pending_wrap(true);
        assert!(grid.pending_wrap());
        grid.selective_erase_from_start_of_line();
        assert!(!grid.pending_wrap());
    }

    // =========================================================================
    // selective_erase_line (DECSEL 2)
    // =========================================================================

    #[test]
    fn test_sel2_erases_unprotected_entire_line() {
        let mut grid = Grid::new(4, 10);
        fill_row(&mut grid, 0, 10);

        // Protect columns 0, 5, 9.
        protect_cell(&mut grid, 0, 0);
        protect_cell(&mut grid, 0, 5);
        protect_cell(&mut grid, 0, 9);

        grid.move_cursor_to(0, 3);
        grid.selective_erase_line();

        // Protected cells preserved.
        assert_ne!(char_at(&grid, 0, 0), ' ', "col 0 is protected");
        assert_ne!(char_at(&grid, 0, 5), ' ', "col 5 is protected");
        assert_ne!(char_at(&grid, 0, 9), ' ', "col 9 is protected");
        // Unprotected cells erased.
        for col in [1, 2, 3, 4, 6, 7, 8] {
            assert_eq!(char_at(&grid, 0, col), ' ', "col {col} should be erased");
        }
    }

    #[test]
    fn test_sel2_clears_pending_wrap() {
        let mut grid = Grid::new(4, 5);
        grid.set_pending_wrap(true);
        assert!(grid.pending_wrap());
        grid.selective_erase_line();
        assert!(!grid.pending_wrap());
    }

    // =========================================================================
    // selective_erase_to_end_of_screen (DECSED 0)
    // =========================================================================

    #[test]
    fn test_decsed0_erases_unprotected_to_end_of_screen() {
        let mut grid = Grid::new(3, 5);
        fill_grid(&mut grid, 3, 5);

        // Protect cell at row 1, col 3 and row 2, col 0.
        protect_cell(&mut grid, 1, 3);
        protect_cell(&mut grid, 2, 0);

        grid.move_cursor_to(1, 2);
        grid.selective_erase_to_end_of_screen();

        // Row 0 entirely preserved.
        for col in 0..5 {
            assert!(!is_empty_at(&grid, 0, col), "row 0 col {col} preserved");
        }
        // Row 1, cols 0-1 preserved (before cursor).
        assert!(!is_empty_at(&grid, 1, 0));
        assert!(!is_empty_at(&grid, 1, 1));
        // Row 1, col 2 erased (unprotected, at cursor).
        assert_eq!(char_at(&grid, 1, 2), ' ');
        // Row 1, col 3 protected -> preserved.
        assert_ne!(char_at(&grid, 1, 3), ' ');
        // Row 1, col 4 erased.
        assert_eq!(char_at(&grid, 1, 4), ' ');
        // Row 2, col 0 protected -> preserved.
        assert_ne!(char_at(&grid, 2, 0), ' ');
        // Row 2, cols 1-4 erased.
        for col in 1..5 {
            assert_eq!(char_at(&grid, 2, col), ' ', "row 2 col {col} erased");
        }
    }

    #[test]
    fn test_decsed0_clears_pending_wrap() {
        let mut grid = Grid::new(4, 5);
        grid.set_pending_wrap(true);
        assert!(grid.pending_wrap());
        grid.selective_erase_to_end_of_screen();
        assert!(!grid.pending_wrap());
    }

    // =========================================================================
    // selective_erase_from_start_of_screen (DECSED 1)
    // =========================================================================

    #[test]
    fn test_decsed1_erases_unprotected_from_start_of_screen() {
        let mut grid = Grid::new(3, 5);
        fill_grid(&mut grid, 3, 5);

        // Protect cell at row 0, col 2 and row 1, col 1.
        protect_cell(&mut grid, 0, 2);
        protect_cell(&mut grid, 1, 1);

        grid.move_cursor_to(1, 3);
        grid.selective_erase_from_start_of_screen();

        // Row 0, col 2 protected.
        assert_ne!(char_at(&grid, 0, 2), ' ');
        // Row 0, other cols erased.
        assert_eq!(char_at(&grid, 0, 0), ' ');
        assert_eq!(char_at(&grid, 0, 1), ' ');
        assert_eq!(char_at(&grid, 0, 3), ' ');
        assert_eq!(char_at(&grid, 0, 4), ' ');
        // Row 1, col 1 protected.
        assert_ne!(char_at(&grid, 1, 1), ' ');
        // Row 1, cols 0, 2, 3 erased (up to and including cursor).
        assert_eq!(char_at(&grid, 1, 0), ' ');
        assert_eq!(char_at(&grid, 1, 2), ' ');
        assert_eq!(char_at(&grid, 1, 3), ' ');
        // Row 1, col 4 preserved (after cursor).
        assert!(!is_empty_at(&grid, 1, 4));
        // Row 2 entirely preserved.
        for col in 0..5 {
            assert!(!is_empty_at(&grid, 2, col), "row 2 col {col} preserved");
        }
    }

    #[test]
    fn test_decsed1_clears_pending_wrap() {
        let mut grid = Grid::new(4, 5);
        grid.set_pending_wrap(true);
        assert!(grid.pending_wrap());
        grid.selective_erase_from_start_of_screen();
        assert!(!grid.pending_wrap());
    }

    // =========================================================================
    // selective_erase_screen (DECSED 2)
    // =========================================================================

    #[test]
    fn test_decsed2_erases_unprotected_entire_screen() {
        let mut grid = Grid::new(3, 5);
        fill_grid(&mut grid, 3, 5);

        // Protect one cell per row.
        protect_cell(&mut grid, 0, 0);
        protect_cell(&mut grid, 1, 2);
        protect_cell(&mut grid, 2, 4);

        grid.selective_erase_screen();

        // Protected cells preserved.
        assert_ne!(char_at(&grid, 0, 0), ' ');
        assert_ne!(char_at(&grid, 1, 2), ' ');
        assert_ne!(char_at(&grid, 2, 4), ' ');

        // All other cells erased.
        for col in 1..5 {
            assert_eq!(char_at(&grid, 0, col), ' ', "row 0 col {col}");
        }
        for col in [0, 1, 3, 4] {
            assert_eq!(char_at(&grid, 1, col), ' ', "row 1 col {col}");
        }
        for col in 0..4 {
            assert_eq!(char_at(&grid, 2, col), ' ', "row 2 col {col}");
        }
    }

    #[test]
    fn test_decsed2_clears_pending_wrap() {
        let mut grid = Grid::new(4, 5);
        grid.set_pending_wrap(true);
        assert!(grid.pending_wrap());
        grid.selective_erase_screen();
        assert!(!grid.pending_wrap());
    }

    // =========================================================================
    // screen_alignment_pattern (DECALN)
    // =========================================================================

    #[test]
    fn test_decaln_fills_entire_screen_with_e() {
        let mut grid = Grid::new(4, 10);
        grid.screen_alignment_pattern();

        for row in 0..4 {
            for col in 0..10 {
                assert_eq!(char_at(&grid, row, col), 'E', "({row},{col}) should be 'E'");
            }
        }
    }

    #[test]
    fn test_decaln_resets_cursor_to_home() {
        let mut grid = Grid::new(4, 10);
        grid.move_cursor_to(3, 8);
        grid.screen_alignment_pattern();

        assert_eq!(grid.cursor_row(), 0, "cursor row should be 0 after DECALN");
        assert_eq!(grid.cursor_col(), 0, "cursor col should be 0 after DECALN");
    }

    #[test]
    fn test_decaln_resets_scroll_region() {
        let mut grid = Grid::new(4, 10);
        // The scroll_region should be full after DECALN.
        grid.screen_alignment_pattern();

        let sr = grid.scroll_region();
        assert_eq!(sr.top, 0);
        assert_eq!(sr.bottom, 3);
    }

    #[test]
    fn test_decaln_overwrites_existing_content() {
        let mut grid = Grid::new(4, 10);
        fill_grid(&mut grid, 4, 10);

        // Verify some cells are not 'E'.
        assert_ne!(char_at(&grid, 0, 0), 'E');

        grid.screen_alignment_pattern();

        for row in 0..4 {
            for col in 0..10 {
                assert_eq!(char_at(&grid, row, col), 'E');
            }
        }
    }

    #[test]
    fn test_decaln_clears_pending_wrap() {
        let mut grid = Grid::new(4, 5);
        grid.set_pending_wrap(true);
        assert!(grid.pending_wrap());
        grid.screen_alignment_pattern();
        assert!(!grid.pending_wrap());
    }

    #[test]
    fn test_decaln_cells_have_default_attributes() {
        let mut grid = Grid::new(2, 5);
        grid.screen_alignment_pattern();

        // Cells should have 'E' with default colors and no flags.
        let cell = grid.cell(0, 0).unwrap();
        assert_eq!(cell.char(), 'E');
        assert!(cell.flags().is_empty(), "DECALN cells should have no flags");
    }

    /// DECALN must clear the `any_double_width` optimization flag after
    /// resetting all rows to single-width via `clear()`.  Without this,
    /// cursor ops unnecessarily check for double-width column clamping.
    #[test]
    fn test_decaln_clears_any_double_width_flag() {
        let mut grid = Grid::new(5, 40);
        // Set a row to double-width via the row API, then mark the grid flag.
        if let Some(r) = grid.row_mut(1) {
            r.set_line_size(crate::LineSize::DoubleWidth);
        }
        grid.mark_has_double_width();
        assert!(grid.storage.any_double_width);

        // DECALN resets all rows to single-width.
        grid.screen_alignment_pattern();
        assert!(
            !grid.storage.any_double_width,
            "DECALN should clear any_double_width after resetting all rows"
        );
    }

    // =========================================================================
    // erase_rect (DECERA)
    // =========================================================================

    #[test]
    fn test_erase_rect_clears_region() {
        let mut grid = Grid::new(4, 10);
        fill_grid(&mut grid, 4, 10);
        grid.erase_rect(1, 2, 2, 5);

        // Erased region: rows 1-2, cols 2-5.
        for row in 1..=2 {
            for col in 2..=5 {
                assert_eq!(
                    char_at(&grid, row, col),
                    ' ',
                    "({row},{col}) should be erased"
                );
            }
        }
        // Surrounding cells should be preserved.
        assert!(!is_empty_at(&grid, 0, 2));
        assert!(!is_empty_at(&grid, 3, 2));
        assert!(!is_empty_at(&grid, 1, 1));
        assert!(!is_empty_at(&grid, 1, 6));
    }

    #[test]
    fn test_erase_rect_single_cell() {
        let mut grid = Grid::new(4, 10);
        fill_grid(&mut grid, 4, 10);
        grid.erase_rect(2, 3, 2, 3);

        assert_eq!(char_at(&grid, 2, 3), ' ');
        assert!(!is_empty_at(&grid, 2, 2));
        assert!(!is_empty_at(&grid, 2, 4));
    }

    #[test]
    fn test_erase_rect_inverted_coords_noop() {
        let mut grid = Grid::new(4, 10);
        fill_grid(&mut grid, 4, 10);
        grid.erase_rect(3, 5, 1, 2); // top > bottom, left > right

        // Nothing should be erased.
        for row in 0..4 {
            for col in 0..10 {
                assert!(
                    !is_empty_at(&grid, row, col),
                    "({row},{col}) should still have content"
                );
            }
        }
    }

    #[test]
    fn test_erase_rect_clamps_to_bounds() {
        let mut grid = Grid::new(4, 10);
        fill_grid(&mut grid, 4, 10);
        // Coordinates exceed grid bounds — should clamp.
        grid.erase_rect(2, 8, 100, 100);

        // Should erase rows 2-3, cols 8-9 (clamped).
        for row in 2..4 {
            for col in 8..10 {
                assert_eq!(
                    char_at(&grid, row, col),
                    ' ',
                    "({row},{col}) should be erased"
                );
            }
        }
    }

    // =========================================================================
    // fill_rect (DECFRA)
    // =========================================================================

    #[test]
    fn test_fill_rect_fills_with_character() {
        let mut grid = Grid::new(4, 10);
        let fill = crate::Cell::from_raw_parts(
            'X' as u16,
            crate::PackedColors::DEFAULT,
            CellFlags::empty(),
        );
        grid.fill_rect(fill, 1, 2, 2, 5);

        for row in 1..=2 {
            for col in 2..=5 {
                assert_eq!(char_at(&grid, row, col), 'X', "({row},{col}) should be 'X'");
            }
        }
    }

    #[test]
    fn test_fill_rect_does_not_affect_surrounding() {
        let mut grid = Grid::new(4, 10);
        fill_grid(&mut grid, 4, 10);
        let fill = crate::Cell::from_raw_parts(
            'Z' as u16,
            crate::PackedColors::DEFAULT,
            CellFlags::empty(),
        );
        grid.fill_rect(fill, 1, 3, 2, 6);

        // Outside the rect should be unchanged.
        assert_eq!(char_at(&grid, 0, 3), 'D');
        assert_eq!(char_at(&grid, 3, 3), 'D');
        assert_eq!(char_at(&grid, 1, 2), 'C');
    }

    // =========================================================================
    // selective_erase_rect (DECSERA)
    // =========================================================================

    #[test]
    fn test_selective_erase_rect_respects_protection() {
        let mut grid = Grid::new(4, 10);
        fill_grid(&mut grid, 4, 10);

        // Protect cell at (1, 3) and (2, 4).
        protect_cell(&mut grid, 1, 3);
        protect_cell(&mut grid, 2, 4);

        grid.selective_erase_rect(1, 2, 2, 5);

        // Protected cells preserved.
        assert_ne!(char_at(&grid, 1, 3), ' ', "(1,3) is protected");
        assert_ne!(char_at(&grid, 2, 4), ' ', "(2,4) is protected");

        // Unprotected cells in rect erased.
        assert_eq!(char_at(&grid, 1, 2), ' ');
        assert_eq!(char_at(&grid, 1, 4), ' ');
        assert_eq!(char_at(&grid, 1, 5), ' ');
        assert_eq!(char_at(&grid, 2, 2), ' ');
        assert_eq!(char_at(&grid, 2, 3), ' ');
        assert_eq!(char_at(&grid, 2, 5), ' ');
    }

    // =========================================================================
    // change_attrs_rect (DECCARA)
    // =========================================================================

    #[test]
    fn test_change_attrs_rect_sets_bold() {
        let mut grid = Grid::new(4, 10);
        fill_grid(&mut grid, 4, 10);

        grid.change_attrs_rect(1, 2, 2, 5, CellFlags::BOLD, CellFlags::empty());

        for row in 1..=2 {
            for col in 2..=5 {
                let cell = grid.cell(row, col).unwrap();
                assert!(
                    cell.flags().contains(CellFlags::BOLD),
                    "({row},{col}) should be bold"
                );
            }
        }
        // Outside the rect should not be bold.
        let cell_outside = grid.cell(0, 0).unwrap();
        assert!(!cell_outside.flags().contains(CellFlags::BOLD));
    }

    #[test]
    fn test_change_attrs_rect_clears_flags() {
        let mut grid = Grid::new(4, 10);
        fill_grid(&mut grid, 4, 10);

        // First set bold on a region.
        grid.change_attrs_rect(0, 0, 3, 9, CellFlags::BOLD, CellFlags::empty());
        // Then clear bold on a sub-region.
        grid.change_attrs_rect(1, 2, 2, 5, CellFlags::empty(), CellFlags::BOLD);

        // Inside the cleared sub-region should not be bold.
        for row in 1..=2 {
            for col in 2..=5 {
                let cell = grid.cell(row, col).unwrap();
                assert!(
                    !cell.flags().contains(CellFlags::BOLD),
                    "({row},{col}) should not be bold"
                );
            }
        }
        // Outside the sub-region should still be bold.
        assert!(grid.cell(0, 0).unwrap().flags().contains(CellFlags::BOLD));
        assert!(grid.cell(3, 9).unwrap().flags().contains(CellFlags::BOLD));
    }

    // =========================================================================
    // copy_rect (DECCRA)
    // =========================================================================

    #[test]
    fn test_copy_rect_basic() {
        let mut grid = Grid::new(4, 10);
        fill_grid(&mut grid, 4, 10);

        // Copy row 0, cols 0-4 to row 2, col 5.
        grid.copy_rect(0, 0, 0, 4, 2, 5);

        for col in 0..5 {
            assert_eq!(
                char_at(&grid, 2, col + 5),
                char_at(&grid, 0, col),
                "copied cell at col {} mismatch",
                col + 5
            );
        }
    }

    #[test]
    fn test_copy_rect_inverted_source_noop() {
        let mut grid = Grid::new(4, 10);
        fill_grid(&mut grid, 4, 10);

        // Save original state at destination.
        let orig = char_at(&grid, 2, 0);
        grid.copy_rect(3, 5, 1, 2, 2, 0); // inverted src

        // Destination unchanged.
        assert_eq!(char_at(&grid, 2, 0), orig);
    }

    // =========================================================================
    // clear_line_attributes
    // =========================================================================

    #[test]
    fn test_clear_line_attributes_runs_without_panic() {
        let mut grid = Grid::new(4, 10);
        // Should not panic on a fresh grid.
        grid.clear_line_attributes();
    }

    // =========================================================================
    // Edge cases
    // =========================================================================

    #[test]
    fn test_erase_on_1x1_grid() {
        let mut grid = Grid::new(1, 1);
        grid.write_char('Z');
        grid.move_cursor_to(0, 0);
        grid.erase_to_end_of_line();
        assert_eq!(char_at(&grid, 0, 0), ' ');
    }

    #[test]
    fn test_erase_line_on_1x1_grid() {
        let mut grid = Grid::new(1, 1);
        grid.write_char('Z');
        grid.erase_line();
        assert_eq!(char_at(&grid, 0, 0), ' ');
    }

    #[test]
    fn test_erase_screen_on_1x1_grid() {
        let mut grid = Grid::new(1, 1);
        grid.write_char('Z');
        grid.erase_screen();
        assert_eq!(char_at(&grid, 0, 0), ' ');
    }

    #[test]
    fn test_ed0_on_single_row_grid() {
        let mut grid = Grid::new(1, 10);
        fill_row(&mut grid, 0, 10);
        grid.move_cursor_to(0, 5);
        grid.erase_to_end_of_screen();

        for col in 0..5 {
            assert!(!is_empty_at(&grid, 0, col), "col {col} preserved");
        }
        for col in 5..10 {
            assert_eq!(char_at(&grid, 0, col), ' ', "col {col} erased");
        }
    }

    #[test]
    fn test_decaln_on_1x1_grid() {
        let mut grid = Grid::new(1, 1);
        grid.screen_alignment_pattern();
        assert_eq!(char_at(&grid, 0, 0), 'E');
        assert_eq!(grid.cursor_row(), 0);
        assert_eq!(grid.cursor_col(), 0);
    }

    #[test]
    fn test_selective_erase_no_protected_cells_erases_all() {
        let mut grid = Grid::new(2, 5);
        fill_grid(&mut grid, 2, 5);

        grid.selective_erase_screen();

        for row in 0..2 {
            for col in 0..5 {
                assert_eq!(
                    char_at(&grid, row, col),
                    ' ',
                    "({row},{col}) should be erased"
                );
            }
        }
    }

    #[test]
    fn test_multiple_erases_in_sequence() {
        let mut grid = Grid::new(4, 10);
        fill_grid(&mut grid, 4, 10);

        // First erase from cursor to end of line on row 1.
        grid.move_cursor_to(1, 5);
        grid.erase_to_end_of_line();
        for col in 5..10 {
            assert_eq!(char_at(&grid, 1, col), ' ');
        }

        // Then erase entire row 2.
        grid.move_cursor_to(2, 0);
        grid.erase_line();
        for col in 0..10 {
            assert_eq!(char_at(&grid, 2, col), ' ');
        }

        // Rows 0 and 3 should be completely untouched.
        for col in 0..10 {
            assert!(!is_empty_at(&grid, 0, col));
            assert!(!is_empty_at(&grid, 3, col));
        }
    }

    #[test]
    fn test_erase_rect_full_screen() {
        let mut grid = Grid::new(3, 5);
        fill_grid(&mut grid, 3, 5);
        grid.erase_rect(0, 0, 2, 4);

        for row in 0..3 {
            for col in 0..5 {
                assert_eq!(char_at(&grid, row, col), ' ', "({row},{col}) erased");
            }
        }
    }

    #[test]
    fn test_fill_rect_single_cell() {
        let mut grid = Grid::new(4, 10);
        let fill = crate::Cell::from_raw_parts(
            'Q' as u16,
            crate::PackedColors::DEFAULT,
            CellFlags::empty(),
        );
        grid.fill_rect(fill, 2, 3, 2, 3);
        assert_eq!(char_at(&grid, 2, 3), 'Q');
    }

    #[test]
    fn test_erase_rect_clears_pending_wrap() {
        let mut grid = Grid::new(4, 5);
        grid.set_pending_wrap(true);
        assert!(grid.pending_wrap());
        grid.erase_rect(0, 0, 0, 4);
        assert!(!grid.pending_wrap());
    }

    #[test]
    fn test_fill_rect_clears_pending_wrap() {
        let mut grid = Grid::new(4, 5);
        grid.set_pending_wrap(true);
        assert!(grid.pending_wrap());
        let fill = crate::Cell::from_raw_parts(
            'X' as u16,
            crate::PackedColors::DEFAULT,
            CellFlags::empty(),
        );
        grid.fill_rect(fill, 0, 0, 0, 4);
        assert!(!grid.pending_wrap());
    }

    #[test]
    fn test_selective_erase_rect_clears_pending_wrap() {
        let mut grid = Grid::new(4, 5);
        grid.set_pending_wrap(true);
        assert!(grid.pending_wrap());
        grid.selective_erase_rect(0, 0, 0, 4);
        assert!(!grid.pending_wrap());
    }

    #[test]
    fn test_change_attrs_rect_clears_pending_wrap() {
        let mut grid = Grid::new(4, 5);
        grid.set_pending_wrap(true);
        assert!(grid.pending_wrap());
        grid.change_attrs_rect(0, 0, 0, 4, CellFlags::BOLD, CellFlags::empty());
        assert!(!grid.pending_wrap());
    }

    #[test]
    fn test_copy_rect_clears_pending_wrap() {
        let mut grid = Grid::new(4, 5);
        grid.set_pending_wrap(true);
        assert!(grid.pending_wrap());
        grid.copy_rect(0, 0, 0, 4, 1, 0);
        assert!(!grid.pending_wrap());
    }

    #[test]
    fn test_fill_rect_preserves_cell_attributes() {
        use crate::{Cell, PackedColors};
        let mut grid = Grid::new(4, 10);
        // Create a styled cell template: 'X' with BOLD flag and indexed FG color.
        let colors = PackedColors::with_indexed(196, 0);
        let flags = CellFlags::BOLD;
        let template = Cell::from_ascii_styled(b'X', colors, flags);
        grid.fill_rect(template, 1, 2, 2, 5);

        for row in 1..=2 {
            for col in 2..=5 {
                let cell = grid.cell(row, col).expect("cell should exist");
                assert_eq!(cell.char(), 'X', "({row},{col}) should be 'X'");
                assert!(
                    cell.flags().contains(CellFlags::BOLD),
                    "({row},{col}) should have BOLD flag (#7655)"
                );
                assert!(
                    cell.colors().fg_is_indexed(),
                    "({row},{col}) should have indexed FG color (#7655)"
                );
            }
        }
    }
}
