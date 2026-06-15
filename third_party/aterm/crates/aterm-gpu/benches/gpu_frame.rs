// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors
//
// GPU frame-cost benchmark: ms/frame for a full GPU render+readback of a busy
// grid, at a typical terminal size (24x80) and a large one (50x200). This is the
// per-frame GPU rendering cost (atlas build + instances + two passes + readback).
//   cargo bench -p aterm-gpu --bench gpu_frame
//
// Skips cleanly (prints a note, no benchmarks) when there is no GPU/font.

use aterm_core::terminal::Terminal;
use aterm_gpu::GpuRenderer;
use aterm_render::Theme;
use criterion::{black_box, Criterion};

/// A busy grid: every cell filled with cycling text + a few colour runs, so the
/// atlas, instance buffers, and both passes are all exercised.
fn busy_term(rows: usize, cols: usize) -> Terminal {
    let mut term = Terminal::new(rows as u16, cols as u16);
    let alphabet = b"abcdefghijklmnopqrstuvwxyz0123456789 ";
    let mut line = Vec::with_capacity(cols + 16);
    for r in 0..rows {
        line.clear();
        // a colour run at the start of each row, then plain cycling glyphs
        line.extend_from_slice(b"\x1b[3");
        line.push(b'1' + (r % 6) as u8);
        line.push(b'm');
        for c in 0..cols {
            line.push(alphabet[(r + c) % alphabet.len()]);
        }
        line.extend_from_slice(b"\x1b[0m");
        if r + 1 < rows {
            line.extend_from_slice(b"\r\n");
        }
        term.process(&line);
    }
    term
}

fn main() {
    let theme = Theme::default();
    let mut gpu = match GpuRenderer::new(16.0, theme) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("SKIP bench: no GPU/font available: {e}");
            return;
        }
    };
    let (name, backend) = gpu.adapter();
    eprintln!("GPU: {name} (backend {backend})");

    let mut c = Criterion::default().configure_from_args();
    for (rows, cols) in [(24usize, 80usize), (50usize, 200usize)] {
        let term = busy_term(rows, cols);
        c.bench_function(&format!("gpu_frame_{rows}x{cols}"), |b| {
            b.iter(|| {
                let f = gpu.render(&term, rows, cols);
                black_box(f);
            });
        });
    }
    c.final_summary();
}
