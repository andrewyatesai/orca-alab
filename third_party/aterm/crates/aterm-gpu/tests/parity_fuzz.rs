// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors
//
// CPU==GPU rendering parity FUZZ. The GPU renderer's sole correctness oracle is
// the (independently verified) CPU renderer; the example tests in
// `gpu_matches_cpu.rs` pin specific features. This sweeps RANDOM mixed content —
// ASCII, colour emoji (single/VS16/ZWJ/flag), every SGR style + decoration,
// wide CJK, combining diacritics, procedural box/block/braille/sextant/Powerline
// glyphs, and DECDWL/DECDHL line sizes — and asserts the two paths stay within
// the usual glyph-antialiasing tolerance on EVERY frame. A deterministic PRNG
// (no proptest dep, like the lz4 fuzz) keeps it reproducible; gated on a GPU.

use aterm_core::terminal::Terminal;
use aterm_render::{Frame, Renderer, Theme};

fn max_channel_delta(a: &Frame, b: &Frame) -> i32 {
    let mut m = 0;
    for (&pa, &pb) in a.pixels.iter().zip(b.pixels.iter()) {
        for sh in [16, 8, 0] {
            let (ca, cb) = (((pa >> sh) & 0xff) as i32, ((pb >> sh) & 0xff) as i32);
            m = m.max((ca - cb).abs());
        }
    }
    m
}

/// Tokens the fuzz strings together. Each is raw bytes fed to the terminal.
const TOKENS: &[&[u8]] = &[
    b"abc", b"XY", b"  ", b"123", b".rs", b"/usr",
    b"\x1b[1m", b"\x1b[3m", b"\x1b[4m", b"\x1b[9m", b"\x1b[21m", b"\x1b[4:3m",
    b"\x1b[53m", b"\x1b[0m", b"\x1b[31m", b"\x1b[42m", b"\x1b[7m", b"\x1b[2m",
    b"\x1b[38;2;200;120;40m", b"\x1b[4;58:2::255:0:0m",
    "\u{1F680}".as_bytes(),            // rocket
    "\u{2764}\u{FE0F}".as_bytes(),     // VS16 heart
    "\u{1F468}\u{200D}\u{1F4BB}".as_bytes(), // ZWJ tech
    "\u{1F1FA}\u{1F1F8}".as_bytes(),   // US flag
    "\u{1F44D}\u{1F3FD}".as_bytes(),   // skin-tone thumb
    "\u{65E5}\u{672C}".as_bytes(),     // CJK
    "e\u{0301}".as_bytes(),            // é decomposed
    "\u{250C}\u{2500}\u{2510}".as_bytes(), // box
    "\u{2588}\u{2592}".as_bytes(),     // block + shade
    "\u{2847}".as_bytes(),             // braille
    "\u{1FB13}".as_bytes(),            // sextant
    "\u{E0B0}\u{E0B6}".as_bytes(),     // powerline
    b"\r\n", b"\r\n",
    b"\x1b#6",                          // DECDWL (line start)
    b"\x1b#3", b"\x1b#4",               // DECDHL top/bottom
];

#[test]
fn cpu_gpu_parity_fuzz() {
    let theme = Theme::default();
    let px = 17.0;
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

    let (rows, cols) = (8usize, 24usize);
    let mut state: u64 = 0x243F_6A88_85A3_08D3;
    let mut next = move || {
        state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        (state >> 33) as u32
    };

    let mut worst = 0i32;
    let iters = 160;
    for it in 0..iters {
        let mut term = Terminal::new(rows as u16, cols as u16);
        // Half the frames hide the cursor; the rest exercise the block cursor too.
        if next() & 1 == 0 {
            term.process(b"\x1b[?25l");
        }
        let token_count = 12 + (next() % 40) as usize;
        for _ in 0..token_count {
            let tok = TOKENS[(next() as usize) % TOKENS.len()];
            term.process(tok);
        }
        let cpu_frame = cpu.render(&term, rows, cols);
        let gpu_frame = gpu.render(&term, rows, cols);
        assert_eq!(
            (cpu_frame.width, cpu_frame.height),
            (gpu_frame.width, gpu_frame.height),
            "iter {it}: dimensions diverge"
        );
        let d = max_channel_delta(&cpu_frame, &gpu_frame);
        worst = worst.max(d);
        assert!(d <= 8, "iter {it}: CPU/GPU diverge by {d} > 8 LSB");
    }
    eprintln!("parity fuzz: {iters} random frames, worst per-channel delta = {worst}");
}
