// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors
//
// cells_mut_with_fixup behavioral tests.

use super::super::*;
use super::make_row;

#[test]
fn cells_mut_with_fixup_clears_orphaned_continuation() {
    use crate::{CellFlags, PackedColor};

    let (_pages, mut row) = make_row(10);
    let fg = PackedColor::DEFAULT_FG;
    let bg = PackedColor::DEFAULT_BG;

    // Place a wide char at cols 0-1
    row.write_wide_char(0, '\u{4E2D}', fg, bg, CellFlags::empty());
    assert!(row.get(0).unwrap().is_wide());
    assert!(row.get(1).unwrap().is_wide_continuation());

    // Request mutable access starting at the continuation. The helper should
    // clear the orphaned leading half before exposing the slice.
    {
        let target = row
            .cells_mut_with_fixup(1, 3)
            .expect("range inside row should produce a slice");
        assert_eq!(target.len(), 3);
    }
    assert_eq!(row.get(0).unwrap().char(), ' ');
    assert!(!row.get(0).unwrap().is_wide());
}

#[test]
fn cells_mut_with_fixup_clears_wide_continuation_past_range() {
    use crate::{CellFlags, PackedColor};

    let (_pages, mut row) = make_row(10);
    let fg = PackedColor::DEFAULT_FG;
    let bg = PackedColor::DEFAULT_BG;

    // Place a wide char at cols 4-5
    row.write_wide_char(4, '\u{4E2D}', fg, bg, CellFlags::empty());

    // The helper range covers col 4 (the WIDE cell) but not col 5
    // (continuation), so col 5 should be cleared as orphaned.
    {
        let target = row
            .cells_mut_with_fixup(3, 2)
            .expect("range inside row should produce a slice");
        assert_eq!(target.len(), 2);
    }
    assert_eq!(row.get(5).unwrap().char(), ' ');
    assert!(!row.get(5).unwrap().is_wide_continuation());
}

#[test]
fn cells_mut_with_fixup_is_noop_without_wide_chars() {
    let (_pages, mut row) = make_row(10);
    for i in 0..5 {
        row.write_char(i, 'A');
    }

    {
        let target = row
            .cells_mut_with_fixup(0, 5)
            .expect("range inside row should produce a slice");
        assert_eq!(target.len(), 5);
    }
    for i in 0..5 {
        assert_eq!(row.get(i).unwrap().char(), 'A');
    }
}

#[test]
fn cells_mut_with_fixup_allows_zero_count() {
    use crate::{CellFlags, PackedColor};

    let (_pages, mut row) = make_row(10);
    let fg = PackedColor::DEFAULT_FG;
    let bg = PackedColor::DEFAULT_BG;

    row.write_wide_char(0, '\u{4E2D}', fg, bg, CellFlags::empty());

    {
        let target = row
            .cells_mut_with_fixup(1, 0)
            .expect("zero-length range inside row should return an empty slice");
        assert!(target.is_empty());
    }
    assert!(row.get(0).unwrap().is_wide());
    assert!(row.get(1).unwrap().is_wide_continuation());
}

#[test]
fn cells_mut_with_fixup_returns_none_when_start_past_end() {
    use crate::{CellFlags, PackedColor};

    let (_pages, mut row) = make_row(10);
    let fg = PackedColor::DEFAULT_FG;
    let bg = PackedColor::DEFAULT_BG;

    row.write_wide_char(0, '\u{4E2D}', fg, bg, CellFlags::empty());

    assert!(
        row.cells_mut_with_fixup(11, 5).is_none(),
        "start past row should return None"
    );
    assert!(row.get(0).unwrap().is_wide());
    assert!(row.get(1).unwrap().is_wide_continuation());
}

#[test]
fn cells_mut_with_fixup_handles_multiple_wide_chars() {
    use crate::{CellFlags, PackedColor};

    let (_pages, mut row) = make_row(10);
    let fg = PackedColor::DEFAULT_FG;
    let bg = PackedColor::DEFAULT_BG;

    // Place wide chars at 0-1, 2-3, 4-5
    row.write_wide_char(0, '\u{4E00}', fg, bg, CellFlags::empty());
    row.write_wide_char(2, '\u{4E8C}', fg, bg, CellFlags::empty());
    row.write_wide_char(4, '\u{4E09}', fg, bg, CellFlags::empty());

    // The helper range covers cols 1-4 (starts on continuation of first,
    // includes WIDE of third).
    {
        let target = row
            .cells_mut_with_fixup(1, 4)
            .expect("range inside row should produce a slice");
        assert_eq!(target.len(), 4);
    }

    // Col 0 should be cleared (orphaned first half when continuation at 1 is overwritten)
    assert_eq!(row.get(0).unwrap().char(), ' ');
    assert!(!row.get(0).unwrap().is_wide());

    // Col 5 should be cleared (orphaned continuation when WIDE at 4 is overwritten)
    assert_eq!(row.get(5).unwrap().char(), ' ');
    assert!(!row.get(5).unwrap().is_wide_continuation());
}

#[test]
fn cells_mut_with_fixup_ignores_out_of_bounds_start_without_overflow() {
    let mut pages = PageStore::new();
    // SAFETY: Test-local `pages` outlives `row` for the full scope.
    let mut row = unsafe { Row::new(4, &mut pages) };

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        assert!(
            row.cells_mut_with_fixup(u16::MAX, 1).is_none(),
            "out-of-bounds start should return None"
        );
    }));

    assert!(
        result.is_ok(),
        "out-of-bounds start should return early without overflow panic"
    );
    assert_eq!(row.len(), 0);
}

/// Regression test for #7669: `fixup_wide_boundary` must clear an orphaned
/// WIDE_CONTINUATION at `left` when `left-1` is NOT a WIDE cell.
#[test]
fn fixup_wide_boundary_clears_orphaned_continuation_at_left() {
    use crate::{CellFlags, PackedColor};

    let (_pages, mut row) = make_row(10);
    let fg = PackedColor::DEFAULT_FG;
    let bg = PackedColor::DEFAULT_BG;

    // Place a wide char at cols 2-3 so col 3 is WIDE_CONTINUATION.
    row.write_wide_char(2, '\u{4E2D}', fg, bg, CellFlags::empty());
    assert!(row.get(2).unwrap().is_wide());
    assert!(row.get(3).unwrap().is_wide_continuation());

    // Overwrite col 2 (the WIDE half) with a normal char via get_mut,
    // leaving col 3 as an orphaned WIDE_CONTINUATION.
    *row.get_mut(2).unwrap() =
        crate::Cell::with_style_id('A', crate::StyleId::DEFAULT, CellFlags::empty());
    assert!(!row.get(2).unwrap().is_wide());
    assert!(row.get(3).unwrap().is_wide_continuation());

    // Call fixup_wide_boundary with left=3 (the orphaned continuation).
    // Before the fix for #7669, this was a no-op: the left boundary only
    // checked (prev_wide && !cur_cont), not (cur_cont && !prev_wide).
    row.fixup_wide_boundary(3, 6, 10);

    // The orphaned WIDE_CONTINUATION at col 3 should now be cleared.
    assert!(
        !row.get(3).unwrap().is_wide_continuation(),
        "orphaned WIDE_CONTINUATION at left boundary should be cleared"
    );
    assert_eq!(
        row.get(3).unwrap().char(),
        ' ',
        "cleared cell should be empty (space)"
    );
}
