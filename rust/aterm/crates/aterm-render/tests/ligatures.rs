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

use aterm_core::terminal::Terminal;
use aterm_render::{LigatureMode, Renderer, TextShapingConfig, Theme};

// aterm-render manifest dir is rust/aterm/crates/aterm-render; the bundled font
// lives at orc/src/renderer/... — four levels up (aterm-render→crates→aterm→rust→orc).
const JETBRAINS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../../src/renderer/src/assets/fonts/jetbrains-mono.ttf"
);

fn renderer(mode: LigatureMode) -> Option<Renderer> {
    let bytes = std::fs::read(JETBRAINS).ok()?;
    let mut r = Renderer::from_bytes(&bytes, 18.0, Theme::default()).ok()?;
    r.set_text_shaping(TextShapingConfig {
        ligature_mode: mode,
        ..Default::default()
    });
    Some(r)
}

fn render(mode: LigatureMode, text: &[u8]) -> aterm_render::Frame {
    let mut r = renderer(mode).expect("jetbrains-mono font loads");
    let (rows, cols) = (1usize, 16usize);
    let mut term = Terminal::new(rows as u16, cols as u16);
    // Hide the cursor so the cursor overlay never perturbs the comparison.
    term.process(b"\x1b[?25l");
    term.process(text);
    let input = term.cell_frame(rows, cols);
    r.render_input(&input)
}

/// PRE-CHECK (documents the shaping mechanism): a standalone rustybuzz shape of
/// "=>" with liga+calt yields the SAME glyph COUNT as input chars for this
/// monospace-preserving font (it uses empty placeholder glyphs, not collapsing),
/// but the glyph IDS differ from the plain cmap glyphs — i.e. a ligature exists.
#[test]
fn rustybuzz_shapes_arrow_ligature() {
    let bytes = std::fs::read(JETBRAINS).expect("font");
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
    let on = render(LigatureMode::Enabled, text);
    let off = render(LigatureMode::Disabled, text);
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
    let on = render(LigatureMode::Enabled, text);
    let off = render(LigatureMode::Disabled, text);
    assert_eq!(
        on.pixels, off.pixels,
        "spaced operators 'a = > b' must render identically on/off — no run should ligate"
    );
}
