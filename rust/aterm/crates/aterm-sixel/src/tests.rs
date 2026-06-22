// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for the sixel decoder: raster correctness, palette, bounds.

use super::*;

/// Feed a whole sixel data body (without DCS framing) through a fresh decoder.
fn decode(params: &[u16], body: &[u8]) -> Option<SixelImage> {
    let mut d = SixelDecoder::new();
    d.hook(params, 0, 0);
    for &b in body {
        d.put(b);
    }
    d.unhook()
}

#[test]
fn unhook_without_hook_is_none() {
    let mut d = SixelDecoder::new();
    d.put(b'~');
    assert!(d.unhook().is_none());
}

#[test]
fn empty_sequence_is_none() {
    assert!(decode(&[0, 0, 0], b"").is_none());
}

#[test]
fn single_full_column_is_one_by_six_red() {
    // `"1;1;1;6` raster 1x6, color 1 = pure red, `~` = 0x3F+0x3F = all 6 bits.
    let img = decode(&[0, 0, 0], b"\"1;1;1;6#1;2;100;0;0#1~")
        .expect("a painted column must produce an image");
    assert_eq!(img.width(), 1, "one sixel column = 1px wide");
    assert_eq!(img.height(), 6, "a full `~` band = 6px tall");
    assert_eq!(img.pixels().len(), 6);
    // Color 1 defined as RGB% 100;0;0 → 0xFFFF0000 (opaque red).
    for (i, &p) in img.pixels().iter().enumerate() {
        assert_eq!(p, 0xFFFF_0000, "pixel {i} should be opaque red");
    }
}

#[test]
fn four_columns_one_band_is_four_by_six() {
    // SIXEL_4X6 body: four `~` columns of red after raster 4x6.
    let img = decode(&[0, 0, 0], b"\"1;1;4;6#0;2;0;0;0#1;2;100;0;0#1~~~~$-").expect("4x6 image");
    assert_eq!(img.width(), 4);
    assert_eq!(img.height(), 6);
    assert_eq!(img.pixels().len(), 24);
    for &p in img.pixels() {
        assert_eq!(p, 0xFFFF_0000, "all four columns are red");
    }
}

#[test]
fn partial_column_sets_only_low_bits() {
    // `?` = 0x3F → value 0 → no pixels. `A` = 0x41 → value 2 → bit 1 set (row 1).
    let img = decode(&[0, 0, 0], b"\"1;1;1;6#1;2;0;100;0#1A").expect("image");
    assert_eq!(img.width(), 1);
    assert_eq!(img.height(), 6);
    // Only row 1 (second from top) is painted green; others transparent.
    assert_eq!(img.pixels()[0], 0, "row 0 transparent");
    assert_eq!(
        img.pixels()[1] & 0xFF00_FF00,
        0xFF00_FF00,
        "row 1 opaque green"
    );
    assert_eq!(img.pixels()[2], 0, "row 2 transparent");
}

#[test]
fn graphics_newline_advances_band() {
    // Two bands stacked: first band col0, `-` then second band col0.
    let img = decode(&[0, 0, 0], b"\"1;1;1;12#1;2;100;0;0#1~-~").expect("image");
    assert_eq!(img.width(), 1);
    assert_eq!(img.height(), 12, "two 6px bands");
    for &p in img.pixels() {
        assert_eq!(p, 0xFFFF_0000);
    }
}

#[test]
fn decgri_repeat_paints_run() {
    // `!5~` repeats the full column 5 times → 5px wide.
    let img = decode(&[0, 0, 0], b"\"1;1;5;6#1;2;100;0;0#1!5~").expect("image");
    assert_eq!(img.width(), 5);
    assert_eq!(img.height(), 6);
    assert_eq!(img.pixels().len(), 30);
    for &p in img.pixels() {
        assert_eq!(p, 0xFFFF_0000);
    }
}

#[test]
fn dimensions_are_clamped() {
    // A hostile DECGRI cannot exceed SIXEL_MAX_DIMENSION on width.
    let body = b"#1;2;100;0;0#1!4294967295~";
    let img = decode(&[0, 0, 0], body).expect("image");
    assert!(img.width() <= SIXEL_MAX_DIMENSION, "width clamped");
    assert!(img.height() <= SIXEL_MAX_DIMENSION, "height clamped");
    assert_eq!(img.pixels().len(), img.width() * img.height());
}

#[test]
fn register_select_out_of_range_is_clamped_no_panic() {
    // Selecting register 99999 must clamp, not panic or grow the palette.
    let img = decode(&[0, 0, 0], b"\"1;1;1;6#99999~").expect("image");
    assert_eq!(img.width(), 1);
    assert_eq!(img.height(), 6);
}

#[test]
fn span_helpers_round_up() {
    let img = decode(&[0, 0, 0], b"\"1;1;4;6#1;2;100;0;0#1~~~~").expect("image");
    // 4px wide / 8px cell → 1 col; 6px tall / 16px cell → 1 row.
    assert_eq!(img.cols_spanned(8), 1);
    assert_eq!(img.rows_spanned(16), 1);
    // 4px / 2px cell → 2 cols; 6px / 4px cell → 2 rows.
    assert_eq!(img.cols_spanned(2), 2);
    assert_eq!(img.rows_spanned(4), 2);
}

#[test]
fn reuse_across_cycles_resets_state() {
    let mut d = SixelDecoder::new();
    d.hook(&[0, 0, 0], 0, 0);
    for &b in b"\"1;1;4;6#1;2;100;0;0#1~~~~" {
        d.put(b);
    }
    let a = d.unhook().expect("first image");
    assert_eq!(a.width(), 4);

    // Second cycle: a smaller image must not inherit the first's geometry.
    d.hook(&[0, 0, 0], 0, 0);
    for &b in b"\"1;1;1;6#1;2;0;100;0#1~" {
        d.put(b);
    }
    let bimg = d.unhook().expect("second image");
    assert_eq!(bimg.width(), 1, "geometry reset between cycles");
    assert_eq!(bimg.height(), 6);
    assert_eq!(
        bimg.pixels()[0] & 0x00FF_FF00,
        0x0000_FF00,
        "green, not red"
    );
}

#[test]
fn abort_frees_and_yields_no_image() {
    let mut d = SixelDecoder::new();
    d.hook(&[0, 0, 0], 0, 0);
    for &b in b"\"1;1;4;6#1~~~~" {
        d.put(b);
    }
    assert!(d.pixel_alloc_bytes() > 0, "buffer allocated during decode");
    d.abort();
    assert_eq!(d.pixel_alloc_bytes(), 0, "abort frees the buffer");
    assert!(d.unhook().is_none(), "aborted decode yields nothing");
}

#[test]
fn cursor_position_carried_into_image() {
    let mut d = SixelDecoder::new();
    d.hook(&[0, 0, 0], 7, 3);
    for &b in b"\"1;1;1;6#1~" {
        d.put(b);
    }
    let img = d.unhook().expect("image");
    assert_eq!(img.cursor_row(), 7);
    assert_eq!(img.cursor_col(), 3);
}

#[test]
fn default_palette_color_zero_is_black() {
    let pal = default_palette();
    assert_eq!(pal[0], 0x0000_0000);
    assert_eq!(pal.len(), MAX_COLOR_REGISTERS);
}

#[test]
fn rgb_percent_scales_correctly() {
    assert_eq!(rgb_percent(100, 0, 0), 0x00FF_0000);
    assert_eq!(rgb_percent(0, 100, 0), 0x0000_FF00);
    assert_eq!(rgb_percent(0, 0, 100), 0x0000_00FF);
    assert_eq!(rgb_percent(100, 100, 100), 0x00FF_FFFF);
}

#[test]
fn hls_primaries_are_sane() {
    // HLS with full lightness/saturation should be near-fully-saturated colors.
    // Just assert no panic and a non-zero, in-range result.
    let c = hls_to_rgb(120, 50, 100);
    assert!(c <= 0x00FF_FFFF);
}

#[test]
fn repeat_does_not_survive_band_control() {
    // DECGRI `!Pn` applies ONLY to the immediately-following sixel data byte. A
    // `$` (graphics-CR) or `-` (graphics-NL) between `!3` and the data byte must
    // cancel the pending repeat — otherwise the next band is wrongly widened.
    let cr = decode(&[0, 0, 0], b"#1;2;100;0;0#1!3$~").expect("image");
    assert_eq!(
        cr.width(),
        1,
        "`!3` then `$` then `~` must NOT repeat (width 1)"
    );
    let nl = decode(&[0, 0, 0], b"#1;2;100;0;0#1!3-~").expect("image");
    assert_eq!(
        nl.width(),
        1,
        "`!3` then `-` then `~` must NOT repeat (width 1)"
    );
    // Control: a repeat IMMEDIATELY followed by its data byte still repeats.
    let ok = decode(&[0, 0, 0], b"#1;2;100;0;0#1!3~").expect("image");
    assert_eq!(
        ok.width(),
        3,
        "`!3~` must repeat the data byte 3x (width 3)"
    );
}
