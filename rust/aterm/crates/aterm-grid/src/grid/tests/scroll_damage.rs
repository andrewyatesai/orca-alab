// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
// Author: Andrew Yates

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

// =========================================================================
// content_gen (P1.0): advances on CONTENT mutation, NOT on viewport scroll.
// =========================================================================

/// content_gen is initialized NONZERO in every construct path, so a reader can
/// use `0` as a "never observed" sentinel.
#[test]
fn content_gen_starts_nonzero() {
    assert!(Grid::new(24, 80).content_gen() > 0);
    assert!(Grid::with_scrollback(5, 10, 100).content_gen() > 0);
    let sb = aterm_scrollback::Scrollback::new(50, 500, 500_000);
    assert!(Grid::with_tiered_scrollback(10, 40, 500, sb).content_gen() > 0);
}

/// A cell write, a line erase, and a content scroll each advance content_gen;
/// content_gen is monotonic.
#[test]
fn content_gen_advances_on_content_mutations() {
    let mut grid = Grid::with_scrollback(4, 10, 100);
    let g0 = grid.content_gen();

    // Cell write.
    grid.write_char('X');
    let g1 = grid.content_gen();
    assert!(g1 > g0, "write_char must advance content_gen");

    // Line erase (EL).
    grid.erase_line();
    let g2 = grid.content_gen();
    assert!(g2 > g1, "erase_line must advance content_gen");

    // Content scroll (rows move into scrollback).
    grid.scroll_up(1);
    let g3 = grid.content_gen();
    assert!(g3 > g2, "scroll_up must advance content_gen");

    // A region scroll also mutates content and must advance.
    grid.set_scroll_region(0, 3);
    grid.scroll_region_up(1);
    let g4 = grid.content_gen();
    assert!(g4 > g3, "scroll_region_up must advance content_gen");
}

/// A pure VIEWPORT scroll (scroll_display) damages the grid but must NOT advance
/// content_gen — this is the content/viewport divergence at the heart of P1.0.
#[test]
fn content_gen_unchanged_on_viewport_scroll() {
    let mut grid = build_scrollback_grid(5, 10, 20, 10);
    let before = grid.content_gen();

    // Scroll into history: viewport-only change.
    grid.scroll_display(3);
    assert!(grid.damage().has_damage(), "viewport scroll still damages");
    assert_eq!(
        grid.content_gen(),
        before,
        "scroll_display must NOT advance content_gen"
    );

    // scroll_to_top / scroll_to_bottom / scroll_display(0) are all viewport-only.
    grid.scroll_to_top();
    assert_eq!(grid.content_gen(), before, "scroll_to_top is viewport-only");
    grid.scroll_to_bottom();
    assert_eq!(
        grid.content_gen(),
        before,
        "scroll_to_bottom is viewport-only"
    );
    grid.scroll_display(0);
    assert_eq!(
        grid.content_gen(),
        before,
        "zero-delta scroll is viewport-only"
    );
}

/// A no-op scroll_up(0) must not advance content_gen.
#[test]
fn content_gen_unchanged_on_noop_scroll_up() {
    let mut grid = Grid::with_scrollback(4, 10, 100);
    let before = grid.content_gen();
    grid.scroll_up(0);
    assert_eq!(grid.content_gen(), before, "scroll_up(0) is a no-op");
}

proptest::proptest! {
    /// content_gen never decreases across any op (content OR viewport); and
    /// whenever the LIVE row text (read at display_offset == 0, i.e. the actual
    /// content, not the windowed view) changes, content_gen strictly increases.
    ///
    /// Viewport ops (`scroll_display`/`scroll_to_top`) change which scrollback
    /// lines `row_text` returns, so the text comparison is only meaningful when
    /// the viewport is snapped to live (display_offset == 0). We snap to bottom
    /// before each text snapshot so the comparison reflects content, not window.
    #[test]
    fn content_gen_tracks_row_text_changes(
        ops in proptest::collection::vec(0u8..6, 1..40)
    ) {
        let mut grid = Grid::with_scrollback(6, 12, 200);

        for op in ops {
            // Snapshot LIVE content (snap viewport to bottom first). Snapping is
            // itself viewport-only and must not advance content_gen.
            grid.scroll_to_bottom();
            let gen_before = grid.content_gen();
            let text_before: Vec<Option<String>> =
                (0..grid.rows()).map(|r| grid.row_text(r)).collect();
            proptest::prop_assert_eq!(
                grid.content_gen(),
                gen_before,
                "scroll_to_bottom (viewport-only) must not advance content_gen"
            );

            match op {
                0 => grid.write_char('A'),
                1 => grid.erase_line(),
                2 => grid.scroll_up(1),
                3 => grid.line_feed(),
                4 => grid.scroll_display(1),   // viewport-only
                _ => grid.scroll_to_top(),     // viewport-only
            }

            // Never decreases (holds for content AND viewport ops).
            let gen_after_op = grid.content_gen();
            proptest::prop_assert!(
                gen_after_op >= gen_before,
                "content_gen decreased: {} -> {}", gen_before, gen_after_op
            );

            // Re-snap to live and compare CONTENT.
            grid.scroll_to_bottom();
            let text_after: Vec<Option<String>> =
                (0..grid.rows()).map(|r| grid.row_text(r)).collect();
            let gen_after = grid.content_gen();

            // If the live content changed, content_gen must have advanced.
            if text_after != text_before {
                proptest::prop_assert!(
                    gen_after > gen_before,
                    "live row text changed but content_gen did not advance"
                );
            }
        }
    }
}
