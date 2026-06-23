// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors
//
// CPU==GPU parity with an ACTIVELY-LIGATING font. The other parity tests use the
// host system font (which may not ligate the demo text), so this one points BOTH
// renderers at the bundled JetBrains Mono via $ATERM_FONT and renders a row full
// of programming operators ("a => b != c == d -> e <= f"). It asserts:
//   1. the CPU frame actually ligated (it differs from the same renderer with
//      ligatures forced off — so the test is non-vacuous), and
//   2. the GPU frame matches the CPU frame within the usual <=8 LSB blend
//      tolerance — i.e. the shared shaping plan keys + places the IDENTICAL
//      ligature glyph on both paths.
// Its own test BINARY (separate process) so the $ATERM_FONT env set here never
// races the other parity tests. Gated: no GPU / font -> skip cleanly.

use aterm_core::terminal::Terminal;
use aterm_render::{Frame, LigatureMode, Renderer, TextShapingConfig, Theme};

const JETBRAINS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../../src/renderer/src/assets/fonts/jetbrains-mono.ttf"
);

fn rr(p: u32) -> i32 {
    ((p >> 16) & 0xff) as i32
}
fn gg(p: u32) -> i32 {
    ((p >> 8) & 0xff) as i32
}
fn bb(p: u32) -> i32 {
    (p & 0xff) as i32
}

fn max_channel_delta(a: &Frame, b: &Frame) -> i32 {
    let mut m = 0;
    for (&pa, &pb) in a.pixels.iter().zip(b.pixels.iter()) {
        m = m.max((rr(pa) - rr(pb)).abs());
        m = m.max((gg(pa) - gg(pb)).abs());
        m = m.max((bb(pa) - bb(pb)).abs());
    }
    m
}

#[test]
fn ligature_font_gpu_matches_cpu() {
    let theme = Theme::default();
    let px = 18.0;

    if std::fs::metadata(JETBRAINS).is_err() {
        eprintln!("SKIP: bundled JetBrains Mono not found at {JETBRAINS}");
        return;
    }
    // Point BOTH renderers at the ligature font. SAFETY: this test runs in its own
    // binary, so no other test observes the mutation. (set_var is unsafe in 2024.)
    unsafe {
        std::env::set_var("ATERM_FONT", JETBRAINS);
    }

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
    // A CPU renderer with ligatures FORCED OFF, to prove the ligated frame is not
    // vacuously equal (the font really ligates the operators).
    let Some(mut cpu_off) = Renderer::from_system(px, theme) else {
        return;
    };
    cpu_off.set_text_shaping(TextShapingConfig {
        ligature_mode: LigatureMode::Disabled,
        ..Default::default()
    });

    let (rows, cols) = (1usize, 28usize);
    let mut term = Terminal::new(rows as u16, cols as u16);
    term.process(b"\x1b[?25la => b != c == d -> e");

    let mut win = aterm_gpu::WindowGpu::new();
    let input = term.cell_frame(rows, cols);
    let cpu_frame = cpu.render_input(&input);
    let cpu_off_frame = cpu_off.render_input(&input);
    let gpu_frame = gpu.render_input(&mut win, &input);

    assert_eq!(
        (gpu_frame.width, gpu_frame.height),
        (cpu_frame.width, cpu_frame.height),
        "dimensions differ"
    );

    // Non-vacuous: with this font the operators actually ligate (ligated != off).
    assert_ne!(
        cpu_frame.pixels, cpu_off_frame.pixels,
        "operators did not ligate — test would be vacuous (is this really a ligature font?)"
    );

    // The core gate: GPU reproduces the CPU ligature frame within the blend
    // tolerance, because both keyed + placed the IDENTICAL `mono_gid` glyph.
    let delta = max_channel_delta(&cpu_frame, &gpu_frame);
    eprintln!("ligature GPU vs CPU max per-channel delta = {delta}");
    assert!(
        delta <= 8,
        "GPU/CPU ligature pixels diverge: max per-channel delta {delta} > 8"
    );
}
