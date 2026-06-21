// Copyright 2026 The aterm Authors
// Author: The aterm Authors
// SPDX-License-Identifier: Apache-2.0

//! Scrollback tests — tiered storage, attach/detach, content preservation.
//!
//! Migrated from aterm-core as part of #6556 Batch 2.

use super::super::*;

#[test]
fn grid_scroll_display() {
    let mut grid = Grid::with_scrollback(3, 80, 100);

    // Fill some content
    for i in 0..10 {
        grid.write_char((b'A' + i) as char);
        grid.line_feed();
    }

    assert!(grid.scrollback_lines() > 0);

    grid.scroll_display(2);
    assert_eq!(grid.display_offset(), 2);

    grid.scroll_to_bottom();
    assert_eq!(grid.display_offset(), 0);
}

#[test]
fn grid_erase_scrollback_preserves_live_rows() {
    let scrollback = Scrollback::new(100, 1000, 10_000_000);
    let mut grid = Grid::with_tiered_scrollback(3, 4, 2, scrollback);

    for i in 0..8 {
        grid.carriage_return();
        for c in format!("L{i}").chars() {
            grid.write_char(c);
        }
        if i < 7 {
            grid.line_feed();
        }
    }

    assert!(grid.scrollback_lines() > 0);
    assert!(grid.tiered_scrollback_lines() > 0);

    let live_rows: Vec<String> = (0..grid.rows())
        .map(|row| grid.row(row).unwrap().to_string())
        .collect();

    grid.scroll_display(1);
    assert!(grid.display_offset() > 0);

    grid.erase_scrollback();

    assert_eq!(grid.scrollback_lines(), 0);
    assert_eq!(grid.tiered_scrollback_lines(), 0);
    assert_eq!(grid.display_offset(), 0);
    assert_eq!(grid.total_lines(), grid.rows() as usize);

    for (row_idx, expected) in live_rows.iter().enumerate() {
        assert_eq!(grid.row(row_idx as u16).unwrap().to_string(), *expected);
    }
}

#[test]
fn grid_with_tiered_scrollback() {
    let scrollback = Scrollback::new(100, 1000, 10_000_000);
    let mut grid = Grid::with_tiered_scrollback(3, 80, 5, scrollback);

    let sb = grid
        .scrollback()
        .expect("scrollback must be present after construction");
    assert_eq!(sb.line_count(), 0, "initial scrollback should have 0 lines");
    assert_eq!(grid.tiered_scrollback_lines(), 0);

    // Fill content to trigger scrollback
    for i in 0..20 {
        for c in format!("Line {i}").chars() {
            grid.write_char(c);
        }
        grid.line_feed();
    }

    // Some lines should be in tiered scrollback now
    assert!(grid.tiered_scrollback_lines() > 0);

    // Total scrollback should include both ring buffer and tiered
    assert!(grid.scrollback_lines() > grid.storage.ring_buffer_scrollback());
}

#[test]
fn grid_scrollback_content_preserved() {
    let scrollback = Scrollback::new(100, 1000, 10_000_000);
    // Small ring buffer of 2 lines to force early promotion
    let mut grid = Grid::with_tiered_scrollback(3, 80, 2, scrollback);

    // Write 10 lines
    for i in 0..10 {
        for c in format!("Line {i}").chars() {
            grid.write_char(c);
        }
        grid.line_feed();
    }

    // Check that content is preserved in tiered scrollback
    let sb = grid.scrollback_mut().unwrap();
    assert!(
        sb.line_count() > 0,
        "Expected scrollback lines after writing 10 lines"
    );

    // First line in scrollback must be retrievable and contain expected content
    let line = sb
        .get_line(0)
        .expect("get_line(0) should not error")
        .expect("get_line(0) must return data when line_count > 0");
    let text = line.to_string();
    assert!(text.starts_with("Line "), "Expected 'Line X', got '{text}'");
}

#[test]
fn grid_attach_detach_scrollback() {
    let mut grid = Grid::new(24, 80);
    assert!(grid.scrollback().is_none());

    let scrollback = Scrollback::new(100, 1000, 10_000_000);
    grid.attach_scrollback(scrollback);
    let sb = grid
        .scrollback()
        .expect("scrollback must be present after attach");
    assert_eq!(
        sb.line_count(),
        0,
        "freshly attached scrollback should have 0 lines"
    );

    let detached = grid
        .detach_scrollback()
        .expect("detach must return the scrollback");
    assert_eq!(
        detached.line_count(),
        0,
        "detached scrollback should still have 0 lines"
    );
    assert!(
        grid.scrollback().is_none(),
        "scrollback must be None after detach"
    );
}

#[test]
fn grid_scrollback_wrapped_lines() {
    let scrollback = Scrollback::new(100, 1000, 10_000_000);
    let mut grid = Grid::with_tiered_scrollback(3, 5, 2, scrollback);

    // Write a line that wraps
    for c in "HelloWorld".chars() {
        grid.write_char_wrap(c);
    }
    grid.line_feed();

    // Force more scrolling to push lines to tiered scrollback
    for _ in 0..10 {
        grid.line_feed();
    }

    // Check that wrapped flag is preserved
    let sb = grid.scrollback_mut().unwrap();
    assert!(
        sb.line_count() > 1,
        "Expected >1 scrollback lines after wrapping; got {}",
        sb.line_count()
    );
    // The second line should be marked as wrapped
    let line = sb.get_line(1).expect("no I/O error").expect("line present");
    assert!(
        line.is_wrapped(),
        "Wrapped line should preserve wrapped flag"
    );
}

#[test]
fn ring_buffer_scrollback_preserves_hyperlinks() {
    // Regression test for #4149: hyperlinks are preserved when rows scroll from
    // the visible area into ring buffer scrollback (max_scrollback > 0).
    // Before the fix, row_to_line_static used empty CellExtras, losing hyperlinks.
    use std::sync::Arc;

    let mut grid = Grid::with_scrollback(4, 10, 10);

    // Write "Hello" on row 0 with a hyperlink on columns 0-4
    let url: Arc<str> = Arc::from("https://example.com");
    grid.set_cursor(0, 0);
    for c in "Hello".chars() {
        grid.write_char(c);
    }
    for col in 0..5u16 {
        grid.extras_mut()
            .get_or_create(CellCoord::new(0, col))
            .set_hyperlink(Some(url.clone()));
    }

    // Scroll 4 times to push row 0 into ring buffer scrollback (not tiered,
    // since max_scrollback=10 means ring buffer has room for 10 scrollback rows).
    for _ in 0..4 {
        grid.line_feed();
    }

    // Row 0 is now in ring buffer scrollback. Retrieve it via history_line_rev.
    let ring_sb = grid.storage.ring_buffer_scrollback();
    assert!(ring_sb > 0, "should have ring buffer scrollback");

    // The oldest scrollback line (rev_idx = ring_sb-1) should have hyperlinks.
    let line = grid
        .history_line_rev(ring_sb - 1)
        .expect("scrollback line should exist");
    let spans = line
        .hyperlinks()
        .expect("ring buffer scrollback line should preserve hyperlinks");
    assert!(!spans.is_empty(), "should have at least one hyperlink span");
    assert_eq!(&*spans[0].url, "https://example.com");
}

/// Verify invariant: ring_extras.len() == ring_buffer_scrollback() after
/// scroll_up, across both growth phase (under capacity) and reuse phase (at
/// capacity). This invariant is critical for correct extras display in
/// ring buffer scrollback (#4149, #4215).
#[test]
fn ring_extras_len_equals_ring_buffer_scrollback() {
    // Grid with 4 visible rows and max_scrollback=6.
    // Total capacity = 4 + 6 = 10 rows.
    let mut grid = Grid::with_scrollback(4, 10, 6);

    // Helper: check the invariant
    let check = |grid: &Grid, label: &str| {
        let ring_sb = grid.storage.ring_buffer_scrollback();
        let extras_len = grid.storage.ring_extras.len();
        assert_eq!(
            extras_len, ring_sb,
            "{label}: ring_extras.len()={extras_len} != ring_buffer_scrollback()={ring_sb}"
        );
    };

    check(&grid, "initial");

    // Phase 1: Growth — each scroll_up(1) adds a row until capacity.
    // ring_buffer_scrollback = total_lines - visible_rows
    for i in 1..=6 {
        grid.scroll_up(1);
        check(&grid, &format!("growth scroll {i}"));
    }
    // Now at capacity: total_lines=10, ring_buffer_scrollback=6

    // Phase 2: Reuse — rows are recycled, oldest goes to (no) tiered scrollback.
    for i in 1..=5 {
        grid.scroll_up(1);
        check(&grid, &format!("reuse scroll {i}"));
    }

    // Batch scroll
    grid.scroll_up(3);
    check(&grid, "batch scroll 3");

    // Large batch exceeding total rows
    grid.scroll_up(20);
    check(&grid, "large batch scroll 20");
}

/// Verify that complex chars, RGB colors, and combining marks survive
/// scrollback transit through the ring buffer (#4215).
#[test]
fn scrollback_preserves_complex_chars_rgb_and_combining() {
    use std::sync::Arc;

    // Grid: 3 visible rows, 10 cols, max_scrollback=5 (ring buffer only)
    let mut grid = Grid::with_scrollback(3, 10, 5);

    // Col 0: 'A' with RGB fg red — set cell with RGB-flagged colors + extras
    let fg_red = PackedColor::rgb(0xFF, 0x00, 0x00);
    let bg_default = PackedColor::DEFAULT_BG;
    let cell_a = Cell::with_style('A', fg_red, bg_default, CellFlags::empty());
    grid.row_mut(0).unwrap().set(0, cell_a);
    grid.storage
        .extras
        .get_or_create(CellCoord::new(0, 0))
        .set_fg_rgb(Some([0xFF, 0x00, 0x00]));

    // Col 1: complex char 🐛 — set overflow index + complex_char in extras
    let cell_complex = Cell::with_overflow_index(42);
    grid.row_mut(0).unwrap().set(1, cell_complex);
    grid.storage
        .extras
        .get_or_create(CellCoord::new(0, 1))
        .set_complex_char(Some(Arc::from("🐛")));

    // Col 2: 'e' with combining acute accent
    grid.row_mut(0).unwrap().set(2, Cell::new('e'));
    grid.storage
        .extras
        .get_or_create(CellCoord::new(0, 2))
        .add_combining('\u{0301}');

    // Scroll 3 lines so row 0 enters ring buffer scrollback.
    grid.scroll_up(3);

    let ring_sb = grid.storage.ring_buffer_scrollback();
    assert!(
        ring_sb >= 3,
        "expected at least 3 ring buffer scrollback lines"
    );

    // Row 0 was the first to scroll, so it's the oldest line (index 0).
    let line = grid
        .get_history_line(0)
        .expect("first scrollback line should exist");
    let text = line.as_str().expect("line should have text");

    // Complex char 🐛 should be in the text (not U+FFFD)
    assert!(
        text.contains('🐛'),
        "scrollback text should contain 🐛, got: {text:?}"
    );

    // 'A' should be in the text
    assert!(
        text.contains('A'),
        "scrollback text should contain 'A', got: {text:?}"
    );

    // Combining accent should be in the text
    assert!(
        text.contains('\u{0301}'),
        "scrollback text should contain combining accent U+0301, got: {text:?}"
    );

    // RGB fg for first char ('A') should be 0x01_FF0000 (red)
    let attrs_a = line.get_attr(0);
    assert_eq!(
        attrs_a.fg, 0x01_FF0000,
        "scrollback 'A' fg should be RGB red (0x01_FF0000), got: {:#010x}",
        attrs_a.fg
    );

    // attrs should not be default (RGB was preserved)
    assert_ne!(
        attrs_a,
        CellAttrs::DEFAULT,
        "scrollback 'A' attrs should not be default"
    );
}

/// Erase scrollback when ring buffer is exactly at capacity.
#[test]
fn erase_scrollback_at_ring_buffer_capacity() {
    // 4 visible rows, max_scrollback=6 → capacity=10 rows
    let mut grid = Grid::with_scrollback(4, 10, 6);

    // Fill visible rows with identifiable content
    for row in 0..4u16 {
        grid.set_cursor(row, 0);
        grid.write_char((b'A' + row as u8) as char);
    }

    // Scroll enough to fill ring buffer to capacity (6 scrollback lines)
    for _ in 0..6 {
        grid.set_cursor(3, 0);
        grid.line_feed();
    }
    assert_eq!(
        grid.scrollback_lines(),
        6,
        "ring buffer should be at max scrollback"
    );

    // Scroll a few more to exercise reuse phase (ring_head advances)
    for i in 0..4u16 {
        grid.set_cursor(3, 0);
        grid.write_char((b'W' + i as u8) as char);
        grid.line_feed();
    }
    assert_eq!(
        grid.scrollback_lines(),
        6,
        "scrollback should stay at max after reuse"
    );
    assert!(
        grid.storage.ring_head != 0,
        "ring_head should have advanced"
    );

    // Snapshot live rows before erase
    let live_rows: Vec<String> = (0..grid.rows())
        .map(|row| grid.row(row).unwrap().to_string())
        .collect();

    // The critical operation: erase scrollback with non-zero ring_head
    grid.erase_scrollback();

    // Invariants must hold
    assert_eq!(grid.scrollback_lines(), 0);
    assert_eq!(grid.display_offset(), 0);
    assert_eq!(grid.total_lines(), grid.rows() as usize);
    assert_eq!(grid.storage.ring_head, 0, "ring_head should reset to 0");

    // Live rows must be preserved identically
    for (row_idx, expected) in live_rows.iter().enumerate() {
        assert_eq!(
            grid.row(row_idx as u16).unwrap().to_string(),
            *expected,
            "live row {row_idx} must be preserved after erase_scrollback at capacity"
        );
    }
    grid.assert_invariants();
}

/// scroll_up at the exact growth-to-reuse transition boundary.
#[test]
fn scroll_up_spans_growth_and_reuse_phases() {
    // 3 visible rows, max_scrollback=2 → capacity=5
    let mut grid = Grid::with_scrollback(3, 10, 2);

    // Fill visible rows
    for row in 0..3u16 {
        grid.set_cursor(row, 0);
        grid.write_char((b'A' + row as u8) as char);
    }

    // Scroll up by 1 to enter growth phase (total_lines: 3→4)
    grid.scroll_up(1);
    assert_eq!(grid.total_lines(), 4);
    assert_eq!(grid.scrollback_lines(), 1);

    // Scroll up by 3: first 1 goes to growth (4→5, at capacity),
    // remaining 2 go to reuse phase (oldest rows recycled).
    grid.scroll_up(3);
    assert_eq!(grid.total_lines(), 5, "should be at capacity");
    assert_eq!(grid.scrollback_lines(), 2, "max_scrollback=2");

    // Verify ring buffer is valid
    grid.assert_invariants();

    // Bottom visible row should be empty (newly scrolled in)
    assert!(
        grid.row(2).unwrap().is_empty(),
        "bottom row should be blank after scroll"
    );
}

/// Erase scrollback with exactly 1 line of scrollback (minimal edge case).
#[test]
fn erase_scrollback_single_line() {
    let mut grid = Grid::with_scrollback(4, 10, 10);

    // Write identifiable content
    for row in 0..4u16 {
        grid.set_cursor(row, 0);
        grid.write_char((b'P' + row as u8) as char);
    }

    // Generate exactly 1 scrollback line
    grid.set_cursor(3, 0);
    grid.line_feed();
    assert_eq!(grid.scrollback_lines(), 1);

    let live_rows: Vec<String> = (0..grid.rows())
        .map(|row| grid.row(row).unwrap().to_string())
        .collect();

    grid.erase_scrollback();

    assert_eq!(grid.scrollback_lines(), 0);
    assert_eq!(grid.total_lines(), grid.rows() as usize);
    for (row_idx, expected) in live_rows.iter().enumerate() {
        assert_eq!(
            grid.row(row_idx as u16).unwrap().to_string(),
            *expected,
            "live row {row_idx} preserved after erasing single scrollback line"
        );
    }
    grid.assert_invariants();
}

/// scroll_up with n larger than total capacity still produces valid state.
#[test]
fn scroll_up_n_exceeds_capacity() {
    let mut grid = Grid::with_scrollback(3, 5, 4);

    for row in 0..3u16 {
        grid.set_cursor(row, 0);
        grid.write_char((b'X' + row as u8) as char);
    }

    // Scroll by a huge amount (well beyond capacity of 7)
    grid.scroll_up(100);

    // State must be valid
    assert!(
        grid.total_lines() <= 3 + 4,
        "total_lines bounded by capacity"
    );
    assert!(grid.scrollback_lines() <= 4, "scrollback bounded by max");
    grid.assert_invariants();

    // All visible rows should be blank (100 blank rows scrolled in)
    for row in 0..3u16 {
        assert!(
            grid.row(row).unwrap().is_empty(),
            "row {row} should be blank after scrolling 100 lines in a 3-row grid"
        );
    }
}

/// Regression test for #7783: extras (hyperlinks, RGB colors) on visible rows
/// must be preserved when those rows are pushed to scrollback during terminal
/// height decrease.
///
/// Before the fix, `adjust_row_count` used `u16::MAX` as `row_idx` for
/// `extract_row_extras`, causing HashMap-keyed extras (hyperlinks, combining
/// marks) to be orphaned because `CellCoord::new(u16::MAX, col)` never matches
/// any entry stored at the actual visible row index.
#[test]
fn height_decrease_preserves_extras_on_pushed_rows() {
    use std::sync::Arc;

    // 6 visible rows, 10 columns, tiered scrollback so adjust_row_count
    // pushes excess rows to the lazy buffer instead of dropping them.
    let scrollback = Scrollback::new(100, 1000, 10_000_000);
    let mut grid = Grid::with_tiered_scrollback(6, 10, 2, scrollback);

    // Write "Hello" on row 4 with a hyperlink on columns 0-4.
    let url: Arc<str> = Arc::from("https://example.com/7783");
    grid.set_cursor(4, 0);
    for c in "Hello".chars() {
        grid.write_char(c);
    }
    for col in 0..5u16 {
        grid.extras_mut()
            .get_or_create(CellCoord::new(4, col))
            .set_hyperlink(Some(url.clone()));
    }

    // Verify the hyperlink is present before resize.
    assert!(
        grid.extras().row_has_hyperlinks(4),
        "row 4 should have hyperlinks before resize"
    );

    // Shrink height from 6 to 3. Row 4 (visible) should be pushed to
    // scrollback via adjust_row_count's from_back path.
    grid.resize_no_reflow(3, 10);

    // The pushed rows should now be in scrollback (lazy buffer or tiered).
    let sb_lines = grid.scrollback_lines();
    assert!(
        sb_lines > 0,
        "should have scrollback lines after height decrease"
    );

    // Search the scrollback for the line with "Hello" and check its hyperlinks.
    let mut found_hyperlink = false;
    for rev_idx in 0..sb_lines {
        if let Some(line) = grid.history_line_rev(rev_idx) {
            let text = line.as_str().unwrap_or("");
            if text.contains("Hello") {
                if let Some(spans) = line.hyperlinks() {
                    if !spans.is_empty() {
                        assert_eq!(
                            &*spans[0].url, "https://example.com/7783",
                            "hyperlink URL should be preserved"
                        );
                        found_hyperlink = true;
                        break;
                    }
                }
            }
        }
    }
    assert!(
        found_hyperlink,
        "hyperlink on row 4 should survive height decrease into scrollback (#7783)"
    );
    grid.assert_invariants();
}
