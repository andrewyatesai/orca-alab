// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! The engine-owned render SNAPSHOT (`read_image` boundary, REARCH A-3).
//!
//! [`RenderInput`] is the plain-owned, `Terminal`-free value a renderer reads to
//! paint one frame. Historically the CPU renderer reached into `&Terminal`
//! internals to build it (`Renderer::extract_into`); A-3 inverts that boundary so
//! the ENGINE produces the snapshot ([`Terminal::cell_frame_into`]) and the
//! renderer becomes a PURE consumer of this value — no `&Terminal`, no reach into
//! core. Hosting the type HERE (rather than in `aterm-render-api`) lets
//! `aterm-core` build the snapshot without a dependency cycle: `aterm-render-api`
//! re-exports `RenderInput` from here, so every existing
//! `aterm_render::RenderInput` / `aterm_render_api::RenderInput` call site is
//! unchanged.

use crate::grid::LineSize;
use crate::selection::TextSelection;
use crate::terminal::{CursorStyle, RenderCell};

/// Everything a renderer reads from a `&Terminal` for one frame, snapshotted into
/// plain owned data — the engine emits it via [`crate::terminal::Terminal::cell_frame_into`].
///
/// The windowed frontend holds the `Terminal` mutex only long enough to extract
/// this struct, then renders WITHOUT the lock — so the PTY reader thread is no
/// longer starved for the multi-millisecond duration of a frame (CPU
/// rasterization or GPU encode + readback).
///
/// `PartialEq`/`Eq` are hand-written and compare only the rendered CONTENT (every
/// field EXCEPT [`snapshot_seq`](RenderInput::snapshot_seq)): the CPU renderer's
/// damage-tracking fast path compares a fresh `RenderInput` against the one it
/// cached last frame — overall and per row — to decide which rows changed (and
/// whether anything changed at all). `snapshot_seq` is pure metadata that
/// advances on every damaged frame, so including it in the comparison would make
/// equality ALWAYS differ and defeat the row-level reuse / dirty-gate; it is
/// therefore excluded. Content equality stays exact (byte-for-byte intent), which
/// is what the no-visual-regression contract requires.
#[derive(Debug)]
pub struct RenderInput {
    /// Number of visible rows this frame was extracted for.
    pub rows: usize,
    /// Number of columns this frame was extracted for.
    pub cols: usize,
    /// One resolved `RenderCell` row per visible row, in viewport order.
    pub cells: Vec<Vec<RenderCell>>,
    /// Cursor cell row.
    pub cursor_row: usize,
    /// Cursor cell column.
    pub cursor_col: usize,
    /// DECTCEM cursor visibility.
    pub cursor_visible: bool,
    /// The terminal's own DECSCUSR style. The frontend's unfocused override is
    /// NOT baked in here — it lives on the renderer and is applied in
    /// `render_input`.
    pub cursor_style: CursorStyle,
    /// Scrollback offset: viewport row `r` shows live row `r - display_offset`.
    pub display_offset: i32,
    /// A clone of the active text selection, for per-cell highlighting.
    pub selection: TextSelection,
    /// Per-row, sparse emoji grapheme-cluster strings (`term.cluster_row(r)`):
    /// `(col, cluster)` for cells whose combining marks form a ZWJ / skin-tone /
    /// keycap sequence. The renderer shapes each to a single colour glyph; cells
    /// absent here take the ordinary single-codepoint dispatch.
    pub clusters: Vec<Vec<(usize, Box<str>)>>,
    /// Per-row, sparse combining MARKS (`term.combining_row(r)`): `(col, marks)`
    /// for cells with diacritics (é, ñ, …). The renderer overlays each mark's
    /// glyph on the base so accents render.
    pub combining: Vec<Vec<(usize, Box<[char]>)>>,
    /// Per-row DEC line size (DECDWL/DECDHL via `ESC # 3..6`): the renderer draws
    /// double-width / double-height rows scaled. `SingleWidth` (the default) is
    /// the ordinary path.
    pub line_sizes: Vec<LineSize>,
    /// Per-row, sparse inline-image placements (`term.images_row(r)`):
    /// `(col, ImageRef)` for every cell covered by an iTerm2 OSC 1337 `File=`
    /// image. The renderer decodes each image once (keyed by the `Arc` inside the
    /// ref) and blits the cell's tile; a covered cell SKIPS its glyph (the bg
    /// still fills). Cells absent here take the ordinary glyph dispatch, so a
    /// frame with no images is byte-identical to the pre-image path.
    pub images: Vec<Vec<(usize, aterm_grid::ImageRef)>>,
    /// The engine's monotone damage epoch at snapshot time (A-3 read_image seq):
    /// the value of [`Terminal::damage_epoch`](crate::terminal::Terminal::damage_epoch)
    /// captured under the SAME lock that filled the rest of this snapshot. It is a
    /// version stamp, not rendered content — a consumer that records it can detect
    /// staleness (compare against a later `damage_epoch()`), and because the whole
    /// snapshot is filled under one lock, the value is internally consistent (no
    /// torn read). Deliberately EXCLUDED from `PartialEq`/`Eq` (see the type doc):
    /// it advances every damaged frame, so counting it would defeat the renderer's
    /// content-based damage cache.
    pub snapshot_seq: u64,
}

impl Clone for RenderInput {
    /// A fresh deep copy of every field (`snapshot_seq` included). Equivalent to a
    /// derived `clone`; used by the snapshot seed paths.
    fn clone(&self) -> Self {
        RenderInput {
            rows: self.rows,
            cols: self.cols,
            cells: self.cells.clone(),
            cursor_row: self.cursor_row,
            cursor_col: self.cursor_col,
            cursor_visible: self.cursor_visible,
            cursor_style: self.cursor_style,
            display_offset: self.display_offset,
            selection: self.selection.clone(),
            clusters: self.clusters.clone(),
            combining: self.combining.clone(),
            line_sizes: self.line_sizes.clone(),
            images: self.images.clone(),
            snapshot_seq: self.snapshot_seq,
        }
    }

    /// CAPACITY-REUSING in-place update — the persistent-snapshot path the GPU
    /// present + CPU damage caches use to store the prior frame each changed frame.
    /// The derived `clone_from` falls back to `*self = source.clone()`, which
    /// deep-clones a fresh grid and drops the old one every call; this override
    /// delegates to each field's `clone_from` so `Vec::clone_from` reuses the
    /// destination's existing allocation for the common prefix (inner per-row Vecs
    /// recurse), so a stable-dimension frame reallocates NOTHING for the grid. The
    /// result is byte-for-byte identical to `*self = source.clone()`; only the
    /// allocation lifetime changes, so the same dirty sets follow from the same
    /// stored snapshot. (Ported from the prior render-api location under A-3.)
    fn clone_from(&mut self, source: &Self) {
        self.rows = source.rows;
        self.cols = source.cols;
        self.cells.clone_from(&source.cells);
        self.cursor_row = source.cursor_row;
        self.cursor_col = source.cursor_col;
        self.cursor_visible = source.cursor_visible;
        self.cursor_style = source.cursor_style;
        self.display_offset = source.display_offset;
        self.selection.clone_from(&source.selection);
        self.clusters.clone_from(&source.clusters);
        self.combining.clone_from(&source.combining);
        self.line_sizes.clone_from(&source.line_sizes);
        self.images.clone_from(&source.images);
        self.snapshot_seq = source.snapshot_seq;
    }
}

// Hand-written equality: compare rendered CONTENT only, NOT `snapshot_seq`.
// The CPU damage cache (`aterm-render`) compares the incoming snapshot against the
// previous frame's to decide which rows are dirty; `snapshot_seq` is metadata that
// changes every damaged frame, so including it would make every frame compare
// unequal and defeat row-level reuse. Every content field is itself `Eq`.
impl PartialEq for RenderInput {
    fn eq(&self, other: &Self) -> bool {
        self.rows == other.rows
            && self.cols == other.cols
            && self.cells == other.cells
            && self.cursor_row == other.cursor_row
            && self.cursor_col == other.cursor_col
            && self.cursor_visible == other.cursor_visible
            && self.cursor_style == other.cursor_style
            && self.display_offset == other.display_offset
            && self.selection == other.selection
            && self.clusters == other.clusters
            && self.combining == other.combining
            && self.line_sizes == other.line_sizes
            && self.images == other.images
        // `snapshot_seq` intentionally NOT compared — see the impl comment.
    }
}

impl Eq for RenderInput {}

impl Default for RenderInput {
    fn default() -> Self {
        Self::empty()
    }
}

impl RenderInput {
    /// An empty 0×0 snapshot with no allocations — the seed for a persistent
    /// scratch buffer that [`Terminal::cell_frame_into`](crate::terminal::Terminal::cell_frame_into)
    /// refills in place each frame (C-1). Cursor scalars default to off/origin and
    /// `snapshot_seq` to 0; the first `cell_frame_into` overwrites every field.
    #[must_use]
    pub fn empty() -> Self {
        RenderInput {
            rows: 0,
            cols: 0,
            cells: Vec::new(),
            cursor_row: 0,
            cursor_col: 0,
            cursor_visible: false,
            cursor_style: CursorStyle::default(),
            display_offset: 0,
            selection: TextSelection::new(),
            clusters: Vec::new(),
            combining: Vec::new(),
            line_sizes: Vec::new(),
            images: Vec::new(),
            snapshot_seq: 0,
        }
    }

    /// The emoji grapheme-cluster string at viewport cell `(row, col)`, if this
    /// frame captured one there (a ZWJ / skin-tone / keycap sequence). Used by
    /// the GPU atlas builder to resolve keys exactly as the CPU blit does.
    #[must_use]
    pub fn cluster_at(&self, row: usize, col: usize) -> Option<&str> {
        self.clusters
            .get(row)?
            .iter()
            .find(|(c, _)| *c == col)
            .map(|(_, s)| s.as_ref())
    }

    /// The combining marks to overlay at viewport cell `(row, col)`, if any.
    #[must_use]
    pub fn combining_at(&self, row: usize, col: usize) -> Option<&[char]> {
        self.combining
            .get(row)?
            .iter()
            .find(|(c, _)| *c == col)
            .map(|(_, m)| m.as_ref())
    }

    /// The inline-image reference covering viewport cell `(row, col)`, if any.
    /// A cell with an image SKIPS its glyph on both the CPU and GPU paths; the
    /// renderer blits the image tile instead. Used by both renderers to stay in
    /// lockstep on the image-vs-glyph precedence rule.
    #[must_use]
    pub fn image_at(&self, row: usize, col: usize) -> Option<&aterm_grid::ImageRef> {
        self.images
            .get(row)?
            .iter()
            .find(|(c, _)| *c == col)
            .map(|(_, r)| r)
    }

    /// Whether the image at (`row`,`col`), if any, HIDES the cell's glyph — i.e. it
    /// is drawn OVER the text (`z_index >= 0`, the default). A Kitty `z < 0` image is
    /// drawn BEHIND the text, so it does NOT hide the glyph and this returns `false`.
    /// Both renderers gate glyph drawing on this, so the image/text z-order matches.
    #[must_use]
    pub fn image_hides_glyph_at(&self, row: usize, col: usize) -> bool {
        self.image_at(row, col)
            .is_some_and(|r| r.image.z_index >= 0)
    }
}

#[cfg(test)]
mod z_index_tests {
    use super::RenderInput;
    use aterm_grid::{ImageData, ImageFormat, ImageRef};
    use std::sync::Arc;

    fn image_ref(z: i32) -> ImageRef {
        ImageRef {
            image: Arc::new(ImageData {
                bytes: Vec::new(),
                format: ImageFormat::Png,
                cols: 1,
                rows: 1,
                z_index: z,
            }),
            cell_row: 0,
            cell_col: 0,
        }
    }

    #[test]
    fn image_hides_glyph_only_when_z_is_nonnegative() {
        let mut input = RenderInput::empty();
        // col 0: z=0 (over text, default) — hides; col 1: z=-1 (behind) — does NOT;
        // col 2: z=5 (over) — hides; col 3: no image.
        input.images = vec![vec![
            (0, image_ref(0)),
            (1, image_ref(-1)),
            (2, image_ref(5)),
        ]];
        assert!(
            input.image_hides_glyph_at(0, 0),
            "z=0 image hides the glyph"
        );
        assert!(
            !input.image_hides_glyph_at(0, 1),
            "z<0 image draws BEHIND text — glyph still paints"
        );
        assert!(
            input.image_hides_glyph_at(0, 2),
            "z>0 image hides the glyph"
        );
        assert!(
            !input.image_hides_glyph_at(0, 3),
            "no image at the column — nothing hides the glyph"
        );
    }
}
