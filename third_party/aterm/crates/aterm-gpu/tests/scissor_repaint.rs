// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors
//
// GPU SCISSORED DIRTY-ROW REPAINT — the byte-identity gate.
//
// `GpuRenderer::present_input` re-encodes ONLY the rows that differ from the
// previous presented frame: LoadOp::Load over the persistent offscreen (which
// still holds the prior frame), a scissor over the dirty rows' bounding band,
// and instances built for the dirty rows only. The dirty set is the SHARED
// `aterm_render::compute_dirty_rows` — the SAME one the CPU damage path uses, so
// the GPU and CPU cannot diverge. The hard contract: the scissored offscreen
// must be BYTE-IDENTICAL to a fresh full GPU render of the same input.
//
// This test drives ONE reused GpuRenderer at FIXED dims through a multi-frame
// sequence (prompt, single-keystroke typing, blink toggles, cursor moves, wide
// CJK, combining marks, DECDWL, DECDHL, selection set/clear, scrollback scroll,
// full-screen TUI repaint). After EACH frame it reads back the scissored
// offscreen (`present_input_readback`, the exact present-path encode + a
// readback) and asserts the pixels `==` a FRESH full GPU render of that input on
// a SEPARATE GpuRenderer (the oracle pattern from `dirty_gate.rs`). It also
// asserts, via the scissor/full counters, that:
//   * the scissor path is ACTUALLY taken on the typing/cursor/wide/combining/
//     DECDWL frames (the optimisation is exercised, not silently dead), and
//   * the DECDHL / selection / scroll frames correctly FELL BACK to full repaint
//     (the conservative always-correct path), so a re-shaded double-height /
//     scrollback / selection frame can never leak a seam.
//
// Gated: no GPU or no system font ⇒ the test no-ops (returns).

use std::time::Instant;

use aterm_core::selection::{SelectionSide, SelectionType};
use aterm_core::terminal::{CursorStyle, Terminal};
use aterm_gpu::GpuRenderer;
use aterm_render::{RenderInput, Theme};

const ROWS: usize = 10;
const COLS: usize = 32;

/// A fresh GpuRenderer (or skip-marker) at the suite's standard px/theme.
fn fresh_gpu() -> Option<GpuRenderer> {
    match GpuRenderer::new(18.0, Theme::default()) {
        Ok(g) => Some(g),
        Err(e) => {
            eprintln!("SKIP: no GPU/font available: {e}");
            None
        }
    }
}

/// The ground truth: a brand-new GpuRenderer renders `input` with a FULL repaint
/// (Clear + every row) and reads it back. No scissor, no prior frame — exactly
/// the pixels the scissored offscreen must match.
fn fresh_render(input: &RenderInput, blink: bool, override_: Option<CursorStyle>) -> Vec<u32> {
    let mut g = fresh_gpu().expect("GPU was available a moment ago");
    g.set_cursor_blink_phase(blink);
    g.set_cursor_style_override(override_);
    g.render_input(input).pixels
}

/// What a step expects of the repaint path for THAT frame.
#[derive(Clone, Copy, PartialEq)]
enum Path {
    /// Must take the SCISSORED dirty-row path.
    Scissor,
    /// Must FALL BACK to a full Clear+all-rows repaint.
    Full,
    /// Don't assert the path (e.g. the first frame, or a blink toggle whose
    /// hit/miss depends on the terminal's DECSCUSR default).
    Any,
}

struct Step {
    desc: &'static str,
    act: Box<dyn Fn(&mut Terminal)>,
    blink: bool,
    override_: Option<CursorStyle>,
    path: Path,
}

fn step(
    desc: &'static str,
    act: impl Fn(&mut Terminal) + 'static,
    blink: bool,
    override_: Option<CursorStyle>,
    path: Path,
) -> Step {
    Step { desc, act: Box::new(act), blink, override_, path }
}

#[test]
fn gpu_scissor_repaint_byte_identical() {
    let Some(mut gpu) = fresh_gpu() else { return };

    let mut term = Terminal::new(ROWS as u16, COLS as u16);

    let steps = vec![
        // 1. First paint: a shell prompt. First frame always FULL (no prior).
        step("prompt", |t| t.process(b"$ "), true, None, Path::Full),
        // 2. Re-present the SAME frame — reusable, zero dirty rows ⇒ scissor path
        //    (Load + empty band; preserves the prior frame).
        step("idle (unchanged)", |_| {}, true, None, Path::Scissor),
        // 3-7. Single-keystroke typing — each changes ONE row ⇒ SCISSOR.
        step("type 'l'", |t| t.process(b"l"), true, None, Path::Scissor),
        step("type 's'", |t| t.process(b"s"), true, None, Path::Scissor),
        step("type ' '", |t| t.process(b" "), true, None, Path::Scissor),
        step("type '-'", |t| t.process(b"-"), true, None, Path::Scissor),
        step("type 'a'", |t| t.process(b"a"), true, None, Path::Scissor),
        // 8. Idle after typing — scissor (zero dirty rows).
        step("idle after type", |_| {}, true, None, Path::Scissor),
        // 9. Blink toggle (no content change). Don't assert path: whether the
        //    cursor's shown-ness flips depends on the DECSCUSR style. Always
        //    byte-identical though.
        step("blink off", |_| {}, false, None, Path::Any),
        step("blink on", |_| {}, true, None, Path::Any),
        // 11. Newline then a cursor MOVE without content change (CUP) — the old +
        //     new cursor rows are dirty ⇒ scissor.
        step("newline + text", |t| t.process(b"\r\nrow two text"), true, None, Path::Scissor),
        step("cursor home", |t| t.process(b"\x1b[1;1H"), true, None, Path::Scissor),
        step("cursor r3c5", |t| t.process(b"\x1b[3;5H"), true, None, Path::Scissor),
        // 14. Wide CJK on a fresh row — changes one row ⇒ scissor.
        step("wide cjk", |t| t.process("\x1b[4;1H日本語".as_bytes()), true, None, Path::Scissor),
        // 15. Combining mark (é = e + U+0301) — changes one row ⇒ scissor.
        step("combining é", |t| t.process("\x1b[5;1He\u{0301}".as_bytes()), true, None, Path::Scissor),
        // 16. DECDWL double-WIDTH row. Double-width stays within ONE row band, so
        //     it is REUSABLE — the changed row is dirty ⇒ scissor (byte-identical).
        step("decdwl", |t| t.process(b"\x1b[6;1H\x1b#6WIDE"), true, None, Path::Scissor),
        // 17. DECDHL double-HEIGHT row. A DECDHL glyph spans TWO row bands, so the
        //     whole frame is NOT reusable ⇒ FULL repaint (the seam-safe fallback).
        step("decdhl top", |t| t.process(b"\x1b[7;1H\x1b#3TALL"), true, None, Path::Full),
        // 18. Still double-height present ⇒ idle also FULL (no per-row reuse while
        //     a double-height row exists in either frame).
        step("idle with decdhl", |_| {}, true, None, Path::Full),
        // 19. Clear the double-height: rewrite row 7 single-size. Prior frame had a
        //     double-height row ⇒ NOT reusable ⇒ FULL.
        step("clear decdhl", |t| t.process(b"\x1b[7;1H\x1b#5plain   "), true, None, Path::Full),
        // 20. Idle now (no double-height anywhere) ⇒ scissor again.
        step("idle no decdhl", |_| {}, true, None, Path::Scissor),
        // 21. Style override (unfocused HollowBlock) — cursor style changes; the
        //     cursor row is dirty ⇒ scissor (byte-identical).
        step("focus lost", |_| {}, true, Some(CursorStyle::HollowBlock), Path::Scissor),
        step("focus gained", |_| {}, true, None, Path::Scissor),
        // 23. Selection set — frame-global change ⇒ FULL fallback.
        step(
            "select",
            |t| {
                let sel = t.text_selection_mut();
                sel.start_selection(2, 0, SelectionSide::Left, SelectionType::Simple);
                sel.update_selection(2, 6, SelectionSide::Right);
                sel.complete_selection();
            },
            true,
            None,
            Path::Full,
        ),
        // 24. Idle WITH a selection — selection unchanged, reusable ⇒ scissor.
        step("idle selected", |_| {}, true, None, Path::Scissor),
        // 25. Clear selection — frame-global change ⇒ FULL.
        step("clear selection", |t| t.text_selection_mut().clear(), true, None, Path::Full),
        // 26. Generate scrollback so there is history to scroll into.
        step(
            "run output",
            |t| {
                for n in 0..30 {
                    t.process(format!("\r\nfile{n} contents here").as_bytes());
                }
            },
            true,
            None,
            Path::Any,
        ),
        // 27. Scroll back into history — display_offset changes ⇒ FULL fallback.
        step("scroll back", |t| t.scroll_display(3), true, None, Path::Full),
        // 28. Idle scrolled — offset unchanged ⇒ scissor.
        step("idle scrolled", |_| {}, true, None, Path::Scissor),
        // 29. Scroll to bottom — offset changes ⇒ FULL.
        step("scroll to bottom", |t| t.scroll_to_bottom(), true, None, Path::Full),
        // 30. Full-screen TUI repaint (clear + redraw): MANY rows change at once.
        //     Reusable (same dims/offset/selection, no double-height) ⇒ scissor
        //     over the (large) dirty band — still byte-identical.
        step(
            "full tui repaint",
            |t| {
                t.process(b"\x1b[2J\x1b[H");
                for r in 0..ROWS {
                    t.process(format!("\x1b[{};1Hline {r:02} ::::::::::::::::::", r + 1).as_bytes());
                }
            },
            true,
            None,
            Path::Scissor,
        ),
        // 31. Idle on the TUI screen ⇒ scissor.
        step("idle tui", |_| {}, true, None, Path::Scissor),
        // 32. One keystroke on the TUI ⇒ scissor (one row).
        step("type on tui", |t| t.process(b"\x1b[1;1HX"), true, None, Path::Scissor),
    ];

    let mut scissor_seen = 0u64;
    let mut full_seen = 0u64;

    for (i, s) in steps.iter().enumerate() {
        (s.act)(&mut term);
        gpu.set_cursor_blink_phase(s.blink);
        gpu.set_cursor_style_override(s.override_);

        let input = GpuRenderer::extract(&term, ROWS, COLS);

        let scissor_before = gpu.scissor_taken();
        let full_before = gpu.full_repaints();

        // The scissored present-path encode + readback (the path under test).
        let got = gpu.present_input_readback(&input).pixels;

        let took_scissor = gpu.scissor_taken() > scissor_before;
        let took_full = gpu.full_repaints() > full_before;
        assert!(
            took_scissor ^ took_full,
            "step {i} ({}): exactly one of scissor/full must be taken",
            s.desc
        );

        // (a) BYTE-IDENTITY — the cardinal contract. Whether this frame scissored
        // or fell back, the offscreen pixels MUST equal a fresh full GPU render of
        // the same input + cursor state. On a scissor this proves the dirty band
        // is bit-identical AND the untouched rows were preserved verbatim.
        let oracle = fresh_render(&input, s.blink, s.override_);
        assert_eq!(got.len(), oracle.len(), "step {i} ({}): pixel count differs", s.desc);
        assert!(
            got == oracle,
            "step {i} ({}): {} pixels are NOT byte-identical to a fresh GPU render",
            s.desc,
            if took_scissor { "SCISSORED" } else { "full-repaint" },
        );

        // (b) the path must be what the step declares.
        match s.path {
            Path::Scissor => assert!(
                took_scissor,
                "step {i} ({}): expected the SCISSOR path but it fell back to full",
                s.desc
            ),
            Path::Full => assert!(
                took_full,
                "step {i} ({}): expected a FULL repaint but it took the scissor",
                s.desc
            ),
            Path::Any => {}
        }

        if took_scissor {
            scissor_seen += 1;
        } else {
            full_seen += 1;
        }
        eprintln!(
            "step {i:2} {:<20} path={} (scissor={}, full={})",
            s.desc,
            if took_scissor { "SCISSOR" } else { "FULL   " },
            gpu.scissor_taken(),
            gpu.full_repaints(),
        );
    }

    // The optimisation must be EXERCISED (many scissor frames) and the fallback
    // must be REACHED (DECDHL / selection / scroll).
    assert!(scissor_seen >= 10, "scissor path barely fired ({scissor_seen}) — not exercised");
    assert!(full_seen >= 4, "full-repaint fallback barely fired ({full_seen}) — not exercised");
    assert_eq!(
        gpu.scissor_taken() + gpu.full_repaints(),
        steps.len() as u64,
        "every frame must be exactly one of scissor/full",
    );
    eprintln!("scissor-repaint: {scissor_seen} scissor frames, {full_seen} full repaints");
}

/// A scissored frame immediately followed by a ONE-CELL change must repaint
/// correctly: the dirty row is re-encoded over the preserved prior frame and the
/// result matches a fresh render of the CHANGED input — i.e. the offscreen does
/// not "stick" on the stale prior pixels, and untouched rows are not corrupted.
#[test]
fn gpu_scissor_one_cell_change_preserves_other_rows() {
    let Some(mut gpu) = fresh_gpu() else { return };

    let mut term = Terminal::new(ROWS as u16, COLS as u16);
    // Two rows of content so we can prove the UNCHANGED row survives a scissor.
    term.process(b"first row\r\nsecond row");
    gpu.set_cursor_blink_phase(true);
    gpu.set_cursor_style_override(None);

    // Frame 1: first paint — FULL (no prior frame).
    let in1 = GpuRenderer::extract(&term, ROWS, COLS);
    let _ = gpu.present_input_readback(&in1);
    assert_eq!(gpu.full_repaints(), 1, "first frame must be a full repaint");
    assert_eq!(gpu.scissor_taken(), 0);

    // Frame 2: change ONE cell on row 0 ('first' → 'First'). Must SCISSOR and
    // match a fresh render of the changed input — not the stale frame.
    term.process(b"\x1b[1;1HF");
    let in2 = GpuRenderer::extract(&term, ROWS, COLS);
    let scissor_before = gpu.scissor_taken();
    let got2 = gpu.present_input_readback(&in2).pixels;
    assert!(gpu.scissor_taken() > scissor_before, "one-cell change must take the scissor");
    let oracle2 = fresh_render(&in2, true, None);
    assert!(got2 == oracle2, "scissored one-cell change diverges from a fresh render");

    // Row 1 ("second row") was NOT dirty: prove its pixels survived the scissor
    // by checking they equal the fresh render's row-1 band exactly (they do, since
    // the whole frame matched — but assert the band explicitly for clarity).
    let (cw, ch) = gpu.cell_size();
    let w = COLS * cw;
    let band1 = (ch * w)..(2 * ch * w);
    assert!(
        got2[band1.clone()] == oracle2[band1],
        "the untouched row-1 band was corrupted by the scissored repaint",
    );

    // Frame 3: idle — zero dirty rows ⇒ scissor, byte-identical, re-presents the
    // exact prior frame.
    let in3 = GpuRenderer::extract(&term, ROWS, COLS);
    let got3 = gpu.present_input_readback(&in3).pixels;
    assert!(got3 == got2, "idle scissor frame must re-present the prior frame verbatim");
    assert!(got3 == fresh_render(&in3, true, None), "idle scissor diverges from a fresh render");
}

/// Diagnostic (run with `--ignored --nocapture`): the changed-frame GPU
/// encode/instance-build cost for a 1-ROW change at 50x200 via the SCISSORED
/// present path vs a FULL repaint of the same frame. Both read the whole texture
/// back (constant cost), so the delta is the scissor's encode/fill saving. Not an
/// assertion — prints the reduction.
#[test]
#[ignore = "diagnostic benchmark; run with --ignored --nocapture"]
fn gpu_scissor_changed_frame_cost() {
    let Some(mut gpu) = fresh_gpu() else { return };
    let (rows, cols) = (50usize, 200usize);
    let mut term = Terminal::new(rows as u16, cols as u16);
    // Fill every row so a full repaint is non-trivial.
    for r in 0..rows {
        term.process(format!("\x1b[{};1Hline {r:02} ", r + 1).as_bytes());
        term.process(b"the quick brown fox jumps over the lazy dog 0123456789 abcdef");
    }
    gpu.set_cursor_blink_phase(true);
    gpu.set_cursor_style_override(None);

    // Prime the present path (first frame is a full repaint, fills present_prev).
    let in0 = GpuRenderer::extract(&term, rows, cols);
    gpu.present_encode_poll(&in0);

    const N: u32 = 500;

    // Measure the ENCODE + instance-build + GPU fill only (no readback — it is
    // scope-independent and would swamp the scissor's saving).
    //
    // SCISSORED 1-row change: toggle a single char on row 0 each iter — exactly
    // one dirty row ⇒ a one-band scissor + one row's instances.
    let scissor_before = gpu.scissor_taken();
    let t = Instant::now();
    for i in 0..N {
        let ch = if i % 2 == 0 { b'A' } else { b'B' };
        term.process(b"\x1b[1;1H");
        term.process(&[ch]);
        let input = GpuRenderer::extract(&term, rows, cols);
        gpu.present_encode_poll(&input);
    }
    let scissor_us = t.elapsed().as_secs_f64() * 1e6 / f64::from(N);
    let scissor_inst = gpu.last_instances();
    assert_eq!(gpu.scissor_taken() - scissor_before, u64::from(N), "all iters should scissor");

    // FULL repaint of the SAME 1-row-change frames on a SEPARATE renderer. Toggle
    // the display_offset every frame so `compute_dirty_rows` returns FullRepaint
    // (a scrollback change is never reusable) — this forces the full Clear+all-
    // rows encode for the SAME screen, isolating the repaint scope.
    let mut full = fresh_gpu().expect("GPU available");
    full.set_cursor_blink_phase(true);
    let mut input_a = GpuRenderer::extract(&term, rows, cols);
    let mut input_b = input_a.clone();
    input_b.display_offset = 1; // a different offset ⇒ forced full repaint
    // Prime with B so the loop's first frame (A) already differs ⇒ every
    // strictly-alternating frame's offset differs from the prior ⇒ all full.
    full.present_encode_poll(&input_b);
    let full_before = full.full_repaints();
    let t = Instant::now();
    for i in 0..N {
        let input = if i % 2 == 0 { &input_a } else { &input_b };
        full.present_encode_poll(input);
    }
    let full_us = t.elapsed().as_secs_f64() * 1e6 / f64::from(N);
    let full_inst = full.last_instances();
    std::hint::black_box((&mut input_a, &mut input_b));
    assert_eq!(full.full_repaints() - full_before, u64::from(N), "all iters should full-repaint");

    eprintln!(
        "1-row change @ {rows}x{cols} (encode only, no readback): \
         SCISSOR present = {scissor_us:.1} us/frame, \
         FULL repaint = {full_us:.1} us/frame, reduction = {:.2}x",
        full_us / scissor_us.max(0.0001),
    );
    eprintln!(
        "instances built: SCISSOR (1 dirty row) = {scissor_inst}, FULL ({rows} rows) = {full_inst}, \
         reduction = {:.1}x",
        full_inst as f64 / scissor_inst.max(1) as f64,
    );
}
