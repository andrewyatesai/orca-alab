// Copyright 2026 The aterm Authors
// Author: The aterm Authors
// SPDX-License-Identifier: Apache-2.0

//! Edge-case verification for `advance_grapheme_unit` (#5951 Prover verification).
//!
//! Migrated from aterm-core as part of #6556 Batch 2.

use super::super::*;

/// Unit test for `advance_grapheme_unit` with orphan ZWJ mid-text.
#[test]
fn advance_grapheme_unit_orphan_zwj_mid_text() {
    use crate::grid::scroll_materialize::advance_grapheme_unit;

    let text = "A\u{200D}B";

    let mut byte_idx = 0;
    let chars_consumed = advance_grapheme_unit(text, &mut byte_idx);
    assert_eq!(chars_consumed, 3, "orphan ZWJ should consume A + ZWJ + B");
    assert_eq!(byte_idx, text.len(), "should consume entire string");
    assert_eq!(&text[..byte_idx], "A\u{200D}B");
}

/// Unit test for consecutive combining marks.
#[test]
fn advance_grapheme_unit_consecutive_combining_marks() {
    use crate::grid::scroll_materialize::advance_grapheme_unit;

    let text = "e\u{0301}\u{0302}";

    let mut byte_idx = 0;
    let chars_consumed = advance_grapheme_unit(text, &mut byte_idx);
    assert_eq!(
        chars_consumed, 3,
        "consecutive combining marks should all join base char"
    );
    assert_eq!(byte_idx, text.len());
}

/// Variation selector after emoji: char + VS16 should be one grapheme unit.
#[test]
fn advance_grapheme_unit_variation_selector() {
    use crate::grid::scroll_materialize::advance_grapheme_unit;

    let text = "\u{2764}\u{FE0F}";

    let mut byte_idx = 0;
    let chars_consumed = advance_grapheme_unit(text, &mut byte_idx);
    assert_eq!(
        chars_consumed, 2,
        "variation selector should join base char"
    );
    assert_eq!(byte_idx, text.len());
}

/// Single ASCII char: advance_grapheme_unit should consume exactly 1 char.
#[test]
fn advance_grapheme_unit_single_ascii() {
    use crate::grid::scroll_materialize::advance_grapheme_unit;

    let text = "ABC";
    let mut byte_idx = 0;
    let chars_consumed = advance_grapheme_unit(text, &mut byte_idx);
    assert_eq!(
        chars_consumed, 1,
        "single ASCII should consume exactly 1 char"
    );
    assert_eq!(byte_idx, 1, "should advance exactly 1 byte for ASCII");
}

/// ZWJ emoji sequence: 👨‍👩‍👧 should be consumed as one grapheme unit.
#[test]
fn advance_grapheme_unit_zwj_emoji_sequence() {
    use crate::grid::scroll_materialize::advance_grapheme_unit;

    let text = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}";

    let mut byte_idx = 0;
    let chars_consumed = advance_grapheme_unit(text, &mut byte_idx);
    assert_eq!(
        chars_consumed, 5,
        "ZWJ family emoji should consume all 5 codepoints"
    );
    assert_eq!(byte_idx, text.len());
}

/// Materialize round-trip for consecutive combining marks.
#[test]
fn materialize_consecutive_combining_marks() {
    use crate::grid::scroll_materialize::materialize_from_line;

    let text = "e\u{0301}\u{0302}X";
    let attr_count = text.chars().count();
    let attrs: Rle<CellAttrs> = std::iter::repeat_n(CellAttrs::DEFAULT, attr_count).collect();
    let line = Line::with_attrs(text, attrs);

    let row = materialize_from_line(&line, 10);

    let extra = row.get_extra(0);
    assert!(
        extra.is_some(),
        "col 0 should have extras for double combining"
    );
    assert_eq!(
        extra.unwrap().complex_char().map(|s| &**s),
        Some("e\u{0301}\u{0302}"),
        "both combining marks should be preserved"
    );

    assert_eq!(row.cells[1].char(), 'X', "col 1 should be 'X'");
}

/// Materialize round-trip for orphan ZWJ between visible chars.
#[test]
fn materialize_orphan_zwj_between_visible_chars() {
    use crate::grid::scroll_materialize::materialize_from_line;

    let text = "A\u{200D}BX";
    let attr_count = text.chars().count();
    let attrs: Rle<CellAttrs> = std::iter::repeat_n(CellAttrs::DEFAULT, attr_count).collect();
    let line = Line::with_attrs(text, attrs);

    let row = materialize_from_line(&line, 10);

    let extra = row.get_extra(0);
    assert!(
        extra.is_some(),
        "col 0 should have extras for ZWJ-joined unit"
    );
    assert_eq!(
        extra.unwrap().complex_char().map(|s| &**s),
        Some("A\u{200D}B"),
        "ZWJ-joined unit should be preserved as complex_char"
    );

    assert_eq!(row.cells[1].char(), 'X', "col 1 should be 'X'");
}
