// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors
//
// Cursor-shape regression for the CPU renderer: DECSCUSR bytes (CSI Ps SP q)
// fed through a real Terminal drive the rendered cursor SHAPE end-to-end
// (bytes -> engine -> pixels). CPU fills are exact, so each shape is asserted
// as an exact pixel pattern against `Theme::cursor`:
//   - block: the whole cursor cell, glyph "cut out" in the cell bg;
//   - underline: ONLY the bottom strip (max(2, cell_h/8) px), glyph normal;
//   - bar: ONLY the left strip (max(2, cell_w/8) px), glyph normal;
//   - hollow block (frontend override): outline yes, center no;
//   - blink phase off (Blinking* styles only) and DECTCEM-hidden: no cursor.

use aterm_core::terminal::{CursorStyle, Terminal};
use aterm_render::{Frame, Renderer, Theme};

const CURSOR: u32 = 0x0050_FA7B; // Theme::default().cursor
const FG: u32 = 0x00D0_D0D0; // Theme::default().fg

fn renderer() -> Option<Renderer> {
    Renderer::from_system(18.0, Theme::default())
}

/// All (x, y) positions whose pixel is exactly the cursor colour.
fn cursor_positions(f: &Frame) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for y in 0..f.height {
        for x in 0..f.width {
            if f.pixels[y * f.width + x] == CURSOR {
                out.push((x, y));
            }
        }
    }
    out
}

/// A blank 2x4 terminal (cursor at (0,0)) with the given bytes processed.
fn term_with(bytes: &[u8]) -> Terminal {
    let mut t = Terminal::new(2, 4);
    t.process(bytes);
    t
}

#[test]
fn steady_block_fills_whole_cursor_cell() {
    let Some(mut r) = renderer() else {
        eprintln!("SKIP: no system monospace font");
        return;
    };
    let (cw, ch) = r.cell_size();
    let term = term_with(b"\x1b[2 q"); // DECSCUSR 2 = steady block
    let f = r.render(&term, 2, 4);
    let pos = cursor_positions(&f);
    // Every pixel of cell (0,0) is the cursor colour and nothing else is.
    assert_eq!(pos.len(), cw * ch, "block cursor should fill the whole cell");
    assert!(pos.iter().all(|&(x, y)| x < cw && y < ch), "cursor pixels outside the cursor cell");
}

#[test]
fn underline_cursor_fills_only_bottom_strip() {
    let Some(mut r) = renderer() else {
        eprintln!("SKIP: no system monospace font");
        return;
    };
    let (cw, ch) = r.cell_size();
    let term = term_with(b"\x1b[4 q"); // DECSCUSR 4 = steady underline
    let f = r.render(&term, 2, 4);
    let t = (ch / 8).max(2);
    let pos = cursor_positions(&f);
    assert_eq!(pos.len(), cw * t, "underline cursor should fill exactly the bottom strip");
    assert!(
        pos.iter().all(|&(x, y)| x < cw && y >= ch - t && y < ch),
        "underline cursor pixels outside the bottom strip of the cursor cell"
    );
}

#[test]
fn bar_cursor_fills_only_left_strip() {
    let Some(mut r) = renderer() else {
        eprintln!("SKIP: no system monospace font");
        return;
    };
    let (cw, ch) = r.cell_size();
    let term = term_with(b"\x1b[6 q"); // DECSCUSR 6 = steady bar
    let f = r.render(&term, 2, 4);
    let t = (cw / 8).max(2);
    let pos = cursor_positions(&f);
    assert_eq!(pos.len(), t * ch, "bar cursor should fill exactly the left strip");
    assert!(
        pos.iter().all(|&(x, y)| x < t && y < ch),
        "bar cursor pixels outside the left strip of the cursor cell"
    );
}

#[test]
fn hollow_block_draws_outline_but_not_center() {
    let Some(mut r) = renderer() else {
        eprintln!("SKIP: no system monospace font");
        return;
    };
    let (cw, ch) = r.cell_size();
    // HollowBlock is not a DECSCUSR parameter: the windowed frontend forces it
    // for unfocused windows via the renderer's style override.
    r.set_cursor_style_override(Some(CursorStyle::HollowBlock));
    let term = term_with(b"");
    let f = r.render(&term, 2, 4);
    let t = (ch / 16).max(1);
    let border = 2 * cw * t + 2 * t * (ch - 2 * t);
    let pos = cursor_positions(&f);
    assert_eq!(pos.len(), border, "hollow block should paint exactly the outline");
    // The four edges are cursor-coloured; the cell center is not.
    for &(x, y) in &[(0, 0), (cw - 1, 0), (0, ch - 1), (cw - 1, ch - 1)] {
        assert_eq!(f.pixels[y * f.width + x], CURSOR, "corner ({x},{y}) should be outlined");
    }
    let (mx, my) = (cw / 2, ch / 2);
    assert_ne!(f.pixels[my * f.width + mx], CURSOR, "hollow center must stay unfilled");

    // Clearing the override restores the terminal's own style (default block).
    r.set_cursor_style_override(None);
    let f2 = r.render(&term, 2, 4);
    assert_eq!(cursor_positions(&f2).len(), cw * ch, "override cleared -> block again");
}

#[test]
fn blink_phase_off_suppresses_blinking_styles_only() {
    let Some(mut r) = renderer() else {
        eprintln!("SKIP: no system monospace font");
        return;
    };
    let (cw, ch) = r.cell_size();

    // Default style is BlinkingBlock: the off phase draws no cursor at all.
    let term = term_with(b"\x1b[1 q"); // DECSCUSR 1 = blinking block
    r.set_cursor_blink_phase(false);
    let off = r.render(&term, 2, 4);
    assert!(cursor_positions(&off).is_empty(), "blink phase off -> no cursor pixels");
    r.set_cursor_blink_phase(true);
    let on = r.render(&term, 2, 4);
    assert_eq!(cursor_positions(&on).len(), cw * ch, "blink phase on -> full block again");

    // A STEADY style ignores the phase entirely.
    let steady = term_with(b"\x1b[2 q");
    r.set_cursor_blink_phase(false);
    let f = r.render(&steady, 2, 4);
    assert_eq!(cursor_positions(&f).len(), cw * ch, "steady block must ignore the blink phase");

    // Blinking underline/bar respect the phase too.
    for (bytes, label) in [(&b"\x1b[3 q"[..], "underline"), (&b"\x1b[5 q"[..], "bar")] {
        let t = term_with(bytes);
        r.set_cursor_blink_phase(false);
        assert!(
            cursor_positions(&r.render(&t, 2, 4)).is_empty(),
            "blinking {label}: phase off -> no cursor"
        );
        r.set_cursor_blink_phase(true);
        assert!(
            !cursor_positions(&r.render(&t, 2, 4)).is_empty(),
            "blinking {label}: phase on -> cursor drawn"
        );
    }
}

#[test]
fn hidden_cursor_draws_nothing() {
    let Some(mut r) = renderer() else {
        eprintln!("SKIP: no system monospace font");
        return;
    };
    // DECTCEM off (CSI ?25l) hides the cursor entirely.
    let term = term_with(b"\x1b[?25l");
    let f = r.render(&term, 2, 4);
    assert!(cursor_positions(&f).is_empty(), "DECTCEM off -> no cursor pixels");
    // ... and DECSET 25 brings it back.
    let term = term_with(b"\x1b[?25l\x1b[?25h");
    let f = r.render(&term, 2, 4);
    assert!(!cursor_positions(&f).is_empty(), "DECTCEM on -> cursor drawn again");
}

#[test]
fn underline_and_bar_keep_glyph_in_normal_colors() {
    let Some(mut r) = renderer() else {
        eprintln!("SKIP: no system monospace font");
        return;
    };
    let (cw, ch) = r.cell_size();
    let strip = (ch / 8).max(2);
    // Put the cursor back ON the 'a' it just typed, with an underline cursor:
    // the glyph must be drawn normally (light fg pixels ABOVE the strip), not
    // cut out as the block style does.
    let term = term_with(b"a\x1b[1;1H\x1b[4 q");
    let f = r.render(&term, 2, 4);
    let near_fg = |p: u32| {
        let d = |a: u32, b: u32| ((a as i32) - (b as i32)).abs();
        d(p >> 16 & 0xff, FG >> 16 & 0xff) < 0x40
            && d(p >> 8 & 0xff, FG >> 8 & 0xff) < 0x40
            && d(p & 0xff, FG & 0xff) < 0x40
    };
    let glyph_above_strip = (0..ch - strip)
        .flat_map(|y| (0..cw).map(move |x| (x, y)))
        .any(|(x, y)| near_fg(f.pixels[y * f.width + x]));
    assert!(glyph_above_strip, "underline cursor must leave the glyph in its own fg");
    // The strip itself is solid cursor colour even where the glyph descends.
    for y in ch - strip..ch {
        for x in 0..cw {
            assert_eq!(f.pixels[y * f.width + x], CURSOR, "strip pixel ({x},{y}) overwritten");
        }
    }
}
