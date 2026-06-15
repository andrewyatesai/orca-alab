// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! Procedural box-drawing / block-element / braille glyphs.
//!
//! Fonts draw U+2500–257F (box drawing), U+2580–259F (block elements) and
//! U+2800–28FF (braille) with glyph metrics that rarely fill the cell exactly,
//! leaving hairline gaps or overlaps where strokes should meet across cell
//! boundaries — the classic tmux pane-border / powerline seam artifact. This
//! module synthesizes those glyphs from the cell geometry instead: every
//! bitmap is exactly `cell_w x cell_h`, strokes always reach the cell edge,
//! and coverage is HARD 0/255 (no antialiasing), so the CPU coverage blend and
//! the GPU alpha blend produce EXACTLY the same pixels on these cells
//! (coverage 255 -> pure foreground, coverage 0 -> untouched background).
//!
//! Dispatch lives in [`crate::Renderer::glyph_key`], which routes these ranges
//! to [`crate::FaceId::Procedural`] BEFORE any font lookup. The escape hatch
//! `ATERM_NO_PROCEDURAL_GLYPHS=1` (read at renderer construction, documented
//! with the other font env vars in `lib.rs`) restores font glyphs.
//!
//! ## The shared rounding rule
//!
//! Every stroke is sized and placed by ONE rule, so glyphs in adjacent cells
//! always meet exactly:
//!
//! * `light = max(1, round(min(w, h) / 8))` — in integers, `(min(w, h) + 4) / 8`.
//! * `heavy = 3 * light` — the same parity as `light`, so the heavy span
//!   exactly contains the light span on both axes (a heavy stroke widens a
//!   light one by `light` on each side, never shifting its centre).
//! * a stroke of thickness `t` across an extent `e` covers the half-open span
//!   `[(e - t) / 2, (e - t) / 2 + t)` (integer division: when `e - t` is odd
//!   the extra pixel goes to the right/bottom).
//! * a double line is the heavy span split into rails: its first and last
//!   `light` pixels, leaving a gap that is exactly the light span — so a
//!   single line threading a double junction passes through the gap.
//! * block-element fractions use `eighth(k, e) = (k * e + 4) / 8` (round half
//!   up), each block anchored to its defining edge — complementary halves
//!   (▀/▄, ▌/▐, the quadrants) overlap by one pixel on odd extents rather
//!   than leaving a seam.
//!
//! Within those rules, full coverage of all three blocks: solid, dashed
//! (double/triple/quadruple), rounded arcs (U+256D–2570), diagonals
//! (U+2571–2573), every light/heavy/double junction, eighth blocks, quadrants
//! and braille. The shade characters ░▒▓ (U+2591–2593) are necessarily
//! rendered as 0/255 ordered dithers (25% / 50% checkerboard / 75%) instead of
//! translucent grey, keeping the CPU==GPU exactness guarantee.

/// Whether `ch` is in a range this module draws (box drawing U+2500–257F,
/// block elements U+2580–259F, braille U+2800–28FF, Powerline separators
/// U+E0B0–E0BF — centred solid/outline triangles, rounded half-circles, and the
/// four corner ("angled") triangles + outlines).
pub fn covers(ch: char) -> bool {
    matches!(
        u32::from(ch),
        0x2500..=0x259F | 0x2800..=0x28FF | 0x1FB00..=0x1FB3B | 0xE0B0..=0xE0BF
    )
}

/// The procedural coverage bitmap for `ch` at a `cell_w x cell_h` cell:
/// row-major `cell_w * cell_h` bytes, each EXACTLY 0 or 255. `None` when `ch`
/// is outside the procedural ranges (or the cell is degenerate).
pub fn coverage(ch: char, cell_w: usize, cell_h: usize) -> Option<Vec<u8>> {
    if cell_w == 0 || cell_h == 0 || !covers(ch) {
        return None;
    }
    let cp = u32::from(ch);
    let mut c = Canvas::new(cell_w, cell_h);
    let m = Metrics::new(cell_w, cell_h);
    match cp {
        0x2500..=0x257F => draw_box(&mut c, &m, cp),
        0x2580..=0x259F => draw_block(&mut c, cp),
        0x2800..=0x28FF => draw_braille(&mut c, cp),
        0x1FB00..=0x1FB3B => draw_sextant(&mut c, cp),
        0xE0B0..=0xE0BF => draw_powerline(&mut c, cp),
        _ => unreachable!("covers() gates the ranges"),
    }
    Some(c.buf)
}

/// A hard-coverage canvas: `w * h` bytes, painted 0 or 255 only.
struct Canvas {
    w: usize,
    h: usize,
    buf: Vec<u8>,
}

impl Canvas {
    fn new(w: usize, h: usize) -> Canvas {
        Canvas { w, h, buf: vec![0; w * h] }
    }

    fn set(&mut self, x: usize, y: usize) {
        if x < self.w && y < self.h {
            self.buf[y * self.w + x] = 255;
        }
    }

    /// Fill the half-open rect `[x0, x1) x [y0, y1)`, clamped to the canvas.
    fn rect(&mut self, x0: usize, y0: usize, x1: usize, y1: usize) {
        let (x1, y1) = (x1.min(self.w), y1.min(self.h));
        for y in y0..y1 {
            for x in x0..x1 {
                self.buf[y * self.w + x] = 255;
            }
        }
    }
}

/// A centred stroke of thickness `t` across extent `e`: the half-open span
/// `[(e - t) / 2, (e - t) / 2 + t)`. THE placement rule (see module docs).
fn span(e: usize, t: usize) -> (usize, usize) {
    let t = t.min(e);
    let s = (e - t) / 2;
    (s, s + t)
}

/// `round(k * e / 8)` — the block-element eighth boundary (round half up).
fn eighth(k: u32, e: usize) -> usize {
    (k as usize * e + 4) / 8
}

/// Per-cell stroke geometry, all derived from the module's single rounding
/// rule so every glyph (and every neighbouring cell) agrees on positions.
struct Metrics {
    w: usize,
    h: usize,
    light: usize,
    heavy: usize,
    /// Light vertical stroke columns `[vl0, vl1)`.
    vl0: usize,
    vl1: usize,
    /// Light horizontal stroke rows `[hl0, hl1)`.
    hl0: usize,
    hl1: usize,
    /// Heavy vertical stroke columns `[vh0, vh1)` (also the double-line outer
    /// envelope: rails are its first/last `light` columns).
    vh0: usize,
    vh1: usize,
    /// Heavy horizontal stroke rows `[hh0, hh1)` (double-line envelope too).
    hh0: usize,
    hh1: usize,
}

impl Metrics {
    fn new(w: usize, h: usize) -> Metrics {
        let base = w.min(h);
        let light = ((base + 4) / 8).max(1);
        let heavy = (3 * light).min(base.max(1));
        let (vl0, vl1) = span(w, light);
        let (hl0, hl1) = span(h, light);
        let (vh0, vh1) = span(w, heavy);
        let (hh0, hh1) = span(h, heavy);
        Metrics { w, h, light, heavy, vl0, vl1, hl0, hl1, vh0, vh1, hh0, hh1 }
    }
}

/// One arm of a box-drawing junction: absent, light or heavy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Arm {
    None,
    Light,
    Heavy,
}

impl Arm {
    fn thickness(self, m: &Metrics) -> Option<usize> {
        match self {
            Arm::None => None,
            Arm::Light => Some(m.light),
            Arm::Heavy => Some(m.heavy),
        }
    }
}

const N: Arm = Arm::None;
const L: Arm = Arm::Light;
const H: Arm = Arm::Heavy;

/// `[up, down, left, right]` arm weights for the solid light/heavy glyphs:
/// U+2500–254B (the dashed slots 2504–250B hold their solid equivalents;
/// `draw_box` intercepts those before this table is consulted) and the half
/// lines / weight transitions U+2574–257F.
fn arms(cp: u32) -> [Arm; 4] {
    #[rustfmt::skip]
    const TABLE: [[Arm; 4]; 0x4C] = [
        [N, N, L, L], // 2500 ─
        [N, N, H, H], // 2501 ━
        [L, L, N, N], // 2502 │
        [H, H, N, N], // 2503 ┃
        [N, N, L, L], // 2504 ┄ (dashed; intercepted)
        [N, N, H, H], // 2505 ┅ (dashed; intercepted)
        [L, L, N, N], // 2506 ┆ (dashed; intercepted)
        [H, H, N, N], // 2507 ┇ (dashed; intercepted)
        [N, N, L, L], // 2508 ┈ (dashed; intercepted)
        [N, N, H, H], // 2509 ┉ (dashed; intercepted)
        [L, L, N, N], // 250A ┊ (dashed; intercepted)
        [H, H, N, N], // 250B ┋ (dashed; intercepted)
        [N, L, N, L], // 250C ┌
        [N, L, N, H], // 250D ┍
        [N, H, N, L], // 250E ┎
        [N, H, N, H], // 250F ┏
        [N, L, L, N], // 2510 ┐
        [N, L, H, N], // 2511 ┑
        [N, H, L, N], // 2512 ┒
        [N, H, H, N], // 2513 ┓
        [L, N, N, L], // 2514 └
        [L, N, N, H], // 2515 ┕
        [H, N, N, L], // 2516 ┖
        [H, N, N, H], // 2517 ┗
        [L, N, L, N], // 2518 ┘
        [L, N, H, N], // 2519 ┙
        [H, N, L, N], // 251A ┚
        [H, N, H, N], // 251B ┛
        [L, L, N, L], // 251C ├
        [L, L, N, H], // 251D ┝
        [H, L, N, L], // 251E ┞
        [L, H, N, L], // 251F ┟
        [H, H, N, L], // 2520 ┠
        [H, L, N, H], // 2521 ┡
        [L, H, N, H], // 2522 ┢
        [H, H, N, H], // 2523 ┣
        [L, L, L, N], // 2524 ┤
        [L, L, H, N], // 2525 ┥
        [H, L, L, N], // 2526 ┦
        [L, H, L, N], // 2527 ┧
        [H, H, L, N], // 2528 ┨
        [H, L, H, N], // 2529 ┩
        [L, H, H, N], // 252A ┪
        [H, H, H, N], // 252B ┫
        [N, L, L, L], // 252C ┬
        [N, L, H, L], // 252D ┭
        [N, L, L, H], // 252E ┮
        [N, L, H, H], // 252F ┯
        [N, H, L, L], // 2530 ┰
        [N, H, H, L], // 2531 ┱
        [N, H, L, H], // 2532 ┲
        [N, H, H, H], // 2533 ┳
        [L, N, L, L], // 2534 ┴
        [L, N, H, L], // 2535 ┵
        [L, N, L, H], // 2536 ┶
        [L, N, H, H], // 2537 ┷
        [H, N, L, L], // 2538 ┸
        [H, N, H, L], // 2539 ┹
        [H, N, L, H], // 253A ┺
        [H, N, H, H], // 253B ┻
        [L, L, L, L], // 253C ┼
        [L, L, H, L], // 253D ┽
        [L, L, L, H], // 253E ┾
        [L, L, H, H], // 253F ┿
        [H, L, L, L], // 2540 ╀
        [L, H, L, L], // 2541 ╁
        [H, H, L, L], // 2542 ╂
        [H, L, H, L], // 2543 ╃
        [H, L, L, H], // 2544 ╄
        [L, H, H, L], // 2545 ╅
        [L, H, L, H], // 2546 ╆
        [H, L, H, H], // 2547 ╇
        [L, H, H, H], // 2548 ╈
        [H, H, H, L], // 2549 ╉
        [H, H, L, H], // 254A ╊
        [H, H, H, H], // 254B ╋
    ];
    #[rustfmt::skip]
    const HALF: [[Arm; 4]; 12] = [
        [N, N, L, N], // 2574 ╴
        [L, N, N, N], // 2575 ╵
        [N, N, N, L], // 2576 ╶
        [N, L, N, N], // 2577 ╷
        [N, N, H, N], // 2578 ╸
        [H, N, N, N], // 2579 ╹
        [N, N, N, H], // 257A ╺
        [N, H, N, N], // 257B ╻
        [N, N, L, H], // 257C ╼
        [L, H, N, N], // 257D ╽
        [N, N, H, L], // 257E ╾
        [H, L, N, N], // 257F ╿
    ];
    match cp {
        0x2500..=0x254B => TABLE[(cp - 0x2500) as usize],
        0x2574..=0x257F => HALF[(cp - 0x2574) as usize],
        _ => [N, N, N, N],
    }
}

/// Box drawing U+2500–257F.
fn draw_box(c: &mut Canvas, m: &Metrics, cp: u32) {
    match cp {
        0x2504 => dash_h(c, m, 3, m.light),
        0x2505 => dash_h(c, m, 3, m.heavy),
        0x2506 => dash_v(c, m, 3, m.light),
        0x2507 => dash_v(c, m, 3, m.heavy),
        0x2508 => dash_h(c, m, 4, m.light),
        0x2509 => dash_h(c, m, 4, m.heavy),
        0x250A => dash_v(c, m, 4, m.light),
        0x250B => dash_v(c, m, 4, m.heavy),
        0x254C => dash_h(c, m, 2, m.light),
        0x254D => dash_h(c, m, 2, m.heavy),
        0x254E => dash_v(c, m, 2, m.light),
        0x254F => dash_v(c, m, 2, m.heavy),
        0x2550..=0x256C => draw_double(c, m, cp),
        0x256D..=0x2570 => draw_arc(c, m, cp),
        0x2571 => draw_diag(c, m, true, false),
        0x2572 => draw_diag(c, m, false, true),
        0x2573 => draw_diag(c, m, true, true),
        _ => {
            let [up, down, left, right] = arms(cp);
            draw_arms(c, m, up, down, left, right);
        }
    }
}

/// Solid junctions: each present arm is a stroke from its cell edge through
/// the light centre span, so any combination joins solidly (the heavy span
/// contains the light span, and every arm reaches past the centre).
fn draw_arms(c: &mut Canvas, m: &Metrics, up: Arm, down: Arm, left: Arm, right: Arm) {
    if let Some(t) = up.thickness(m) {
        let (x0, x1) = span(m.w, t);
        c.rect(x0, 0, x1, m.hl1);
    }
    if let Some(t) = down.thickness(m) {
        let (x0, x1) = span(m.w, t);
        c.rect(x0, m.hl0, x1, m.h);
    }
    if let Some(t) = left.thickness(m) {
        let (y0, y1) = span(m.h, t);
        c.rect(0, y0, m.vl1, y1);
    }
    if let Some(t) = right.thickness(m) {
        let (y0, y1) = span(m.h, t);
        c.rect(m.vl0, y0, m.w, y1);
    }
}

/// Horizontal dashed line: `n` dashes, each centred in its `w/n` segment with
/// a `max(1, seg/3)` gap split across the segment ends (so the gaps stay
/// inside the cell — dashes are the one family that must NOT touch the seam).
fn dash_h(c: &mut Canvas, m: &Metrics, n: usize, t: usize) {
    let (y0, y1) = span(m.h, t);
    for i in 0..n {
        let s0 = i * m.w / n;
        let s1 = (i + 1) * m.w / n;
        let seg = s1.saturating_sub(s0);
        if seg == 0 {
            continue;
        }
        let gap = if seg >= 2 { (seg / 3).max(1) } else { 0 };
        let (g0, g1) = (gap / 2, gap - gap / 2);
        c.rect(s0 + g0, y0, s1 - g1, y1);
    }
}

/// Vertical dashed line (see [`dash_h`]).
fn dash_v(c: &mut Canvas, m: &Metrics, n: usize, t: usize) {
    let (x0, x1) = span(m.w, t);
    for i in 0..n {
        let s0 = i * m.h / n;
        let s1 = (i + 1) * m.h / n;
        let seg = s1.saturating_sub(s0);
        if seg == 0 {
            continue;
        }
        let gap = if seg >= 2 { (seg / 3).max(1) } else { 0 };
        let (g0, g1) = (gap / 2, gap - gap / 2);
        c.rect(x0, s0 + g0, x1, s1 - g1);
    }
}

/// Double-line glyphs U+2550–256C. Rails sit at the outer thirds of the heavy
/// envelope (`Metrics` docs); junction shapes follow the Unicode charts: outer
/// rails meet at the outer corner, inner rails at the inner corner, and a
/// branch breaks only the rail it attaches to.
fn draw_double(c: &mut Canvas, m: &Metrics, cp: u32) {
    let t = m.light;
    let (w, h) = (m.w, m.h);
    // Horizontal rail rows: top `[tr0, tr1)`, bottom `[br0, br1)`.
    let (tr0, tr1) = (m.hh0, m.hh0 + t);
    let (br0, br1) = (m.hh1 - t, m.hh1);
    // Vertical rail columns: left `[lr0, lr1)`, right `[rr0, rr1)`.
    let (lr0, lr1) = (m.vh0, m.vh0 + t);
    let (rr0, rr1) = (m.vh1 - t, m.vh1);
    match cp {
        0x2550 => {
            // ═
            c.rect(0, tr0, w, tr1);
            c.rect(0, br0, w, br1);
        }
        0x2551 => {
            // ║
            c.rect(lr0, 0, lr1, h);
            c.rect(rr0, 0, rr1, h);
        }
        0x2552 => {
            // ╒ down single, right double
            c.rect(m.vl0, tr0, w, tr1);
            c.rect(m.vl0, br0, w, br1);
            c.rect(m.vl0, tr0, m.vl1, h);
        }
        0x2553 => {
            // ╓ down double, right single
            c.rect(lr0, m.hl0, w, m.hl1);
            c.rect(lr0, m.hl0, lr1, h);
            c.rect(rr0, m.hl0, rr1, h);
        }
        0x2554 => {
            // ╔
            c.rect(lr0, tr0, w, tr1); // outer top rail
            c.rect(lr0, tr0, lr1, h); // outer left rail
            c.rect(rr0, br0, w, br1); // inner bottom rail
            c.rect(rr0, br0, rr1, h); // inner right rail
        }
        0x2555 => {
            // ╕ down single, left double
            c.rect(0, tr0, m.vl1, tr1);
            c.rect(0, br0, m.vl1, br1);
            c.rect(m.vl0, tr0, m.vl1, h);
        }
        0x2556 => {
            // ╖ down double, left single
            c.rect(0, m.hl0, rr1, m.hl1);
            c.rect(lr0, m.hl0, lr1, h);
            c.rect(rr0, m.hl0, rr1, h);
        }
        0x2557 => {
            // ╗
            c.rect(0, tr0, rr1, tr1); // outer top rail
            c.rect(rr0, tr0, rr1, h); // outer right rail
            c.rect(0, br0, lr1, br1); // inner bottom rail
            c.rect(lr0, br0, lr1, h); // inner left rail
        }
        0x2558 => {
            // ╘ up single, right double
            c.rect(m.vl0, tr0, w, tr1);
            c.rect(m.vl0, br0, w, br1);
            c.rect(m.vl0, 0, m.vl1, br1);
        }
        0x2559 => {
            // ╙ up double, right single
            c.rect(lr0, m.hl0, w, m.hl1);
            c.rect(lr0, 0, lr1, m.hl1);
            c.rect(rr0, 0, rr1, m.hl1);
        }
        0x255A => {
            // ╚
            c.rect(lr0, br0, w, br1); // outer bottom rail
            c.rect(lr0, 0, lr1, br1); // outer left rail
            c.rect(rr0, tr0, w, tr1); // inner top rail
            c.rect(rr0, 0, rr1, tr1); // inner right rail
        }
        0x255B => {
            // ╛ up single, left double
            c.rect(0, tr0, m.vl1, tr1);
            c.rect(0, br0, m.vl1, br1);
            c.rect(m.vl0, 0, m.vl1, br1);
        }
        0x255C => {
            // ╜ up double, left single
            c.rect(0, m.hl0, rr1, m.hl1);
            c.rect(lr0, 0, lr1, m.hl1);
            c.rect(rr0, 0, rr1, m.hl1);
        }
        0x255D => {
            // ╝
            c.rect(0, br0, rr1, br1); // outer bottom rail
            c.rect(rr0, 0, rr1, br1); // outer right rail
            c.rect(0, tr0, lr1, tr1); // inner top rail
            c.rect(lr0, 0, lr1, tr1); // inner left rail
        }
        0x255E => {
            // ╞ vertical single, right double
            c.rect(m.vl0, 0, m.vl1, h);
            c.rect(m.vl0, tr0, w, tr1);
            c.rect(m.vl0, br0, w, br1);
        }
        0x255F => {
            // ╟ vertical double, right single
            c.rect(lr0, 0, lr1, h);
            c.rect(rr0, 0, rr1, h);
            c.rect(rr0, m.hl0, w, m.hl1);
        }
        0x2560 => {
            // ╠
            c.rect(lr0, 0, lr1, h); // left rail, unbroken
            c.rect(rr0, 0, rr1, tr1); // right rail above the branch
            c.rect(rr0, br0, rr1, h); // right rail below the branch
            c.rect(rr0, tr0, w, tr1); // top branch rail
            c.rect(rr0, br0, w, br1); // bottom branch rail
        }
        0x2561 => {
            // ╡ vertical single, left double
            c.rect(m.vl0, 0, m.vl1, h);
            c.rect(0, tr0, m.vl1, tr1);
            c.rect(0, br0, m.vl1, br1);
        }
        0x2562 => {
            // ╢ vertical double, left single
            c.rect(lr0, 0, lr1, h);
            c.rect(rr0, 0, rr1, h);
            c.rect(0, m.hl0, lr1, m.hl1);
        }
        0x2563 => {
            // ╣
            c.rect(rr0, 0, rr1, h); // right rail, unbroken
            c.rect(lr0, 0, lr1, tr1); // left rail above the branch
            c.rect(lr0, br0, lr1, h); // left rail below the branch
            c.rect(0, tr0, lr1, tr1); // top branch rail
            c.rect(0, br0, lr1, br1); // bottom branch rail
        }
        0x2564 => {
            // ╤ down single, horizontal double
            c.rect(0, tr0, w, tr1);
            c.rect(0, br0, w, br1);
            c.rect(m.vl0, br0, m.vl1, h);
        }
        0x2565 => {
            // ╥ down double, horizontal single
            c.rect(0, m.hl0, w, m.hl1);
            c.rect(lr0, m.hl0, lr1, h);
            c.rect(rr0, m.hl0, rr1, h);
        }
        0x2566 => {
            // ╦
            c.rect(0, tr0, w, tr1); // top rail, unbroken
            c.rect(0, br0, lr1, br1); // bottom rail, left piece
            c.rect(rr0, br0, w, br1); // bottom rail, right piece
            c.rect(lr0, br0, lr1, h); // left descender
            c.rect(rr0, br0, rr1, h); // right descender
        }
        0x2567 => {
            // ╧ up single, horizontal double
            c.rect(0, tr0, w, tr1);
            c.rect(0, br0, w, br1);
            c.rect(m.vl0, 0, m.vl1, tr1);
        }
        0x2568 => {
            // ╨ up double, horizontal single
            c.rect(0, m.hl0, w, m.hl1);
            c.rect(lr0, 0, lr1, m.hl1);
            c.rect(rr0, 0, rr1, m.hl1);
        }
        0x2569 => {
            // ╩
            c.rect(0, br0, w, br1); // bottom rail, unbroken
            c.rect(0, tr0, lr1, tr1); // top rail, left piece
            c.rect(rr0, tr0, w, tr1); // top rail, right piece
            c.rect(lr0, 0, lr1, tr1); // left ascender
            c.rect(rr0, 0, rr1, tr1); // right ascender
        }
        0x256A => {
            // ╪ vertical single threads both rails
            c.rect(m.vl0, 0, m.vl1, h);
            c.rect(0, tr0, w, tr1);
            c.rect(0, br0, w, br1);
        }
        0x256B => {
            // ╫ horizontal single threads both rails
            c.rect(0, m.hl0, w, m.hl1);
            c.rect(lr0, 0, lr1, h);
            c.rect(rr0, 0, rr1, h);
        }
        0x256C => {
            // ╬ four corner pieces around an open centre
            c.rect(lr0, 0, lr1, tr1);
            c.rect(0, tr0, lr1, tr1);
            c.rect(rr0, 0, rr1, tr1);
            c.rect(rr0, tr0, w, tr1);
            c.rect(lr0, br0, lr1, h);
            c.rect(0, br0, lr1, br1);
            c.rect(rr0, br0, rr1, h);
            c.rect(rr0, br0, w, br1);
        }
        _ => unreachable!("draw_double covers 0x2550..=0x256C"),
    }
}

/// Light arcs U+256D–2570: the two straight half-arms of the matching corner,
/// joined by a quarter circle whose radius is the largest that fits between
/// the centre cross and the cell edges. The arc band is at least one pixel
/// wide at every angle so the curve stays connected at `light == 1`.
fn draw_arc(c: &mut Canvas, m: &Metrics, cp: u32) {
    let t = m.light as f32;
    let vmid = (m.vl0 + m.vl1) as f32 / 2.0;
    let hmid = (m.hl0 + m.hl1) as f32 / 2.0;
    let (wf, hf) = (m.w as f32, m.h as f32);
    // Arm directions: +1 = right/down. ╭ down+right, ╮ down+left,
    // ╯ up+left, ╰ up+right.
    let (dx, dy): (f32, f32) = match cp {
        0x256D => (1.0, 1.0),
        0x256E => (-1.0, 1.0),
        0x256F => (-1.0, -1.0),
        0x2570 => (1.0, -1.0),
        _ => unreachable!("draw_arc covers 0x256D..=0x2570"),
    };
    let rx = if dx > 0.0 { wf - vmid } else { vmid };
    let ry = if dy > 0.0 { hf - hmid } else { hmid };
    let r = rx.min(ry).max(t);
    // Centre of curvature, displaced from the stroke cross toward the corner
    // the arms point at; the arc joins the vertical stroke at y = cy and the
    // horizontal stroke at x = cx.
    let cx = vmid + dx * r;
    let cy = hmid + dy * r;
    let half = (t / 2.0).max(0.71);
    for y in 0..m.h {
        for x in 0..m.w {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            // Keep only the quarter between the two arm joints.
            if dx * (cx - px) < 0.0 || dy * (cy - py) < 0.0 {
                continue;
            }
            let d = ((px - cx).powi(2) + (py - cy).powi(2)).sqrt();
            if (d - r).abs() <= half {
                c.set(x, y);
            }
        }
    }
    // Straight stubs from the arc joints to the cell edges (the floor/ceil
    // overlaps the arc end by up to a pixel, never gaps).
    if dx > 0.0 {
        c.rect((cx.floor() as usize).min(m.w), m.hl0, m.w, m.hl1);
    } else {
        c.rect(0, m.hl0, (cx.ceil().max(0.0) as usize).min(m.w), m.hl1);
    }
    if dy > 0.0 {
        c.rect(m.vl0, (cy.floor() as usize).min(m.h), m.vl1, m.h);
    } else {
        c.rect(m.vl0, 0, m.vl1, (cy.ceil().max(0.0) as usize).min(m.h));
    }
}

/// Light diagonals U+2571–2573, corner to corner. A pixel is lit when its
/// centre lies within half a light stroke of the ideal line (floored at 0.6
/// so the line stays 8-connected — and meets the cell corners, so diagonals
/// in adjacent cells chain without a break).
fn draw_diag(c: &mut Canvas, m: &Metrics, fwd: bool, back: bool) {
    let (wf, hf) = (m.w as f32, m.h as f32);
    let half = (m.light as f32 / 2.0).max(0.6);
    let norm = (wf * wf + hf * hf).sqrt();
    for y in 0..m.h {
        for x in 0..m.w {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            // ╱ runs (0, h) -> (w, 0): h*x + w*y - w*h = 0.
            let df = (hf * px + wf * py - wf * hf).abs() / norm;
            // ╲ runs (0, 0) -> (w, h): h*x - w*y = 0.
            let db = (hf * px - wf * py).abs() / norm;
            if (fwd && df <= half) || (back && db <= half) {
                c.set(x, y);
            }
        }
    }
}

/// Block elements U+2580–259F.
fn draw_block(c: &mut Canvas, cp: u32) {
    let (w, h) = (c.w, c.h);
    match cp {
        0x2580 => c.rect(0, 0, w, eighth(4, h)), // ▀ upper half
        0x2581..=0x2588 => {
            // ▁▂▃▄▅▆▇█ lower k/8, anchored to the bottom edge
            let k = cp - 0x2580;
            c.rect(0, h - eighth(k, h), w, h);
        }
        0x2589..=0x258F => {
            // ▉▊▋▌▍▎▏ left k/8, anchored to the left edge
            let k = 8 - (cp - 0x2588);
            c.rect(0, 0, eighth(k, w), h);
        }
        0x2590 => c.rect(w - eighth(4, w), 0, w, h), // ▐ right half
        // ░▒▓: 0/255 ordered dithers (25% / 50% / 75%) — see module docs.
        0x2591 => shade(c, 1),
        0x2592 => shade(c, 2),
        0x2593 => shade(c, 3),
        0x2594 => c.rect(0, 0, w, eighth(1, h)), // ▔ upper eighth
        0x2595 => c.rect(w - eighth(1, w), 0, w, h), // ▕ right eighth
        0x2596..=0x259F => quadrants(c, cp),
        _ => unreachable!("draw_block covers 0x2580..=0x259F"),
    }
}

/// The shade dithers: 1 = ░ 25% (even x AND even y), 2 = ▒ 50% checkerboard,
/// 3 = ▓ 75% (the complement of the 25% pattern's odd/odd holes). Cell-local
/// parity, so the pattern is identical in every cell.
fn shade(c: &mut Canvas, level: u8) {
    for y in 0..c.h {
        for x in 0..c.w {
            let on = match level {
                1 => x % 2 == 0 && y % 2 == 0,
                2 => (x + y) % 2 == 0,
                _ => !(x % 2 == 1 && y % 2 == 1),
            };
            if on {
                c.set(x, y);
            }
        }
    }
}

/// Quadrant blocks U+2596–259F: each lit quadrant is a half-by-half rect
/// anchored to its own corner and sized `eighth(4, ..)` (round half up), so
/// unions never leave an interior seam.
fn quadrants(c: &mut Canvas, cp: u32) {
    // bit 0 = upper-left, 1 = upper-right, 2 = lower-left, 3 = lower-right.
    let bits: u8 = match cp {
        0x2596 => 0b0100, // ▖
        0x2597 => 0b1000, // ▗
        0x2598 => 0b0001, // ▘
        0x2599 => 0b1101, // ▙
        0x259A => 0b1001, // ▚
        0x259B => 0b0111, // ▛
        0x259C => 0b1011, // ▜
        0x259D => 0b0010, // ▝
        0x259E => 0b0110, // ▞
        0x259F => 0b1110, // ▟
        _ => unreachable!("quadrants covers 0x2596..=0x259F"),
    };
    let (w, h) = (c.w, c.h);
    let (mw, mh) = (eighth(4, w), eighth(4, h));
    if bits & 0b0001 != 0 {
        c.rect(0, 0, mw, mh);
    }
    if bits & 0b0010 != 0 {
        c.rect(w - mw, 0, w, mh);
    }
    if bits & 0b0100 != 0 {
        c.rect(0, h - mh, mw, h);
    }
    if bits & 0b1000 != 0 {
        c.rect(w - mw, h - mh, w, h);
    }
}

/// Powerline separators U+E0B0–E0B7, synthesized full-bleed so they tile
/// seamlessly with the adjacent segment's background (the glyph paints the
/// segment colour; the rest of the cell shows the next segment's bg):
/// E0B0/E0B2 solid right/left triangles, E0B1/E0B3 their chevron outlines,
/// E0B4/E0B6 solid right/left rounded (half-ellipse) caps, E0B5/E0B7 outlines.
/// Per-row hard fills keep the CPU==GPU exactness guarantee (no AA).
fn draw_powerline(c: &mut Canvas, cp: u32) {
    let (w, h) = (c.w, c.h);
    if w == 0 || h == 0 {
        return;
    }
    let mid = h as f32 / 2.0;
    // Stroke thickness for the outline variants — the box "light" rule.
    let t = ((w.min(h) + 4) / 8).max(1);

    // Corner ("angled") triangles E0B8–E0BF: the cell is split by a diagonal and
    // one corner is filled (odd code points are the hypotenuse outline). `main`
    // is the main-diagonal x at row y (0→w); `anti` the anti-diagonal (w→0).
    if (0xE0B8..=0xE0BF).contains(&cp) {
        for y in 0..h {
            let main = (((y as f32 + 0.5) / h as f32) * w as f32).round() as usize;
            let anti = w - main;
            match cp {
                0xE0B8 => c.rect(0, y, main, y + 1),                       // lower-left solid
                0xE0B9 => c.rect(main.saturating_sub(t), y, main, y + 1),  // lower-left outline
                0xE0BA => c.rect(anti, y, w, y + 1),                       // lower-right solid
                0xE0BB => c.rect(anti, y, (anti + t).min(w), y + 1),       // lower-right outline
                0xE0BC => c.rect(0, y, anti, y + 1),                       // upper-left solid
                0xE0BD => c.rect(anti.saturating_sub(t), y, anti, y + 1),  // upper-left outline
                0xE0BE => c.rect(main, y, w, y + 1),                       // upper-right solid
                0xE0BF => c.rect(main, y, (main + t).min(w), y + 1),       // upper-right outline
                _ => {}
            }
        }
        return;
    }
    // Horizontal extent (0..=w) of the shape at row `y`, measured from the
    // FLAT side (left for right-pointing glyphs): triangle tapers linearly,
    // rounded tapers as a half-ellipse. Both are `w` at the vertical middle
    // and 0 at the top/bottom edges.
    let tri = |y: usize| -> usize {
        let f = (1.0 - ((y as f32 + 0.5) - mid).abs() / mid).max(0.0);
        (f * w as f32).round() as usize
    };
    let round = |y: usize| -> usize {
        let d = ((y as f32 + 0.5) - mid) / mid;
        let f = (1.0 - d * d).max(0.0).sqrt();
        (f * w as f32).round() as usize
    };
    // Apply an extent function as a SOLID fill or an OUTLINE stroke, on the
    // right-pointing (flat side left) or left-pointing (flat side right) layout.
    let edge = match cp {
        0xE0B0 | 0xE0B1 | 0xE0B4 | 0xE0B5 => true,  // right-pointing (apex right)
        _ => false,                                  // left-pointing (apex left)
    };
    let rounded = matches!(cp, 0xE0B4..=0xE0B7);
    let outline = matches!(cp, 0xE0B1 | 0xE0B3 | 0xE0B5 | 0xE0B7);
    for y in 0..h {
        let e = if rounded { round(y) } else { tri(y) };
        if edge {
            // Apex on the right: extent measured rightward from x=0.
            if outline {
                c.rect(e.saturating_sub(t), y, e, y + 1);
            } else {
                c.rect(0, y, e, y + 1);
            }
        } else {
            // Apex on the left: extent measured leftward from x=w.
            let x = w - e;
            if outline {
                c.rect(x, y, (x + t).min(w), y + 1);
            } else {
                c.rect(x, y, w, y + 1);
            }
        }
    }
}

/// Block sextants U+1FB00–1FB3B: a 2×3 grid of filled sub-cells. The 60 code
/// points map to the 6-bit fill masks 1..=62 EXCLUDING 21 (left column) and 42
/// (right column) — those, plus 0 (space) and 63 (full block), have their own
/// characters. Bits: 1=upper-left 2=upper-right 4=mid-left 8=mid-right
/// 16=lower-left 32=lower-right. Sub-cells fully tile the cell (no AA) so the
/// CPU==GPU exactness holds and adjacent sextants seam perfectly.
fn draw_sextant(c: &mut Canvas, cp: u32) {
    let k = cp - 0x1FB00;
    // The k-th mask in 1..=62 skipping the two whole-column masks.
    let mut mask = 0u32;
    let mut idx = 0u32;
    for cand in 1..=62u32 {
        if cand == 21 || cand == 42 {
            continue;
        }
        if idx == k {
            mask = cand;
            break;
        }
        idx += 1;
    }
    let (w, h) = (c.w, c.h);
    let xm = eighth(4, w); // shared half-column boundary (matches ▌/▐)
    let y1 = (h + 1) / 3; // upper/middle split (round)
    let y2 = (2 * h + 1) / 3; // middle/lower split (round)
    if mask & 1 != 0 {
        c.rect(0, 0, xm, y1);
    }
    if mask & 2 != 0 {
        c.rect(xm, 0, w, y1);
    }
    if mask & 4 != 0 {
        c.rect(0, y1, xm, y2);
    }
    if mask & 8 != 0 {
        c.rect(xm, y1, w, y2);
    }
    if mask & 16 != 0 {
        c.rect(0, y2, xm, h);
    }
    if mask & 32 != 0 {
        c.rect(xm, y2, w, h);
    }
}

/// Braille U+2800–28FF: bit `n` of `cp - 0x2800` lights dot `n+1` in the
/// standard 2x4 layout (dots 1-3 left column top-down, 4-6 right column,
/// 7/8 the bottom pair). Each dot is a centred square-ish fill covering about
/// half of its 2x4 grid compartment.
fn draw_braille(c: &mut Canvas, cp: u32) {
    let bits = cp - 0x2800;
    let (w, h) = (c.w, c.h);
    // (column, row) of each dot bit.
    const DOTS: [(usize, usize); 8] =
        [(0, 0), (0, 1), (0, 2), (1, 0), (1, 1), (1, 2), (0, 3), (1, 3)];
    let xb = |i: usize| (i * w).div_ceil(2); // column band boundaries (round half up == div_ceil for /2)
    let yb = |i: usize| (i * h + 2) / 4; // row band boundaries (round half up)
    for (bit, &(col, row)) in DOTS.iter().enumerate() {
        if bits & (1 << bit) == 0 {
            continue;
        }
        let (x0, x1) = (xb(col), xb(col + 1));
        let (y0, y1) = (yb(row), yb(row + 1));
        let (bw, bh) = (x1 - x0, y1 - y0);
        if bw == 0 || bh == 0 {
            continue; // cell too small for this dot's compartment
        }
        let dw = bw.div_ceil(2);
        let dh = bh.div_ceil(2);
        let dx = x0 + (bw - dw) / 2;
        let dy = y0 + (bh - dh) / 2;
        c.rect(dx, dy, dx + dw, dy + dh);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cell sizes exercised by the invariants below: odd/even mixes, squat,
    /// tall, and degenerate-tiny.
    const SIZES: &[(usize, usize)] = &[(1, 1), (2, 2), (3, 7), (7, 15), (8, 16), (9, 19), (10, 20), (11, 21), (12, 22), (20, 8)];

    fn all_procedural_chars() -> impl Iterator<Item = char> {
        (0x2500u32..=0x259F).chain(0x2800..=0x28FF).map(|cp| char::from_u32(cp).unwrap())
    }

    /// THE 0/255 contract: every glyph in all three blocks, at every size, is
    /// exactly cell-sized and contains only hard 0/255 coverage — the property
    /// that makes CPU and GPU blending bit-identical on these cells.
    #[test]
    fn every_glyph_is_cell_sized_hard_coverage() {
        for &(w, h) in SIZES {
            for ch in all_procedural_chars() {
                let cov = coverage(ch, w, h)
                    .unwrap_or_else(|| panic!("{ch:?} must be procedural at {w}x{h}"));
                assert_eq!(cov.len(), w * h, "{ch:?} at {w}x{h}: wrong size");
                assert!(
                    cov.iter().all(|&b| b == 0 || b == 255),
                    "{ch:?} at {w}x{h}: non-hard coverage byte"
                );
            }
        }
    }

    /// Chars outside the three blocks are not intercepted.
    #[test]
    fn non_procedural_chars_are_not_covered() {
        for ch in ['A', ' ', '日', '\u{24FF}', '\u{25A0}', '\u{27FF}', '\u{2900}'] {
            assert!(!covers(ch), "{ch:?} must stay font-rendered");
            assert!(coverage(ch, 8, 16).is_none());
        }
    }

    /// Solid lines reach both cell edges (the seam guarantee in one cell).
    #[test]
    fn solid_lines_touch_their_edges() {
        for &(w, h) in SIZES {
            for ch in ['─', '━'] {
                let cov = coverage(ch, w, h).unwrap();
                let lit_col = |x: usize| (0..h).any(|y| cov[y * w + x] != 0);
                assert!(lit_col(0) && lit_col(w - 1), "{ch:?} at {w}x{h} must span the width");
            }
            for ch in ['│', '┃', '║'] {
                let cov = coverage(ch, w, h).unwrap();
                let lit_row = |y: usize| (0..w).any(|x| cov[y * w + x] != 0);
                assert!(lit_row(0) && lit_row(h - 1), "{ch:?} at {w}x{h} must span the height");
            }
        }
    }

    /// The heavy stroke span exactly contains the light span on both axes —
    /// the parity property the module's rounding rule promises.
    #[test]
    fn heavy_span_contains_light_span_centred() {
        for &(w, h) in SIZES {
            let m = Metrics::new(w, h);
            assert!(m.vh0 <= m.vl0 && m.vl1 <= m.vh1, "{w}x{h}: vertical containment");
            assert!(m.hh0 <= m.hl0 && m.hl1 <= m.hh1, "{w}x{h}: horizontal containment");
            if m.heavy == 3 * m.light {
                assert_eq!(m.vl0 - m.vh0, m.vh1 - m.vl1, "{w}x{h}: vertical centring");
                assert_eq!(m.hl0 - m.hh0, m.hh1 - m.hl1, "{w}x{h}: horizontal centring");
            }
        }
    }

    /// The full block is all-255; the empty braille pattern is all-0.
    #[test]
    fn full_block_and_braille_blank_are_extremes() {
        for &(w, h) in SIZES {
            assert!(coverage('█', w, h).unwrap().iter().all(|&b| b == 255));
            assert!(coverage('\u{2800}', w, h).unwrap().iter().all(|&b| b == 0));
        }
    }

    /// ▀/▄ and ▌/▐ cover the whole cell between them (overlap allowed on odd
    /// extents, gaps never) — the half-block tiling rule.
    #[test]
    fn complementary_halves_leave_no_gap() {
        for &(w, h) in SIZES {
            let top = coverage('▀', w, h).unwrap();
            let bottom = coverage('▄', w, h).unwrap();
            assert!(
                top.iter().zip(&bottom).all(|(&a, &b)| a == 255 || b == 255),
                "{w}x{h}: ▀+▄ must tile the cell"
            );
            let left = coverage('▌', w, h).unwrap();
            let right = coverage('▐', w, h).unwrap();
            assert!(
                left.iter().zip(&right).all(|(&a, &b)| a == 255 || b == 255),
                "{w}x{h}: ▌+▐ must tile the cell"
            );
        }
    }

    /// Braille dots land in their compartments: dot 1 is top-left, dot 8 is
    /// bottom-right, and they never bleed across the column midline.
    #[test]
    fn braille_dot_positions() {
        let (w, h) = (10, 20);
        let mid_x = (w + 1) / 2;
        let d1 = coverage('\u{2801}', w, h).unwrap(); // dot 1: left column, top row
        let d8 = coverage('\u{2880}', w, h).unwrap(); // dot 8: right column, bottom row
        let lit = |cov: &[u8]| {
            (0..h).flat_map(|y| (0..w).map(move |x| (x, y))).filter(|&(x, y)| cov[y * w + x] != 0).collect::<Vec<_>>()
        };
        let l1 = lit(&d1);
        let l8 = lit(&d8);
        assert!(!l1.is_empty() && !l8.is_empty());
        assert!(l1.iter().all(|&(x, y)| x < mid_x && y < h / 4 + 1), "dot 1 confined to top-left");
        assert!(l8.iter().all(|&(x, y)| x >= mid_x && y >= 3 * h / 4 - 1), "dot 8 confined to bottom-right");
    }

    /// The shades dither at their nominal densities (exact for even dims).
    #[test]
    fn shades_have_correct_density() {
        let (w, h) = (8, 16);
        let count = |ch: char| coverage(ch, w, h).unwrap().iter().filter(|&&b| b == 255).count();
        assert_eq!(count('░'), w * h / 4);
        assert_eq!(count('▒'), w * h / 2);
        assert_eq!(count('▓'), 3 * w * h / 4);
    }

    /// Sextants are covered, hard 0/255, and laid out on the 2×3 grid: U+1FB00
    /// fills ONLY the upper-left sub-cell; the next-to-last (mask 62) fills all
    /// but the upper-left. Every one of the 60 draws ink and they are distinct.
    #[test]
    fn sextants_fill_the_2x3_grid() {
        let (w, h) = (12, 24);
        let mut seen = std::collections::HashSet::new();
        for cp in 0x1FB00u32..=0x1FB3B {
            let ch = char::from_u32(cp).unwrap();
            assert!(covers(ch), "U+{cp:04X} covered");
            let cov = coverage(ch, w, h).unwrap();
            assert_eq!(cov.len(), w * h);
            assert!(cov.iter().all(|&b| b == 0 || b == 255), "hard coverage");
            assert!(cov.iter().any(|&b| b == 255), "U+{cp:04X} draws ink");
            assert!(seen.insert(cov.clone()), "U+{cp:04X} duplicates another sextant");
        }
        let at = |cov: &[u8], x: usize, y: usize| cov[y * w + x] == 255;
        let ul = coverage('\u{1FB00}', w, h).unwrap(); // upper-left only
        assert!(at(&ul, 0, 0), "U+1FB00 fills upper-left");
        assert!(!at(&ul, w - 1, 0) && !at(&ul, 0, h - 1), "U+1FB00 only upper-left");
    }

    /// Powerline separators are covered, hard 0/255, and shaped: a SOLID right
    /// triangle (E0B0) is widest at the vertical middle and empty at the edges,
    /// fills the apex column at mid-height, and is the mirror of the solid left
    /// triangle (E0B2). Outlines (E0B1) cover far less than the solid fill.
    #[test]
    fn powerline_separators_are_shaped() {
        let (w, h) = (12, 24);
        for cp in 0xE0B0u32..=0xE0BF {
            let ch = char::from_u32(cp).unwrap();
            assert!(covers(ch), "U+{cp:04X} must be covered");
            let cov = coverage(ch, w, h).unwrap();
            assert_eq!(cov.len(), w * h);
            assert!(cov.iter().all(|&b| b == 0 || b == 255), "hard coverage only");
            assert!(cov.iter().any(|&b| b == 255), "U+{cp:04X} draws ink");
        }
        let at = |cov: &[u8], x: usize, y: usize| cov[y * w + x] == 255;
        // Corner triangles fill the named corner and leave the opposite empty.
        let ll = coverage('\u{E0B8}', w, h).unwrap(); // lower-left
        assert!(at(&ll, 0, h - 1) && !at(&ll, w - 1, 0), "E0B8 fills lower-left");
        let ur = coverage('\u{E0BE}', w, h).unwrap(); // upper-right
        assert!(at(&ur, w - 1, 0) && !at(&ur, 0, h - 1), "E0BE fills upper-right");
        let right = coverage('\u{E0B0}', w, h).unwrap();
        let left = coverage('\u{E0B2}', w, h).unwrap();
        // Apex column lit at mid-height, empty near the top row.
        assert!(at(&right, w - 1, h / 2), "E0B0 apex at right-middle");
        assert!(!at(&right, w - 1, 0), "E0B0 empty at top-right");
        assert!(at(&left, 0, h / 2), "E0B2 apex at left-middle");
        // Mirror image: row by row, right(x) == left(w-1-x).
        for y in 0..h {
            for x in 0..w {
                assert_eq!(at(&right, x, y), at(&left, w - 1 - x, y), "E0B0/E0B2 mirror at ({x},{y})");
            }
        }
        // The outline covers strictly less than the solid fill.
        let ink = |cov: &[u8]| cov.iter().filter(|&&b| b == 255).count();
        assert!(ink(&coverage('\u{E0B1}', w, h).unwrap()) < ink(&right), "outline < solid");
    }
}
