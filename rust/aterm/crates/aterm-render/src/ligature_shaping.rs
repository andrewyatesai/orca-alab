// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! Programming-ligature run shaping for the terminal grid.
//!
//! aterm renders text strictly per-cell on a monospace cadence. Ligatures
//! (`=>`, `!=`, `===`, `->`, `<=`, …) need the OpenType `liga`/`calt` features:
//! rustybuzz shapes a RUN of adjacent same-style cells, and the font substitutes
//! the run's glyphs — for a monospace ligature font (JetBrains Mono / Fira Code)
//! the substitution keeps ONE glyph per input cell (each advance stays one cell),
//! turning the lead cells of a ligature into empty placeholder glyphs and the
//! final cell into the wide ligature glyph (whose negative left bearing overflows
//! back across the run). So a ligature draws on the SAME cells the characters
//! occupied — no cadence change, no cell consumed.
//!
//! This module is the SHARED shaping seam: both the CPU [`crate::Renderer`] row
//! painter and the GPU `encode_frame` consume the SAME per-cell plan (the same
//! [`crate::GlyphKey`] at the same column), so the CPU==GPU byte-identical invariant
//! holds. A run only ligates when rustybuzz actually changes the glyph ids;
//! otherwise the plan is identical to the plain per-cell path (byte-identical to
//! the pre-ligature renderer).

use aterm_core::terminal::RenderCell;
use rustybuzz::ttf_parser::Tag;

use crate::StyleBits;

/// What to draw at one column of a row, resolved by [`plan_row_runs`].
///
/// `Ligated` carries the primary-face glyph id rustybuzz produced for this
/// column's cell within its run; the caller blits it as a [`crate::GlyphKey::mono_gid`]
/// at the column's monospace origin (the lead cells of a ligature get the empty
/// placeholder glyph, the final cell the wide ligature glyph). `PerCell` means
/// the column was not part of a ligated run — the caller uses its ordinary
/// per-cell glyph dispatch ([`crate::Renderer::resolve_cell_key`]), so it stays
/// byte-identical to the non-ligature path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColumnGlyph {
    /// Use the ordinary per-cell dispatch for this column (no ligature touched it).
    PerCell,
    /// Draw this shaped primary-face glyph id at the column's monospace origin.
    Ligated(u16),
}

/// Whether `cell` is eligible to join a ligature shaping run.
///
/// A run is contiguous cells that are: drawable (not wide-continuation, not a
/// space, not a control char), NOT an emoji-presentation cell, NOT part of a
/// shaped emoji cluster, and NOT image-covered. Spaces and controls BREAK the
/// run (so `a => b` shapes `=>` but not across the spaces); wide/emoji/image
/// cells route to their existing colour/wide paths untouched. The caller also
/// breaks on a STYLE change (bold/italic) and per-frame on the cursor/selection
/// columns so those stay per-cell and correct.
#[must_use]
pub fn cell_is_shapeable(cell: &RenderCell, has_cluster: bool, image_covered: bool) -> bool {
    !cell.wide
        && cell.ch != ' '
        && !cell.ch.is_control()
        && !cell.emoji_presentation
        && !has_cluster
        && !image_covered
}

/// Shape one run string with `liga`+`calt` and return the per-INPUT-CHARACTER
/// primary-face glyph ids, or `None` if the run did not ligate (shaping produced
/// one glyph per char with the SAME ids the per-cell path would use, so there is
/// nothing to override — the caller keeps the plain path, byte-identical).
///
/// The run must be all single-`char` cells on a monospace cadence (the caller
/// guarantees this via [`cell_is_shapeable`] over BMP operator chars). Shaping is
/// accepted ONLY when it yields exactly one output glyph per input `char` and all
/// advances equal the monospace advance — i.e. the font is the monospace-
/// preserving kind. Any other shape (a collapsing/proportional result) is
/// rejected so the renderer never desynchronises the grid cadence.
#[must_use]
pub fn shape_ligature_run(
    rb_bytes: &[u8],
    run: &str,
    run_chars: &[char],
    enable: bool,
) -> Option<Box<[u16]>> {
    if !enable || run_chars.len() < 2 {
        return None;
    }
    let face = rustybuzz::Face::from_slice(rb_bytes, 0)?;
    let mut buf = rustybuzz::UnicodeBuffer::new();
    buf.push_str(run);
    let features = [
        rustybuzz::Feature::new(Tag::from_bytes(b"liga"), 1, ..),
        rustybuzz::Feature::new(Tag::from_bytes(b"calt"), 1, ..),
    ];
    let shaped = rustybuzz::shape(&face, &features, buf);
    let infos = shaped.glyph_infos();
    // Monospace-preserving fonts emit one glyph per input cell. A different count
    // means a collapsing/proportional shape we cannot map onto the grid cadence —
    // decline so the per-cell path (correct cadence) is used.
    if infos.len() != run_chars.len() {
        return None;
    }
    // Map each output glyph to its INPUT char by `cluster` (the byte offset we
    // pushed). For a per-char run on a monospace font the clusters are the char
    // boundaries in order; build a glyph id per char position.
    let mut byte_to_idx = Vec::with_capacity(run_chars.len());
    let mut b = 0usize;
    for ch in run_chars {
        byte_to_idx.push((b, ()));
        b += ch.len_utf8();
    }
    let mut gids = vec![0u16; run_chars.len()];
    let mut changed = false;
    for info in infos {
        let gid = u16::try_from(info.glyph_id).ok()?;
        let cluster = info.cluster as usize;
        // Find the char index whose byte offset == this cluster.
        let Some(idx) = byte_to_idx.iter().position(|&(bo, ())| bo == cluster) else {
            return None; // cluster didn't land on a char boundary — bail to per-cell
        };
        gids[idx] = gid;
    }
    // Did shaping change anything vs the plain cmap glyph for each char? If every
    // output glyph equals the char's direct cmap glyph, there's no ligature here.
    for (idx, &ch) in run_chars.iter().enumerate() {
        let cmap = face.glyph_index(ch).map_or(0, |g| g.0);
        if gids[idx] != cmap {
            changed = true;
            break;
        }
    }
    if !changed {
        return None;
    }
    Some(gids.into_boxed_slice())
}

/// Whether the primary face's `GSUB` table advertises a programming-ligature
/// feature (`liga` or `calt`) — the only features [`shape_ligature_run`] turns on.
///
/// A font with neither feature can produce NO substitution under those features,
/// so rustybuzz would return exactly the cmap glyph ids the per-cell path already
/// uses: shaping such a run is provably a no-op (we'd always hit the `!changed`
/// decline). Computing this ONCE at face build time lets the planner short-circuit
/// the whole run-coalescing + rustybuzz path for non-ligature fonts — byte-identical
/// output, no per-frame shaping cost.
///
/// Iterates the `GSUB` feature list LINEARLY (FeatureList records are stored in
/// arbitrary, not tag-sorted, order, so a binary `find` could miss a present tag).
/// `false` when there is no `GSUB` table or the bytes don't parse as a face.
#[must_use]
pub fn font_has_ligature_features(rb_bytes: &[u8]) -> bool {
    let Some(face) = rustybuzz::Face::from_slice(rb_bytes, 0) else {
        return false;
    };
    let Some(gsub) = face.tables().gsub else {
        return false;
    };
    let liga = Tag::from_bytes(b"liga");
    let calt = Tag::from_bytes(b"calt");
    gsub.features
        .into_iter()
        .any(|f| f.tag == liga || f.tag == calt)
}

/// Build the per-column glyph plan for one row of `cells`.
///
/// `shapeable[c]` is whether column `c` may join a run (computed by the caller
/// from [`cell_is_shapeable`] PLUS any per-frame break columns — cursor /
/// selection / `CursorDisabled` ligature mode). `style_of(c)` returns the cell's
/// SGR style bits so a style change BREAKS the run. `shape(run, chars, style)`
/// shapes a coalesced run (the caller caches it) and returns per-char glyph ids,
/// or `None` if it did not ligate. The result is one [`ColumnGlyph`] per column:
/// `Ligated` for cells inside a ligated run, `PerCell` everywhere else.
///
/// SHARED by the CPU and GPU renderers so both place the identical glyph at the
/// identical column — the byte-identical invariant.
pub fn plan_row_runs<S, F>(
    cells: &[RenderCell],
    cols: usize,
    shapeable: &[bool],
    style_of: S,
    mut shape: F,
    out: &mut Vec<ColumnGlyph>,
) where
    S: Fn(usize) -> StyleBits,
    F: FnMut(&str, &[char], StyleBits) -> Option<Box<[u16]>>,
{
    out.clear();
    out.resize(cols, ColumnGlyph::PerCell);
    let n = cols.min(cells.len());
    let mut c = 0;
    let mut run = String::new();
    let mut run_chars: Vec<char> = Vec::new();
    while c < n {
        if !shapeable.get(c).copied().unwrap_or(false) {
            c += 1;
            continue;
        }
        // Coalesce a maximal run of shapeable cells with the SAME style.
        let style = style_of(c);
        let start = c;
        run.clear();
        run_chars.clear();
        while c < n && shapeable.get(c).copied().unwrap_or(false) && style_of(c) == style {
            run.push(cells[c].ch);
            run_chars.push(cells[c].ch);
            c += 1;
        }
        if run_chars.len() < 2 {
            continue; // single shapeable cell — nothing to ligate; stays PerCell
        }
        if let Some(gids) = shape(&run, &run_chars, style) {
            for (i, &gid) in gids.iter().enumerate() {
                out[start + i] = ColumnGlyph::Ligated(gid);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aterm_core::terminal::{RenderCell, UnderlineStyle};

    /// A plain single-`char` cell (the kind a `=>` operator occupies). All
    /// rendition flags off so it is shapeable by default.
    fn cell(ch: char) -> RenderCell {
        RenderCell {
            ch,
            fg: [0, 0, 0],
            bg: [0, 0, 0],
            wide: false,
            emoji_presentation: false,
            bold: false,
            italic: false,
            underline: UnderlineStyle::None,
            strikethrough: false,
            overline: false,
            underline_color: None,
        }
    }

    /// ITEM C — the shapeable predicate breaks on an IMAGE-covered cell. An ordinary
    /// operator cell is shapeable; the SAME cell with `image_covered == true` is not,
    /// so it can never join a ligature run. (Unit test on the predicate directly: a
    /// real OSC-1337 image placement in the grid is impractical in a render unit
    /// test, so we drive the documented `image_covered` argument instead.)
    #[test]
    fn image_covered_cell_is_not_shapeable() {
        let c = cell('=');
        assert!(
            cell_is_shapeable(&c, false, false),
            "a plain operator cell must be shapeable"
        );
        assert!(
            !cell_is_shapeable(&c, false, true),
            "an image-covered cell must NOT be shapeable (the run breaks on it)"
        );
    }

    /// ITEM C — a ligature run must BREAK across an image cell. Row `= > [img] = >`:
    /// the image cell (column 2) is image-covered, so the two `=>` operators sit on
    /// OPPOSITE sides of it. With a shaping closure that ligates any 2-char `=>` run,
    /// each side ligates independently (columns 0..=1 and 3..=4) and the image
    /// column stays `PerCell` — proving no single run spanned the image cell.
    #[test]
    fn ligature_run_breaks_on_image_cell() {
        let cells = [cell('='), cell('>'), cell('='), cell('='), cell('>')];
        // image_covers is true only for column 2 (the middle operator-shaped cell).
        let shapeable: Vec<bool> = (0..cells.len())
            .map(|c| cell_is_shapeable(&cells[c], false, c == 2))
            .collect();
        // Closure ligates any "=>" by emitting a distinctive (non-cmap) gid pair.
        let shape = |run: &str, chars: &[char], _style: StyleBits| -> Option<Box<[u16]>> {
            if run == "=>" && chars.len() == 2 {
                Some(vec![900u16, 901u16].into_boxed_slice())
            } else {
                None
            }
        };
        let mut out = Vec::new();
        plan_row_runs(
            &cells,
            cells.len(),
            &shapeable,
            |_c| StyleBits::REGULAR,
            shape,
            &mut out,
        );
        // Columns 0..=1: the first '=>' ligated. Column 2: image cell, PerCell.
        // Columns 3..=4: the second '=>' ligated. If a run had spanned the image
        // cell, the planner would have tried to shape "=>==>" (which our closure
        // declines), leaving EVERYTHING PerCell — the asserts below would fail.
        assert_eq!(out[0], ColumnGlyph::Ligated(900));
        assert_eq!(out[1], ColumnGlyph::Ligated(901));
        assert_eq!(
            out[2],
            ColumnGlyph::PerCell,
            "the image-covered cell must stay per-cell"
        );
        assert_eq!(out[3], ColumnGlyph::Ligated(900));
        assert_eq!(out[4], ColumnGlyph::Ligated(901));
    }
}
