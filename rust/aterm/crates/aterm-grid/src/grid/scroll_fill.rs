// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Fill grid rows from scrollback lines (#4241).
//!
//! Converts scrollback [`Line`]s back to visible grid cells during
//! Kitty CSI +T unscroll. Uses grapheme-unit-aware iteration (via
//! [`advance_grapheme_unit`](super::scroll_materialize::advance_grapheme_unit))
//! to correctly handle combining marks, ZWJ emoji sequences, and other
//! multi-codepoint grapheme clusters.
//!
//! Extracted from `scroll_convert.rs` to keep both files under the
//! 500-line limit.

use std::sync::Arc;

use aterm_scrollback::Line;

use super::Grid;
use crate::Row;
use crate::{CellCoord, CellExtras};

impl Grid {
    /// Fill a row with content from a scrollback Line.
    ///
    /// Uses grapheme-unit-aware iteration to correctly handle combining marks,
    /// ZWJ emoji sequences, and other multi-codepoint grapheme clusters (#4241).
    ///
    /// Two-phase approach to satisfy the borrow checker: Phase 1 populates
    /// row cells (requires `&mut self` via `row_mut`), Phase 2 populates
    /// CellExtras for complex characters after the row borrow is released.
    pub(crate) fn fill_row_from_line(&mut self, row_idx: u16, line: &Line, cols: u16) {
        let Some(text) = line.as_str() else {
            if let Some(row) = self.row_mut(row_idx) {
                row.clear();
                row.set_wrapped(line.is_wrapped());
            }
            return;
        };

        // Phase 1: populate row cells, collect deferred extras.
        let deferred = {
            let Some(row) = self.row_mut(row_idx) else {
                return;
            };
            row.clear();
            let result = fill_row_cells(row, text, line, cols);
            row.set_wrapped(line.is_wrapped());
            result
        };

        // Phase 2: populate CellExtras (row borrow released).
        // Collect column indices before consuming `deferred` so we can set HAS_EXTRAS.
        let deferred_cols: aterm_alloc::SmallVec<u16, 4> = deferred
            .iter()
            .map(|d| match d {
                DeferredExtra::CombiningMarks(col, _)
                | DeferredExtra::ComplexChar(col, _)
                | DeferredExtra::RgbFg(col, _)
                | DeferredExtra::RgbBg(col, _) => *col,
            })
            .collect();
        apply_deferred_extras(&mut self.storage.extras, row_idx, deferred);
        // Set HAS_EXTRAS flags on cells that got deferred extras.
        if !deferred_cols.is_empty()
            && let Some(idx) = self.storage.row_index(row_idx)
            && let Some(r) = self.storage.rows.get_mut(idx)
        {
            for col in deferred_cols {
                if let Some(cell) = r.get_mut(col) {
                    cell.set_has_extras(true);
                }
            }
        }

        // Restore hyperlinks from scrollback Line into CellExtras.
        if let Some(spans) = line.hyperlinks() {
            let mut any_hyperlink = false;
            for span in spans {
                for hcol in span.start_col..span.end_col.min(cols) {
                    let extra = self
                        .storage
                        .extras
                        .get_or_create(CellCoord::new(row_idx, hcol));
                    extra.set_hyperlink(Some(span.url.clone()));
                    extra.set_hyperlink_id(span.id.clone());
                    any_hyperlink = true;
                }
                // Set HAS_EXTRAS flags on cells with restored hyperlinks.
                if let Some(idx) = self.storage.row_index(row_idx)
                    && let Some(r) = self.storage.rows.get_mut(idx)
                {
                    for hcol in span.start_col..span.end_col.min(cols) {
                        if let Some(cell) = r.get_mut(hcol) {
                            cell.set_has_extras(true);
                        }
                    }
                }
            }
            // Defense-in-depth: enforce hyperlink limit after restoring from
            // scrollback, in case materialized lines carry accumulated spam (#7172).
            if any_hyperlink {
                self.storage.extras.enforce_hyperlink_limit();
            }
        }
    }
}

/// Deferred extra to apply after the row borrow is released.
enum DeferredExtra {
    /// BMP base + combining marks: attach marks via `add_combining`.
    /// SmallVec avoids heap allocation for the typical 1-3 combining mark case.
    CombiningMarks(u16, aterm_alloc::SmallVec<char, 4>),
    /// Non-BMP / ZWJ sequence: store as complex char string.
    ComplexChar(u16, Arc<str>),
    /// RGB foreground color from scrollback Line attrs.
    RgbFg(u16, [u8; 3]),
    /// RGB background color from scrollback Line attrs.
    RgbBg(u16, [u8; 3]),
}

/// Phase 1: Populate row cells from scrollback Line text, returning deferred extras.
///
/// Iterates grapheme units (via [`advance_grapheme_unit`]) and classifies each as:
/// - Simple BMP → normal cell
/// - BMP + combining marks → normal cell + deferred combining extras
/// - Non-BMP / ZWJ → complex cell + deferred complex char extras
fn fill_row_cells(row: &mut Row, text: &str, line: &Line, cols: u16) -> Vec<DeferredExtra> {
    use super::scroll_materialize::advance_grapheme_unit;
    use crate::{CellFlags, PackedColor};

    let mut deferred = Vec::new();
    let mut byte_idx: usize = 0;
    let mut char_idx: usize = 0;
    let mut col: u16 = 0;

    while byte_idx < text.len() && col < cols {
        let c = text[byte_idx..]
            .chars()
            .next()
            .expect("invariant: byte_idx < text.len()");
        let base_width = aterm_grapheme::char_width(c);
        if base_width == 0 {
            byte_idx += c.len_utf8();
            char_idx += 1;
            continue;
        }

        let unit_byte_start = byte_idx;
        let unit_char_start = char_idx;
        let chars_consumed = advance_grapheme_unit(text, &mut byte_idx);
        char_idx += chars_consumed;
        let unit_str = &text[unit_byte_start..byte_idx];
        let attrs = line.get_attr(unit_char_start);
        let style = LineCellStyle {
            fg: PackedColor(attrs.fg),
            bg: PackedColor(attrs.bg),
            flags: CellFlags::from_bits(attrs.flags),
        };
        let is_wide = base_width >= 2;

        let (new_col, extra) =
            place_line_cell(row, col, cols, unit_str, chars_consumed, &style, is_wide);
        if let Some(e) = extra {
            deferred.push(e);
        }
        // Restore RGB colors from scrollback Line attrs into CellExtras.
        if new_col > col {
            if style.fg.is_rgb() {
                let (r, g, b) = style.fg.rgb_components();
                deferred.push(DeferredExtra::RgbFg(col, [r, g, b]));
            }
            if style.bg.is_rgb() {
                let (r, g, b) = style.bg.rgb_components();
                deferred.push(DeferredExtra::RgbBg(col, [r, g, b]));
            }
        }
        col = new_col;
    }
    deferred
}

/// Bundled style for a cell being placed from a scrollback Line.
struct LineCellStyle {
    fg: crate::PackedColor,
    bg: crate::PackedColor,
    flags: crate::CellFlags,
}

/// Classify and place a grapheme unit into the row.
///
/// Accepts `unit_str` as a `&str` slice borrowed from the source text,
/// with `chars_consumed` indicating the character count (#5949).
/// Derives the base character from `unit_str` internally.
///
/// Returns `(new_column, optional_deferred_extra)`.
fn place_line_cell(
    row: &mut Row,
    col: u16,
    cols: u16,
    unit_str: &str,
    chars_consumed: usize,
    style: &LineCellStyle,
    is_wide: bool,
) -> (u16, Option<DeferredExtra>) {
    use crate::Cell;

    let c = unit_str
        .chars()
        .next()
        .expect("invariant: unit_str is non-empty");
    let base_is_bmp = (c as u32) <= Cell::MAX_DIRECT_CODEPOINT;

    // BMP base + only combining marks → normal cell + deferred combining.
    let combining_only = base_is_bmp
        && chars_consumed > 1
        && unit_str[c.len_utf8()..]
            .chars()
            .all(|ch| aterm_grapheme::char_width(ch) == 0);

    if chars_consumed == 1 && base_is_bmp {
        (set_cell(row, col, cols, c, style, is_wide), None)
    } else if combining_only {
        let new_col = set_cell(row, col, cols, c, style, is_wide);
        let extra = if new_col > col {
            let marks: aterm_alloc::SmallVec<char, 4> = unit_str[c.len_utf8()..].chars().collect();
            Some(DeferredExtra::CombiningMarks(col, marks))
        } else {
            None
        };
        (new_col, extra)
    } else {
        set_complex_cell(row, col, cols, unit_str, style, is_wide)
    }
}

/// Set a normal (non-complex) cell, handling wide chars.
fn set_cell(row: &mut Row, col: u16, cols: u16, c: char, s: &LineCellStyle, is_wide: bool) -> u16 {
    use crate::{Cell, CellFlags};
    if is_wide && col + 1 < cols {
        row.set(
            col,
            Cell::with_style(c, s.fg, s.bg, s.flags.union(CellFlags::WIDE)),
        );
        row.set(
            col + 1,
            Cell::with_style(' ', s.fg, s.bg, CellFlags::WIDE_CONTINUATION),
        );
        col.saturating_add(2)
    } else if is_wide {
        col // wide at last column — can't fit
    } else {
        row.set(col, Cell::with_style(c, s.fg, s.bg, s.flags));
        col.saturating_add(1)
    }
}

/// Set a complex cell (non-BMP, ZWJ sequence, etc.) with overflow index.
///
/// Uses `unit_str` directly as a `&str` borrowed from the source text —
/// `Arc::from(unit_str)` avoids the intermediate `String` allocation (#5949).
fn set_complex_cell(
    row: &mut Row,
    col: u16,
    cols: u16,
    unit_str: &str,
    s: &LineCellStyle,
    is_wide: bool,
) -> (u16, Option<DeferredExtra>) {
    use crate::{Cell, CellFlags};
    let flags = if is_wide {
        s.flags.union(CellFlags::WIDE)
    } else {
        s.flags
    };
    let mut cell = Cell::with_style(' ', s.fg, s.bg, flags);
    cell.set_overflow_index(0);

    if is_wide && col + 1 < cols {
        row.set(col, cell);
        row.set(
            col + 1,
            Cell::with_style(' ', s.fg, s.bg, CellFlags::WIDE_CONTINUATION),
        );
        (
            col.saturating_add(2),
            Some(DeferredExtra::ComplexChar(col, Arc::from(unit_str))),
        )
    } else if is_wide {
        (col, None)
    } else {
        row.set(col, cell);
        (
            col.saturating_add(1),
            Some(DeferredExtra::ComplexChar(col, Arc::from(unit_str))),
        )
    }
}

/// Apply deferred extras (combining marks, complex chars, RGB colors) to CellExtras.
fn apply_deferred_extras(extras: &mut CellExtras, row_idx: u16, deferred: Vec<DeferredExtra>) {
    for extra in deferred {
        match extra {
            DeferredExtra::CombiningMarks(col, marks) => {
                let entry = extras.get_or_create(CellCoord::new(row_idx, col));
                for mark in marks {
                    entry.add_combining(mark);
                }
            }
            DeferredExtra::ComplexChar(col, s) => {
                extras
                    .get_or_create(CellCoord::new(row_idx, col))
                    .set_complex_char(Some(s));
            }
            DeferredExtra::RgbFg(col, rgb) => {
                extras
                    .get_or_create(CellCoord::new(row_idx, col))
                    .set_fg_rgb(Some(rgb));
            }
            DeferredExtra::RgbBg(col, rgb) => {
                extras
                    .get_or_create(CellCoord::new(row_idx, col))
                    .set_bg_rgb(Some(rgb));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aterm_scrollback::{CellAttrs, Line, Rle};

    // =========================================================================
    // fill_row_from_line — basic text
    // =========================================================================

    #[test]
    fn test_fill_row_from_empty_line() {
        let mut grid = Grid::new(3, 10);
        let line = Line::new();
        grid.fill_row_from_line(0, &line, 10);

        let row = grid.row(0).expect("row 0 should exist");
        assert!(
            row.is_empty(),
            "filling from empty line should leave row empty"
        );
    }

    #[test]
    fn test_fill_row_from_ascii_line() {
        let mut grid = Grid::new(3, 20);
        let line = Line::from_bytes(b"Hello");
        grid.fill_row_from_line(0, &line, 20);

        let row = grid.row(0).expect("row 0 should exist");
        for (i, expected) in "Hello".chars().enumerate() {
            assert_eq!(
                row.get(i as u16).unwrap().char(),
                expected,
                "char mismatch at col {i}"
            );
        }
        // Remaining cells should be space (cleared)
        for col in 5..20u16 {
            assert_eq!(
                row.get(col).unwrap().char(),
                ' ',
                "col {col} should be space after text"
            );
        }
    }

    #[test]
    fn test_fill_row_from_line_preserves_wrapped_flag() {
        let mut grid = Grid::new(3, 20);
        let mut line = Line::from_bytes(b"wrapped");
        line.set_wrapped(true);
        grid.fill_row_from_line(0, &line, 20);

        let row = grid.row(0).expect("row 0 should exist");
        assert!(
            row.is_wrapped(),
            "wrapped flag should be preserved from scrollback Line"
        );
    }

    #[test]
    fn test_fill_row_from_line_not_wrapped() {
        let mut grid = Grid::new(3, 20);
        let line = Line::from_bytes(b"not wrapped");
        grid.fill_row_from_line(0, &line, 20);

        let row = grid.row(0).expect("row 0 should exist");
        assert!(
            !row.is_wrapped(),
            "non-wrapped line should not set wrapped flag"
        );
    }

    #[test]
    fn test_fill_row_truncates_to_cols() {
        let mut grid = Grid::new(3, 5);
        let line = Line::from_bytes(b"LongTextThatExceedsColumns");
        grid.fill_row_from_line(0, &line, 5);

        // Only the first 5 characters should be placed
        let row = grid.row(0).expect("row 0 should exist");
        for (i, expected) in "LongT".chars().enumerate() {
            assert_eq!(
                row.get(i as u16).unwrap().char(),
                expected,
                "char mismatch at col {i} when truncating"
            );
        }
    }

    #[test]
    fn test_fill_row_from_line_clears_previous_content() {
        let mut grid = Grid::new(3, 10);

        // Write some content first
        grid.write_char('X');

        // Now fill from a different line
        let line = Line::from_bytes(b"AB");
        grid.fill_row_from_line(0, &line, 10);

        let row = grid.row(0).expect("row 0 should exist");
        assert_eq!(row.get(0).unwrap().char(), 'A');
        assert_eq!(row.get(1).unwrap().char(), 'B');
        // The old 'X' should be gone (row was cleared)
    }

    #[test]
    fn test_fill_row_different_row_index() {
        let mut grid = Grid::new(5, 10);
        let line = Line::from_bytes(b"Row2");
        grid.fill_row_from_line(2, &line, 10);

        // Row 2 should have content
        let row = grid.row(2).expect("row 2 should exist");
        assert_eq!(row.get(0).unwrap().char(), 'R');
        assert_eq!(row.get(3).unwrap().char(), '2');

        // Row 0 should still be blank
        let row0 = grid.row(0).expect("row 0 should exist");
        assert_eq!(row0.get(0).unwrap().char(), ' ');
    }

    #[test]
    fn test_fill_row_out_of_bounds_row_is_noop() {
        let mut grid = Grid::new(3, 10);
        let line = Line::from_bytes(b"test");
        // Row 99 doesn't exist; should not panic
        grid.fill_row_from_line(99, &line, 10);

        // Existing rows should be unaffected
        let row = grid.row(0).expect("row 0 should exist");
        assert_eq!(row.get(0).unwrap().char(), ' ');
    }

    // =========================================================================
    // fill_row_from_line — styled content
    // =========================================================================

    #[test]
    fn test_fill_row_from_line_with_default_attrs() {
        let mut grid = Grid::new(3, 20);
        let attrs = Rle::with_value(CellAttrs::DEFAULT, 5);
        let line = Line::with_attrs("Hello", attrs);
        grid.fill_row_from_line(0, &line, 20);

        let row = grid.row(0).expect("row 0 should exist");
        for (i, expected) in "Hello".chars().enumerate() {
            assert_eq!(
                row.get(i as u16).unwrap().char(),
                expected,
                "char mismatch at col {i} with default attrs"
            );
        }
    }

    // =========================================================================
    // Scroll behavioral tests — scroll_up
    // =========================================================================

    #[test]
    fn test_scroll_up_content_shifts_up() {
        let mut grid = Grid::new(3, 10);
        // Write A on row 0, B on row 1, C on row 2
        grid.set_cursor(0, 0);
        grid.write_char('A');
        grid.set_cursor(1, 0);
        grid.write_char('B');
        grid.set_cursor(2, 0);
        grid.write_char('C');

        grid.scroll_up(1);

        // After scroll_up(1): row 0 = B, row 1 = C, row 2 = blank
        assert_eq!(grid.cell(0, 0).unwrap().char(), 'B');
        assert_eq!(grid.cell(1, 0).unwrap().char(), 'C');
        assert_eq!(grid.cell(2, 0).unwrap().char(), ' ');
    }

    #[test]
    fn test_scroll_up_bottom_row_blank() {
        let mut grid = Grid::new(4, 10);
        for row in 0..4u16 {
            grid.set_cursor(row, 0);
            grid.write_char((b'A' + row as u8) as char);
        }

        grid.scroll_up(1);

        // Bottom row should be blank
        assert_eq!(grid.cell(3, 0).unwrap().char(), ' ');
    }

    #[test]
    fn test_scroll_down_content_shifts_down() {
        let mut grid = Grid::new(3, 10);
        grid.set_cursor(0, 0);
        grid.write_char('A');
        grid.set_cursor(1, 0);
        grid.write_char('B');
        grid.set_cursor(2, 0);
        grid.write_char('C');

        grid.scroll_down(1);

        // After scroll_down(1): row 0 = blank, row 1 = A, row 2 = B
        assert_eq!(grid.cell(0, 0).unwrap().char(), ' ');
        assert_eq!(grid.cell(1, 0).unwrap().char(), 'A');
        assert_eq!(grid.cell(2, 0).unwrap().char(), 'B');
    }

    #[test]
    fn test_scroll_down_top_row_blank() {
        let mut grid = Grid::new(3, 10);
        grid.set_cursor(0, 0);
        grid.write_char('X');

        grid.scroll_down(1);

        assert_eq!(grid.cell(0, 0).unwrap().char(), ' ');
    }

    // =========================================================================
    // Scroll within scroll region (DECSTBM)
    // =========================================================================

    #[test]
    fn test_scroll_region_up_within_region() {
        let mut grid = Grid::new(5, 10);
        // Write A-E on rows 0-4
        for row in 0..5u16 {
            grid.set_cursor(row, 0);
            grid.write_char((b'A' + row as u8) as char);
        }

        // Set scroll region: rows 1-3
        grid.set_scroll_region(1, 3);
        grid.scroll_region_up(1);

        // Row 0 unchanged (outside region)
        assert_eq!(grid.cell(0, 0).unwrap().char(), 'A');
        // Region rows 1-3: B shifted out, C->1, D->2, blank->3
        assert_eq!(grid.cell(1, 0).unwrap().char(), 'C');
        assert_eq!(grid.cell(2, 0).unwrap().char(), 'D');
        assert_eq!(grid.cell(3, 0).unwrap().char(), ' ');
        // Row 4 unchanged (outside region)
        assert_eq!(grid.cell(4, 0).unwrap().char(), 'E');
    }

    #[test]
    fn test_scroll_region_down_within_region() {
        let mut grid = Grid::new(5, 10);
        for row in 0..5u16 {
            grid.set_cursor(row, 0);
            grid.write_char((b'A' + row as u8) as char);
        }

        // Set scroll region: rows 1-3
        grid.set_scroll_region(1, 3);
        grid.scroll_region_down(1);

        // Row 0 unchanged
        assert_eq!(grid.cell(0, 0).unwrap().char(), 'A');
        // Region: blank at top, B->2, C->3, D pushed out
        assert_eq!(grid.cell(1, 0).unwrap().char(), ' ');
        assert_eq!(grid.cell(2, 0).unwrap().char(), 'B');
        assert_eq!(grid.cell(3, 0).unwrap().char(), 'C');
        // Row 4 unchanged
        assert_eq!(grid.cell(4, 0).unwrap().char(), 'E');
    }

    // =========================================================================
    // Multiple scroll operations
    // =========================================================================

    #[test]
    fn test_multiple_scroll_up() {
        let mut grid = Grid::new(4, 10);
        for row in 0..4u16 {
            grid.set_cursor(row, 0);
            grid.write_char((b'A' + row as u8) as char);
        }

        grid.scroll_up(2);

        // After scrolling up by 2: C->0, D->1, blank->2, blank->3
        assert_eq!(grid.cell(0, 0).unwrap().char(), 'C');
        assert_eq!(grid.cell(1, 0).unwrap().char(), 'D');
        assert_eq!(grid.cell(2, 0).unwrap().char(), ' ');
        assert_eq!(grid.cell(3, 0).unwrap().char(), ' ');
    }

    #[test]
    fn test_multiple_scroll_down() {
        let mut grid = Grid::new(4, 10);
        for row in 0..4u16 {
            grid.set_cursor(row, 0);
            grid.write_char((b'A' + row as u8) as char);
        }

        grid.scroll_down(2);

        // After scrolling down by 2: blank->0, blank->1, A->2, B->3
        assert_eq!(grid.cell(0, 0).unwrap().char(), ' ');
        assert_eq!(grid.cell(1, 0).unwrap().char(), ' ');
        assert_eq!(grid.cell(2, 0).unwrap().char(), 'A');
        assert_eq!(grid.cell(3, 0).unwrap().char(), 'B');
    }

    // =========================================================================
    // Edge cases
    // =========================================================================

    #[test]
    fn test_scroll_region_1_row_region_up() {
        let mut grid = Grid::new(5, 10);
        grid.set_cursor(2, 0);
        grid.write_char('X');

        // Single-row scroll region
        grid.set_scroll_region(2, 2);
        grid.scroll_region_up(1);

        // The single row should be cleared
        assert_eq!(grid.cell(2, 0).unwrap().char(), ' ');
    }

    #[test]
    fn test_scroll_region_1_row_region_down() {
        let mut grid = Grid::new(5, 10);
        grid.set_cursor(2, 0);
        grid.write_char('Y');

        grid.set_scroll_region(2, 2);
        grid.scroll_region_down(1);

        // The single row should be cleared
        assert_eq!(grid.cell(2, 0).unwrap().char(), ' ');
    }

    #[test]
    fn test_scroll_up_count_exceeds_region() {
        let mut grid = Grid::new(3, 10);
        for row in 0..3u16 {
            grid.set_cursor(row, 0);
            grid.write_char((b'A' + row as u8) as char);
        }

        // Scroll up by 100 in a 3-row region
        grid.set_scroll_region(0, 2);
        grid.scroll_region_up(100);

        // All rows should be blank (scrolled everything out)
        for row in 0..3u16 {
            assert_eq!(
                grid.cell(row, 0).unwrap().char(),
                ' ',
                "row {row} should be blank after excessive scroll"
            );
        }
    }

    #[test]
    fn test_scroll_down_count_exceeds_region() {
        let mut grid = Grid::new(3, 10);
        for row in 0..3u16 {
            grid.set_cursor(row, 0);
            grid.write_char((b'A' + row as u8) as char);
        }

        grid.set_scroll_region(0, 2);
        grid.scroll_region_down(100);

        // All rows should be blank
        for row in 0..3u16 {
            assert_eq!(
                grid.cell(row, 0).unwrap().char(),
                ' ',
                "row {row} should be blank after excessive scroll down"
            );
        }
    }

    #[test]
    fn test_scroll_up_zero_is_noop() {
        let mut grid = Grid::new(3, 10);
        grid.set_cursor(0, 0);
        grid.write_char('A');

        grid.scroll_up(0);

        assert_eq!(grid.cell(0, 0).unwrap().char(), 'A');
    }

    #[test]
    fn test_scroll_region_full_screen_delegates_to_scroll_up() {
        let mut grid = Grid::new(3, 10);
        for row in 0..3u16 {
            grid.set_cursor(row, 0);
            grid.write_char((b'A' + row as u8) as char);
        }

        // Full-screen region scroll should act like regular scroll_up
        grid.set_scroll_region(0, 2);
        grid.scroll_region_up(1);

        assert_eq!(grid.cell(0, 0).unwrap().char(), 'B');
        assert_eq!(grid.cell(1, 0).unwrap().char(), 'C');
        assert_eq!(grid.cell(2, 0).unwrap().char(), ' ');
    }
}
