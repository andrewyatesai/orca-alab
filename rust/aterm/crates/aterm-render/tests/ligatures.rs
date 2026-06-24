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
    if let Ok(path) = std::env::var("ATERM_FONT")
        && let Ok(bytes) = std::fs::read(&path)
    {
        return Some(bytes);
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
    let features = aterm_render::ligature_shaping::build_feature_list(&[]);
    let shaped = aterm_render::ligature_shaping::shape_ligature_run(
        &bytes,
        "=>",
        &['=', '>'],
        true,
        &features,
    );
    assert!(
        shaped.is_some(),
        "=> must shape to a ligature (glyph ids differ from plain cmap)"
    );
    let gids = shaped.unwrap();
    assert_eq!(
        gids.len(),
        2,
        "monospace-preserving font keeps one glyph per cell"
    );
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
        band.contains(&theme_sel),
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

/// ITEM B — STYLED ligature cache (italic): a synthetic-italic `=>` must render
/// DIFFERENTLY from a regular `=>`. The arrow draws as a single `mono_gid`
/// ligature glyph; for the slant to reach the pixels the ITALIC style bit must
/// survive (a) the shaped-run cache key `(run, style)` — so the italic run shapes
/// independently of the regular one — and (b) the per-column ligature glyph key
/// `mono_gid(gid, style)` — so the italic ligature glyph rasterizes with the
/// synthetic slant. A pixel difference proves both. (Bold is already covered by
/// `ligature_breaks_on_style_change`; this adds the italic axis.)
#[test]
fn italic_ligature_differs_from_regular() {
    let (Some(regular), Some(italic)) = (
        render(LigatureMode::Enabled, b"a\x1b[0m=>"),
        render(LigatureMode::Enabled, b"a\x1b[3m=>\x1b[0m"),
    ) else {
        eprintln!("SKIP: no ligature test font (set ATERM_FONT or add the repo fixture)");
        return;
    };
    assert_eq!(
        (regular.width, regular.height),
        (italic.width, italic.height),
        "italic must not change frame dimensions"
    );
    assert_ne!(
        regular.pixels, italic.pixels,
        "an italic '=>' must ink differently than a regular '=>' — the ITALIC style \
         bit must persist through the shaped-run cache key and the mono_gid ligature \
         glyph (synthetic slant), not be dropped on the ligature path"
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

/// Remove the `GSUB` table from an sfnt font's table directory, yielding a font
/// that ttf-parser/rustybuzz see as having NO ligature features. The sfnt header
/// is `[u32 sfntVersion][u16 numTables][u16 searchRange][u16 entrySelector]
/// [u16 rangeShift]` (12 bytes), followed by `numTables` × 16-byte table records
/// `[Tag tag][u32 checksum][u32 offset][u32 length]`. Dropping the `GSUB` record
/// and decrementing `numTables` is enough: ttf-parser locates every table strictly
/// via the directory, so the (still-present) table BYTES become unreachable. The
/// search-range fields are hints, not used for lookup, so they need no fix-up.
/// Returns `None` if the input isn't a directory-style sfnt with a `GSUB` table.
fn strip_gsub(bytes: &[u8]) -> Option<Vec<u8>> {
    const HEADER: usize = 12;
    const RECORD: usize = 16;
    if bytes.len() < HEADER {
        return None;
    }
    let num_tables = u16::from_be_bytes([bytes[4], bytes[5]]) as usize;
    let dir_end = HEADER + num_tables * RECORD;
    if bytes.len() < dir_end {
        return None;
    }
    let gsub_rec = (0..num_tables)
        .map(|i| HEADER + i * RECORD)
        .find(|&off| &bytes[off..off + 4] == b"GSUB")?;
    let mut out = Vec::with_capacity(bytes.len() - RECORD);
    // Header with numTables decremented; other (hint) fields copied verbatim.
    out.extend_from_slice(&bytes[..4]);
    out.extend_from_slice(&u16::try_from(num_tables - 1).ok()?.to_be_bytes());
    out.extend_from_slice(&bytes[6..HEADER]);
    // All directory records except GSUB's (offsets into the table body are unchanged
    // because the body bytes after the directory keep their absolute positions).
    for i in 0..num_tables {
        let off = HEADER + i * RECORD;
        if off == gsub_rec {
            continue;
        }
        out.extend_from_slice(&bytes[off..off + RECORD]);
    }
    // The table body, shifted forward by one dropped record (RECORD bytes). The
    // directory offsets are absolute from file start, so re-point them by -RECORD.
    let body = &bytes[dir_end..];
    let mut shifted = out.clone();
    // Fix every kept record's offset (subtract one record's worth of bytes).
    for i in 0..(num_tables - 1) {
        let off = HEADER + i * RECORD;
        let o = u32::from_be_bytes([
            shifted[off + 8],
            shifted[off + 9],
            shifted[off + 10],
            shifted[off + 11],
        ]);
        let no = o.checked_sub(RECORD as u32)?;
        shifted[off + 8..off + 12].copy_from_slice(&no.to_be_bytes());
    }
    shifted.extend_from_slice(body);
    Some(shifted)
}

/// ITEM A — feature gating, no-ligature half: a font with its `GSUB` table removed
/// reports `has_ligature_features() == false`, and rendering a `=>` through it is
/// BYTE-IDENTICAL whether ligatures are enabled or disabled — proving the planner
/// short-circuits shaping (no `liga`/`calt` => no substitution => the per-cell cmap
/// glyphs either way). The stripped font is derived from the fixture so the cmap
/// (hence the per-cell glyphs) is identical to the ligating control.
#[test]
fn no_ligature_font_skips_shaping_and_is_identical() {
    let Some(bytes) = ligature_test_font() else {
        eprintln!("SKIP: no ligature test font (set ATERM_FONT or add the repo fixture)");
        return;
    };
    let Some(stripped) = strip_gsub(&bytes) else {
        eprintln!("SKIP: fixture is not a directory-style sfnt with a GSUB table");
        return;
    };

    // The probe must see no liga/calt feature once GSUB is gone.
    let mut r_on = Renderer::from_bytes(&stripped, 18.0, Theme::default()).unwrap();
    assert!(
        !r_on.has_ligature_features(),
        "a font with no GSUB table must report has_ligature_features() == false"
    );

    // Render '=>' with ligatures ON and OFF through the no-feature font; the
    // short-circuited plan is the per-cell plan in BOTH cases, so the pixels match.
    let mut r_off = Renderer::from_bytes(&stripped, 18.0, Theme::default()).unwrap();
    r_on.set_text_shaping(TextShapingConfig {
        ligature_mode: LigatureMode::Enabled,
        ..Default::default()
    });
    r_off.set_text_shaping(TextShapingConfig {
        ligature_mode: LigatureMode::Disabled,
        ..Default::default()
    });
    let (rows, cols) = (1usize, 16usize);
    let mut term = Terminal::new(rows as u16, cols as u16);
    term.process(b"\x1b[?25l");
    term.process(b"a=>b");
    let input = term.cell_frame(rows, cols);
    let on = r_on.render_input(&input);
    let off = r_off.render_input(&input);
    assert_eq!(
        on.pixels, off.pixels,
        "with no liga/calt feature, ligatures on vs off must be byte-identical — \
         the shaping fast-path was not skipped"
    );
}

/// ITEM A — feature gating, ligature half: the JetBrains Mono fixture DOES
/// advertise liga/calt (`has_ligature_features() == true`) and, with the fast-path
/// flag set, still ligates `=>` (its ligated render differs from the unligated one).
/// This guards against the short-circuit ever wrongly firing on a ligature font.
#[test]
fn fixture_font_has_ligature_features_and_still_ligates() {
    let Some(bytes) = ligature_test_font() else {
        eprintln!("SKIP: no ligature test font (set ATERM_FONT or add the repo fixture)");
        return;
    };
    let r = Renderer::from_bytes(&bytes, 18.0, Theme::default()).unwrap();
    assert!(
        r.has_ligature_features(),
        "JetBrains Mono must report has_ligature_features() == true"
    );
    // And the gated path still produces a real ligature.
    let (Some(on), Some(off)) = (
        render(LigatureMode::Enabled, b"a => b"),
        render(LigatureMode::Disabled, b"a => b"),
    ) else {
        eprintln!("SKIP: no ligature test font");
        return;
    };
    assert_ne!(
        on.pixels, off.pixels,
        "the fixture must still ligate with the GSUB-gated fast-path in place"
    );
}

// ---------------------------------------------------------------------------
// WIRE-FONTFEAT-OPENTYPE: the user's `font_features` (ss01/cv01/zero/stylistic
// sets) must actually reach `rustybuzz::shape`, not be silently dropped. These
// tests drive the SHARED feature-list seam plus an end-to-end rustybuzz shape
// against the embedded DejaVu Sans Mono face (always present — no SKIP).
// ---------------------------------------------------------------------------

use aterm_types::text_shaping::{FontFeature, FontFeatureSet};
use rustybuzz::ttf_parser::Tag;

/// The embedded last-resort face (DejaVu Sans Mono). Always available, so these
/// tests never SKIP. DejaVu's GSUB advertises `liga`, `rlig`, `dlig`, … — we use
/// the discretionary-ligature `dlig` feature as a concrete user feature whose
/// effect on `rustybuzz::shape` is observable (it ligates "fi" -> one glyph).
const DEJAVU: &[u8] = include_bytes!("../assets/DejaVuSansMono.ttf");

fn shape_gids(bytes: &[u8], s: &str, features: &[rustybuzz::Feature]) -> Vec<u32> {
    let face = rustybuzz::Face::from_slice(bytes, 0).unwrap();
    let mut buf = rustybuzz::UnicodeBuffer::new();
    buf.push_str(s);
    let out = rustybuzz::shape(&face, features, buf);
    out.glyph_infos().iter().map(|i| i.glyph_id).collect()
}

/// END-TO-END: a user `FontFeature` built by the renderer's feature seam
/// genuinely changes `rustybuzz::shape` output. The base `[liga, calt]` list
/// leaves "fi" as two glyphs; adding the user `dlig=1` feature collapses it to
/// the single discretionary-ligature glyph. If the user feature were dropped
/// (the pre-WIRE-FONTFEAT bug) the two shapes would be identical.
#[test]
fn user_feature_changes_rustybuzz_shape_output() {
    let base = aterm_render::ligature_shaping::build_feature_list(&[]);
    let with_dlig =
        aterm_render::ligature_shaping::build_feature_list(&[FontFeature::new(*b"dlig", 1)]);
    let base_gids = shape_gids(DEJAVU, "fi", &base);
    let dlig_gids = shape_gids(DEJAVU, "fi", &with_dlig);
    assert_ne!(
        base_gids, dlig_gids,
        "the user 'dlig' feature must reach rustybuzz::shape and change the result \
         (base={base_gids:?}, dlig={dlig_gids:?}) — it was silently dropped"
    );
    // Sanity: the user feature really is present in the built array with the
    // right tag + value (the testable seam), appended after the base pair.
    assert!(
        with_dlig
            .iter()
            .any(|f| f.tag == Tag::from_bytes(b"dlig") && f.value == 1),
        "built feature array must contain the user 'dlig=1' feature"
    );
}

/// The renderer resolves `TextShapingConfig.font_features` (the per-font Vec the
/// config round-trips) into its feature array. A config with an `ss01` feature on
/// the PRIMARY font (`font_id == 0`) must yield a built array whose shape of a
/// stylistic-set input differs from the no-feature config — proving the Vec is
/// applied, not carried-and-dropped. We assert at the seam: setting the config
/// changes what the renderer shapes.
#[test]
fn renderer_applies_config_font_features() {
    // Build the resolved array the way the renderer does, from a config that
    // carries a primary-font 'dlig' feature.
    let cfg = TextShapingConfig {
        font_features: vec![FontFeatureSet {
            font_id: 0,
            features: vec![FontFeature::new(*b"dlig", 1)],
        }],
        ..Default::default()
    };
    // A second font_id (1) must NOT leak into the primary face's features.
    let cfg_other_font = TextShapingConfig {
        font_features: vec![FontFeatureSet {
            font_id: 1,
            features: vec![FontFeature::new(*b"dlig", 1)],
        }],
        ..Default::default()
    };

    let mut r = Renderer::from_bytes(DEJAVU, 18.0, Theme::default()).unwrap();

    // Apply the dlig config and confirm the renderer's resolved feature array
    // changes shaping output for "fi" versus the empty-features default.
    r.set_text_shaping(cfg);
    let dlig_gids = shape_gids(DEJAVU, "fi", r.resolved_features_for_test());

    r.set_text_shaping(TextShapingConfig::default());
    let base_gids = shape_gids(DEJAVU, "fi", r.resolved_features_for_test());
    assert_ne!(
        base_gids, dlig_gids,
        "configuring a primary-font 'dlig' feature must change the renderer's \
         resolved shaping (base={base_gids:?}, dlig={dlig_gids:?})"
    );

    // A feature scoped to a DIFFERENT font_id must not affect the primary face:
    // its resolved array equals the empty/default one.
    r.set_text_shaping(cfg_other_font);
    let other_gids = shape_gids(DEJAVU, "fi", r.resolved_features_for_test());
    assert_eq!(
        base_gids, other_gids,
        "a feature scoped to font_id != 0 must not apply to the primary face"
    );
}
