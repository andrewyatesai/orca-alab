// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Row clear and selective erase operations.
//!
//! Handles full row clear, partial clear (from column to end), range clear,
//! and DECSCA-aware selective erase that preserves protected cells.

use super::super::cell::Cell;
use super::super::cell_flags::CellFlags;
use super::{Row, RowFlags};

impl Row {
    /// Clear the entire row.
    #[inline]
    pub fn clear(&mut self) {
        self.cells.fill(Cell::EMPTY);
        self.len = 0;
        self.flags = RowFlags::DIRTY;
    }

    /// Erase all cell content but preserve DEC line attributes (DECDWL/DECDHL).
    ///
    /// Per VT420/VT510 spec and xterm, erase operations (ED/EL) clear
    /// character positions but do not change line attributes. Use this
    /// instead of `clear()` in erase code paths (#7497).
    #[inline]
    pub fn erase(&mut self) {
        self.cells.fill(Cell::EMPTY);
        self.len = 0;
        self.flags = (self.flags & RowFlags::LINE_ATTRIBUTES) | RowFlags::DIRTY;
    }

    /// Erase with BCE fill cell, preserving DEC line attributes (#7522).
    ///
    /// Like `erase()` but fills cells with `fill` instead of `Cell::EMPTY`,
    /// supporting BCE (Background Color Erase) per VT420/xterm spec.
    #[inline]
    pub fn erase_with(&mut self, fill: Cell) {
        self.cells.fill(fill);
        // BCE fill cells are not "content" for len tracking.
        // If fill has a non-default bg, cells are still conceptually blank
        // (space char) but we set len to cols so renderers draw the bg.
        if fill.colors() == Cell::EMPTY.colors() {
            self.len = 0;
        } else {
            self.len = super::u16_from_usize(self.cells.len());
        }
        self.flags = (self.flags & RowFlags::LINE_ATTRIBUTES) | RowFlags::DIRTY;
    }

    /// Fully reset the row with a BCE fill cell in a single pass.
    ///
    /// Semantically identical to `clear()` followed by `erase_with(fill)`
    /// (cells = `fill`, len per BCE rule, flags = DIRTY with line attributes
    /// dropped) but writes each cell exactly once. Used by the scroll path
    /// when recycling ring-buffer rows, where the old content and line
    /// attributes are always discarded.
    #[inline]
    pub fn reset_with(&mut self, fill: Cell) {
        self.cells.fill(fill);
        // BCE fill cells are not "content" for len tracking (see erase_with).
        if fill.colors() == Cell::EMPTY.colors() {
            self.len = 0;
        } else {
            self.len = super::u16_from_usize(self.cells.len());
        }
        self.flags = RowFlags::DIRTY;
    }

    /// Clear cells from `start` to end of row.
    #[cfg(test)]
    #[inline]
    pub(crate) fn clear_from(&mut self, start: u16) {
        let start_usize = usize::from(start);
        if start_usize < self.cells.len() {
            let old_len = self.len as usize;

            // Wide character fixup: if start is a WIDE_CONTINUATION cell,
            // the WIDE cell at start-1 loses its pair and must be cleared.
            if start_usize > 0
                && self.cells[start_usize]
                    .flags()
                    .contains(CellFlags::WIDE_CONTINUATION)
            {
                self.cells[start_usize - 1] = Cell::EMPTY;
            }

            self.cells[start_usize..].fill(Cell::EMPTY);
            if start_usize < old_len {
                self.recalculate_len_up_to(start_usize);
            }
            self.flags |= RowFlags::DIRTY;
        }
    }

    /// Clear cells from start to `end` (exclusive).
    #[inline]
    #[allow(dead_code, reason = "used by Kani proofs and integration tests")]
    pub(crate) fn clear_range(&mut self, start: u16, end: u16) {
        let start = start as usize;
        let cols = self.cells.len();
        let end = (end as usize).min(cols);
        if start < end {
            let old_len = self.len as usize;

            // Wide character fixup: clearing at a boundary that bisects a wide
            // character pair creates orphaned halves that must be cleared.

            // Left boundary: if the first cleared cell is a WIDE_CONTINUATION,
            // the WIDE cell at start-1 loses its pair.
            if start > 0
                && self.cells[start]
                    .flags()
                    .contains(CellFlags::WIDE_CONTINUATION)
            {
                self.cells[start - 1] = Cell::EMPTY;
            }

            // Right boundary: if the cell just before `end` is WIDE, its
            // continuation at `end` is not cleared and becomes orphaned.
            if end > start && end < cols && self.cells[end - 1].flags().contains(CellFlags::WIDE) {
                self.cells[end] = Cell::EMPTY;
            }

            self.cells[start..end].fill(Cell::EMPTY);
            self.flags |= RowFlags::DIRTY;
            if start < old_len && end >= old_len {
                self.recalculate_len_up_to(start);
            }
        }
    }

    /// Clear cells from start to `end` (exclusive) with a BCE fill cell (#7522).
    ///
    /// Like `clear_range()` but fills with `fill` instead of `Cell::EMPTY`.
    #[inline]
    pub(crate) fn clear_range_with(&mut self, start: u16, end: u16, fill: Cell) {
        let start = start as usize;
        let cols = self.cells.len();
        let end = (end as usize).min(cols);
        if start < end {
            let old_len = self.len as usize;

            // Wide character fixup (same as clear_range but with fill cell).
            if start > 0
                && self.cells[start]
                    .flags()
                    .contains(CellFlags::WIDE_CONTINUATION)
            {
                self.cells[start - 1] = fill;
            }

            if end > start && end < cols && self.cells[end - 1].flags().contains(CellFlags::WIDE) {
                self.cells[end] = fill;
            }

            self.cells[start..end].fill(fill);
            self.flags |= RowFlags::DIRTY;
            if !fill.is_empty() {
                // Visible fill — a BCE background, a DECFRA fill character,
                // or attribute flags: len must cover the filled range so the
                // read path (row_text/render_row) does not drop it (a DECFRA
                // fill with default colors is still content, per VT420/VT520
                // DECFRA the filled characters are displayed).
                let new_end = end.max(old_len);
                self.len = super::u16_from_usize(new_end);
            } else if start < old_len && end >= old_len {
                self.recalculate_len_up_to(start);
            }
        }
    }

    /// Fix orphaned wide character halves at rectangular operation boundaries.
    ///
    /// After copying/clearing cells within columns [left, right], wide
    /// character pairs that span the boundary may have been bisected.
    /// Clears the orphaned half of any such pair (#7500).
    ///
    /// `left` and `right` are inclusive column indices. `cols` is the total
    /// number of columns in the row.
    #[inline]
    pub(crate) fn fixup_wide_boundary(&mut self, left: usize, right: usize, cols: usize) {
        // Left boundary: if left-1 is WIDE but left is not its continuation,
        // clear the orphaned WIDE cell.
        if left > 0 && left < self.cells.len() && left - 1 < self.cells.len() {
            let prev_wide = self.cells[left - 1].flags().contains(CellFlags::WIDE);
            let cur_cont = self.cells[left]
                .flags()
                .contains(CellFlags::WIDE_CONTINUATION);
            if prev_wide && !cur_cont {
                self.cells[left - 1] = Cell::EMPTY;
            }
            if cur_cont && !prev_wide {
                self.cells[left] = Cell::EMPTY;
            }
        }
        // Right boundary: fix orphaned pairs at right/right+1.
        if right < self.cells.len() && right + 1 < cols && right + 1 < self.cells.len() {
            let cur_wide = self.cells[right].flags().contains(CellFlags::WIDE);
            let next_cont = self.cells[right + 1]
                .flags()
                .contains(CellFlags::WIDE_CONTINUATION);
            if cur_wide && !next_cont {
                self.cells[right] = Cell::EMPTY;
            }
            if next_cont && !cur_wide {
                self.cells[right + 1] = Cell::EMPTY;
            }
        }
    }

    /// Context-aware protection check that disambiguates the shared
    /// `PROTECTED` / `WIDE_CONTINUATION` bit using neighbor information.
    ///
    /// `Cell::is_protected()` is unreliable for wide characters because
    /// `PROTECTED` and `WIDE_CONTINUATION` share bit 10. This method
    /// checks the previous cell to distinguish the two cases.
    #[inline]
    pub(crate) fn is_cell_protected(&self, col: u16) -> bool {
        let col = col as usize;
        if col >= self.cells.len() {
            return false;
        }
        let flags = self.cells[col].flags();

        // Bit 10 not set → definitely not protected
        if !flags.contains(CellFlags::PROTECTED) {
            return false;
        }

        // Bit 9 (WIDE) set → wide main cell, bit 10 = PROTECTED
        if flags.contains(CellFlags::WIDE) {
            return true;
        }

        // Bit 10 set, bit 9 clear → PROTECTED or WIDE_CONTINUATION?
        // Check if previous cell is a wide main cell → this is a continuation.
        // A continuation cell inherits protection from its WIDE parent.
        if col > 0 && self.cells[col - 1].flags().contains(CellFlags::WIDE) {
            // This is a continuation cell. It is protected iff the WIDE cell is.
            return self.cells[col - 1].flags().contains(CellFlags::PROTECTED);
        }

        true // normal protected cell
    }

    /// Context-aware wide-continuation check that disambiguates the shared
    /// `WIDE_CONTINUATION` / `PROTECTED` bit using neighbor information.
    ///
    /// `Cell::is_wide_continuation()` is unreliable for protected cells
    /// because `PROTECTED` and `WIDE_CONTINUATION` share bit 10. A true
    /// continuation spacer always immediately follows its `WIDE` main cell;
    /// a bit-10 cell without that neighbor is a DECSCA-protected cell.
    #[inline]
    pub(crate) fn is_cell_wide_continuation(&self, col: u16) -> bool {
        let col = col as usize;
        if col >= self.cells.len() {
            return false;
        }
        let flags = self.cells[col].flags();

        // Bit 10 not set → definitely not a continuation
        if !flags.contains(CellFlags::WIDE_CONTINUATION) {
            return false;
        }

        // Bit 9 (WIDE) set → wide main cell, bit 10 = PROTECTED
        if flags.contains(CellFlags::WIDE) {
            return false;
        }

        // Continuation iff the previous cell is the wide main cell.
        col > 0 && self.cells[col - 1].flags().contains(CellFlags::WIDE)
    }

    /// Selectively clear cells from start to `end` (exclusive).
    ///
    /// Only erases cells that are NOT protected (DECSCA).
    /// Protected cells are skipped. Uses context-aware protection
    /// check to correctly handle wide characters.
    #[inline]
    pub(crate) fn selective_clear_range(&mut self, start: u16, end: u16) {
        debug_assert!(
            (self.len as usize) <= self.cells.len(),
            "Row::selective_clear_range: self.len ({}) > cells.len() ({})",
            self.len,
            self.cells.len()
        );
        let start_usize = start as usize;
        let end_usize = (end as usize).min(self.cells.len());
        if start_usize < end_usize {
            let old_len = self.len as usize;
            let mut any_erased = false;

            // Left boundary fixup: if the first cell in the range is a
            // WIDE_CONTINUATION of an unprotected WIDE cell outside the
            // range, clearing the continuation creates an orphaned WIDE
            // cell. Clear the WIDE cell too (#7462).
            if start_usize > 0
                && self.cells[start_usize]
                    .flags()
                    .contains(CellFlags::WIDE_CONTINUATION)
                && self.cells[start_usize - 1]
                    .flags()
                    .contains(CellFlags::WIDE)
                && !self.is_cell_protected((start_usize - 1) as u16)
            {
                self.cells[start_usize - 1] = Cell::EMPTY;
                any_erased = true;
            }

            for col in start_usize..end_usize {
                if !self.is_cell_protected(col as u16) {
                    // If this is a WIDE cell, also clear its continuation so the
                    // forward iteration doesn't see an orphaned continuation whose
                    // WIDE parent was already cleared.
                    if self.cells[col].flags().contains(CellFlags::WIDE)
                        && col + 1 < self.cells.len()
                    {
                        self.cells[col + 1] = Cell::EMPTY;
                    }
                    self.cells[col] = Cell::EMPTY;
                    any_erased = true;
                }
            }
            if any_erased {
                self.flags |= RowFlags::DIRTY;
                if start_usize < old_len
                    && end_usize >= old_len
                    && self.cells[old_len - 1].is_empty()
                {
                    self.recalculate_len_up_to(old_len);
                }
            }
        }
    }

    /// Selectively wipe characters from `start` to `end` (exclusive),
    /// preserving visual attributes (DECSERA semantics).
    ///
    /// Per VT520 (EK-VT520-RM, DECSERA): erased positions become spaces, but
    /// "DECSERA does not change: visual attributes set by the select graphic
    /// rendition (SGR) function; protection attributes set by DECSCA; line
    /// attributes." xterm matches (ScrnWipeRectangle writes ' ' into
    /// charData without touching the attribute/color arrays). Contrast with
    /// [`Row::selective_clear_range`], which DECSED/DECSEL use and which
    /// resets the cells entirely (xterm ClearCells).
    ///
    /// Cells protected by DECSCA are skipped. Uses the context-aware
    /// protection check to correctly handle wide characters; a wiped wide
    /// character becomes two plain-width spaces (`set_char` clears the
    /// COMPLEX/WIDE/WIDE_CONTINUATION structural flags but leaves SGR
    /// flags, colors, and any interned style id intact).
    #[inline]
    pub(crate) fn selective_wipe_range(&mut self, start: u16, end: u16) {
        debug_assert!(
            (self.len as usize) <= self.cells.len(),
            "Row::selective_wipe_range: self.len ({}) > cells.len() ({})",
            self.len,
            self.cells.len()
        );
        let start_usize = start as usize;
        let end_usize = (end as usize).min(self.cells.len());
        if start_usize < end_usize {
            let old_len = self.len as usize;
            let mut any_erased = false;

            // Left boundary fixup: wiping the WIDE_CONTINUATION of an
            // unprotected WIDE cell outside the range would orphan the WIDE
            // cell — wipe it too (mirrors selective_clear_range, #7462).
            if start_usize > 0
                && self.cells[start_usize]
                    .flags()
                    .contains(CellFlags::WIDE_CONTINUATION)
                && self.cells[start_usize - 1]
                    .flags()
                    .contains(CellFlags::WIDE)
                && !self.is_cell_protected((start_usize - 1) as u16)
            {
                self.cells[start_usize - 1].set_char(' ');
                any_erased = true;
            }

            for col in start_usize..end_usize {
                if !self.is_cell_protected(col as u16) {
                    // If this is a WIDE cell, also wipe its continuation so
                    // the forward iteration never sees an orphaned spacer.
                    if self.cells[col].flags().contains(CellFlags::WIDE)
                        && col + 1 < self.cells.len()
                    {
                        self.cells[col + 1].set_char(' ');
                    }
                    self.cells[col].set_char(' ');
                    any_erased = true;
                }
            }
            if any_erased {
                self.flags |= RowFlags::DIRTY;
                if start_usize < old_len
                    && end_usize >= old_len
                    && self.cells[old_len - 1].is_empty()
                {
                    self.recalculate_len_up_to(old_len);
                }
            }
        }
    }

    /// Selectively clear the entire row.
    ///
    /// Only erases cells that are NOT protected (DECSCA).
    /// Protected cells are skipped. Uses context-aware protection
    /// check to correctly handle wide characters.
    #[inline]
    pub(crate) fn selective_clear(&mut self) {
        debug_assert!(
            (self.len as usize) <= self.cells.len(),
            "Row::selective_clear: self.len ({}) > cells.len() ({})",
            self.len,
            self.cells.len()
        );
        let old_len = self.len as usize;
        let mut any_erased = false;
        let cols = self.cells.len();
        for col in 0..cols {
            if !self.is_cell_protected(col as u16) {
                // If this is a WIDE cell, also clear its continuation so the
                // forward iteration doesn't see an orphaned continuation whose
                // WIDE parent was already cleared.
                if self.cells[col].flags().contains(CellFlags::WIDE) && col + 1 < cols {
                    self.cells[col + 1] = Cell::EMPTY;
                }
                self.cells[col] = Cell::EMPTY;
                any_erased = true;
            }
        }
        if any_erased {
            self.flags |= RowFlags::DIRTY;
            if old_len > 0 && self.cells[old_len - 1].is_empty() {
                self.recalculate_len_up_to(old_len);
            }
        }
    }
}
