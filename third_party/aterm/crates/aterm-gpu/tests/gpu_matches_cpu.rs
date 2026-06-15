// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors
//
// Correctness gate for the GPU renderer: render the SAME terminal on the CPU
// (`aterm_render::Renderer`, already verified) and on the GPU
// (`aterm_gpu::GpuRenderer`) at the same px/theme, and prove numerically that
// the GPU output matches. The GPU has no human to "see" it; this is its oracle.
//
// Checks:
//   1. identical frame dimensions,
//   2. per-channel pixel delta within a small tolerance across the whole frame
//      (geometry + coverage blend match; only round-vs-floor rounding differs),
//   3. the same SEMANTIC properties the CPU visual-regression test asserts hold
//      on the GPU frame (red cell is red, blue-bg cell is blue, CJK cell is
//      non-blank, blank cell is background).
//
// Gated: if there is no GPU or no system font, the test no-ops (returns).

use aterm_core::terminal::Terminal;
use aterm_render::{Frame, Renderer, Theme};

const BG: u32 = 0x0011_1318; // Theme::default().bg

fn rr(p: u32) -> i32 { ((p >> 16) & 0xff) as i32 }
fn gg(p: u32) -> i32 { ((p >> 8) & 0xff) as i32 }
fn bb(p: u32) -> i32 { (p & 0xff) as i32 }

fn dist(a: u32, c: u32) -> i32 {
    (rr(a) - rr(c)).abs() + (gg(a) - gg(c)).abs() + (bb(a) - bb(c)).abs()
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

fn non_bg_count(px: &[u32]) -> usize {
    px.iter().filter(|&&p| dist(p, BG) > 24).count()
}

/// The visual-regression demo grid (same as aterm-render's visual_regression test).
fn demo_term() -> (Terminal, usize, usize) {
    let (rows, cols) = (6usize, 12usize);
    let mut term = Terminal::new(rows as u16, cols as u16);
    term.process(
        b"\x1b[31mRR\x1b[0m\r\n\
\x1b[44m  \x1b[0m\r\n\
\xe6\x97\xa5\xe6\x9c\xac\r\n\
\x1b[7mXX\x1b[0m\r\n\
ab\r\n",
    );
    (term, rows, cols)
}

#[test]
fn gpu_matches_cpu() {
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

    let (term, rows, cols) = demo_term();
    let (cw, ch) = cpu.cell_size();
    let cpu_frame = cpu.render(&term, rows, cols);
    let gpu_frame = gpu.render(&term, rows, cols);

    // 1. identical dimensions
    assert_eq!((gpu_frame.width, gpu_frame.height), (cpu_frame.width, cpu_frame.height), "dimensions differ");
    assert_eq!((gpu_frame.width, gpu_frame.height), (cols * cw, rows * ch), "unexpected frame size");

    // 2. near-identical pixels: geometry + blend match, so only rounding differs.
    let delta = max_channel_delta(&cpu_frame, &gpu_frame);
    eprintln!("GPU vs CPU max per-channel delta = {delta}");
    assert!(delta <= 8, "GPU/CPU pixels diverge: max per-channel delta {delta} > 8");

    // 3. semantic checks (same as aterm-render's visual_regression) on the GPU frame.
    // red 'R' cell (0,0)
    let red = cell_pixels(&gpu_frame, cw, ch, 0, 0)
        .iter()
        .any(|&p| rr(p) > 140 && gg(p) < 90 && bb(p) < 90);
    assert!(red, "GPU: expected red glyph pixels in cell (0,0)");

    // blue-bg space cell (1,0)
    let blue_px = cell_pixels(&gpu_frame, cw, ch, 1, 0);
    let blue = blue_px.iter().filter(|&&p| bb(p) > 110 && rr(p) < 90).count();
    assert!(blue > blue_px.len() / 2, "GPU: expected blue background in cell (1,0) ({}/{})", blue, blue_px.len());

    // CJK 日 cell (2,0): non-blank via font fallback
    let cjk = non_bg_count(&cell_pixels(&gpu_frame, cw, ch, 2, 0));
    assert!(cjk > 12, "GPU: CJK cell (2,0) is blank ({cjk} non-bg pixels)");

    // blank cell (5,8): stays background
    let blank_px = cell_pixels(&gpu_frame, cw, ch, 5, 8);
    let blank_non_bg = non_bg_count(&blank_px);
    assert!(blank_non_bg < blank_px.len() / 20, "GPU: blank cell (5,8) should be background ({blank_non_bg} non-bg)");
}

/// EXACT parity on procedural cells: box-drawing / block / braille glyphs are
/// synthesized as hard 0/255 coverage sized to the cell, so the CPU coverage
/// blend and the GPU alpha blend agree on EVERY pixel — max per-channel delta
/// must be 0, not merely within the antialiasing tolerance above. The frame
/// holds only procedural glyphs and solid fills (cursor hidden via DECTCEM),
/// i.e. the whole frame is in the exactness domain. This EXTENDS the parity
/// regression net; never weaken it back to a tolerance.
#[test]
fn procedural_cells_match_cpu_exactly() {
    let theme = Theme::default();
    let px = 18.0;

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

    let (rows, cols) = (4usize, 16usize);
    let mut term = Terminal::new(rows as u16, cols as u16);
    // Hide the cursor so no cell mixes in cursor styling; every pixel is then
    // a solid bg fill or hard procedural coverage. Row 2 paints a red double
    // junction run to exercise the fg tint path; the rest uses default fg.
    term.process(
        "\x1b[?25l\
\u{250C}\u{2500}\u{252C}\u{2500}\u{2510}\u{2554}\u{2550}\u{2566}\u{2550}\u{2557}\u{2501}\u{2513}\u{2517}\u{2503}\u{254B}\r\n\
\u{251C}\u{2500}\u{253C}\u{2500}\u{2524}\x1b[31m\u{2560}\u{2550}\u{256C}\u{2550}\u{2563}\x1b[0m\u{2580}\u{2584}\u{258C}\u{2590}\u{2588}\r\n\
\u{2514}\u{2500}\u{2534}\u{2500}\u{2518}\u{255A}\u{2550}\u{2569}\u{2550}\u{255D}\u{2591}\u{2592}\u{2593}\u{2847}\u{28FF}\r\n\
\u{256D}\u{2500}\u{256E}\u{2570}\u{256F}\u{2571}\u{2572}\u{2573}\u{2504}\u{2508}\u{254C}\u{2581}\u{2582}\u{258E}\u{1FB13}"
            .as_bytes(),
    );

    let cpu_frame = cpu.render(&term, rows, cols);
    let gpu_frame = gpu.render(&term, rows, cols);

    assert_eq!(
        (gpu_frame.width, gpu_frame.height),
        (cpu_frame.width, cpu_frame.height),
        "dimensions differ"
    );
    // Sanity that the fixture really drew glyphs (default fg pixels exist —
    // procedural coverage is hard 0/255, so stroke pixels are EXACTLY the
    // terminal's default foreground — and the red SGR run produced
    // red-dominant pixels). Guards a false pass on an all-background frame.
    let dfg = term.default_foreground();
    let dfg = (u32::from(dfg.r) << 16) | (u32::from(dfg.g) << 8) | u32::from(dfg.b);
    assert!(cpu_frame.pixels.iter().any(|&p| p == dfg), "no default-fg glyph pixels");
    assert!(
        cpu_frame.pixels.iter().any(|&p| rr(p) > 100 && rr(p) > gg(p) && rr(p) > bb(p)),
        "no red glyph pixels from the SGR run"
    );

    let delta = max_channel_delta(&cpu_frame, &gpu_frame);
    assert_eq!(delta, 0, "procedural cells must match EXACTLY between CPU and GPU");
}

/// Colour-emoji parity: the GPU must reproduce the CPU's RGBA emoji blit, not
/// drop it. Before the colour atlas existed the GPU skipped every `Rgba` glyph
/// and emoji rendered BLANK on the Metal path — a silent parity hole. This
/// renders a row of emoji on both paths and asserts (a) the GPU emoji cells are
/// substantially non-background (the glyph was actually drawn) and (b) the GPU
/// frame matches the CPU frame within the usual blend tolerance. Gated twice:
/// no GPU/font -> skip; no colour-emoji font on this host (the CPU cell comes
/// back blank) -> skip, since there's nothing to reproduce.
#[test]
fn colour_emoji_gpu_matches_cpu() {
    let theme = Theme::default();
    let px = 18.0;

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

    let (rows, cols) = (2usize, 12usize);
    let mut term = Terminal::new(rows as u16, cols as u16);
    // 🚀😀🎉🔥 — each a wide (2-cell) colour glyph from the sbix font.
    term.process("\u{1F680}\u{1F600}\u{1F389}\u{1F525}".as_bytes());

    let (cw, ch) = cpu.cell_size();
    let cpu_frame = cpu.render(&term, rows, cols);
    let gpu_frame = gpu.render(&term, rows, cols);

    assert_eq!(
        (gpu_frame.width, gpu_frame.height),
        (cpu_frame.width, cpu_frame.height),
        "dimensions differ"
    );

    // Gate on colour-emoji availability: if the CPU drew nothing in the first
    // emoji's lead cell, this host has no colour-emoji font — nothing to test.
    let cpu_lead = non_bg_count(&cell_pixels(&cpu_frame, cw, ch, 0, 0));
    if cpu_lead < 12 {
        eprintln!("SKIP: no colour-emoji font on this host (CPU emoji cell is blank)");
        return;
    }

    // (a) the GPU actually drew the emoji — every emoji lead cell is non-blank.
    for (i, col) in [0usize, 2, 4, 6].iter().enumerate() {
        let gpu_cell = non_bg_count(&cell_pixels(&gpu_frame, cw, ch, 0, *col));
        assert!(
            gpu_cell > 12,
            "GPU emoji #{i} (cell 0,{col}) is blank ({gpu_cell} non-bg pixels) — colour glyph dropped"
        );
    }

    // (b) GPU reproduces the CPU emoji within the blend tolerance. The colour
    // atlas holds the CPU's exact scaled pixels (1:1 NEAREST), so only the
    // edge alpha-blend rounding differs — the same <=8 LSB the mono path allows.
    let delta = max_channel_delta(&cpu_frame, &gpu_frame);
    eprintln!("colour-emoji GPU vs CPU max per-channel delta = {delta}");
    assert!(delta <= 8, "GPU/CPU emoji pixels diverge: max per-channel delta {delta} > 8");

    // The emoji are genuinely colourful (not a mono fallback rendered identically
    // on both): the row has pixels from clearly different hues.
    let row0 = {
        let mut v = Vec::new();
        for col in 0..8 {
            v.extend(cell_pixels(&gpu_frame, cw, ch, 0, col));
        }
        v
    };
    let reddish = row0.iter().any(|&p| rr(p) > 140 && rr(p) > gg(p) + 30 && rr(p) > bb(p) + 30);
    let other = row0.iter().any(|&p| gg(p) > 120 || bb(p) > 120);
    assert!(reddish && other, "GPU emoji row is not multi-coloured (reddish={reddish}, other={other})");
}

/// VS16 emoji-presentation parity end-to-end: `❤️` (U+2764 + VS16) has a
/// MONOCHROME glyph in the text fonts, so without presentation handling it
/// would render as a grey heart. The core flags the VS16-widened cell
/// (`RenderCell::emoji_presentation`), and BOTH renderers must then prefer the
/// colour face — drawing a RED heart. This proves the GPU honours the flag
/// through `extract` -> `cell_key` -> colour atlas, matching the CPU. Gated on a
/// colour-emoji font (if the CPU heart isn't red, the host has none -> skip).
#[test]
fn vs16_emoji_gpu_matches_cpu() {
    let theme = Theme::default();
    let px = 18.0;

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

    let (rows, cols) = (2usize, 8usize);
    let mut term = Terminal::new(rows as u16, cols as u16);
    term.process("\u{2764}\u{FE0F}".as_bytes()); // ❤️

    let (cw, ch) = cpu.cell_size();
    let cpu_frame = cpu.render(&term, rows, cols);
    let gpu_frame = gpu.render(&term, rows, cols);
    assert_eq!(
        (gpu_frame.width, gpu_frame.height),
        (cpu_frame.width, cpu_frame.height),
        "dimensions differ"
    );

    // A RED-dominant pixel marks the colour heart (the mono glyph is drawn in
    // the light default fg, so it is never red-dominant).
    let red = |f: &Frame| {
        cell_pixels(f, cw, ch, 0, 0)
            .iter()
            .filter(|&&p| rr(p) > 120 && rr(p) > gg(p) + 40 && rr(p) > bb(p) + 40)
            .count()
    };
    if red(&cpu_frame) == 0 {
        eprintln!("SKIP: no colour ❤ on this host (CPU heart is not red)");
        return;
    }
    assert!(red(&gpu_frame) > 0, "GPU did not render the VS16 ❤️ in colour (emoji_presentation ignored)");

    let delta = max_channel_delta(&cpu_frame, &gpu_frame);
    eprintln!("VS16 emoji GPU vs CPU max per-channel delta = {delta}");
    assert!(delta <= 8, "GPU/CPU VS16 emoji pixels diverge: max per-channel delta {delta} > 8");
}

/// Emoji grapheme-CLUSTER parity end-to-end: a ZWJ family (👨‍👩‍👧), a skin-tone
/// thumbs-up (👍🏽), and a keycap (1️⃣) are multi-codepoint clusters the renderer
/// SHAPES (rustybuzz) to a single colour glyph. Both paths must draw that glyph,
/// not just the base codepoint. Proves the GPU resolves cluster keys via the
/// shared `resolve_cell_key` and atlases them identically to the CPU. Gated on a
/// colour-emoji font (if the CPU family cell is blank, the host has none).
#[test]
fn cluster_emoji_gpu_matches_cpu() {
    let theme = Theme::default();
    let px = 18.0;

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

    let (rows, cols) = (2usize, 16usize);
    let mut term = Terminal::new(rows as u16, cols as u16);
    // family (0-1) sp(2) skin (3-4) sp(5) keycap (6) sp(7) flag (8-9)
    term.process(
        "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467} \u{1F44D}\u{1F3FD} \u{31}\u{FE0F}\u{20E3} \u{1F1FA}\u{1F1F8}".as_bytes(),
    );

    let (cw, ch) = cpu.cell_size();
    let cpu_frame = cpu.render(&term, rows, cols);
    let gpu_frame = gpu.render(&term, rows, cols);
    assert_eq!(
        (gpu_frame.width, gpu_frame.height),
        (cpu_frame.width, cpu_frame.height),
        "dimensions differ"
    );

    // Gate: the family cluster (lead col 0) must be a non-blank colour glyph on
    // the CPU; if not, this host lacks a colour-emoji font -> skip.
    let cpu_family = non_bg_count(&cell_pixels(&cpu_frame, cw, ch, 0, 0));
    if cpu_family < 12 {
        eprintln!("SKIP: no colour-emoji font on this host (CPU family cluster is blank)");
        return;
    }

    // The GPU drew each cluster (family col 0, skin col 3, keycap col 6,
    // regional-indicator flag col 8).
    for (label, col) in [("family", 0usize), ("skin", 3), ("keycap", 6), ("flag", 8)] {
        let gpu_cell = non_bg_count(&cell_pixels(&gpu_frame, cw, ch, 0, col));
        assert!(
            gpu_cell > 12,
            "GPU {label} cluster (cell 0,{col}) is blank ({gpu_cell} non-bg pixels) — cluster not shaped"
        );
    }

    let delta = max_channel_delta(&cpu_frame, &gpu_frame);
    eprintln!("cluster emoji GPU vs CPU max per-channel delta = {delta}");
    assert!(delta <= 8, "GPU/CPU cluster emoji pixels diverge: max per-channel delta {delta} > 8");
}

/// Line decorations (underline / strikethrough / double underline) are drawn as
/// hard-edged rects OVER the glyphs. Both paths use the same
/// `aterm_render::underline_rects` / `strike_overline_rects` geometry, so the
/// GPU must match the CPU within the glyph tolerance — AND the decorated frame
/// must differ from an undecorated one (proving the line is actually drawn, on
/// both paths).
#[test]
fn decorations_gpu_match_cpu() {
    let theme = Theme::default();
    let px = 18.0;

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

    let (rows, cols) = (3usize, 8usize);
    let render = |cpu: &mut Renderer, gpu: &mut aterm_gpu::GpuRenderer, bytes: &[u8]| {
        let mut term = Terminal::new(rows as u16, cols as u16);
        term.process(bytes);
        (cpu.render(&term, rows, cols), gpu.render(&term, rows, cols))
    };

    // Underlined, strikethrough, double-underlined rows.
    let (cpu_deco, gpu_deco) = render(&mut cpu, &mut gpu, b"\x1b[4mUU\x1b[0m\r\n\x1b[9mSS\x1b[0m\r\n\x1b[21mDD\x1b[0m");
    // Same glyphs, no decorations.
    let (cpu_plain, _) = render(&mut cpu, &mut gpu, b"UU\r\nSS\r\nDD");

    assert_eq!(
        (gpu_deco.width, gpu_deco.height),
        (cpu_deco.width, cpu_deco.height),
        "dimensions differ"
    );

    // GPU reproduces the CPU decorated frame (hard rects + glyph AA <= 8).
    let delta = max_channel_delta(&cpu_deco, &gpu_deco);
    eprintln!("decorations GPU vs CPU max per-channel delta = {delta}");
    assert!(delta <= 8, "GPU/CPU decorated pixels diverge: max per-channel delta {delta} > 8");

    // The decorations are actually drawn: the decorated frame differs from the
    // plain one, on BOTH paths (so neither silently skips the lines).
    assert!(
        cpu_deco.pixels != cpu_plain.pixels,
        "CPU decorated frame is identical to the undecorated one — no lines drawn"
    );
    assert!(
        gpu_deco.pixels != cpu_plain.pixels,
        "GPU decorated frame is identical to the undecorated one — no lines drawn"
    );
}

/// Synthetic BOLD / ITALIC are baked into the cached glyph coverage, which the
/// GPU atlas pulls by `GlyphKey` (style included) — so the GPU reproduces them
/// with no shader change. Assert parity AND that styled text differs from plain
/// (the weight/slant is actually applied on both paths).
#[test]
fn bold_italic_gpu_match_cpu() {
    let theme = Theme::default();
    let px = 18.0;

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

    let (rows, cols) = (3usize, 8usize);
    let render = |cpu: &mut Renderer, gpu: &mut aterm_gpu::GpuRenderer, bytes: &[u8]| {
        let mut term = Terminal::new(rows as u16, cols as u16);
        term.process(bytes);
        (cpu.render(&term, rows, cols), gpu.render(&term, rows, cols))
    };

    // Cursor visible: a wide synthetic-bold-italic glyph overflows into the next
    // cell, and the block cursor now composites the same on both paths (see
    // block_cursor_over_glyph_overflow_matches_cpu).
    let (cpu_styled, gpu_styled) = render(
        &mut cpu,
        &mut gpu,
        b"\x1b[1mBB\x1b[0m\r\n\x1b[3mII\x1b[0m\r\n\x1b[1;3mWW\x1b[0m",
    );
    let (cpu_plain, _) = render(&mut cpu, &mut gpu, b"BB\r\nII\r\nWW");

    assert_eq!(
        (gpu_styled.width, gpu_styled.height),
        (cpu_styled.width, cpu_styled.height),
        "dimensions differ"
    );
    let delta = max_channel_delta(&cpu_styled, &gpu_styled);
    eprintln!("bold/italic GPU vs CPU max per-channel delta = {delta}");
    assert!(delta <= 8, "GPU/CPU bold-italic pixels diverge: max per-channel delta {delta} > 8");

    assert!(
        cpu_styled.pixels != cpu_plain.pixels,
        "CPU bold/italic frame identical to plain — synthetic styling not applied"
    );
    assert!(
        gpu_styled.pixels != cpu_plain.pixels,
        "GPU bold/italic frame identical to plain — synthetic styling not applied"
    );
}

/// DECDWL double-width lines (`ESC # 6`) draw every cell twice as wide via 2×
/// NEAREST replication. The GPU's 2×-wide nearest-sampled quad must match the
/// CPU's 2× column replicate, AND the row must actually be wider than the same
/// text on a single-width line (the doubling is applied).
#[test]
fn decdwl_double_width_gpu_matches_cpu() {
    let theme = Theme::default();
    let px = 18.0;

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

    let (rows, cols) = (1usize, 16usize);
    let render = |cpu: &mut Renderer, gpu: &mut aterm_gpu::GpuRenderer, bytes: &[u8]| {
        let mut term = Terminal::new(rows as u16, cols as u16);
        term.process(bytes);
        (cpu.render(&term, rows, cols), gpu.render(&term, rows, cols))
    };
    let (cw, ch) = cpu.cell_size();

    // DECDWL line vs the same text single-width, cursor hidden.
    let (cpu_dw, gpu_dw) = render(&mut cpu, &mut gpu, b"\x1b[?25l\x1b#6ABCD");
    let (cpu_sw, _) = render(&mut cpu, &mut gpu, b"\x1b[?25lABCD");

    assert_eq!((gpu_dw.width, gpu_dw.height), (cpu_dw.width, cpu_dw.height), "dims");
    let delta = max_channel_delta(&cpu_dw, &gpu_dw);
    eprintln!("DECDWL GPU vs CPU max per-channel delta = {delta}");
    assert!(delta <= 8, "GPU/CPU double-width pixels diverge: max per-channel delta {delta} > 8");

    // The 'C' (col 2) sits at single-width col 2 but double-width col 2*2=4.
    // The double-width cell (0,4) is non-blank; the single-width frame's col 4
    // is already past "ABCD" (blank) — so the row is genuinely twice as wide.
    let dw_at4 = non_bg_count(&cell_pixels(&cpu_dw, cw, ch, 0, 4));
    let sw_at4 = non_bg_count(&cell_pixels(&cpu_sw, cw, ch, 0, 4));
    assert!(dw_at4 > 12 && sw_at4 < 12, "DECDWL not 2× wide (dw col4={dw_at4}, sw col4={sw_at4})");
}

/// DECDHL double-height lines (`ESC # 3`/`# 4`): the same text on two rows forms
/// ONE 2×-both line — the top row shows the upper half of the doubled glyphs, the
/// bottom row the lower half (a dest-row clip of the 2× glyph). The GPU computes
/// the visible slice (rect + UV) via the shared `glyph_quad`, so its NEAREST
/// quad reproduces the CPU's 2× replicate + clip.
#[test]
fn decdhl_double_height_gpu_matches_cpu() {
    let theme = Theme::default();
    let px = 18.0;

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

    let (rows, cols) = (2usize, 16usize);
    let render = |cpu: &mut Renderer, gpu: &mut aterm_gpu::GpuRenderer, bytes: &[u8]| {
        let mut term = Terminal::new(rows as u16, cols as u16);
        term.process(bytes);
        (cpu.render(&term, rows, cols), gpu.render(&term, rows, cols))
    };

    // DECDHL top + bottom halves vs the same text plain (cursor hidden).
    let (cpu_dh, gpu_dh) = render(&mut cpu, &mut gpu, b"\x1b[?25l\x1b#3BIG\r\n\x1b#4BIG");
    let (cpu_plain, _) = render(&mut cpu, &mut gpu, b"\x1b[?25lBIG\r\nBIG");

    assert_eq!((gpu_dh.width, gpu_dh.height), (cpu_dh.width, cpu_dh.height), "dims");
    let delta = max_channel_delta(&cpu_dh, &gpu_dh);
    eprintln!("DECDHL GPU vs CPU max per-channel delta = {delta}");
    assert!(delta <= 8, "GPU/CPU double-height pixels diverge: max per-channel delta {delta} > 8");
    // Double-height is genuinely different from the plain duplicated text.
    assert!(cpu_dh.pixels != cpu_plain.pixels, "DECDHL renders the same as plain text");
}

/// Powerline separators (U+E0B0–E0B7) are synthesized as procedural hard
/// coverage, so — like box-drawing — the CPU coverage blend and GPU alpha blend
/// must agree on EVERY pixel (delta 0), and the glyphs must actually be drawn.
#[test]
fn powerline_cells_match_cpu_exactly() {
    let theme = Theme::default();
    let px = 18.0;

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

    let (rows, cols) = (1usize, 8usize);
    let mut term = Terminal::new(rows as u16, cols as u16);
    // Cursor hidden so the whole frame is procedural coverage + solid fills.
    term.process(
        "\x1b[?25l\u{E0B0}\u{E0B2}\u{E0B4}\u{E0B6}\u{E0B8}\u{E0BA}\u{E0BC}\u{E0BE}".as_bytes(),
    );
    let (cw, ch) = cpu.cell_size();
    let cpu_frame = cpu.render(&term, rows, cols);
    let gpu_frame = gpu.render(&term, rows, cols);

    assert_eq!((gpu_frame.width, gpu_frame.height), (cpu_frame.width, cpu_frame.height), "dims");
    let delta = max_channel_delta(&cpu_frame, &gpu_frame);
    assert_eq!(delta, 0, "Powerline procedural cells must match CPU EXACTLY, got delta {delta}");
    // Each separator actually drew ink (the solid triangle/cap cells especially).
    for col in [0usize, 2, 4, 6] {
        let n = non_bg_count(&cell_pixels(&cpu_frame, cw, ch, 0, col));
        assert!(n > 12, "Powerline cell (0,{col}) is blank ({n} non-bg) — not synthesized");
    }
}

/// A glyph overflowing into the BLOCK-cursor cell must composite the same on
/// both paths: the CPU paints the block cursor LAST (over the overflow), and the
/// GPU now fills the block cursor AFTER the glyph passes too (was: cursor bg in
/// the bg pass, so overflow drew on top — a ~137-LSB divergence). A wide
/// synthetic bold-italic glyph sits immediately left of the cursor.
#[test]
fn block_cursor_over_glyph_overflow_matches_cpu() {
    let theme = Theme::default();
    let px = 18.0;

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

    let (rows, cols) = (1usize, 4usize);
    let mut term = Terminal::new(rows as u16, cols as u16);
    // Bold-italic W at col 0 (overflows right); the block cursor lands at col 1.
    term.process(b"\x1b[1;3mW");
    let cpu_frame = cpu.render(&term, rows, cols);
    let gpu_frame = gpu.render(&term, rows, cols);

    assert_eq!((gpu_frame.width, gpu_frame.height), (cpu_frame.width, cpu_frame.height), "dims");
    let delta = max_channel_delta(&cpu_frame, &gpu_frame);
    eprintln!("cursor-over-overflow GPU vs CPU max per-channel delta = {delta}");
    assert!(delta <= 8, "block cursor vs glyph overflow diverges: max per-channel delta {delta} > 8");
}

/// Combining diacritics (é = e + U+0301, …) are overlaid as extra mono-glyph
/// instances on the base cell. The GPU pulls each mark from the same atlas, so
/// it must match the CPU — AND the accented frame must differ from the bare-base
/// one (the mark is actually drawn on both paths).
#[test]
fn combining_marks_gpu_match_cpu() {
    let theme = Theme::default();
    let px = 18.0;

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

    let (rows, cols) = (1usize, 8usize);
    let render = |cpu: &mut Renderer, gpu: &mut aterm_gpu::GpuRenderer, bytes: &[u8]| {
        let mut term = Terminal::new(rows as u16, cols as u16);
        term.process(bytes);
        (cpu.render(&term, rows, cols), gpu.render(&term, rows, cols))
    };

    // é ñ å — base + combining mark.
    let (cpu_acc, gpu_acc) =
        render(&mut cpu, &mut gpu, "\x1b[?25le\u{0301}n\u{0303}a\u{030A}".as_bytes());
    let (cpu_bare, _) = render(&mut cpu, &mut gpu, b"\x1b[?25lena");

    assert_eq!((gpu_acc.width, gpu_acc.height), (cpu_acc.width, cpu_acc.height), "dimensions differ");
    let delta = max_channel_delta(&cpu_acc, &gpu_acc);
    eprintln!("combining marks GPU vs CPU max per-channel delta = {delta}");
    assert!(delta <= 8, "GPU/CPU combining-mark pixels diverge: max per-channel delta {delta} > 8");
    assert!(cpu_acc.pixels != cpu_bare.pixels, "CPU: combining marks not drawn (accented == bare)");
    assert!(gpu_acc.pixels != cpu_bare.pixels, "GPU: combining marks not drawn (accented == bare)");
}
