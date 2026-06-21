// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Semantic and whitespace word motions (w/b/e/ge, W/B/E/gE).
//!
//! Semantic motions use a configurable separator string. Whitespace
//! (WORD) motions use a fixed set: space, tab, null.

use aterm_types::BufferAccess;

use super::cell_char;
use super::navigation::{point_backward, point_forward};
use super::types::ViPoint;

// ---------------------------------------------------------------------------
// Character classification
// ---------------------------------------------------------------------------

/// Whether `ch` is a semantic word separator.
fn is_separator(ch: char, separators: &str) -> bool {
    ch == '\0' || separators.contains(ch)
}

/// Whether `ch` is whitespace for WORD motions.
fn is_whitespace(ch: char) -> bool {
    ch == ' ' || ch == '\t' || ch == '\0'
}

// ---------------------------------------------------------------------------
// Semantic word motions (w / b / e / ge)
// ---------------------------------------------------------------------------

/// Move to the start of the next semantic word (vim `w`).
pub fn semantic_word_right(grid: &dyn BufferAccess, point: ViPoint, separators: &str) -> ViPoint {
    let mut cur = point;

    // Skip current word characters (non-separators).
    while !is_separator(cell_char(grid, cur), separators) {
        match point_forward(grid, cur) {
            Some(p) => cur = p,
            None => return cur,
        }
    }

    // Skip separators / spaces.
    while is_separator(cell_char(grid, cur), separators) {
        match point_forward(grid, cur) {
            Some(p) => cur = p,
            None => return cur,
        }
    }

    cur
}

/// Move to the start of the previous semantic word (vim `b`).
pub fn semantic_word_left(grid: &dyn BufferAccess, point: ViPoint, separators: &str) -> ViPoint {
    let Some(mut cur) = point_backward(grid, point) else {
        return point;
    };

    // Skip separators backward.
    while is_separator(cell_char(grid, cur), separators) {
        match point_backward(grid, cur) {
            Some(p) => cur = p,
            None => return cur,
        }
    }

    // Skip word characters backward until a separator is found.
    while !is_separator(cell_char(grid, cur), separators) {
        match point_backward(grid, cur) {
            Some(p) => {
                if is_separator(cell_char(grid, p), separators) {
                    return cur;
                }
                cur = p;
            }
            None => return cur,
        }
    }

    // Step forward past the separator we landed on.
    point_forward(grid, cur).unwrap_or(cur)
}

/// Move to the end of the current/next semantic word (vim `e`).
pub fn semantic_word_right_end(
    grid: &dyn BufferAccess,
    point: ViPoint,
    separators: &str,
) -> ViPoint {
    let Some(mut cur) = point_forward(grid, point) else {
        return point;
    };

    // Skip separators.
    while is_separator(cell_char(grid, cur), separators) {
        match point_forward(grid, cur) {
            Some(p) => cur = p,
            None => return cur,
        }
    }

    // Advance through word characters, stopping at the last one.
    loop {
        match point_forward(grid, cur) {
            Some(p) => {
                if is_separator(cell_char(grid, p), separators) {
                    return cur;
                }
                cur = p;
            }
            None => return cur,
        }
    }
}

/// Move to the end of the previous semantic word (vim `ge`).
pub fn semantic_word_left_end(
    grid: &dyn BufferAccess,
    point: ViPoint,
    separators: &str,
) -> ViPoint {
    let Some(mut cur) = point_backward(grid, point) else {
        return point;
    };

    // Skip current word characters backward.
    while !is_separator(cell_char(grid, cur), separators) {
        match point_backward(grid, cur) {
            Some(p) => cur = p,
            None => return cur,
        }
    }

    // Skip separators backward.
    while is_separator(cell_char(grid, cur), separators) {
        match point_backward(grid, cur) {
            Some(p) => cur = p,
            None => return cur,
        }
    }

    cur
}

// ---------------------------------------------------------------------------
// Whitespace word motions (W / B / E / gE)
// ---------------------------------------------------------------------------

/// Move to the start of the next WORD (vim `W`).
pub fn whitespace_word_right(grid: &dyn BufferAccess, point: ViPoint) -> ViPoint {
    let mut cur = point;

    // Skip non-whitespace.
    while !is_whitespace(cell_char(grid, cur)) {
        match point_forward(grid, cur) {
            Some(p) => cur = p,
            None => return cur,
        }
    }

    // Skip whitespace.
    while is_whitespace(cell_char(grid, cur)) {
        match point_forward(grid, cur) {
            Some(p) => cur = p,
            None => return cur,
        }
    }

    cur
}

/// Move to the start of the previous WORD (vim `B`).
pub fn whitespace_word_left(grid: &dyn BufferAccess, point: ViPoint) -> ViPoint {
    let Some(mut cur) = point_backward(grid, point) else {
        return point;
    };

    // Skip whitespace backward.
    while is_whitespace(cell_char(grid, cur)) {
        match point_backward(grid, cur) {
            Some(p) => cur = p,
            None => return cur,
        }
    }

    // Skip non-whitespace backward until whitespace is found.
    while !is_whitespace(cell_char(grid, cur)) {
        match point_backward(grid, cur) {
            Some(p) => {
                if is_whitespace(cell_char(grid, p)) {
                    return cur;
                }
                cur = p;
            }
            None => return cur,
        }
    }

    point_forward(grid, cur).unwrap_or(cur)
}

/// Move to the end of the current/next WORD (vim `E`).
pub fn whitespace_word_right_end(grid: &dyn BufferAccess, point: ViPoint) -> ViPoint {
    let Some(mut cur) = point_forward(grid, point) else {
        return point;
    };

    // Skip whitespace.
    while is_whitespace(cell_char(grid, cur)) {
        match point_forward(grid, cur) {
            Some(p) => cur = p,
            None => return cur,
        }
    }

    // Advance through non-whitespace, stopping at the last one.
    loop {
        match point_forward(grid, cur) {
            Some(p) => {
                if is_whitespace(cell_char(grid, p)) {
                    return cur;
                }
                cur = p;
            }
            None => return cur,
        }
    }
}

/// Move to the end of the previous WORD (vim `gE`).
pub fn whitespace_word_left_end(grid: &dyn BufferAccess, point: ViPoint) -> ViPoint {
    let Some(mut cur) = point_backward(grid, point) else {
        return point;
    };

    // Skip non-whitespace backward.
    while !is_whitespace(cell_char(grid, cur)) {
        match point_backward(grid, cur) {
            Some(p) => cur = p,
            None => return cur,
        }
    }

    // Skip whitespace backward.
    while is_whitespace(cell_char(grid, cur)) {
        match point_backward(grid, cur) {
            Some(p) => cur = p,
            None => return cur,
        }
    }

    cur
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::navigation::DEFAULT_SEPARATORS;
    use crate::test_utils::MockGrid;

    const SEP: &str = DEFAULT_SEPARATORS;

    // ---- Semantic word motions (w / b / e / ge) ----

    #[test]
    fn word_right_skips_word_then_spaces() {
        let grid = MockGrid::new(1, 20).with_line(0, "hello world");
        let result = semantic_word_right(&grid, ViPoint::new(0, 0), SEP);
        assert_eq!(result.col, 6);
    }

    #[test]
    fn word_right_stops_at_end() {
        let grid = MockGrid::new(1, 10).with_line(0, "abcdef");
        let result = semantic_word_right(&grid, ViPoint::new(0, 0), SEP);
        assert_eq!(result.line, 0);
    }

    #[test]
    fn word_left_finds_start_of_current_word() {
        let grid = MockGrid::new(1, 20).with_line(0, "hello world");
        let result = semantic_word_left(&grid, ViPoint::new(0, 8), SEP);
        assert_eq!(result.col, 6);
    }

    #[test]
    fn word_left_jumps_to_previous_word() {
        let grid = MockGrid::new(1, 20).with_line(0, "hello world");
        let result = semantic_word_left(&grid, ViPoint::new(0, 6), SEP);
        assert_eq!(result.col, 0);
    }

    #[test]
    fn word_right_end_finds_end_of_next_word() {
        let grid = MockGrid::new(1, 20).with_line(0, "hello world");
        let result = semantic_word_right_end(&grid, ViPoint::new(0, 0), SEP);
        assert_eq!(result.col, 4);
    }

    #[test]
    fn word_left_end_finds_end_of_previous_word() {
        let grid = MockGrid::new(1, 20).with_line(0, "hello world");
        let result = semantic_word_left_end(&grid, ViPoint::new(0, 8), SEP);
        assert_eq!(result.col, 4);
    }

    // ---- Whitespace word motions (W / B / E / gE) ----

    #[test]
    fn ws_word_right_skips_over_punctuation() {
        let grid = MockGrid::new(1, 30).with_line(0, "foo.bar baz");
        let result = whitespace_word_right(&grid, ViPoint::new(0, 0));
        assert_eq!(result.col, 8);
    }

    #[test]
    fn ws_word_left_finds_start() {
        let grid = MockGrid::new(1, 30).with_line(0, "foo.bar baz");
        let result = whitespace_word_left(&grid, ViPoint::new(0, 9));
        assert_eq!(result.col, 8);
    }

    #[test]
    fn ws_word_right_end_finds_end() {
        let grid = MockGrid::new(1, 30).with_line(0, "foo.bar baz");
        let result = whitespace_word_right_end(&grid, ViPoint::new(0, 0));
        assert_eq!(result.col, 6);
    }

    #[test]
    fn ws_word_left_end_finds_end_of_previous() {
        let grid = MockGrid::new(1, 30).with_line(0, "foo.bar baz");
        let result = whitespace_word_left_end(&grid, ViPoint::new(0, 9));
        assert_eq!(result.col, 6);
    }
}
