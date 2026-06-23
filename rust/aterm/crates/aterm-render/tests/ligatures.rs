// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors
//
// Programming-ligature gate for the CPU renderer: with a ligature font
// (JetBrains Mono, bundled with Orca), rendering "a => b != c" must produce
// DIFFERENT ink than the same text with ligatures disabled (proving the shaped
// ligature glyph is actually used), AND a control row of "a = > b" (operators
// separated by spaces, so no run forms) must render IDENTICALLY with ligatures
// on or off (proving the run coalescing breaks on spaces and does not corrupt
// non-ligature text).

use aterm_core::selection::{SelectionSide, SelectionType};
use aterm_core::terminal::Terminal;
use aterm_render::{LigatureMode, Renderer, TextShapingConfig, Theme};

// Layout-independent ligature font discovery. Order: (a) $ATERM_FONT if set and
// readable; (b) the committed repo fixture (sibling of this test, present in both
// canonical and vendored layouts). Returns None -> the test SKIPs cleanly rather
// than panicking, so a host without the fixture never breaks the suite.
fn ligature_test_font() -> Option<Vec<u8>> {
    if let Ok(path) = std::env::var("ATERM_FONT") {
        if let Ok(bytes) = std::fs::read(&path) {
            return Some(bytes);
        }
    }
    const FIXTURE: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/jetbrains-mono.ttf"
    );
    std::fs::read(FIXTURE).ok()
}

fn renderer(mode: LigatureMode) -> Option<Renderer> {
    let bytes = ligature_test_font()?;
    let mut r = Renderer::from_bytes(&bytes, 18.0, Theme::default()).ok()?;
    r.set_text_shaping(TextShapingConfig {
        ligature_mode: mode,
        ..Default::default()
    });
    Some(r)
}

fn render(mode: LigatureMode, text: &[u8]) -> Option<aterm_render::Frame> {
    let mut r = renderer(mode)?;
    let (rows, cols) = (1usize, 16usize);
    let mut term = Terminal::new(rows as u16, cols as u16);
    // Hide the cursor so the cursor overlay never perturbs the comparison.
    term.process(b"\x1b[?25l");
    term.process(text);
    let input = term.cell_frame(rows, cols);
    Some(r.render_input(&input))
}

/// PRE-CHECK (documents the shaping mechanism): a standalone rustybuzz shape of
/// "=>" with liga+calt yields the SAME glyph COUNT as input chars for this
/// monospace-preserving font (it uses empty placeholder glyphs, not collapsing),
/// but the glyph IDS differ from the plain cmap glyphs — i.e. a ligature exists.
#[test]
fn rustybuzz_shapes_arrow_ligature() {
    let Some(bytes) = ligature_test_font() else {
        eprintln!("SKIP: no ligature test font (set ATERM_FONT or add the repo fixture)");
        return;
    };
    let shaped = aterm_render::ligature_shaping::shape_ligature_run(&bytes, "=>", &['=', '>'], true);
    assert!(
        shaped.is_some(),
        "=> must shape to a ligature (glyph ids differ from plain cmap)"
    );
    let gids = shaped.unwrap();
    assert_eq!(gids.len(), 2, "monospace-preserving font keeps one glyph per cell");
    // The lead cell is the empty placeholder, the final cell the wide ligature
    // glyph — at least one differs from the plain '=' / '>' cmap glyph.
    let face = rustybuzz::Face::from_slice(&bytes, 0).unwrap();
    let cmap_eq = face.glyph_index('=').unwrap().0;
    let cmap_gt = face.glyph_index('>').unwrap().0;
    assert!(
        gids[0] != cmap_eq || gids[1] != cmap_gt,
        "shaped ids {gids:?} must differ from plain cmap [{cmap_eq}, {cmap_gt}]"
    );
}

/// The headline gate: ligated "a => b != c" inks differently than the same text
/// with ligatures off. If the renderer ignored shaping, the two frames would be
/// byte-identical — so a difference proves the ligature glyph reached the pixels.
#[test]
fn ligated_render_differs_from_unligated() {
    let text = b"a => b != c";
    let (Some(on), Some(off)) = (
        render(LigatureMode::Enabled, text),
        render(LigatureMode::Disabled, text),
    ) else {
        eprintln!("SKIP: no ligature test font (set ATERM_FONT or add the repo fixture)");
        return;
    };
    assert_eq!(
        (on.width, on.height),
        (off.width, off.height),
        "ligature mode must not change frame dimensions"
    );
    assert_ne!(
        on.pixels, off.pixels,
        "ligated 'a => b != c' must ink differently than the unligated render \
         — the ligature glyph was not used"
    );
}

/// Control: "a = > b" has the operators separated by spaces, so NO ligature run
/// forms (spaces break the run). The frame must be byte-IDENTICAL with ligatures
/// on or off — the run coalescing must not perturb ordinary spaced text.
#[test]
fn spaced_operators_render_identically() {
    let text = b"a = > b";
    let (Some(on), Some(off)) = (
        render(LigatureMode::Enabled, text),
        render(LigatureMode::Disabled, text),
    ) else {
        eprintln!("SKIP: no ligature test font (set ATERM_FONT or add the repo fixture)");
        return;
    };
    assert_eq!(
        on.pixels, off.pixels,
        "spaced operators 'a = > b' must render identically on/off — no run should ligate"
    );
}

/// Render `text` under ligature `mode`, after applying a Simple selection over
/// columns `[sel_start, sel_end]` inclusive on the (only) live row. Returns the
/// frame, or None if the fixture font is absent (SKIP). The selection is driven
/// through the public selection API the way the GUI does, then snapshotted via
/// `cell_frame` so it lands in `RenderInput.selection`.
fn render_with_selection(
    mode: LigatureMode,
    text: &[u8],
    sel_start: u16,
    sel_end: u16,
) -> Option<aterm_render::Frame> {
    let mut r = renderer(mode)?;
    let (rows, cols) = (1usize, 16usize);
    let mut term = Terminal::new(rows as u16, cols as u16);
    term.process(b"\x1b[?25l");
    term.process(text);
    // Cover exactly columns [sel_start, sel_end]: a Left-sided start at sel_start
    // and a Right-sided end at sel_end include both endpoints (see side adjustment).
    let sel = term.text_selection_mut();
    sel.start_selection(0, sel_start, SelectionSide::Left, SelectionType::Simple);
    sel.update_selection(0, sel_end, SelectionSide::Right);
    sel.complete_selection();
    let input = term.cell_frame(rows, cols);
    Some(r.render_input(&input))
}

/// The pixels of column `c`'s vertical band (the whole cell height) in a 1-row,
/// `cols`-column frame. Used to inspect a single cell's background/ink.
fn column_band(frame: &aterm_render::Frame, c: usize, cols: usize) -> Vec<u32> {
    let cw = frame.width / cols;
    let x0 = c * cw;
    let mut out = Vec::with_capacity(cw * frame.height);
    for y in 0..frame.height {
        let row = y * frame.width;
        out.extend_from_slice(&frame.pixels[row + x0..row + x0 + cw]);
    }
    out
}

/// BLOCKER regression: a ligature spanning a SELECTION boundary must render
/// per-cell, not as one glyph painted across the boundary. With a selection
/// covering only the FIRST cell of a `=>` run, the frame must (a) differ from the
/// no-selection ligated render, and (b) show the selection background in that
/// selected cell — proving the ligature did NOT paint across the boundary and the
/// selected cell kept its per-cell highlight.
#[test]
fn ligature_breaks_on_selection_boundary() {
    // "a=>b": '=' at col 1, '>' at col 2 form the arrow run; select only col 1.
    let text = b"a=>b";
    let (Some(no_sel), Some(sel_on), Some(sel_off)) = (
        render(LigatureMode::Enabled, text),
        render_with_selection(LigatureMode::Enabled, text, 1, 1),
        render_with_selection(LigatureMode::Disabled, text, 1, 1),
    ) else {
        eprintln!("SKIP: no ligature test font (set ATERM_FONT or add the repo fixture)");
        return;
    };
    assert_eq!(
        (no_sel.width, no_sel.height),
        (sel_on.width, sel_on.height),
        "a selection must not change frame dimensions"
    );
    // (a) The selection-broken render must differ from the fully-ligated render:
    // breaking the run on the selection boundary changes the ink (and the bg).
    assert_ne!(
        no_sel.pixels, sel_on.pixels,
        "selecting half of '=>' must change the render — the ligature spanned the \
         selection boundary instead of breaking on it"
    );
    // (b) The selected cell (col 1) must carry the theme selection background,
    // proving the highlight is painted there per-cell.
    let theme_sel = Theme::default().selection;
    let band = column_band(&sel_on, 1, 16);
    assert!(
        band.iter().any(|&p| p == theme_sel),
        "selected cell of '=>' must show the selection background (0x{theme_sel:06X})"
    );
    // (c) The discriminating gate: with the selection breaking the run, the
    // selected cell (col 1) must render the PER-CELL '=' glyph — byte-identical to
    // the SAME selected render with ligatures OFF (which also draws a per-cell
    // '='). If the ligature had spanned the boundary, col 1 would instead show the
    // ligature's empty PLACEHOLDER glyph, differing from the ligatures-off render.
    assert_eq!(
        column_band(&sel_on, 1, 16),
        column_band(&sel_off, 1, 16),
        "selected cell must render per-cell (== ligatures-off) — the ligature \
         painted its placeholder across the selection boundary instead of breaking"
    );
}

/// A ligature run must BREAK on an SGR STYLE change: in `a=>` where `=>` is bold
/// vs not, the two ligated renders must differ; and a style change mid-operator
/// ('=' plain, '>' bold) must prevent a single ligature spanning the style
/// boundary — so that render differs from the uniform-bold one too.
#[test]
fn ligature_breaks_on_style_change() {
    // Uniform plain "=>", uniform bold "=>", and a SPLIT "=>" ('=' plain, '>' bold).
    let (Some(plain), Some(bold), Some(split)) = (
        render(LigatureMode::Enabled, b"a\x1b[0m=>"),
        render(LigatureMode::Enabled, b"a\x1b[1m=>\x1b[0m"),
        render(LigatureMode::Enabled, b"a\x1b[0m=\x1b[1m>\x1b[0m"),
    ) else {
        eprintln!("SKIP: no ligature test font (set ATERM_FONT or add the repo fixture)");
        return;
    };
    // Bold weight differs from plain (synthetic embolden widens strokes).
    assert_ne!(
        plain.pixels, bold.pixels,
        "bold '=>' must ink differently than plain '=>'"
    );
    // The mid-operator style change splits the run into two single shapeable
    // cells, neither of which can ligate alone — so '=' renders as a plain '='
    // glyph (plain weight) and '>' as a plain bold '>' glyph, NOT the arrow.
    // That cannot equal either uniform-style ligated render.
    assert_ne!(
        split.pixels, plain.pixels,
        "a style change mid-'=>' must prevent the all-plain arrow ligature"
    );
    assert_ne!(
        split.pixels, bold.pixels,
        "a style change mid-'=>' must prevent the all-bold arrow ligature"
    );
}

/// A run must BREAK on a WIDE/emoji cell: `=>` shaped, but with an emoji
/// immediately after the operator the wide cell does not join (and cannot extend)
/// the run, so the arrow still ligates exactly as it does standalone. Here we
/// assert the emoji-suffixed render still ligates the leading `=>` (differs from
/// the unligated control), proving the wide cell broke the run without disturbing
/// the preceding ligature.
#[test]
fn ligature_breaks_on_wide_cell() {
    let text = "=>🚀".as_bytes();
    let (Some(on), Some(off)) = (
        render(LigatureMode::Enabled, text),
        render(LigatureMode::Disabled, text),
    ) else {
        eprintln!("SKIP: no ligature test font (set ATERM_FONT or add the repo fixture)");
        return;
    };
    // The leading '=>' still ligates (wide emoji breaks the run after it, not the
    // arrow before it), so on != off: if the emoji had poisoned the run the arrow
    // would not ligate and the frames would match.
    assert_ne!(
        on.pixels, off.pixels,
        "'=>' before a wide emoji must still ligate — the wide cell should break the \
         run after the operator, not suppress the preceding ligature"
    );
}
