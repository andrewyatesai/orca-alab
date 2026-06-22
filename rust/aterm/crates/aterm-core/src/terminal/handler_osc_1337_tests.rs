// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Tests for the OSC 1337 `File=` inline-image handler.
//!
//! These exercise PARSING + PLACEMENT only — the engine never decodes pixels
//! (that is the renderer's job), so the payload here is arbitrary bytes with a
//! PNG magic prefix. Footprint sizing uses cell-unit dimensions (`width=N`) so
//! the placement is deterministic without any real image.

use super::super::Terminal;

/// A minimal "PNG" prefix so [`detect_format`](super::detect_format) classifies
/// the payload as PNG. The bytes after it are never decoded by the engine.
const PNG_MAGIC: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// Build an OSC 1337 `File=` sequence: `ESC ] 1337 ; File=<args> : <b64> ST`.
fn osc_1337_file(args: &str, payload: &[u8]) -> Vec<u8> {
    let b64 = aterm_codec::base64::encode(payload);
    let mut out = Vec::new();
    out.extend_from_slice(b"\x1b]1337;File=");
    out.extend_from_slice(args.as_bytes());
    out.push(b':');
    out.extend_from_slice(b64.as_bytes());
    out.extend_from_slice(b"\x1b\\"); // ST
    out
}

/// Count cells in the grid carrying an inline-image ref.
fn image_cell_count(term: &Terminal) -> usize {
    let grid = term.grid();
    let mut n = 0;
    for r in 0..grid.rows() {
        for c in 0..grid.cols() {
            if grid.cell_extra(r, c).is_some_and(|e| e.image().is_some()) {
                n += 1;
            }
        }
    }
    n
}

#[test]
fn file_with_cell_dimensions_covers_exact_footprint() {
    let mut term = Terminal::new(10, 20);
    let payload = [PNG_MAGIC, &[0u8; 16][..]].concat();
    term.process(&osc_1337_file("inline=1;width=4;height=3", &payload));
    // 4 cols × 3 rows = 12 covered cells.
    assert_eq!(image_cell_count(&term), 12);
    // The footprint is anchored at the top-left (column 0).
    let grid = term.grid();
    let extra = grid.cell_extra(0, 0).expect("top-left covered");
    let image = extra.image().expect("image ref at (0,0)");
    assert_eq!(image.cell_row, 0);
    assert_eq!(image.cell_col, 0);
    assert_eq!(image.image.cols, 4);
    assert_eq!(image.image.rows, 3);
}

#[test]
fn tile_coordinates_increase_across_the_footprint() {
    let mut term = Terminal::new(10, 20);
    let payload = [PNG_MAGIC, &[0u8; 16][..]].concat();
    term.process(&osc_1337_file("inline=1;width=3;height=2", &payload));
    let grid = term.grid();
    // Cell (row=1, col=2) is the bottom-right tile.
    let extra = grid.cell_extra(1, 2).expect("bottom-right covered");
    let image = extra.image().expect("image ref");
    assert_eq!(image.cell_row, 1);
    assert_eq!(image.cell_col, 2);
    // All tiles share the SAME underlying payload (one Arc).
    let a = grid.cell_extra(0, 0).unwrap().image().unwrap();
    let b = grid.cell_extra(1, 2).unwrap().image().unwrap();
    assert!(std::sync::Arc::ptr_eq(&a.image, &b.image));
}

#[test]
fn inline_zero_draws_nothing() {
    let mut term = Terminal::new(10, 20);
    let payload = [PNG_MAGIC, &[0u8; 16][..]].concat();
    // inline=0 means "download to disk" — no on-screen effect.
    term.process(&osc_1337_file("inline=0;width=4;height=3", &payload));
    assert_eq!(image_cell_count(&term), 0);
}

#[test]
fn missing_inline_draws_nothing() {
    let mut term = Terminal::new(10, 20);
    let payload = [PNG_MAGIC, &[0u8; 16][..]].concat();
    // No inline= argument at all — default is "not inline".
    term.process(&osc_1337_file("width=4;height=3", &payload));
    assert_eq!(image_cell_count(&term), 0);
}

#[test]
fn footprint_clamps_to_grid_width() {
    let mut term = Terminal::new(10, 8);
    let payload = [PNG_MAGIC, &[0u8; 16][..]].concat();
    // Request 100 cols on an 8-col grid → clamps to 8.
    term.process(&osc_1337_file("inline=1;width=100;height=1", &payload));
    let grid = term.grid();
    let row0: usize = (0..grid.cols())
        .filter(|&c| grid.cell_extra(0, c).is_some_and(|e| e.image().is_some()))
        .count();
    assert_eq!(row0, 8, "width clamped to the 8-col grid");
}

#[test]
fn malformed_no_payload_separator_is_ignored() {
    let mut term = Terminal::new(10, 20);
    // No ':' → no payload → nothing drawn, no panic.
    term.process(b"\x1b]1337;File=inline=1;width=4;height=3\x1b\\");
    assert_eq!(image_cell_count(&term), 0);
}

#[test]
fn malformed_bad_base64_is_ignored() {
    let mut term = Terminal::new(10, 20);
    // '!' is not a valid base64 char → decode fails → nothing drawn.
    term.process(b"\x1b]1337;File=inline=1;width=4;height=3:!!!!\x1b\\");
    assert_eq!(image_cell_count(&term), 0);
}

#[test]
fn empty_payload_is_ignored() {
    let mut term = Terminal::new(10, 20);
    term.process(b"\x1b]1337;File=inline=1;width=4;height=3:\x1b\\");
    assert_eq!(image_cell_count(&term), 0);
}

#[test]
fn non_file_subcommand_is_ignored_gracefully() {
    let mut term = Terminal::new(10, 20);
    // A different OSC 1337 sub-command must not be treated as an image.
    term.process(b"\x1b]1337;SetUserVar=foo=YmFy\x1b\\");
    assert_eq!(image_cell_count(&term), 0);
}

#[test]
fn unknown_format_still_places_cells_but_renders_nothing() {
    // A non-PNG payload is stored (so the cursor advances consistently) and
    // classified Unknown; the renderer draws nothing for it.
    let mut term = Terminal::new(10, 20);
    let payload = b"GIF89a not really a gif".to_vec();
    term.process(&osc_1337_file("inline=1;width=2;height=2", &payload));
    let grid = term.grid();
    let image = grid.cell_extra(0, 0).unwrap().image().unwrap();
    assert_eq!(image.image.format, aterm_grid::ImageFormat::Unknown);
    assert_eq!(image_cell_count(&term), 4);
}

#[test]
fn cursor_advances_below_the_image() {
    let mut term = Terminal::new(10, 20);
    let payload = [PNG_MAGIC, &[0u8; 16][..]].concat();
    term.process(&osc_1337_file("inline=1;width=4;height=3", &payload));
    // After a 3-row image placed at row 0, the cursor sits below it (row 3),
    // column 0, so following text does not overprint the image.
    assert_eq!(term.grid().cursor_row(), 3);
    assert_eq!(term.grid().cursor_col(), 0);
}

#[test]
fn whitespace_in_payload_is_tolerated() {
    let mut term = Terminal::new(10, 20);
    let payload = [PNG_MAGIC, &[0u8; 16][..]].concat();
    let b64 = aterm_codec::base64::encode(&payload);
    // Insert newlines mid-payload (line-wrapped transfer) — must still decode.
    let mut seq = Vec::new();
    seq.extend_from_slice(b"\x1b]1337;File=inline=1;width=2;height=1:");
    for (i, ch) in b64.bytes().enumerate() {
        if i > 0 && i % 4 == 0 {
            seq.push(b'\n');
        }
        seq.push(ch);
    }
    seq.extend_from_slice(b"\x1b\\");
    term.process(&seq);
    assert_eq!(image_cell_count(&term), 2);
}

#[test]
fn large_inline_image_payload_survives_old_64k_cap() {
    // Regression for the OSC capacity bump: before raising the parser's
    // MAX_OSC_DATA from 64 KiB to 8 MiB, the base64 payload of any non-trivial
    // inline image was truncated at the parser, so the handler decoded garbage
    // (or nothing) and the real image never reached the grid.
    //
    // Here we send a ~512 KiB image whose base64 is ~683 KiB — over 10x the old
    // 64 KiB cap — and assert the handler decodes the FULL payload intact.
    let mut term = Terminal::new(10, 20);

    // Deterministic, non-trivial body so a truncated decode could not coincide.
    let body: Vec<u8> = (0..512 * 1024)
        .map(|i| (i as u8).wrapping_mul(31))
        .collect();
    let payload = [PNG_MAGIC, &body[..]].concat();

    // base64 of ~512 KiB is ~683 KiB, far above the old 64 KiB parser cap.
    let b64 = aterm_codec::base64::encode(&payload);
    assert!(
        b64.len() > 64 * 1024,
        "test must exceed the old 64 KiB cap (b64 len = {})",
        b64.len()
    );

    term.process(&osc_1337_file("inline=1;width=4;height=3", &payload));

    // The image must have been placed (4×3 footprint).
    assert_eq!(image_cell_count(&term), 12, "large image must be placed");

    // Crucially, the stored bytes must equal the ORIGINAL payload exactly —
    // proving the parser delivered the whole base64 body without truncation.
    let grid = term.grid();
    let image = grid
        .cell_extra(0, 0)
        .and_then(|e| e.image())
        .expect("image ref at (0,0)");
    assert_eq!(
        image.image.bytes.len(),
        payload.len(),
        "decoded image must be the full payload, not a truncated prefix"
    );
    assert_eq!(
        &image.image.bytes[..],
        &payload[..],
        "decoded image bytes must match the original payload exactly"
    );
    assert_eq!(image.image.format, aterm_grid::ImageFormat::Png);
}

#[test]
fn does_not_panic_on_truncated_sequences() {
    // A battery of malformed/truncated File= sequences must never panic.
    let cases: &[&[u8]] = &[
        b"\x1b]1337;File=\x1b\\",
        b"\x1b]1337;File=:\x1b\\",
        b"\x1b]1337;File=inline=1\x1b\\",
        b"\x1b]1337;File=width=:abc\x1b\\",
        b"\x1b]1337;\x1b\\",
        b"\x1b]1337\x1b\\",
        b"\x1b]1337;File=inline=1;width=999999999;height=999999999:QQ==\x1b\\",
    ];
    for case in cases {
        let mut term = Terminal::new(5, 10);
        term.process(case);
    }
}
