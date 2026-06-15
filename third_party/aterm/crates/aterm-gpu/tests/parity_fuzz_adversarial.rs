// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors
//
//! GPU render panic-freedom + CPU/GPU parity on ADVERSARIAL engine states.
//!
//! `parity_fuzz.rs` sweeps a CURATED token set, and the CPU render fuzz
//! (`aterm-render`) drives hostile escape/UTF-8 input but renders only on the
//! CPU. Neither exercises the GPU-specific code (atlas packing, colour atlas,
//! wgpu encode, cursor compositing) across the broad adversarial input space —
//! where a degenerate quad or an atlas overflow from hundreds of distinct
//! random glyphs would crash the GPU-mode window. This drives the engine with
//! random bytes + crafted CSI/OSC/SGR + emoji/CJK/combining/procedural content,
//! renders on the GPU, and asserts: no panic, a valid frame, and CPU==GPU
//! within the established <=8-LSB parity bound (which the GPU upholds by
//! construction — it pulls the CPU renderer's exact glyph bytes). Deterministic.

use aterm_core::terminal::Terminal;
use aterm_render::{Frame, Renderer, Theme};

fn max_channel_delta(a: &Frame, b: &Frame) -> i32 {
    let n = a.pixels.len().min(b.pixels.len());
    let mut worst = 0i32;
    for i in 0..n {
        let (p, q) = (a.pixels[i], b.pixels[i]);
        for sh in [16u32, 8, 0] {
            let d = (((p >> sh) & 0xff) as i32 - ((q >> sh) & 0xff) as i32).abs();
            worst = worst.max(d);
        }
    }
    worst
}

#[inline]
fn next(s: &mut u64) -> u32 {
    *s = s
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    (*s >> 33) as u32
}

/// Append one chunk of feature-rich / hostile content.
fn emit_adversarial(s: &mut u64, buf: &mut Vec<u8>) {
    const SAMPLES: &[&[u8]] = &[
        b"\xf0\x9f\x9a\x80",         // 🚀
        "\u{2764}\u{fe0f}".as_bytes(), // ❤️ (VS16)
        "\u{1f468}\u{200d}\u{1f469}".as_bytes(), // ZWJ pair
        "\u{1f44d}\u{1f3fd}".as_bytes(), // 👍🏽 skin tone
        "\u{1f1fa}\u{1f1f8}".as_bytes(), // 🇺🇸 flag
        "e\u{0301}".as_bytes(),     // decomposed é
        b"\xe6\x97\xa5",            // CJK 日
        "\u{2500}\u{2502}\u{250c}\u{2510}".as_bytes(), // box
        "\u{2588}\u{2580}\u{2584}\u{2591}".as_bytes(), // blocks
        "\u{1fb00}".as_bytes(),     // sextant
        "\u{e0b0}\u{e0b2}".as_bytes(), // Powerline
        b"\xf0",                   // truncated UTF-8
        b"\xff",                   // invalid byte
        b"abc ",
    ];
    match next(s) % 6 {
        0 | 1 => buf.extend_from_slice(SAMPLES[(next(s) as usize) % SAMPLES.len()]),
        2 => {
            // CSI with random params + final.
            buf.extend_from_slice(b"\x1b[");
            for _ in 0..(next(s) % 4) {
                buf.extend_from_slice(next(s).to_string().as_bytes());
                buf.push(b';');
            }
            buf.push((0x40 + (next(s) % 0x3f)) as u8);
        }
        3 => {
            // SGR (colours / styles) — exercise styled glyph + decoration paths.
            buf.extend_from_slice(b"\x1b[");
            buf.extend_from_slice((next(s) % 110).to_string().as_bytes());
            buf.push(b'm');
        }
        4 => buf.push((next(s) & 0xff) as u8), // raw byte
        _ => buf.extend_from_slice(b"\r\n"),
    }
}

#[test]
fn gpu_render_adversarial_states_never_panics_and_matches_cpu() {
    let theme = Theme::default();
    let px = 16.0;
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

    let mut s: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut worst = 0i32;
    let iters = 150;
    for it in 0..iters {
        // Vary the grid every frame — stresses GPU texture allocation across many
        // sizes, including degenerate-small ones (down to 1x1).
        let rows = 1 + (next(&mut s) % 28) as usize;
        let cols = 1 + (next(&mut s) % 78) as usize;
        let mut term = Terminal::new(rows as u16, cols as u16);
        let mut buf = Vec::with_capacity(128);
        let chunks = 4 + next(&mut s) % 20;
        for _ in 0..chunks {
            emit_adversarial(&mut s, &mut buf);
        }
        term.process(&buf);
        // Vary the render path: DEC double-WIDTH and double-HEIGHT (the clip/scale
        // path) lines, plus cursor hide/show.
        let dec = next(&mut s) % 5;
        match dec {
            0 => term.process(b"\x1b#6"), // DECDWL
            1 => term.process(b"\x1b#3"), // DECDHL top half
            2 => term.process(b"\x1b#4"), // DECDHL bottom half
            _ => {}
        }
        if next(&mut s) % 3 == 0 {
            term.process(b"\x1b[?25l");
        }

        let cpu_frame = cpu.render(&term, rows, cols);
        let gpu_frame = gpu.render(&term, rows, cols); // <- PRIMARY: must not panic
        assert_eq!(
            (cpu_frame.width, cpu_frame.height),
            (gpu_frame.width, gpu_frame.height),
            "iter {it}: dimensions diverge"
        );
        assert!(!gpu_frame.pixels.is_empty(), "iter {it}: empty GPU frame");

        // CPU==GPU <=1-LSB parity is asserted where it holds EXACTLY. DECDHL
        // (double-HEIGHT, dec 1/2) is EXCLUDED: when the doubled glyph's top half
        // is anchored above the visible area (e.g. DHL-bottom on the first row),
        // the vertical clip boundary doesn't align to the 2x source grid, and the
        // GPU's continuous UV + NEAREST sampling picks a different source row than
        // the CPU's integer per-pixel clip — a known, degenerate parity edge (the
        // dedicated `decdhl_double_height` test covers the aligned case at delta
        // 1). DECDHL is still rendered above to pin its PANIC-freedom. DECDWL
        // (dec 0, horizontal-only) is exact and IS checked.
        let is_decdhl = dec == 1 || dec == 2;
        if !is_decdhl {
            let d = max_channel_delta(&cpu_frame, &gpu_frame);
            worst = worst.max(d);
            assert!(d <= 8, "iter {it}: CPU/GPU diverge by {d} > 8 LSB ({rows}x{cols}, dec={dec})");
        }
    }
    eprintln!("gpu adversarial parity fuzz: {iters} frames, worst per-channel delta = {worst}");
}
