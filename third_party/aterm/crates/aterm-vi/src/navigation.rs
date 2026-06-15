// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Vi mode navigation: bracket matching, paragraph motions, inline
//! search, and shared traversal helpers.
//!
//! Word motions (w/b/e/ge, W/B/E/gE) are in [`super::word`].
//! All functions operate through [`BufferAccess`]
//! and [`ViPoint`].

use aterm_types::BufferAccess;

use super::cell_char;
use super::types::ViPoint;

/// Default semantic separator characters (matches Alacritty default).
pub const DEFAULT_SEPARATORS: &str = ",│`|:\"' ()[]{}<>\t";

// ---------------------------------------------------------------------------
// Point traversal (pub(super) for use by word.rs)
// ---------------------------------------------------------------------------

/// Advance one cell forward (right then down), returning `None` at
/// the bottom-right corner of the grid.
pub(super) fn point_forward(grid: &dyn BufferAccess, p: ViPoint) -> Option<ViPoint> {
    let cols = grid.cols();
    if p.col + 1 < cols {
        Some(ViPoint::new(p.line, p.col + 1))
    } else {
        let bottom = i32::from(grid.visible_rows()) - 1;
        if p.line < bottom {
            Some(ViPoint::new(p.line + 1, 0))
        } else {
            None
        }
    }
}

/// Retreat one cell backward (left then up), returning `None` at
/// the top-left corner.
pub(super) fn point_backward(grid: &dyn BufferAccess, p: ViPoint) -> Option<ViPoint> {
    if p.col > 0 {
        Some(ViPoint::new(p.line, p.col - 1))
    } else {
        let top = -(grid.total_lines());
        if p.line > top {
            Some(ViPoint::new(p.line - 1, grid.cols().saturating_sub(1)))
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Bracket matching (%)
// ---------------------------------------------------------------------------

/// Bracket pairs for matching.
const BRACKET_PAIRS: &[(char, char)] = &[('(', ')'), ('[', ']'), ('{', '}'), ('<', '>')];

/// Find the matching bracket for the character at `point` (vim `%`).
///
/// Returns `None` if the character is not a bracket or no match exists.
pub fn bracket_match(grid: &dyn BufferAccess, point: ViPoint) -> Option<ViPoint> {
    let ch = cell_char(grid, point);

    // Determine the paired bracket and scan direction.
    let (pair, forward) = BRACKET_PAIRS.iter().find_map(|&(open, close)| {
        if ch == open {
            Some((close, true))
        } else if ch == close {
            Some((open, false))
        } else {
            None
        }
    })?;

    let mut depth: u32 = 1;
    let mut cur = point;

    loop {
        cur = if forward {
            point_forward(grid, cur)?
        } else {
            point_backward(grid, cur)?
        };

        let c = cell_char(grid, cur);
        if c == ch {
            depth += 1;
        } else if c == pair {
            depth -= 1;
            if depth == 0 {
                return Some(cur);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Paragraph motions ({ / })
// ---------------------------------------------------------------------------

/// Move up to the previous empty line (vim `{`).
pub fn paragraph_up(grid: &dyn BufferAccess, point: ViPoint) -> ViPoint {
    let top = -(grid.total_lines());
    let mut line = point.line;

    // Move up at least one line.
    if line > top {
        line -= 1;
    } else {
        return ViPoint::new(top, 0);
    }

    while line > top {
        if is_line_empty(grid, line) {
            return ViPoint::new(line, 0);
        }
        line -= 1;
    }

    ViPoint::new(top, 0)
}

/// Move down to the next empty line (vim `}`).
pub fn paragraph_down(grid: &dyn BufferAccess, point: ViPoint) -> ViPoint {
    let bottom = i32::from(grid.visible_rows()) - 1;
    let mut line = point.line;

    // Move down at least one line.
    if line < bottom {
        line += 1;
    } else {
        return ViPoint::new(bottom, 0);
    }

    while line < bottom {
        if is_line_empty(grid, line) {
            return ViPoint::new(line, 0);
        }
        line += 1;
    }

    ViPoint::new(bottom, 0)
}

/// Check if every cell on `line` is whitespace or null.
fn is_line_empty(grid: &dyn BufferAccess, line: i32) -> bool {
    let cols = grid.cols();
    for col in 0..cols {
        let ch = cell_char(grid, ViPoint::new(line, col));
        if ch != ' ' && ch != '\t' && ch != '\0' {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// Inline search (f / F / t / T)
// ---------------------------------------------------------------------------

/// Perform an inline character search from `point` to the right.
///
/// Returns the position of the first matching character, or `None`.
pub fn inline_search_right(
    grid: &dyn BufferAccess,
    point: ViPoint,
    needle: char,
) -> Option<ViPoint> {
    let cols = grid.cols();
    let mut col = point.col + 1;
    while col < cols {
        if cell_char(grid, ViPoint::new(point.line, col)) == needle {
            return Some(ViPoint::new(point.line, col));
        }
        col += 1;
    }
    None
}

/// Perform an inline character search from `point` to the left.
///
/// Returns the position of the first matching character, or `None`.
pub fn inline_search_left(
    grid: &dyn BufferAccess,
    point: ViPoint,
    needle: char,
) -> Option<ViPoint> {
    let mut col = point.col;
    while col > 0 {
        col -= 1;
        if cell_char(grid, ViPoint::new(point.line, col)) == needle {
            return Some(ViPoint::new(point.line, col));
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::MockGrid;

    // ---- Bracket matching ----

    #[test]
    fn bracket_match_forward() {
        let grid = MockGrid::new(1, 20).with_line(0, "(hello)");
        assert_eq!(
            bracket_match(&grid, ViPoint::new(0, 0)),
            Some(ViPoint::new(0, 6))
        );
    }

    #[test]
    fn bracket_match_backward() {
        let grid = MockGrid::new(1, 20).with_line(0, "(hello)");
        assert_eq!(
            bracket_match(&grid, ViPoint::new(0, 6)),
            Some(ViPoint::new(0, 0))
        );
    }

    #[test]
    fn bracket_match_nested() {
        let grid = MockGrid::new(1, 20).with_line(0, "((a)(b))");
        assert_eq!(
            bracket_match(&grid, ViPoint::new(0, 0)),
            Some(ViPoint::new(0, 7))
        );
    }

    #[test]
    fn bracket_match_none_for_non_bracket() {
        let grid = MockGrid::new(1, 10).with_line(0, "hello");
        assert_eq!(bracket_match(&grid, ViPoint::new(0, 0)), None);
    }

    #[test]
    fn bracket_match_curly() {
        let grid = MockGrid::new(1, 10).with_line(0, "{x}");
        assert_eq!(
            bracket_match(&grid, ViPoint::new(0, 0)),
            Some(ViPoint::new(0, 2))
        );
    }

    #[test]
    fn bracket_match_angle() {
        let grid = MockGrid::new(1, 20).with_line(0, "<html>");
        assert_eq!(
            bracket_match(&grid, ViPoint::new(0, 0)),
            Some(ViPoint::new(0, 5))
        );
    }

    #[test]
    fn bracket_match_angle_backward() {
        let grid = MockGrid::new(1, 20).with_line(0, "<x>");
        assert_eq!(
            bracket_match(&grid, ViPoint::new(0, 2)),
            Some(ViPoint::new(0, 0))
        );
    }

    #[test]
    fn bracket_match_empty_pair() {
        let grid = MockGrid::new(1, 10).with_line(0, "()");
        assert_eq!(
            bracket_match(&grid, ViPoint::new(0, 0)),
            Some(ViPoint::new(0, 1))
        );
    }

    #[test]
    fn bracket_match_deeply_nested() {
        let grid = MockGrid::new(1, 20).with_line(0, "(((())))");
        assert_eq!(
            bracket_match(&grid, ViPoint::new(0, 0)),
            Some(ViPoint::new(0, 7))
        );
    }

    #[test]
    fn bracket_match_mixed_nesting() {
        let grid = MockGrid::new(1, 20).with_line(0, "([{<>}])");
        assert_eq!(
            bracket_match(&grid, ViPoint::new(0, 0)),
            Some(ViPoint::new(0, 7))
        );
    }

    #[test]
    fn bracket_match_unmatched_open() {
        let grid = MockGrid::new(1, 10).with_line(0, "(abc");
        assert_eq!(bracket_match(&grid, ViPoint::new(0, 0)), None);
    }

    #[test]
    fn bracket_match_unmatched_close() {
        let grid = MockGrid::new(1, 10).with_line(0, "abc)");
        assert_eq!(bracket_match(&grid, ViPoint::new(0, 3)), None);
    }

    #[test]
    fn bracket_match_multirow() {
        let grid = MockGrid::new(3, 10)
            .with_line(0, "(         ")
            .with_line(1, "  hello   ")
            .with_line(2, "         )");
        assert_eq!(
            bracket_match(&grid, ViPoint::new(0, 0)),
            Some(ViPoint::new(2, 9))
        );
    }

    #[test]
    fn bracket_match_all_spaces_grid() {
        // Grid of all spaces — bracket at (0,0) won't match
        let grid = MockGrid::new(1, 10);
        assert_eq!(bracket_match(&grid, ViPoint::new(0, 0)), None);
    }

    // ---- Paragraph motions ----

    #[test]
    fn paragraph_up_finds_empty_line() {
        let grid = MockGrid::new(5, 10)
            .with_line(0, "aaa")
            .with_line(2, "bbb")
            .with_line(3, "ccc")
            .with_line(4, "ddd");
        assert_eq!(paragraph_up(&grid, ViPoint::new(3, 0)).line, 1);
    }

    #[test]
    fn paragraph_down_finds_empty_line() {
        let grid = MockGrid::new(5, 10)
            .with_line(0, "aaa")
            .with_line(1, "bbb")
            .with_line(3, "ccc")
            .with_line(4, "ddd");
        assert_eq!(paragraph_down(&grid, ViPoint::new(0, 0)).line, 2);
    }

    #[test]
    fn paragraph_up_clamps_to_top() {
        let grid = MockGrid::new(3, 10)
            .with_line(0, "aaa")
            .with_line(1, "bbb")
            .with_line(2, "ccc");
        assert_eq!(paragraph_up(&grid, ViPoint::new(2, 0)).line, 0);
    }

    #[test]
    fn paragraph_down_clamps_to_bottom() {
        let grid = MockGrid::new(3, 10)
            .with_line(0, "aaa")
            .with_line(1, "bbb")
            .with_line(2, "ccc");
        assert_eq!(paragraph_down(&grid, ViPoint::new(0, 0)).line, 2);
    }

    // ---- Inline search ----

    #[test]
    fn inline_search_right_finds_char() {
        let grid = MockGrid::new(1, 20).with_line(0, "hello world");
        assert_eq!(
            inline_search_right(&grid, ViPoint::new(0, 0), 'o'),
            Some(ViPoint::new(0, 4))
        );
    }

    #[test]
    fn inline_search_left_finds_char() {
        let grid = MockGrid::new(1, 20).with_line(0, "hello world");
        assert_eq!(
            inline_search_left(&grid, ViPoint::new(0, 10), 'o'),
            Some(ViPoint::new(0, 7))
        );
    }

    #[test]
    fn inline_search_right_not_found() {
        let grid = MockGrid::new(1, 20).with_line(0, "hello");
        assert_eq!(inline_search_right(&grid, ViPoint::new(0, 0), 'z'), None);
    }

    #[test]
    fn inline_search_left_not_found() {
        let grid = MockGrid::new(1, 20).with_line(0, "hello");
        assert_eq!(inline_search_left(&grid, ViPoint::new(0, 4), 'z'), None);
    }

    // ---- Regression: scrollback boundary (#5612) ----

    /// Regression for #5612: point_backward must stop at -total_lines,
    /// not -(display_offset + total_lines). The display_offset controls
    /// which part of the buffer is *visible*, not the navigable extent.
    #[test]
    fn test_point_backward_scrollback_boundary_ignores_display_offset() {
        // 3 visible rows, 5 scrollback lines, display_offset=3
        let grid = MockGrid::new(3, 10)
            .with_scrollback(5)
            .with_display_offset(3);
        // Top should be -5 (total_lines), not -8 (display_offset + total_lines).
        // Navigate backward from the top-left corner of visible area.
        let result = point_backward(&grid, ViPoint::new(-5, 0));
        assert_eq!(result, None, "should stop at -total_lines, not go further");
    }

    /// Regression for #5612: paragraph_up uses the same formula as
    /// point_backward and must also stop at -total_lines.
    ///
    /// Starting from line -5 (exactly at the boundary) ensures the
    /// boundary clamp is exercised, not just empty-line detection.
    /// With the old formula `top = -(offset + total) = -8`, this would
    /// return ViPoint(-6, 0) — below the real scrollback boundary.
    #[test]
    fn test_paragraph_up_scrollback_boundary_ignores_display_offset() {
        let grid = MockGrid::new(3, 10)
            .with_scrollback(5)
            .with_display_offset(3);
        let result = paragraph_up(&grid, ViPoint::new(-5, 0));
        assert_eq!(
            result,
            ViPoint::new(-5, 0),
            "paragraph_up at boundary should clamp to -total_lines, not go to -8"
        );
    }

    /// Regression for #5612: point_backward should allow navigation up to
    /// -total_lines but not beyond, regardless of display_offset.
    #[test]
    fn test_point_backward_allows_navigation_to_scrollback() {
        let grid = MockGrid::new(3, 10)
            .with_scrollback(5)
            .with_display_offset(3);
        // Should be able to navigate backward from line -4 to -5.
        let result = point_backward(&grid, ViPoint::new(-4, 0));
        assert_eq!(
            result,
            Some(ViPoint::new(-5, 9)),
            "should navigate to last col of previous scrollback line"
        );
    }

    /// Regression for #5612: bracket_match into scrollback should use
    /// the correct boundary so it can find brackets in scrollback content.
    #[test]
    fn test_bracket_match_respects_scrollback_boundary() {
        // Use non-zero display_offset (#5618): with display_offset=0, the old
        // buggy boundary -(offset + total) equals the correct -(total), hiding
        // regressions. display_offset=3 separates the two formulas.
        let grid = MockGrid::new(2, 10)
            .with_scrollback(3)
            .with_display_offset(3)
            .with_line(0, "(hello    ")
            .with_line(1, "    world)");
        // Forward search: ( -> )
        let result = bracket_match(&grid, ViPoint::new(0, 0));
        assert_eq!(
            result,
            Some(ViPoint::new(1, 9)),
            "bracket match should find closing paren on visible row"
        );
        // Backward search: ) -> ( — exercises point_backward boundary
        let result = bracket_match(&grid, ViPoint::new(1, 9));
        assert_eq!(
            result,
            Some(ViPoint::new(0, 0)),
            "backward bracket match should find opening paren"
        );
    }
}
