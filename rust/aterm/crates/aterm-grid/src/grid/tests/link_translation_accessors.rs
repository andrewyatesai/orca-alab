// Copyright 2026 The aterm Authors
// Author: The aterm Authors
// SPDX-License-Identifier: Apache-2.0

//! Tests for the link/search-translation accessors exposed to wasm:
//! `base_y`, `display_origin_absolute`, `row_is_wrapped`, `row_len`.

use super::super::*;

#[test]
fn base_y_equals_oldest_absolute_plus_scrollback() {
    let mut grid = Grid::with_scrollback(3, 80, 100);
    for i in 0..10 {
        grid.write_char((b'A' + i) as char);
        grid.line_feed();
    }

    assert!(grid.scrollback_lines() > 0, "lines should have scrolled off");
    // base_y is the absolute row of the live/last line.
    let expected = grid.oldest_absolute_row() as usize + grid.scrollback_lines();
    assert_eq!(grid.base_y(), expected);
}

#[test]
fn display_origin_absolute_tracks_scroll() {
    let mut grid = Grid::with_scrollback(3, 80, 100);
    for i in 0..10 {
        grid.write_char((b'A' + i) as char);
        grid.line_feed();
    }

    // Not scrolled: top visible == base_y.
    assert_eq!(grid.display_offset(), 0);
    assert_eq!(grid.display_origin_absolute(), grid.base_y());

    // Scroll up two lines into history: origin drops by exactly the offset.
    grid.scroll_display(2);
    assert_eq!(grid.display_offset(), 2);
    assert_eq!(grid.display_origin_absolute(), grid.base_y() - 2);
}

#[test]
fn row_is_wrapped_true_on_continuation_row() {
    let mut grid = Grid::new(24, 5);
    for c in "Hello World".chars() {
        grid.write_char_wrap(c);
    }
    // Row 0 is the lead; row 1 is the soft-wrap continuation.
    assert_eq!(grid.row_is_wrapped(0), Some(false));
    assert_eq!(grid.row_is_wrapped(1), Some(true));
    // Out-of-range row yields None.
    assert_eq!(grid.row_is_wrapped(999), None);
}

#[test]
fn row_len_is_logical_length() {
    let mut grid = Grid::new(24, 80);
    for c in "abc".chars() {
        grid.write_char(c);
    }
    // Last non-empty cell + 1.
    assert_eq!(grid.row_len(0), Some(3));
    // A never-written row is blank (len 0).
    assert_eq!(grid.row_len(1), Some(0));
    assert_eq!(grid.row_len(999), None);
}
