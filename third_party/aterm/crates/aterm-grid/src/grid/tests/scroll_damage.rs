// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Targeted damage tests.

use super::*;

fn write_marker_line(grid: &mut Grid, marker: char) {
    grid.carriage_return();
    grid.write_char(marker);
    grid.carriage_return();
    grid.line_feed();
}

fn collect_dirty_rows(grid: &Grid) -> Vec<u16> {
    grid.damage()
        .iter_bounds(grid.rows(), grid.cols())
        .map(|bound| bound.line)
        .collect()
}

fn build_scrollback_grid(rows: u16, cols: u16, scrollback: usize, line_count: usize) -> Grid {
    let mut grid = Grid::with_scrollback(rows, cols, scrollback);
    for i in 0..line_count {
        write_marker_line(&mut grid, (b'A' + (i % 26) as u8) as char);
    }
    grid
}

#[test]
fn scroll_to_top_near_top_uses_partial_damage() {
    let mut grid = build_scrollback_grid(5, 10, 20, 10);
    let near_top = grid.scrollback_lines().saturating_sub(2);
    grid.scroll_display(i32::try_from(near_top).unwrap_or(i32::MAX));
    assert_eq!(grid.display_offset(), near_top);

    grid.clear_damage();
    grid.scroll_to_top();

    assert_eq!(grid.display_offset(), grid.scrollback_lines());
    assert!(grid.damage().has_damage());
    assert!(!grid.damage().is_full());
    assert_eq!(collect_dirty_rows(&grid), vec![0, 1]);
}

#[test]
fn clamp_display_offset_small_delta_marks_bottom_rows() {
    let mut grid = build_scrollback_grid(5, 10, 20, 10);
    let max_offset = grid.scrollback_lines();
    assert!(max_offset > 0);

    grid.clear_damage();
    grid.storage.display_offset = max_offset + 2;
    grid.clamp_display_offset();

    assert_eq!(grid.display_offset(), max_offset);
    assert!(grid.damage().has_damage());
    assert!(!grid.damage().is_full());
    assert_eq!(collect_dirty_rows(&grid), vec![3, 4]);
}
