// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0

//! Text selection to string conversion for Terminal.
//!
//! Contains `selection_to_string()` and `get_line_text()`.
//! Extracted from mod.rs to reduce file size.

use super::Terminal;
use super::content::visible_row_bounds_to_string;

/// Single-pass column range to byte offset conversion (#5581).
///
/// Walks graphemes once and returns byte offsets for both the start and end
/// columns, replacing the 5 sequential O(n) scans in the scrollback path.
///
/// - `start_col`: first column (inclusive)
/// - `end_col`: last column (inclusive); the returned end byte is one past this grapheme
///
/// Returns `(start_byte, end_byte)` suitable for `&s[start_byte..end_byte]`.
/// If `start_col` is past all content, returns `(s.len(), s.len())`.
fn column_range_to_byte_offsets(s: &str, start_col: usize, end_col: usize) -> (usize, usize) {
    use crate::grapheme::split_graphemes;

    let mut current_col = 0usize;
    let mut start_byte = s.len();
    let mut end_byte = s.len();
    let mut found_start = false;

    for g in split_graphemes(s) {
        let width = g.width;
        if width > 0 {
            let next_col = current_col + width;
            if !found_start && start_col < next_col {
                start_byte = g.byte_offset;
                found_start = true;
            }
            if next_col > end_col {
                end_byte = g.byte_offset + g.text.len();
                break;
            }
            current_col = next_col;
        }
    }

    (start_byte, end_byte)
}

impl Terminal {
    /// Get the selected text as a string.
    ///
    /// Returns `None` if there is no selection or if the selection is empty.
    /// For block selections, each row is separated by a newline.
    #[must_use]
    pub fn selection_to_string(&self) -> Option<String> {
        use crate::selection::SelectionType;

        // Use side-adjusted bounds so that the copied text matches the visual
        // highlight. Without this, a Right-sided start or Left-sided end would
        // include an extra character that isn't part of the rendered selection.
        let (adj_start_row, adj_start_col, adj_end_row, adj_end_col) =
            self.text_selection.side_adjusted_bounds()?;

        let mut result = String::new();
        let cols = self.grid.cols();
        if cols == 0 {
            return None;
        }

        match self.text_selection.selection_type() {
            SelectionType::Block => {
                // Rectangular selection: extract adjusted columns from each row
                for row in adj_start_row..=adj_end_row {
                    if row > adj_start_row {
                        result.push('\n');
                    }
                    if let Some(line) = self.get_line_text(row, Some((adj_start_col, adj_end_col)))
                    {
                        result.push_str(&line);
                    }
                }
            }
            // Simple, Semantic, Lines, and future variants all use linear selection
            _ => {
                let visible_rows = i32::from(self.grid.rows());
                for row in adj_start_row..=adj_end_row {
                    if row > adj_start_row {
                        // Only insert newline if this row is NOT a soft-wrap continuation.
                        // Row::is_wrapped() / Line::is_wrapped() means "this row continues
                        // the previous row's content" (soft wrap, not a hard line break).
                        #[allow(
                            clippy::redundant_closure_for_method_calls,
                            reason = "private row/line types prevent method-reference shorthand"
                        )]
                        let is_continuation = if row >= 0 && row < visible_rows {
                            u16::try_from(row)
                                .ok()
                                .and_then(|idx| self.grid.row(idx))
                                .is_some_and(|r| r.is_wrapped())
                        } else if row < 0 {
                            usize::try_from(-(i64::from(row)) - 1)
                                .ok()
                                .and_then(|rev_idx| self.grid.history_line_rev(rev_idx))
                                .is_some_and(|l| l.is_wrapped())
                        } else {
                            false
                        };
                        if !is_continuation {
                            result.push('\n');
                        }
                    }

                    let start_col = if row == adj_start_row {
                        adj_start_col
                    } else {
                        0
                    };
                    let end_col = if row == adj_end_row {
                        adj_end_col
                    } else {
                        cols - 1
                    };

                    if let Some(line) = self.get_line_text(row, Some((start_col, end_col))) {
                        result.push_str(&line);
                    }
                }
            }
        }

        if result.is_empty() {
            None
        } else {
            Some(result)
        }
    }

    /// Get text from a line (visible or scrollback).
    ///
    /// `col_range` specifies the column range to extract (inclusive).
    /// If `None`, extracts the entire line.
    pub fn get_line_text(&self, row: i32, col_range: Option<(u16, u16)>) -> Option<String> {
        let visible_rows = i32::from(self.grid.rows());

        if row >= 0 && row < visible_rows {
            // Visible row: row is in [0, visible_rows) where visible_rows <= u16::MAX
            let row_idx = u16::try_from(row).ok()?;
            let cols = self.grid.cols();
            if cols == 0 && col_range.is_none() {
                return Some(String::new());
            }
            let (start_col, end_col) = col_range.unwrap_or((0, cols.saturating_sub(1)));
            Some(visible_row_bounds_to_string(
                &self.grid, row_idx, start_col, end_col,
            ))
        } else if row < 0 {
            // Scrollback row (negative indices)
            // history_line_rev provides unified access to both ring buffer
            // and tiered scrollback: rev_idx 0 = most recently scrolled-off line.
            let rev_idx = usize::try_from(-(i64::from(row)) - 1).ok()?;
            if let Some(scrollback_line) = self.grid.history_line_rev(rev_idx) {
                let full_line = scrollback_line.to_string();
                let cols = self.grid.cols();
                if cols == 0 && col_range.is_none() {
                    return Some(String::new());
                }
                let (start_col, end_col) = col_range.unwrap_or((0, cols.saturating_sub(1)));

                // Single-pass column → byte offset conversion (#5581).
                // Replaces 5 sequential O(n) scans with one grapheme walk.
                let (start_byte, end_byte) = column_range_to_byte_offsets(
                    &full_line,
                    usize::from(start_col),
                    usize::from(end_col),
                );

                if start_byte < full_line.len() {
                    let slice = &full_line[start_byte..end_byte];
                    return Some(slice.trim_end().to_string());
                }
                return Some(String::new());
            }
            None
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::column_range_to_byte_offsets;

    #[test]
    fn ascii_full_range() {
        // "Hello" cols 0..4 → all 5 chars
        let (s, e) = column_range_to_byte_offsets("Hello", 0, 4);
        assert_eq!(&"Hello"[s..e], "Hello");
    }

    #[test]
    fn ascii_sub_range() {
        // "Hello World" cols 0..4 → "Hello"
        let (s, e) = column_range_to_byte_offsets("Hello World", 0, 4);
        assert_eq!(&"Hello World"[s..e], "Hello");
    }

    #[test]
    fn ascii_mid_range() {
        // "Hello World" cols 6..10 → "World"
        let (s, e) = column_range_to_byte_offsets("Hello World", 6, 10);
        assert_eq!(&"Hello World"[s..e], "World");
    }

    #[test]
    fn wide_char_single() {
        // "你好" — each CJK char is width 2: cols 0..1 → "你"
        let (s, e) = column_range_to_byte_offsets("你好", 0, 1);
        assert_eq!(&"你好"[s..e], "你");
    }

    #[test]
    fn wide_char_both() {
        // "你好" cols 0..3 → "你好" (col 0-1 = 你, col 2-3 = 好)
        let (s, e) = column_range_to_byte_offsets("你好", 0, 3);
        assert_eq!(&"你好"[s..e], "你好");
    }

    #[test]
    fn mixed_ascii_wide() {
        // "A你B" — A=col0, 你=col1-2, B=col3. Extract cols 1..2 → "你"
        let (s, e) = column_range_to_byte_offsets("A你B", 1, 2);
        assert_eq!(&"A你B"[s..e], "你");
    }

    #[test]
    fn start_past_content() {
        let s = "Hi";
        let (start, end) = column_range_to_byte_offsets(s, 10, 20);
        assert_eq!(start, s.len());
        assert_eq!(end, s.len());
    }

    #[test]
    fn empty_string() {
        let (s, e) = column_range_to_byte_offsets("", 0, 5);
        assert_eq!(s, 0);
        assert_eq!(e, 0);
    }

    #[test]
    fn single_column() {
        // "abc" col 1..1 → "b"
        let (s, e) = column_range_to_byte_offsets("abc", 1, 1);
        assert_eq!(&"abc"[s..e], "b");
    }
}
