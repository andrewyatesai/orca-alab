// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Grid scroll operations.
//!
//! This module handles all scrolling operations for the terminal grid:
//! - Display scrolling (viewing history)
//! - Content scrolling (scroll_up, scroll_down)
//! - Region scrolling (within DECSTBM scroll margins)
//!
//! Content-modifying scroll operations PRESERVE the `pending_wrap` flag.
//! This matches xterm: `xtermScroll` explicitly saves and restores
//! `screen->do_wrap` around the scroll and `RevScroll` never touches it
//! (util.c), so a scrolling LF/RI/SU/SD leaves a deferred wrap pending.
//! Only cursor-motion ops (CursorUp/Down/Back/Forward/Set, CR) and the
//! editing ops with an explicit xterm `ResetWrap` (ICH/DCH/ECH/IL/DL and
//! the ED/EL-right family) cancel it.
//!
//! Row-to-line conversion helpers are in [`super::scroll_convert`].

//!
//! ## Ring Buffer Design
//!
//! The grid uses a ring buffer for O(1) display scrolling. When scrolling up:
//! - If not at capacity, new rows are appended
//! - If at capacity, oldest row is reused (optionally pushed to scrollback first)
//!
//! The `ring_head` index tracks the oldest row in the buffer.

use crate::damage::compute_display_offset_damage;
use crate::row::LineSize;

use super::Grid;
use super::row_u16;
use super::scroll_convert::DeferredLine;
use crate::Row;

impl Grid {
    /// Reset display_offset to 0 with targeted damage.
    ///
    /// Marks only the newly-exposed bottom rows instead of `mark_full()`.
    /// Follows `scroll_display`'s down-scroll pattern: bottom N rows are new
    /// content, upper rows shift up via GPU vertex-shift.
    ///
    /// Used by `scroll_to_bottom` and defensive offset resets in operations
    /// that require `display_offset == 0` for row arithmetic.
    pub(crate) fn reset_display_offset_with_damage(&mut self) {
        let old_offset = self.storage.display_offset;
        if old_offset == 0 {
            return;
        }
        self.storage.display_offset = 0;
        let dmg = compute_display_offset_damage(old_offset, 0, self.storage.visible_rows);
        self.storage.damage.apply_display_offset_damage(dmg);
    }

    /// Mark scroll damage: targeted rows for small scrolls, full for large.
    fn mark_scroll_damage(&mut self, n: usize) {
        self.storage
            .cursor_state
            .presentation
            .mark_scroll_damage(self.storage.visible_rows, n);
    }
    /// Scroll the display by delta lines.
    ///
    /// Positive delta = scroll up (show older content).
    /// Negative delta = scroll down (show newer content).
    ///
    /// ENSURES: self.storage.display_offset <= self.storage.scrollback_lines()
    pub fn scroll_display(&mut self, delta: i32) {
        let max_offset = self.storage.scrollback_lines();
        let old_offset = self.storage.display_offset;
        // display_offset is bounded by max scrollback (MAX_SCROLLBACK_LINES = 1M)
        // which fits in i32. Use saturating conversion for safety.
        let current: i32 = self.storage.display_offset.try_into().unwrap_or(i32::MAX);
        let clamped = current.saturating_add(delta).max(0);
        // max(0) ensures non-negative; try_from is lossless for non-negative i32→usize
        let new_offset = usize::try_from(clamped).unwrap_or(0);
        self.storage.display_offset = new_offset.min(max_offset);

        let dmg =
            compute_display_offset_damage(old_offset, self.storage.display_offset, self.rows());
        self.storage.damage.apply_display_offset_damage(dmg);
        debug_assert!(self.storage.display_offset <= self.storage.scrollback_lines());
    }

    /// Re-pin the viewport after a batch of output (SCR-1).
    ///
    /// `prev_offset` is the user's display_offset before processing reset it to
    /// 0; `lines_added` is the number of lines that entered scrollback during
    /// processing (the rise in `absolute_row_counter`). To keep the same content
    /// in view, the new offset is `prev_offset + lines_added`, clamped to
    /// `scrollback_lines()` so `display_offset <= scrollback_lines()` holds even
    /// when eviction discarded some of those lines.
    ///
    /// ENSURES: self.storage.display_offset <= self.storage.scrollback_lines()
    pub fn repin_display_offset(&mut self, prev_offset: usize, lines_added: u64) {
        let max_offset = self.storage.scrollback_lines();
        let target = prev_offset
            .saturating_add(usize::try_from(lines_added).unwrap_or(usize::MAX))
            .min(max_offset);
        let old_offset = self.storage.display_offset;
        if target == old_offset {
            return;
        }
        self.storage.display_offset = target;
        let dmg = compute_display_offset_damage(old_offset, target, self.storage.visible_rows);
        self.storage.damage.apply_display_offset_damage(dmg);
        debug_assert!(self.storage.display_offset <= self.storage.scrollback_lines());
    }

    /// Scroll to the top of scrollback.
    ///
    /// Uses targeted row-level damage when the scroll delta is smaller than
    /// visible rows: only the top N rows are marked dirty. Falls back to
    /// `mark_full()` for large scrolls.
    ///
    /// ENSURES: self.storage.display_offset == self.storage.scrollback_lines()
    pub fn scroll_to_top(&mut self) {
        let target = self.storage.scrollback_lines();
        let old_offset = self.storage.display_offset;
        self.storage.display_offset = target;

        let dmg = compute_display_offset_damage(old_offset, target, self.rows());
        self.storage.damage.apply_display_offset_damage(dmg);
        debug_assert_eq!(self.storage.display_offset, self.storage.scrollback_lines());
    }

    /// Scroll to live position (bottom).
    ///
    /// Uses targeted row-level damage instead of `mark_full()`:
    /// only the newly-exposed bottom rows are marked dirty.
    ///
    /// ENSURES: self.storage.display_offset == 0
    #[inline]
    pub fn scroll_to_bottom(&mut self) {
        self.reset_display_offset_with_damage();
        debug_assert_eq!(self.storage.display_offset, 0);
    }

    /// Clamp display_offset to valid bounds.
    ///
    /// Call this after operations that may reduce scrollback size
    /// (e.g., truncation) to maintain the DisplayOffsetValid invariant.
    ///
    /// Uses targeted row-level damage when the clamping delta is smaller than
    /// visible rows: only the bottom N rows are marked dirty.
    ///
    /// ENSURES: self.storage.display_offset <= self.storage.scrollback_lines()
    pub fn clamp_display_offset(&mut self) {
        let max_offset = self.storage.scrollback_lines();
        if self.storage.display_offset > max_offset {
            let old_offset = self.storage.display_offset;
            self.storage.display_offset = max_offset;
            let dmg = compute_display_offset_damage(old_offset, max_offset, self.rows());
            self.storage.damage.apply_display_offset_damage(dmg);
        }
        debug_assert!(self.storage.display_offset <= self.storage.scrollback_lines());
    }

    /// Scroll content up by n lines (new empty lines at bottom).
    ///
    /// When a scrollback is attached and the ring buffer is at capacity,
    /// the oldest row is converted to a [`Line`] and pushed to the scrollback
    /// before being overwritten.
    ///
    /// ## Complexity
    ///
    /// O(n × cols) where n is the number of lines scrolled and cols is the
    /// grid column count. Each scrolled line requires:
    /// - O(cols) to convert row to scrollback line via `row_to_line_with_stored_extras`
    /// - O(cols) to clear and resize the reused row
    ///
    /// Verified by performance tests: `scroll_up_linear_time`, `scroll_up_handles_many_rows`
    ///
    /// ## Optimization
    ///
    /// This function is optimized for batch operations:
    /// - Pre-calculates how many rows to add vs reuse
    /// - Batch reserves Vec capacity for growth phase
    /// - Updates counters in bulk to reduce loop overhead
    ///
    /// REQUIRES: self.storage.visible_rows > 0
    /// ENSURES: self.storage.rows.len() <= (self.storage.visible_rows as usize) + self.storage.max_scrollback
    #[doc(hidden)] // pub for crate benchmarks; not part of stable API
    pub fn scroll_up(&mut self, n: usize) {
        if n == 0 {
            return;
        }
        debug_assert!(
            !self.storage.rows.is_empty(),
            "scroll_up: ring buffer has zero rows"
        );

        let capacity = (self.storage.visible_rows as usize) + self.storage.max_scrollback;
        let cols = self.storage.cols;

        // Pre-calculate: how many rows can we add before hitting capacity?
        let rows_until_capacity = capacity.saturating_sub(self.storage.total_lines);
        let rows_to_add = n.min(rows_until_capacity);
        let rows_to_reuse = n.saturating_sub(rows_to_add);

        if rows_to_add > 0 {
            self.grow_scrollback_ring(rows_to_add, cols);
        }

        if rows_to_reuse > 0 {
            self.reuse_scrolled_rows(rows_to_reuse, cols);
        }

        self.finish_scroll_up(n);
        debug_assert!(
            self.storage.rows.len()
                <= (self.storage.visible_rows as usize) + self.storage.max_scrollback
        );
    }

    fn grow_scrollback_ring(&mut self, rows_to_add: usize, cols: u16) {
        debug_assert!(
            !self.storage.rows.is_empty(),
            "grow_scrollback_ring: ring buffer has zero rows"
        );
        let ring_sb = self.storage.ring_buffer_scrollback();
        let row_count = self.storage.rows.len();
        for i in 0..rows_to_add {
            let row_idx = row_u16(i);
            let phys = (self.storage.ring_head + ring_sb + i) % row_count;
            let extracted = Self::extract_row_extras(
                &self.storage.rows[phys],
                &self.storage.extras,
                row_idx,
                self.styles(),
            );
            self.storage.push_ring_extras(extracted);
        }

        self.storage.rows.reserve(rows_to_add);
        let fill = self.storage.cursor_template;
        {
            let storage = &mut self.storage;
            let rows = &mut storage.rows;
            let pages = &mut storage.pages;
            // `Row::new` already yields an all-EMPTY row with len 0 and DIRTY
            // flags — exactly the state `erase_with(Cell::EMPTY)` produces —
            // so the BCE fill pass is needed only for a non-default template.
            let needs_fill = fill != crate::Cell::EMPTY;
            for _ in 0..rows_to_add {
                // SAFETY: New rows are stored in the same `GridStorage` that owns
                // `pages`, and rows drop before the backing pages.
                let mut row = unsafe { Row::new(cols, pages) };
                // Apply BCE fill so new bottom rows inherit the current SGR
                // background color per VT420/xterm spec (#7522).
                if needs_fill {
                    row.erase_with(fill);
                }
                rows.push(row);
            }
        }
        self.storage.total_lines += rows_to_add;
        self.storage.absolute_row_counter += rows_to_add as u64;
        self.storage
            .extras
            .shift_rows_up_by(0, row_u16(rows_to_add));
        // Fill BCE RGB in vacated bottom rows after shift (#7685).
        let vis = self.storage.visible_rows;
        self.fill_bce_rgb_rows(vis.saturating_sub(row_u16(rows_to_add))..vis);
    }

    fn reuse_scrolled_rows(&mut self, rows_to_reuse: usize, cols: u16) {
        debug_assert!(
            !self.storage.rows.is_empty(),
            "reuse_scrolled_rows: ring buffer has zero rows"
        );
        let row_count = self.storage.rows.len();
        let ring_sb = self.storage.ring_buffer_scrollback();
        let has_scrollback = self.storage.scrollback.is_some();

        if rows_to_reuse == 1 && !has_scrollback {
            self.reuse_one_scrolled_row_no_scrollback(cols, row_count, ring_sb);
        } else {
            self.reuse_scrolled_rows_general(rows_to_reuse, cols, row_count, ring_sb);
        }

        // Drain lazy buffer to tiered scrollback when threshold is exceeded.
        // This amortizes the materialization cost over many scroll operations.
        // Callers that need all lines in tiered storage (unscroll, reflow)
        // drain explicitly via drain_lazy_buffer().
        if self.storage.lazy_buffer.should_drain() {
            self.drain_lazy_buffer();
        }

        self.storage
            .extras
            .shift_rows_up_by(0, row_u16(rows_to_reuse));
        // Fill BCE RGB in vacated bottom rows after shift (#7685).
        let vis = self.storage.visible_rows;
        self.fill_bce_rgb_rows(vis.saturating_sub(row_u16(rows_to_reuse))..vis);
        self.storage.absolute_row_counter += rows_to_reuse as u64;
    }

    /// Steady-state line-feed fast path: single-row scroll with no tiered
    /// scrollback attached. Semantically identical to the general path but
    /// avoids the intermediate extraction `Vec` and recycles the popped
    /// `ring_extras` allocation as scratch for the new row's extraction,
    /// eliminating per-scroll heap churn (one `Box` + two `Vec`s per styled
    /// row) on the dominant one-line scroll.
    fn reuse_one_scrolled_row_no_scrollback(&mut self, cols: u16, row_count: usize, ring_sb: usize) {
        let fill = self.storage.cursor_template;
        let oldest = self.storage.ring_head;
        let phys = (oldest + ring_sb) % row_count;

        if self.storage.ring_extras.is_empty() {
            // Net no-op in the general path: the freshly extracted extras are
            // pushed and immediately popped, then dropped (no tiered
            // scrollback consumes them). Skip the extraction entirely.
        } else if let Some(mut bx) = self.storage.ring_extras.pop_front().flatten() {
            // Recycle the popped box (and its Vec capacities) as scratch.
            Self::extract_row_extras_into(
                &mut bx,
                &self.storage.rows[phys],
                &self.storage.extras,
                0,
                self.styles(),
            );
            // Preserve the `None ⟺ empty` ring_extras encoding.
            self.storage
                .ring_extras
                .push_back(if bx.is_empty() { None } else { Some(bx) });
        } else {
            // Popped entry was None (plain row) — nothing to recycle.
            let extracted = Self::extract_row_extras(
                &self.storage.rows[phys],
                &self.storage.extras,
                0,
                self.styles(),
            );
            self.storage.push_ring_extras(extracted);
        }

        let evicted_page = self.storage.rows[oldest].page_id();
        self.storage.generations.evict_page(evicted_page);
        {
            let storage = &mut self.storage;
            let pages = &mut storage.pages;
            // SAFETY: The reused row remains stored in `storage.rows`, and
            // `storage.pages` continues to outlive that owner.
            unsafe { storage.rows[oldest].resize(cols, pages) };
            // Single fused fill (replaces clear + erase_with): applies BCE
            // fill so reused bottom rows inherit the current SGR background
            // color per VT420/xterm spec (#7522).
            storage.rows[oldest].reset_with(fill);
        }
        self.storage.ring_head = (self.storage.ring_head + 1) % row_count;
    }

    /// General multi-row (or tiered-scrollback) reuse path.
    fn reuse_scrolled_rows_general(
        &mut self,
        rows_to_reuse: usize,
        cols: u16,
        row_count: usize,
        ring_sb: usize,
    ) {
        let has_scrollback = self.storage.scrollback.is_some();

        // Extract extras for rows entering ring buffer scrollback.
        // When no tiered scrollback is attached, we still need to maintain
        // ring_extras for correct row-to-line conversion if scrollback is
        // attached later, but we can skip the expensive extraction when
        // the CellExtras is empty (common for plain text).
        let new_scrollback_extras: Vec<_> = (0..rows_to_reuse)
            .map(|i| {
                let row_idx = row_u16(i);
                let phys = (self.storage.ring_head + ring_sb + i) % row_count;
                Self::extract_row_extras(
                    &self.storage.rows[phys],
                    &self.storage.extras,
                    row_idx,
                    self.styles(),
                )
            })
            .collect();

        let fill = self.storage.cursor_template;
        for new_extras in new_scrollback_extras {
            let oldest = self.storage.ring_head;
            self.storage.push_ring_extras(new_extras);

            let extras = self
                .storage
                .ring_extras
                .pop_front()
                .flatten()
                .map_or_else(Default::default, |b| *b);

            // Lazy scrollback promotion: snapshot the row as a DeferredLine
            // (O(cells) memcpy) instead of the O(cols) row_to_line conversion.
            // The line is materialized lazily on first read access.
            if has_scrollback {
                let deferred = DeferredLine::new(&self.storage.rows[oldest], extras);
                self.storage.lazy_buffer.push(deferred);
            }

            let evicted_page = self.storage.rows[oldest].page_id();
            self.storage.generations.evict_page(evicted_page);
            {
                let storage = &mut self.storage;
                let pages = &mut storage.pages;
                // SAFETY: The reused row remains stored in `storage.rows`, and
                // `storage.pages` continues to outlive that owner.
                unsafe { storage.rows[oldest].resize(cols, pages) };
                // Single fused fill (replaces clear + erase_with): applies BCE
                // fill so reused bottom rows inherit the current SGR background
                // color per VT420/xterm spec (#7522).
                storage.rows[oldest].reset_with(fill);
            }
            self.storage.ring_head = (self.storage.ring_head + 1) % row_count;
        }
    }

    /// Drain all pending deferred lines from the lazy buffer into tiered scrollback.
    ///
    /// Materializes each `DeferredLine` into a `Line` and pushes it to the
    /// tiered scrollback storage. Called when the lazy buffer exceeds its
    /// threshold, when scrollback is accessed, or at checkpoint time.
    ///
    /// After draining, enforces the memory budget (if configured) by evicting
    /// oldest cold-tier lines to a disk spill file.
    pub(crate) fn drain_lazy_buffer(&mut self) {
        if self.storage.lazy_buffer.is_empty() {
            return;
        }
        let Some(scrollback) = self.storage.scrollback.as_mut() else {
            // No scrollback attached — discard deferred lines.
            self.storage.lazy_buffer.clear();
            return;
        };

        // Collect lines first to avoid borrow conflict (lazy_buffer and scrollback
        // are both behind &mut self.storage).
        let lines: Vec<_> = self.storage.lazy_buffer.drain_all().collect();
        for line in lines {
            if let Err(error) = scrollback.push_line(line) {
                aterm_log::warn!("scrollback push_line failed: {error}");
            }
        }

        // Enforce memory budget: evict oldest cold-tier lines to disk spill
        // if the scrollback exceeds the configured budget.
        if let Some(enforcer) = self.storage.budget_enforcer.as_mut()
            && let Some(scrollback) = self.storage.scrollback.as_mut()
            && let Err(error) = enforcer.enforce(scrollback)
        {
            aterm_log::warn!("scrollback budget enforcement failed: {error}");
        }

        // push_line can trigger line-limit enforcement or memory-pressure
        // eviction, reducing total scrollback lines.  If the user was scrolled
        // back, display_offset may now exceed scrollback_lines(), violating the
        // DisplayOffsetValid invariant.  Clamp to restore it (#7240).
        self.clamp_display_offset();
    }

    fn finish_scroll_up(&mut self, n: usize) {
        let delta = i32::try_from(n).unwrap_or(i32::MAX);
        self.storage.content_scroll_delta = self.storage.content_scroll_delta.saturating_add(delta);
        self.mark_scroll_damage(n);
    }

    /// Scroll content down by n lines (new empty lines at top).
    ///
    /// Shifts all visible rows down by `n` — test convenience wrapper
    /// over [`scroll_region_down`]. Production code uses `scroll_region_down` directly.
    #[cfg(test)]
    pub(crate) fn scroll_down(&mut self, n: usize) {
        self.scroll_region_down(n);
    }

    /// Scroll within scroll region: move content up (blank line at bottom of region).
    ///
    /// This is used when cursor is at bottom of scroll region and line feed is issued.
    /// Only lines within the scroll region are affected.
    ///
    /// REQUIRES: self.storage.scroll_region.top <= self.storage.scroll_region.bottom
    /// REQUIRES: self.storage.scroll_region.bottom < self.storage.visible_rows
    pub fn scroll_region_up(&mut self, n: usize) {
        if n == 0 {
            return;
        }
        // Full-screen scrolls, including 1-row terminals, enter scrollback.
        // Only non-full degenerate regions are no-ops (#7751).
        if self
            .storage
            .scroll_region
            .is_full(self.storage.visible_rows)
        {
            self.reset_display_offset_with_damage();
            self.scroll_up(n);
            return;
        }
        // Degenerate single-row region: nowhere to scroll to — no-op (#7751).
        if self.storage.scroll_region.top == self.storage.scroll_region.bottom {
            return;
        }
        // Row index arithmetic requires display_offset == 0. Callers like
        // line_feed() reset it, but others (advance_autowrap_line, CSI S) may
        // not. Reset here defensively so every path is safe. (#5019)
        self.reset_display_offset_with_damage();

        let top_u16 = self.storage.scroll_region.top;
        let bottom_u16 = self.storage.scroll_region.bottom;
        let top = usize::from(top_u16);
        let bottom = usize::from(bottom_u16);

        // Scroll within the region only (no scrollback)
        let region_size = bottom - top + 1;
        let n = n.min(region_size);

        // Shift rows up within the region using pre-computed physical indices.
        // display_offset == 0 is guaranteed by reset_display_offset_with_damage above.
        self.storage.shift_visible_rows_up(top, bottom, n);

        // Clear the bottom n rows of the region with BCE fill (#7522).
        // Reset line size to SingleWidth so DECDWL/DECDHL flags don't leak
        // from recycled rows that previously had double-width attributes.
        let fill = self.storage.cursor_template;
        for row in (bottom + 1 - n)..=bottom {
            if let Some(r) = self.row_mut(row_u16(row)) {
                r.set_line_size(LineSize::SingleWidth);
                r.erase_with(fill);
            }
        }

        // Batch shift CellExtras within the region: O(E) regardless of n
        let shift_n = row_u16(n);
        self.storage
            .extras
            .shift_region_up_by(top_u16, bottom_u16, shift_n);
        // Fill BCE RGB in vacated bottom rows after shift (#7685).
        self.fill_bce_rgb_rows(row_u16(bottom + 1 - n)..bottom_u16.saturating_add(1));

        // Partial-region scroll invalidates selection coordinates in complex ways.
        // Use saturating_add with large value to force selection clear via adjust_for_scroll.
        self.storage.content_scroll_delta = i32::MAX;
        // Mark only the scroll region rows as dirty, not the full screen.
        self.storage
            .damage
            .mark_rows(top_u16, bottom_u16.saturating_add(1));
    }

    /// Shift rows down within a region (backwards to avoid overwriting).
    ///
    /// Copies `n` rows downward within `[top..=bottom]`. Does NOT clear vacated
    /// rows or shift extras — callers handle those steps.
    /// Uses pre-computed physical indices for sequential access.
    /// REQUIRES: display_offset == 0 (callers guarantee via reset_display_offset_with_damage).
    pub(super) fn shift_rows_down(&mut self, top: usize, bottom: usize, n: usize) {
        self.storage.shift_visible_rows_down(top, bottom, n);
    }

    /// Scroll within scroll region: move content down (blank line at top of region).
    ///
    /// This is used when cursor is at top of scroll region and reverse line feed is issued.
    /// Only lines within the scroll region are affected.
    ///
    /// REQUIRES: self.storage.scroll_region.top <= self.storage.scroll_region.bottom
    /// REQUIRES: self.storage.scroll_region.bottom < self.storage.visible_rows
    pub fn scroll_region_down(&mut self, n: usize) {
        if n == 0 {
            return;
        }
        // Degenerate single-row region: nowhere to scroll to — no-op (#7751).
        if self.storage.scroll_region.top == self.storage.scroll_region.bottom {
            return;
        }
        // Row index arithmetic requires display_offset == 0. Reset
        // defensively for callers that may not have done so. (#5019)
        self.reset_display_offset_with_damage();

        let top_u16 = self.storage.scroll_region.top;
        let bottom_u16 = self.storage.scroll_region.bottom;
        let top = usize::from(top_u16);
        let bottom = usize::from(bottom_u16);
        let region_size = bottom - top + 1;
        let n = n.min(region_size);

        self.shift_rows_down(top, bottom, n);

        // Clear the top n rows of the region with BCE fill (#7522).
        // Reset line size to SingleWidth so DECDWL/DECDHL flags don't leak
        // from recycled rows that previously had double-width attributes.
        let fill = self.storage.cursor_template;
        for row in top..(top + n) {
            if let Some(r) = self.row_mut(row_u16(row)) {
                r.set_line_size(LineSize::SingleWidth);
                r.erase_with(fill);
            }
        }

        // Batch shift CellExtras within the region: O(E) regardless of n
        let shift_n = row_u16(n);
        self.storage
            .extras
            .shift_region_down_by(top_u16, bottom_u16, shift_n);
        // Fill BCE RGB in vacated top rows after shift (#7685).
        self.fill_bce_rgb_rows(top_u16..row_u16(top + n));

        // Force selection clear: partial-region coordinate mapping is non-trivial,
        // and full-screen scroll_down() also delegates here.
        self.storage.content_scroll_delta = i32::MAX;
        // Mark only the scroll region rows as dirty, not the full screen.
        self.storage
            .damage
            .mark_rows(top_u16, bottom_u16.saturating_add(1));
    }

    /// Rectangular scroll up within horizontal margins (DECLRMM + SU).
    ///
    /// When DECLRMM is active, SU only scrolls the cells within the horizontal
    /// margin region on each row, leaving cells outside the margins untouched.
    /// Blank cells fill the vacated positions at the bottom of the margin region.
    pub fn scroll_region_up_margined(&mut self, n: usize, left: u16, right: u16) {
        if n == 0 {
            return;
        }
        // Degenerate single-row region: nowhere to scroll to — no-op (#7751).
        if self.storage.scroll_region.top == self.storage.scroll_region.bottom {
            return;
        }
        self.reset_display_offset_with_damage();

        let top = usize::from(self.storage.scroll_region.top);
        let bottom = usize::from(self.storage.scroll_region.bottom);
        let region_size = bottom - top + 1;
        let n = n.min(region_size);
        let left_usize = usize::from(left);
        let right_usize = usize::from(right);
        let width = right_usize + 1 - left_usize;

        // Copy cells from row (src_row) to row (dst_row) within [left, right].
        // Process top-to-bottom so we don't overwrite source data.
        // Hoist buffer outside loop to avoid per-row heap allocation.
        let cols = self.storage.cols as usize;
        let mut buf = vec![super::Cell::EMPTY; width];
        for dst_offset in 0..(region_size - n) {
            let dst_row = row_u16(top + dst_offset);
            let src_row = row_u16(top + dst_offset + n);
            buf.fill(super::Cell::EMPTY);
            if let Some(src) = self.row(src_row) {
                for (i, col) in (left_usize..=right_usize).enumerate() {
                    if let Some(c) = src.get(row_u16(col)) {
                        buf[i] = *c;
                    }
                }
            }
            if let Some(dst) = self.row_mut(dst_row) {
                for (i, col) in (left_usize..=right_usize).enumerate() {
                    if let Some(c) = dst.get_mut(row_u16(col)) {
                        *c = buf[i];
                    }
                }
                // Wide char fixup at rectangle boundaries (#7500).
                dst.fixup_wide_boundary(left_usize, right_usize, cols);
            }
        }
        // Clear the bottom n rows within margins with BCE fill (#7522).
        let fill = self.storage.cursor_template;
        for clear_offset in (region_size - n)..region_size {
            let clear_row = row_u16(top + clear_offset);
            if let Some(r) = self.row_mut(clear_row) {
                for col in left_usize..=right_usize {
                    if let Some(c) = r.get_mut(row_u16(col)) {
                        *c = fill;
                    }
                }
                // Wide char fixup at rectangle boundaries (#7500).
                r.fixup_wide_boundary(left_usize, right_usize, cols);
            }
        }

        // Shift extras within the margin columns: rows [top+n..bottom] shift
        // up by n, rows [top..top+n) are dropped. Preserves hyperlinks, RGB
        // colors, and combining marks on shifted rows. (#7415)
        let top_u16 = self.storage.scroll_region.top;
        let bottom_u16 = self.storage.scroll_region.bottom;
        self.storage
            .extras
            .shift_rect_up_by(top_u16, bottom_u16, left, right, row_u16(n));
        // Fill BCE RGB in vacated bottom-right rect after shift (#7685).
        self.fill_bce_rgb_rect(
            row_u16(top + region_size - n)..bottom_u16.saturating_add(1),
            left..right.saturating_add(1),
        );

        self.storage.content_scroll_delta = i32::MAX;
        self.storage
            .damage
            .mark_rows(top_u16, bottom_u16.saturating_add(1));
    }

    /// Rectangular scroll down within horizontal margins (DECLRMM + SD).
    ///
    /// When DECLRMM is active, SD only scrolls the cells within the horizontal
    /// margin region on each row, leaving cells outside the margins untouched.
    /// Blank cells fill the vacated positions at the top of the margin region.
    pub fn scroll_region_down_margined(&mut self, n: usize, left: u16, right: u16) {
        if n == 0 {
            return;
        }
        // Degenerate single-row region: nowhere to scroll to — no-op (#7751).
        if self.storage.scroll_region.top == self.storage.scroll_region.bottom {
            return;
        }
        self.reset_display_offset_with_damage();

        let top = usize::from(self.storage.scroll_region.top);
        let bottom = usize::from(self.storage.scroll_region.bottom);
        let region_size = bottom - top + 1;
        let n = n.min(region_size);
        let left_usize = usize::from(left);
        let right_usize = usize::from(right);
        let width = right_usize + 1 - left_usize;

        // Copy cells from row (src_row) to row (dst_row) within [left, right].
        // Process bottom-to-top so we don't overwrite source data.
        // Hoist buffer outside loop to avoid per-row heap allocation.
        let cols = self.storage.cols as usize;
        let mut buf = vec![super::Cell::EMPTY; width];
        for dst_offset in (n..region_size).rev() {
            let dst_row = row_u16(top + dst_offset);
            let src_row = row_u16(top + dst_offset - n);
            buf.fill(super::Cell::EMPTY);
            if let Some(src) = self.row(src_row) {
                for (i, col) in (left_usize..=right_usize).enumerate() {
                    if let Some(c) = src.get(row_u16(col)) {
                        buf[i] = *c;
                    }
                }
            }
            if let Some(dst) = self.row_mut(dst_row) {
                for (i, col) in (left_usize..=right_usize).enumerate() {
                    if let Some(c) = dst.get_mut(row_u16(col)) {
                        *c = buf[i];
                    }
                }
                // Wide char fixup at rectangle boundaries (#7500).
                dst.fixup_wide_boundary(left_usize, right_usize, cols);
            }
        }
        // Clear the top n rows within margins with BCE fill (#7522).
        let fill = self.storage.cursor_template;
        for clear_offset in 0..n {
            let clear_row = row_u16(top + clear_offset);
            if let Some(r) = self.row_mut(clear_row) {
                for col in left_usize..=right_usize {
                    if let Some(c) = r.get_mut(row_u16(col)) {
                        *c = fill;
                    }
                }
                // Wide char fixup at rectangle boundaries (#7500).
                r.fixup_wide_boundary(left_usize, right_usize, cols);
            }
        }

        // Shift extras within the margin columns: rows [top..bottom-n] shift
        // down by n, rows [bottom-n+1..bottom] are dropped. (#7415)
        let top_u16 = self.storage.scroll_region.top;
        let bottom_u16 = self.storage.scroll_region.bottom;
        self.storage
            .extras
            .shift_rect_down_by(top_u16, bottom_u16, left, right, row_u16(n));
        // Fill BCE RGB in vacated top-left rect after shift (#7685).
        self.fill_bce_rgb_rect(top_u16..row_u16(top + n), left..right.saturating_add(1));

        self.storage.content_scroll_delta = i32::MAX;
        self.storage
            .damage
            .mark_rows(top_u16, bottom_u16.saturating_add(1));
    }

}

// Kitty CSI + T unscroll implementation extracted to scroll_unscroll.rs.
