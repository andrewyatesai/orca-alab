// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Text content extraction and smart selection API.
//!
//! Methods for extracting text content from the terminal grid and
//! performing smart selection (URLs, paths, etc.) for triggers and UI.

use crate::grid::{Grid, row_u16};

use super::Terminal;

fn push_cell_text(grid: &Grid, row: u16, col: u16, out: &mut String) {
    let Some(cell) = grid.cell(row, col) else {
        return;
    };
    // Context-aware check: Cell::is_wide_continuation() would false-positive
    // on DECSCA-protected cells (PROTECTED shares bit 10), hiding protected
    // text from the read API.
    if grid.is_wide_continuation_at(row, col) {
        return;
    }
    if cell.is_complex() {
        // Use full string for text extraction (handles multi-char HashMap entries)
        if let Some(s) = grid.complex_char_str_at(row, col) {
            out.push_str(&s);
        } else {
            out.push('\u{FFFD}');
        }
    } else {
        let ch = cell.char();
        out.push(if ch == '\0' { ' ' } else { ch });
    }
    // Combining marks from CellExtra — gate on has_extras() so cells
    // without extras skip the HashMap probe. Stale entries from
    // overwritten cells are removed at the grid write path since #7456
    // was fixed (grid/write.rs `remove_stale_extras*`); the flag check
    // remains as the fast path and as defense in depth.
    if cell.has_extras() {
        if let Some(extra) = grid.cell_extra(row, col) {
            for &combining in extra.combining() {
                out.push(combining);
            }
        }
    }
}

/// Append the combining-aware grapheme text of a SINGLE visible cell.
///
/// Public-facing single-cell counterpart of [`push_cell_text`] used by the
/// introspection `cell` verb: the resolved base char (NUL/`\0` → space) plus
/// any complex-cluster string and trailing combining marks — exactly the
/// content the selection/text paths and the renderer's
/// `cluster_row`/`combining_row` emit, so a cell read never drops accents or
/// ZWJ/emoji clusters. A wide-continuation (right half of a CJK/emoji glyph)
/// yields the empty string (its glyph belongs to the lead cell). Out-of-range
/// coordinates also yield the empty string.
pub(crate) fn cell_grapheme_string(grid: &Grid, row: u16, col: u16) -> String {
    let mut out = String::new();
    push_cell_text(grid, row, col, &mut out);
    out
}

/// Extract visible-row text for an inclusive column range.
#[must_use]
pub(crate) fn visible_row_bounds_to_string(
    grid: &Grid,
    row: u16,
    start_col: u16,
    end_col: u16,
) -> String {
    // Clamp end_col to actual grid width. `side_adjusted_bounds` uses
    // u16::MAX as a sentinel for "entire row" when the end retreats to the
    // previous row; iterating up to 65535 wastes ~65K no-op cell lookups.
    let end_col = end_col.min(grid.cols().saturating_sub(1));

    // If start_col falls on a wide_continuation cell, back up (#7526).
    // Context-aware check so DECSCA-protected cells are not mistaken for
    // continuations (shared bit 10).
    let mut effective_start = start_col;
    if effective_start > 0 && grid.is_wide_continuation_at(row, effective_start) {
        effective_start -= 1;
    }

    let mut line = String::new();
    for col in effective_start..=end_col {
        push_cell_text(grid, row, col, &mut line);
    }

    let trimmed_len = line.trim_end().len();
    line.truncate(trimmed_len);
    line
}

impl Terminal {
    // =========================================================================
    // Trigger evaluation helpers
    // =========================================================================

    /// Get visible content as string (for debugging/testing).
    #[must_use]
    pub fn visible_content(&self) -> String {
        self.grid.visible_content()
    }

    /// Get the text content of a specific visible row.
    ///
    /// Row 0 is the top visible row. Returns `None` if row is out of bounds.
    /// Useful for trigger evaluation on specific lines.
    #[must_use]
    pub fn row_text(&self, row: usize) -> Option<String> {
        let rows = usize::from(self.grid.rows());
        if row >= rows {
            return None;
        }
        self.grid.row_text(row_u16(row))
    }

    /// Get the combining-aware grapheme text of a single VISIBLE cell.
    ///
    /// Returns the resolved base character plus any complex-cluster string and
    /// trailing combining marks for visible-grid cell `(row, col)` — the SAME
    /// content the selection and `row_text`/`get_line_text` paths produce, so an
    /// introspecting reader of one cell never silently drops an NFD accent
    /// (`e`+U+0301) or a ZWJ emoji cluster (👨‍👩‍👧) the pixels and selection show.
    ///
    /// A wide-continuation cell (the blank right half of a CJK/emoji glyph)
    /// returns the empty string; its glyph belongs to the lead cell. Returns
    /// `None` only for an out-of-range row/col (so the caller can report a
    /// distinct "out of range"), and `Some("")` for a genuinely blank cell.
    #[must_use]
    pub fn cell_grapheme(&self, row: usize, col: usize) -> Option<String> {
        let r = u16::try_from(row).ok()?;
        let c = u16::try_from(col).ok()?;
        if usize::from(r) >= usize::from(self.grid.rows())
            || usize::from(c) >= usize::from(self.grid.cols())
        {
            return None;
        }
        Some(cell_grapheme_string(&self.grid, r, c))
    }

    /// Get the text content of a display-relative row, accounting for
    /// `display_offset` (scroll position into history).
    ///
    /// When `display_offset > 0`, display row 0 maps to a scrollback line,
    /// not live grid row 0. This method converts display-relative coordinates
    /// to terminal-relative coordinates and reads from the correct source
    /// (scrollback for negative terminal rows, live grid for non-negative).
    ///
    /// Returns `None` if the row is out of bounds.
    #[must_use]
    pub fn display_row_text(&self, display_row: usize) -> Option<String> {
        let offset = self.grid.display_offset();
        if offset == 0 {
            // Fast path: no scrollback scroll — display row == live grid row.
            return self.row_text(display_row);
        }

        // Convert display-relative row to terminal-relative (i32).
        // terminal_row = display_row - display_offset
        // Negative terminal_row = scrollback line.
        let visible_rows = usize::from(self.grid.rows());
        if display_row >= visible_rows {
            return None;
        }

        // display_row < visible_rows (u16::MAX), offset <= scrollback_lines (bounded).
        // Both fit in i64; subtraction cannot overflow.
        #[allow(
            clippy::cast_possible_wrap,
            reason = "both values bounded well within i64 range"
        )]
        let terminal_row = (display_row as i64) - (offset as i64);

        // Clamp to i32 range for get_line_text.
        let terminal_row_i32 = i32::try_from(terminal_row).ok()?;

        self.get_line_text(terminal_row_i32, None)
    }

    // ========================================================================
    // Smart Selection API
    // ========================================================================

    /// Get smart word boundaries at a position on a display-relative row.
    ///
    /// This uses context-aware selection rules to identify semantic text units
    /// like URLs, file paths, email addresses, git hashes, quoted strings, etc.
    /// Falls back to basic word boundaries for plain text.
    ///
    /// When the terminal is scrolled into history (`display_offset > 0`),
    /// display row 0 corresponds to a scrollback line, not live grid row 0.
    /// This method correctly reads from scrollback when needed.
    ///
    /// # Arguments
    ///
    /// * `row` - The display-relative row index (0 is top of viewport)
    /// * `col` - The column position
    /// * `smart` - The smart selection engine with configured rules
    ///
    /// # Returns
    ///
    /// Returns `Some((start_col, end_col))` if a word/semantic unit is found,
    /// `None` if the position is on whitespace or out of bounds.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use aterm_core::terminal::Terminal;
    /// use aterm_core::selection::SmartSelection;
    ///
    /// let terminal = Terminal::new(24, 80);
    /// let smart = SmartSelection::with_builtin_rules();
    /// let (row, col) = (5, 10);
    /// if let Some((start, end)) = terminal.smart_word_at(row, col, &smart) {
    ///     // Select from start to end column
    /// }
    /// ```
    #[must_use]
    pub fn smart_word_at(
        &self,
        row: usize,
        col: usize,
        smart: &crate::selection::SmartSelection,
    ) -> Option<(usize, usize)> {
        let text = self.display_row_text(row)?;
        smart.word_boundaries_at_column(&text, col)
    }
}

#[cfg(test)]
mod tests {
    use crate::terminal::Terminal;

    // ---- Stale CellExtras cleanup on overwrite (#7456) ----------------------
    //
    // End-to-end proof through the VT byte stream: extras-bearing cells
    // (hyperlink, combining marks) overwritten by plain text must leave NO
    // entry in the extras map (memory leak) and NO stale data reachable by
    // a later styled write on the same coordinate.

    #[test]
    fn plain_overwrite_removes_stale_hyperlink_extras_entries() {
        let mut term = Terminal::new(24, 80);
        term.process(b"\x1b]8;;https://example.com\x1b\\LINK\x1b]8;;\x1b\\");
        assert_eq!(
            term.hyperlink_at(0, 0),
            Some("https://example.com"),
            "linked text must carry the hyperlink"
        );
        assert_eq!(term.grid().extras().len(), 4, "one entry per linked cell");

        // Overwrite all 4 linked cells with plain text (default-style
        // ASCII — the blast fast path).
        term.process(b"\rplain");

        assert_eq!(term.row_text(0).as_deref(), Some("plain"));
        assert_eq!(term.hyperlink_at(0, 0), None);
        assert_eq!(
            term.grid().extras().len(),
            0,
            "stale extras entries must be removed on overwrite (#7456)"
        );
    }

    #[test]
    fn plain_overwrite_removes_stale_combining_mark_entry() {
        let mut term = Terminal::new(24, 80);
        term.process("e\u{0301}".as_bytes()); // 'e' + combining acute
        assert_eq!(term.row_text(0).as_deref(), Some("e\u{0301}"));
        assert_eq!(term.grid().extras().len(), 1, "combining mark entry");

        term.process(b"\rx");

        assert_eq!(
            term.row_text(0).as_deref(),
            Some("x"),
            "text API must show the plain char, not the stale mark"
        );
        assert_eq!(
            term.grid().extras().len(),
            0,
            "stale combining-mark entry must be removed on overwrite (#7456)"
        );
    }

    #[test]
    fn styled_overwrite_does_not_resurrect_stale_hyperlink() {
        // The resurrection case: an RGB-styled write landing on an old
        // hyperlink cell used to `get_or_create` the stale entry and
        // attach the OLD hyperlink to the NEW character.
        let mut term = Terminal::new(24, 80);
        term.process(b"\x1b]8;;https://example.com\x1b\\L\x1b]8;;\x1b\\");
        assert!(term.hyperlink_at(0, 0).is_some());

        term.process(b"\r\x1b[58:2::255:0:0m\x1b[4mZ\x1b[0m"); // underline-colored Z

        assert_eq!(term.row_text(0).as_deref(), Some("Z"));
        assert_eq!(
            term.hyperlink_at(0, 0),
            None,
            "old hyperlink must not attach to the new styled char (#7456)"
        );
    }
}
