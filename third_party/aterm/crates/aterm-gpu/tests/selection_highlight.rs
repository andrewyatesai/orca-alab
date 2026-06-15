// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors
//
// Selection-highlight gate for the GPU renderer: with an active text selection,
// the GPU must paint selected cells with `Theme::selection` as their background
// (glyph foreground unchanged), leave unselected cells alone, and stay
// pixel-equal to the CPU renderer within the same per-channel tolerance the
// `gpu_matches_cpu` test uses.
//
// Gated: if there is no GPU or no system font, the test no-ops (returns).

use aterm_core::selection::{SelectionSide, SelectionType};
use aterm_core::terminal::Terminal;
use aterm_render::{Frame, Renderer, Theme};

fn rr(p: u32) -> i32 {
    ((p >> 16) & 0xff) as i32
}
fn gg(p: u32) -> i32 {
    ((p >> 8) & 0xff) as i32
}
fn bb(p: u32) -> i32 {
    (p & 0xff) as i32
}

/// Max per-channel absolute difference between two same-sized frames.
fn max_channel_delta(a: &Frame, b: &Frame) -> i32 {
    let mut m = 0;
    for (&pa, &pb) in a.pixels.iter().zip(b.pixels.iter()) {
        m = m.max((rr(pa) - rr(pb)).abs());
        m = m.max((gg(pa) - gg(pb)).abs());
        m = m.max((bb(pa) - bb(pb)).abs());
    }
    m
}

fn cell_pixels(f: &Frame, cw: usize, ch: usize, row: usize, col: usize) -> Vec<u32> {
    let mut out = Vec::with_capacity(cw * ch);
    for y in row * ch..(row * ch + ch).min(f.height) {
        for x in col * cw..(col * cw + cw).min(f.width) {
            out.push(f.pixels[y * f.width + x]);
        }
    }
    out
}

/// Per-channel closeness to a packed `0x00RRGGBB` colour.
fn near(p: u32, c: u32, tol: i32) -> bool {
    (rr(p) - rr(c)).abs() <= tol && (gg(p) - gg(c)).abs() <= tol && (bb(p) - bb(c)).abs() <= tol
}

#[test]
fn selection_highlight_gpu_matches_cpu() {
    let theme = Theme::default();
    let px = 18.0;

    // Gate: no GPU or no font -> skip cleanly.
    let mut gpu = match aterm_gpu::GpuRenderer::new(px, theme) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("SKIP: no GPU/font available: {e}");
            return;
        }
    };
    let Some(mut cpu) = Renderer::from_system(px, theme) else {
        eprintln!("SKIP: no system monospace font");
        return;
    };

    let (rows, cols) = (4usize, 10usize);
    let mut term = Terminal::new(rows as u16, cols as u16);
    term.process(b"hello\r\nworld");
    // Select row 0, cols 1..=3 ("ell"). Cursor sits at (1,5) — away from every
    // cell this test inspects.
    let sel = term.text_selection_mut();
    sel.start_selection(0, 1, SelectionSide::Left, SelectionType::Simple);
    sel.update_selection(0, 3, SelectionSide::Right);
    sel.complete_selection();

    let (cw, ch) = cpu.cell_size();
    let cpu_frame = cpu.render(&term, rows, cols);
    let gpu_frame = gpu.render(&term, rows, cols);

    // (iii) frames match within the gpu_matches_cpu tolerance, selection active.
    assert_eq!(
        (gpu_frame.width, gpu_frame.height),
        (cpu_frame.width, cpu_frame.height),
        "dimensions differ"
    );
    let delta = max_channel_delta(&cpu_frame, &gpu_frame);
    eprintln!("selection: GPU vs CPU max per-channel delta = {delta}");
    assert!(delta <= 8, "GPU/CPU pixels diverge with selection: max per-channel delta {delta} > 8");

    for (name, f) in [("cpu", &cpu_frame), ("gpu", &gpu_frame)] {
        // (i) a selected cell's dominant background is ~theme.selection.
        let sel_px = cell_pixels(f, cw, ch, 0, 2); // selected 'l'
        let n_sel = sel_px.iter().filter(|&&p| near(p, theme.selection, 8)).count();
        assert!(
            n_sel > sel_px.len() / 2,
            "{name}: selected cell (0,2) should be selection-coloured ({n_sel}/{})",
            sel_px.len()
        );

        // (ii) unselected cells keep the theme/default background.
        let blank_px = cell_pixels(f, cw, ch, 0, 8); // blank, unselected
        let n_bg = blank_px.iter().filter(|&&p| near(p, theme.bg, 8)).count();
        assert!(
            n_bg == blank_px.len(),
            "{name}: blank cell (0,8) should stay theme bg ({n_bg}/{})",
            blank_px.len()
        );
        for (row, col) in [(0usize, 0usize), (0, 4), (1, 2), (0, 8)] {
            let px_cell = cell_pixels(f, cw, ch, row, col);
            let stray = px_cell.iter().filter(|&&p| near(p, theme.selection, 8)).count();
            assert_eq!(stray, 0, "{name}: unselected cell ({row},{col}) shows selection colour");
        }
    }
}
