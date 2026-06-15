// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Render-ready per-cell extraction for CPU/GPU rasterizers.
//!
//! Bridges the grid's packed cell storage and the central color resolver
//! ([`color_resolve`](super::color_resolve)) into a flat, render-ready row of
//! [`RenderCell`]s. Each cell carries the resolved character plus final
//! foreground/background RGB with every style attribute already applied:
//! palette indices, RGB overflow, bold-to-bright, dim, inverse, hidden, and
//! terminal-level reverse video (DECSCNM).

use super::Terminal;
use super::color_resolve::{resolve_bg_color_raw, resolve_fg_color_raw};
use crate::grid::{Cell, CellFlags};

/// The line-decoration style under a cell (SGR 4 / 4:n / 21). The terminal
/// packs these as `UNDERLINE` / `DOUBLE_UNDERLINE` / `CURLY_UNDERLINE` bit
/// combinations; [`RenderCell`] resolves them to one variant for the renderer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UnderlineStyle {
    /// No underline.
    #[default]
    None,
    /// SGR 4 — a single straight line.
    Single,
    /// SGR 21 / 4:2 — two stacked straight lines.
    Double,
    /// SGR 4:3 — a wavy line (editors' squiggle for diagnostics).
    Curly,
    /// SGR 4:4 — a dotted line.
    Dotted,
    /// SGR 4:5 — a dashed line.
    Dashed,
}

/// A single render-ready terminal cell.
///
/// Colors are final RGB triples; the renderer can fill the cell rect with
/// [`bg`](RenderCell::bg) and blit the glyph for [`ch`](RenderCell::ch) in
/// [`fg`](RenderCell::fg) with no further attribute logic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "each bool is an independent SGR/geometry rendition flag the renderer reads directly; a bitfield would obscure the public render API"
)]
pub struct RenderCell {
    /// The character to draw (`' '` for empty / NUL cells).
    pub ch: char,
    /// Final foreground color as `[r, g, b]`.
    pub fg: [u8; 3],
    /// Final background color as `[r, g, b]`.
    pub bg: [u8; 3],
    /// True when this column is the right half (continuation) of a wide glyph.
    ///
    /// Such a column has no glyph of its own (`ch` is a space); renderers
    /// should fill its background but leave drawing the glyph to the wide
    /// lead cell, whose rasterized bitmap naturally overflows into it.
    pub wide: bool,
    /// True when this (lead) cell requested EMOJI presentation: a text-default
    /// emoji base char (`is_vs16_emoji_capable`) that VS16 (U+FE0F) widened to
    /// two cells. Such a char has a monochrome glyph in the text fonts but the
    /// selector asks for the colour form, so the renderer must prefer the
    /// colour-emoji face over the (otherwise-winning) mono primary/fallback.
    /// `❤️` (U+2764 U+FE0F) is the canonical case. Bare `❤` (no VS16) stays
    /// narrow and mono. SMP emoji (🚀) are already colour via the normal path.
    pub emoji_presentation: bool,
    /// SGR 1 bold: the renderer rasterizes the glyph with extra stroke weight.
    /// (Bold-to-bright colour, when enabled, is already applied in `fg`.)
    pub bold: bool,
    /// SGR 3 italic: the renderer rasterizes the glyph with a synthetic slant.
    pub italic: bool,
    /// Underline decoration (SGR 4 family). Drawn as line(s) in
    /// [`underline_color`](RenderCell::underline_color) (or [`fg`](RenderCell::fg)).
    pub underline: UnderlineStyle,
    /// Strikethrough (SGR 9): a line through the cell middle, in `fg`.
    pub strikethrough: bool,
    /// Overline (SGR 53): a line along the cell top, in `fg`.
    pub overline: bool,
    /// SGR 58 underline colour, when set; otherwise the underline uses `fg`.
    pub underline_color: Option<[u8; 3]>,
}

impl Terminal {
    /// Resolve a visible row into render-ready cells, one per stored column.
    ///
    /// Each returned [`RenderCell`] has its foreground/background fully
    /// resolved through [`color_resolve`](super::color_resolve): palette
    /// indices, RGB overflow (ring buffer + overflow map), bold-to-bright,
    /// dim, inverse, hidden, and terminal-level reverse video (DECSCNM).
    ///
    /// Returns an empty vector for out-of-range rows.
    #[must_use]
    #[allow(
        clippy::too_many_lines,
        reason = "single per-cell resolution pass (colors + all decorations) over a row"
    )]
    pub fn render_row(&self, row: usize) -> Vec<RenderCell> {
        let mut out = Vec::new();
        self.render_row_into(row, &mut out);
        out
    }

    /// Like [`render_row`](Self::render_row), but fills a caller-owned `out`
    /// buffer instead of allocating a fresh `Vec` each call — the per-frame
    /// extract path reuses one buffer across rows/frames. `out` is `clear()`ed
    /// first, then pushed exactly the cells [`render_row`](Self::render_row)
    /// would return (it IS the one code path), so the result is byte-identical.
    /// Out-of-range rows leave `out` empty.
    pub fn render_row_into(&self, row: usize, out: &mut Vec<RenderCell>) {
        out.clear();
        let Ok(visible_row) = u16::try_from(row) else {
            return;
        };
        let grid = self.grid();
        let Some(grid_row) = grid.row(visible_row) else {
            return;
        };

        let palette = self.color_palette();
        let default_fg = self.default_foreground();
        let default_bg = self.default_background();
        let reverse_video = self.modes().reverse_video();

        let cols = grid_row.len();
        out.reserve(cols as usize);
        for col in 0..cols {
            let Some(cell) = grid_row.get(col) else {
                continue;
            };

            // Style-interned cells keep their colors in the StyleTable, so the
            // raw `colors()` of the cell is a StyleId payload. Rehydrate it to
            // an inline-colored cell (+ explicit RGB) before resolving, so the
            // resolver sees real packed colors. Inline cells take the fast path.
            let (eff_cell, fg_rgb, bg_rgb) = if cell.uses_style_id() {
                let extra_flags = cell.flags().difference(CellFlags::USES_STYLE_ID);
                let (fg, bg, flags) = grid.resolve_style_to_colors(cell.style_id(), extra_flags);
                let fg_rgb = fg.is_rgb().then(|| {
                    let (r, g, b) = fg.rgb_components();
                    [r, g, b]
                });
                let bg_rgb = bg.is_rgb().then(|| {
                    let (r, g, b) = bg.rgb_components();
                    [r, g, b]
                });
                (Cell::with_style(cell.char(), fg, bg, flags), fg_rgb, bg_rgb)
            } else {
                (
                    *cell,
                    grid.fg_rgb_at(visible_row, col),
                    grid.bg_rgb_at(visible_row, col),
                )
            };

            let fg = resolve_fg_color_raw(
                &eff_cell,
                fg_rgb,
                bg_rgb,
                palette,
                default_fg,
                default_bg,
                reverse_video,
            );
            let bg = resolve_bg_color_raw(
                &eff_cell,
                fg_rgb,
                bg_rgb,
                palette,
                default_fg,
                default_bg,
                reverse_video,
            );

            // A TRUE wide continuation (the blank right half of a CJK glyph)
            // must be disambiguated from a DECSCA-protected cell: `PROTECTED`
            // and `WIDE_CONTINUATION` share bit 10, so the raw flag alone would
            // blank every protected character. A real continuation has bit 10
            // set, is not itself a WIDE main cell, and sits immediately right of
            // a WIDE cell. (Same rule as `Row::is_cell_wide_continuation`, done
            // inline here to reuse `grid_row` — render_row is a hot path.)
            let wide = cell.is_wide_continuation()
                && !cell.is_wide()
                && col > 0
                && grid_row.get(col - 1).is_some_and(aterm_grid::Cell::is_wide);
            let ch = if wide {
                ' '
            } else {
                // resolved_char transparently handles complex (non-BMP) cells.
                grid.resolved_char(visible_row, col)
                    .map_or(' ', |c| if c == '\0' { ' ' } else { c })
            };

            // Emoji presentation: a text-default emoji base that VS16 widened to
            // 2 cells. Such a char is narrow by default, so a WIDE main cell
            // holding an emoji-capable base can ONLY have been widened by VS16
            // (`widen_previous_cell_for_vs16`). Lead cells only (`!wide`).
            let emoji_presentation =
                !wide && cell.is_wide() && super::handler::is_vs16_emoji_capable(ch);

            // Line decorations (SGR 4 family / 9 / 53). The packed underline bits
            // combine: DOTTED = UNDERLINE|CURLY, DASHED = DOUBLE|CURLY, so the
            // composite styles are checked before the singletons.
            let cflags = eff_cell.flags();
            let underline = if cflags.contains(CellFlags::DOTTED_UNDERLINE) {
                UnderlineStyle::Dotted
            } else if cflags.contains(CellFlags::DASHED_UNDERLINE) {
                UnderlineStyle::Dashed
            } else if cflags.contains(CellFlags::CURLY_UNDERLINE) {
                UnderlineStyle::Curly
            } else if cflags.contains(CellFlags::DOUBLE_UNDERLINE) {
                UnderlineStyle::Double
            } else if cflags.contains(CellFlags::UNDERLINE) {
                UnderlineStyle::Single
            } else {
                UnderlineStyle::None
            };
            let strikethrough = cflags.contains(CellFlags::STRIKETHROUGH);
            let overline = cflags.contains(CellFlags::OVERLINE);
            let bold = cflags.contains(CellFlags::BOLD);
            let italic = cflags.contains(CellFlags::ITALIC);
            // SGR 58 underline colour (only probed when there's an underline).
            let underline_color = if underline == UnderlineStyle::None {
                None
            } else {
                grid.cell_extra(visible_row, col).and_then(aterm_grid::CellExtra::underline_color)
            };

            out.push(RenderCell {
                ch,
                fg: [fg.r, fg.g, fg.b],
                bg: [bg.r, bg.g, bg.b],
                wide,
                emoji_presentation,
                bold,
                italic,
                underline,
                strikethrough,
                overline,
                underline_color,
            });
        }
    }

    /// Emoji grapheme-cluster strings for the visible `row`, sparse: one
    /// `(col, cluster)` per cell whose combining marks form a multi-codepoint
    /// EMOJI sequence — a ZWJ sequence (👨‍👩‍👧), a skin-tone modifier (👍🏽), or
    /// an enclosing keycap (1️⃣). The renderer shapes each cluster to a single
    /// colour glyph; without this it would only see the base codepoint and draw
    /// just the first component.
    ///
    /// Deliberately EXCLUDES pure VS15/VS16 clusters (e.g. ❤️) — those keep the
    /// presentation-selector path ([`RenderCell::emoji_presentation`]), which is
    /// already CPU/GPU-consistent. `col` is the wide lead cell (the base char's
    /// column), matching where the renderer blits the glyph.
    #[must_use]
    pub fn cluster_row(&self, row: usize) -> Vec<(usize, Box<str>)> {
        let mut out = Vec::new();
        self.cluster_row_into(row, &mut out);
        out
    }

    /// Like [`cluster_row`](Self::cluster_row), but fills a caller-owned `out`
    /// buffer instead of allocating a fresh `Vec`. `out` is `clear()`ed first,
    /// then pushed exactly the `(col, cluster)` pairs
    /// [`cluster_row`](Self::cluster_row) would return (the one code path), so
    /// the result is byte-identical. The owned cluster strings (`Box<str>`) are
    /// still allocated per cluster — only the per-row container Vec is reused.
    pub fn cluster_row_into(&self, row: usize, out: &mut Vec<(usize, Box<str>)>) {
        out.clear();
        let Ok(visible_row) = u16::try_from(row) else {
            return;
        };
        let grid = self.grid();
        // Fast path: emoji clusters live in cell extras (combining marks). With
        // no extras anywhere there is nothing to scan — the common case (plain
        // text) pays a single bool check instead of a per-column probe.
        if grid.extras().is_empty() {
            return;
        }
        let Some(grid_row) = grid.row(visible_row) else {
            return;
        };
        let cols = grid_row.len();
        for col in 0..cols {
            let Some(extra) = grid.cell_extra(visible_row, col) else {
                continue;
            };
            let combining = extra.combining();
            if !combining.iter().copied().any(is_emoji_sequence_marker) {
                continue;
            }
            let Some(base) = grid.resolved_char(visible_row, col) else {
                continue;
            };
            if base == '\0' {
                continue;
            }
            let mut s = String::with_capacity(2 + combining.len());
            s.push(base);
            s.extend(combining.iter().copied());
            out.push((col as usize, s.into_boxed_str()));
        }
    }

    /// Combining MARKS to overlay per cell of the visible `row`, sparse: one
    /// `(col, marks)` for each cell carrying combining diacritics (é = e + U+0301,
    /// ñ = n + U+0303, …). The renderer blits each mark's glyph over the base so
    /// the accent shows; without this only the base code point is drawn.
    ///
    /// Excludes cells handled elsewhere: emoji sequences (a sequence marker is
    /// present — [`cluster_row`](Self::cluster_row) shapes those) and the bare
    /// VS15/VS16 selectors ([`RenderCell::emoji_presentation`]). Marks are kept
    /// in arrival order so stacked diacritics layer correctly.
    #[must_use]
    pub fn combining_row(&self, row: usize) -> Vec<(usize, Box<[char]>)> {
        let mut out = Vec::new();
        self.combining_row_into(row, &mut out);
        out
    }

    /// Like [`combining_row`](Self::combining_row), but fills a caller-owned
    /// `out` buffer instead of allocating a fresh `Vec`. `out` is `clear()`ed
    /// first, then pushed exactly the `(col, marks)` pairs
    /// [`combining_row`](Self::combining_row) would return (the one code path),
    /// so the result is byte-identical. The owned mark slices (`Box<[char]>`)
    /// are still allocated per cell — only the per-row container Vec is reused.
    pub fn combining_row_into(&self, row: usize, out: &mut Vec<(usize, Box<[char]>)>) {
        out.clear();
        let Ok(visible_row) = u16::try_from(row) else {
            return;
        };
        let grid = self.grid();
        if grid.extras().is_empty() {
            return;
        }
        let Some(grid_row) = grid.row(visible_row) else {
            return;
        };
        for col in 0..grid_row.len() {
            let Some(extra) = grid.cell_extra(visible_row, col) else {
                continue;
            };
            let combining = extra.combining();
            if combining.is_empty()
                || combining.iter().copied().any(is_emoji_sequence_marker)
            {
                continue;
            }
            // Overlay every combining char except the presentation selectors,
            // which only widen/narrow the base (no glyph of their own).
            let marks: Box<[char]> = combining
                .iter()
                .copied()
                .filter(|&c| c != '\u{FE0E}' && c != '\u{FE0F}')
                .collect();
            if marks.is_empty() {
                continue;
            }
            out.push((col as usize, marks));
        }
    }
}

/// A combining char that marks its cell as a multi-codepoint EMOJI sequence:
/// ZWJ (U+200D, family/role sequences), an emoji skin-tone modifier
/// (U+1F3FB–U+1F3FF), COMBINING ENCLOSING KEYCAP (U+20E3), or a regional
/// indicator (U+1F1E6–U+1F1FF, the second half of a flag pair the writer folds
/// into one cell). VS15/VS16 are presentation selectors, not sequence markers,
/// and are excluded on purpose.
#[inline]
fn is_emoji_sequence_marker(c: char) -> bool {
    matches!(c as u32, 0x200D | 0x20E3 | 0x1F3FB..=0x1F3FF | 0x1F1E6..=0x1F1FF)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_row_out_of_range_is_empty() {
        let term = Terminal::new(2, 4);
        assert!(term.render_row(99).is_empty());
    }

    #[test]
    fn render_row_default_colors() {
        let mut term = Terminal::new(2, 8);
        term.process(b"Hi");
        let cells = term.render_row(0);
        assert!(cells.len() >= 2);
        assert_eq!(cells[0].ch, 'H');
        assert_eq!(cells[1].ch, 'i');
        // Default fg/bg come straight from the terminal defaults.
        let fg = term.default_foreground();
        let bg = term.default_background();
        assert_eq!(cells[0].fg, [fg.r, fg.g, fg.b]);
        assert_eq!(cells[0].bg, [bg.r, bg.g, bg.b]);
    }

    #[test]
    fn vs16_widened_emoji_sets_emoji_presentation() {
        // ❤️ = U+2764 (HEAVY BLACK HEART, text default) + U+FE0F (VS16). VS16
        // widens it to 2 cells AND requests colour presentation.
        let mut term = Terminal::new(2, 8);
        term.process("\u{2764}\u{FE0F}".as_bytes());
        let cells = term.render_row(0);
        assert_eq!(cells[0].ch, '\u{2764}');
        assert!(!cells[0].wide, "lead cell is not a continuation");
        assert!(cells[0].emoji_presentation, "VS16-widened ❤️ lead must request emoji presentation");
        // The right half is a wide continuation carrying no glyph / no flag.
        assert!(cells[1].wide, "second column is the wide continuation");
        assert!(!cells[1].emoji_presentation, "continuation cell carries no presentation flag");
    }

    #[test]
    fn bare_emoji_base_without_vs16_is_text_presentation() {
        // Bare ❤ (no VS16) stays narrow and text — NO emoji presentation, so
        // the renderer keeps drawing the mono black-heart glyph.
        let mut term = Terminal::new(2, 8);
        term.process("\u{2764}".as_bytes());
        let cells = term.render_row(0);
        assert_eq!(cells[0].ch, '\u{2764}');
        assert!(!cells[0].wide);
        assert!(!cells[0].emoji_presentation, "bare ❤ must not request emoji presentation");
    }

    #[test]
    fn cluster_row_emits_zwj_skin_keycap_not_vs16_or_plain() {
        let mut term = Terminal::new(2, 20);
        // family ZWJ (col 0) sp(2) skin (3) sp(5) keycap (6) sp(?) ❤️ VS16 plain 'a'
        term.process(
            "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467} \u{1F44D}\u{1F3FD} \u{31}\u{FE0F}\u{20E3} \u{2764}\u{FE0F}a".as_bytes(),
        );
        let clusters = term.cluster_row(0);
        // family at lead col 0
        let family = clusters.iter().find(|(c, _)| *c == 0).map(|(_, s)| s.as_ref());
        assert_eq!(family, Some("\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}"), "family ZWJ cluster");
        // skin-tone thumbs-up at col 3
        let skin = clusters.iter().find(|(c, _)| *c == 3).map(|(_, s)| s.as_ref());
        assert_eq!(skin, Some("\u{1F44D}\u{1F3FD}"), "skin-tone cluster");
        // keycap at col 6
        let keycap = clusters.iter().find(|(c, _)| *c == 6).map(|(_, s)| s.as_ref());
        assert_eq!(keycap, Some("\u{31}\u{FE0F}\u{20E3}"), "keycap cluster");
        // VS16 ❤️ must NOT be emitted (it keeps the emoji_presentation path).
        assert!(
            clusters.iter().all(|(_, s)| !s.starts_with('\u{2764}')),
            "VS16 ❤️ must not be a shaping cluster, got {clusters:?}"
        );
    }

    #[test]
    fn regional_indicator_pair_folds_into_one_flag_cluster() {
        // 🇺🇸 = regional indicator U + S. The pair must fold into ONE 2-cell
        // grapheme (lead col 0 wide, col 1 continuation), with S as a combining
        // mark, and surface as a flag cluster for shaping.
        let mut term = Terminal::new(2, 12);
        term.process("\u{1F1FA}\u{1F1F8}".as_bytes());
        let cells = term.render_row(0);
        assert_eq!(cells[0].ch, '\u{1F1FA}', "lead cell is regional indicator U");
        assert!(!cells[0].wide, "lead is not a continuation");
        assert!(cells[1].wide, "col 1 is the wide continuation of the flag");
        // The pair occupies exactly 2 cells, not 4 (render_row trims to the
        // occupied width, so a folded pair is a length-2 row).
        assert_eq!(cells.len(), 2, "RI pair folds into one 2-cell flag, not two glyphs");

        let clusters = term.cluster_row(0);
        let flag = clusters.iter().find(|(c, _)| *c == 0).map(|(_, s)| s.as_ref());
        assert_eq!(flag, Some("\u{1F1FA}\u{1F1F8}"), "flag cluster surfaced for shaping");
    }

    #[test]
    fn three_regional_indicators_pair_then_single() {
        // 🇺🇸🇫: GB12/GB13 — the first two pair into a flag; the third stands
        // alone in its own cell (it is NOT folded into the completed pair).
        let mut term = Terminal::new(2, 12);
        term.process("\u{1F1FA}\u{1F1F8}\u{1F1EB}".as_bytes());
        let cells = term.render_row(0);
        assert_eq!(cells[0].ch, '\u{1F1FA}', "pair lead U");
        assert!(cells[1].wide, "pair continuation");
        // The third RI starts a fresh cell at col 2 (wide), not folded in.
        assert_eq!(cells[2].ch, '\u{1F1EB}', "third RI stands alone");
        let clusters = term.cluster_row(0);
        assert_eq!(clusters.len(), 1, "only the completed pair is a cluster, got {clusters:?}");
        assert_eq!(clusters[0].0, 0, "the flag cluster is at the pair lead");
    }


    #[test]
    fn sgr_decorations_surface_on_render_cells() {
        let mut term = Terminal::new(2, 16);
        // SGR 4 underline, 21 double, 4:3 curly, 9 strike, 53 overline.
        term.process(b"\x1b[4mA\x1b[0m\x1b[21mB\x1b[0m\x1b[4:3mC\x1b[0m\x1b[9mD\x1b[0m\x1b[53mE\x1b[0m");
        let cells = term.render_row(0);
        assert_eq!(cells[0].underline, UnderlineStyle::Single, "SGR 4 -> single");
        assert_eq!(cells[1].underline, UnderlineStyle::Double, "SGR 21 -> double");
        assert_eq!(cells[2].underline, UnderlineStyle::Curly, "SGR 4:3 -> curly");
        assert_eq!(cells[3].underline, UnderlineStyle::None);
        assert!(cells[3].strikethrough, "SGR 9 -> strikethrough");
        assert!(cells[4].overline, "SGR 53 -> overline");
        // Plain cells carry no decoration.
        let mut plain = Terminal::new(2, 8);
        plain.process(b"x");
        let pc = plain.render_row(0);
        assert_eq!(pc[0].underline, UnderlineStyle::None);
        assert!(!pc[0].strikethrough && !pc[0].overline);
    }

    #[test]
    fn underline_color_surfaces_from_sgr58() {
        let mut term = Terminal::new(2, 8);
        // SGR 4 underline + 58;2;255;0;0 sets a red underline colour.
        term.process(b"\x1b[4;58:2::255:0:0mU\x1b[0m");
        let cells = term.render_row(0);
        assert_eq!(cells[0].underline, UnderlineStyle::Single);
        assert_eq!(cells[0].underline_color, Some([255, 0, 0]), "SGR 58 red underline colour");
    }

    #[test]
    fn combining_marks_surface_for_diacritics_not_emoji() {
        let mut term = Terminal::new(2, 12);
        // é = e + U+0301, then a ZWJ family (emoji sequence), then plain 'x'.
        term.process("e\u{0301} \u{1F468}\u{200D}\u{1F469} x".as_bytes());
        let comb = term.combining_row(0);
        // The 'e' at col 0 surfaces its acute mark.
        let m0 = comb.iter().find(|(c, _)| *c == 0).map(|(_, m)| m.as_ref());
        assert_eq!(m0, Some(['\u{0301}'].as_slice()), "acute mark overlaid on e");
        // The emoji family is NOT a combining-overlay cell (cluster_row owns it).
        let family_col = 2; // after "e\u{0301} " (cols 0,1)
        assert!(
            comb.iter().all(|(c, _)| *c != family_col),
            "emoji cluster must not be a combining-overlay cell, got {comb:?}"
        );
    }

    #[test]
    fn combining_row_empty_for_plain_and_vs16() {
        let mut term = Terminal::new(2, 8);
        // VS16 ❤️ has a combining selector but NO overlay mark.
        term.process("hi \u{2764}\u{FE0F}".as_bytes());
        assert!(term.combining_row(0).is_empty(), "plain text + VS16 has no overlay marks");
    }

    #[test]
    fn cluster_row_empty_for_plain_text() {
        let mut term = Terminal::new(2, 8);
        term.process(b"hello");
        assert!(term.cluster_row(0).is_empty(), "plain ASCII has no emoji clusters");
    }

    #[test]
    fn wide_cjk_is_not_emoji_presentation() {
        // A naturally-wide CJK char is wide but NOT emoji-capable, so it must
        // not be mistaken for a VS16 emoji.
        let mut term = Terminal::new(2, 8);
        term.process("\u{65E5}".as_bytes()); // 日
        let cells = term.render_row(0);
        assert_eq!(cells[0].ch, '\u{65E5}');
        assert!(!cells[0].wide, "lead cell of a wide glyph is not the continuation");
        assert!(!cells[0].emoji_presentation, "wide CJK must not request emoji presentation");
    }

    #[test]
    fn render_row_indexed_fg_red() {
        let mut term = Terminal::new(2, 8);
        // SGR 31 = red foreground.
        term.process(b"\x1b[31mR\x1b[0m");
        let cells = term.render_row(0);
        assert_eq!(cells[0].ch, 'R');
        let [r, g, b] = cells[0].fg;
        assert!(r > g && r > b, "expected red-dominant fg, got {:?}", cells[0].fg);
    }

    #[test]
    fn render_row_indexed_bg_green() {
        let mut term = Terminal::new(2, 8);
        // SGR 42 = green background.
        term.process(b"\x1b[42mG\x1b[0m");
        let cells = term.render_row(0);
        assert_eq!(cells[0].ch, 'G');
        let [r, g, b] = cells[0].bg;
        assert!(g > r && g > b, "expected green-dominant bg, got {:?}", cells[0].bg);
    }

    #[test]
    fn render_row_truecolor_fg() {
        let mut term = Terminal::new(2, 8);
        // SGR 38;2;10;20;200 = a blue-ish truecolor fg.
        term.process(b"\x1b[38;2;10;20;200mX\x1b[0m");
        let cells = term.render_row(0);
        assert_eq!(cells[0].ch, 'X');
        assert_eq!(cells[0].fg, [10, 20, 200]);
    }

    #[test]
    fn render_row_protected_text_is_visible() {
        // DECSCA (ESC [ 1 " q) sets the PROTECTED flag, which shares bit 10 with
        // WIDE_CONTINUATION. Protected characters must still render their glyph
        // — they are NOT wide-continuation spacers. Regression for the bit-10
        // collision that blanked every DECSCA-protected cell.
        let mut term = Terminal::new(2, 8);
        term.process(b"\x1b[1\"qSECRET\x1b[0\"q");
        let cells = term.render_row(0);
        let text: String = cells.iter().take(6).map(|c| c.ch).collect();
        assert_eq!(text, "SECRET", "protected text must render, not blank");
        assert!(!cells[0].wide, "a protected cell is not a wide continuation");
    }

    #[test]
    fn render_row_wide_continuation_is_blanked() {
        // A real wide char (中, U+4E2D) occupies a WIDE lead cell + a
        // WIDE_CONTINUATION spacer. The lead keeps the glyph; the spacer renders
        // blank and is flagged `wide`. (Counterpart to the protected-cell case.)
        let mut term = Terminal::new(2, 8);
        term.process("中X".as_bytes());
        let cells = term.render_row(0);
        assert_eq!(cells[0].ch, '中');
        assert!(!cells[0].wide, "the wide LEAD is not a continuation");
        assert_eq!(cells[1].ch, ' ', "the continuation spacer renders blank");
        assert!(cells[1].wide, "the continuation spacer is flagged wide");
        assert_eq!(cells[2].ch, 'X', "the next glyph follows the 2-cell wide char");
    }
}
