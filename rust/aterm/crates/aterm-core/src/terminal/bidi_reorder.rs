// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
// Author: Andrew Yates

//! BiDi visual-reordering bridge (feature `bidi`).
//!
//! Wires the engine's BiDi configuration ([`BiDiMode`] + [`ParagraphDirection`])
//! to the UAX #9 implicit reordering in the `aterm-bidi` crate. Compiled ONLY
//! when the off-by-default `bidi` feature is enabled; with the feature off this
//! module is absent and the engine build is byte-identical (the no-op posture in
//! `bidi_stubs.rs` is unaffected).
//!
//! Scope: this produces the per-line visual→logical column permutation AND
//! applies it at render time. [`Terminal::apply_bidi_reorder`] permutes each row
//! of the render snapshot (built by `cell_frame_into`) into visual order, so with
//! the `bidi` feature enabled (it is, in `aterm-gui`) RTL runs display correctly
//! on BOTH the CPU and GPU renderers and in the `image` introspection capture.
//! Runtime-gated by [`BiDiMode`] (default `Implicit`); pure-LTR rows are skipped.

use super::Terminal;
use aterm_types::{BiDiMode, ParagraphDirection};

impl Terminal {
    /// Visual→logical column permutation for a row whose cells hold the scalar
    /// values `scalars`, honoring this terminal's current BiDi mode/direction.
    ///
    /// `result[v] == l` means the logical cell at index `l` is drawn at visual
    /// column `v` (left to right). Returns the identity permutation `0,1,2,…` when
    /// BiDi is disabled or the line is pure left-to-right, so a renderer can apply
    /// the result unconditionally.
    #[must_use]
    pub fn bidi_visual_order(&self, scalars: &[char]) -> Vec<usize> {
        compute_visual_order(self.modes.bidi_mode, self.modes.bidi_direction, scalars)
    }

    /// Visual→logical CELL permutation for a rendered row, honoring this terminal's
    /// BiDi mode/direction AND wide-glyph cell pairing.
    ///
    /// A renderer holds [`RenderCell`](super::RenderCell) rows; a wide glyph (CJK,
    /// wide emoji) occupies two cells — a lead cell plus a right-half continuation
    /// — that must stay paired and unmirrored when the line is reordered.
    /// `result[v] == l` means the logical cell at index `l` is drawn at visual
    /// column `v` (left to right). Returns the identity permutation when BiDi is
    /// disabled or the row is pure left-to-right, so a renderer can apply it
    /// unconditionally. This is the cell-level companion to [`Self::bidi_visual_order`]
    /// and the entry point a renderer integration calls per visible row.
    #[must_use]
    pub fn bidi_visual_order_cells(&self, cells: &[super::RenderCell]) -> Vec<usize> {
        // Fast path: with BiDi off, skip the per-cell scalar/flag allocations.
        if self.modes.bidi_mode == BiDiMode::Disabled {
            return (0..cells.len()).collect();
        }
        let chars: Vec<char> = cells.iter().map(|c| c.ch).collect();
        let wide: Vec<bool> = cells.iter().map(|c| c.wide).collect();
        compute_visual_order_cells(
            self.modes.bidi_mode,
            self.modes.bidi_direction,
            &chars,
            &wide,
        )
    }

    /// Reorder each row of a render snapshot into BiDi VISUAL order, in place.
    ///
    /// This is the render-time application of [`Self::bidi_visual_order_cells`]:
    /// it permutes the dense [`RenderCell`](super::RenderCell) row AND the
    /// column-indexed sparse arrays (clusters / combining marks / inline images)
    /// AND the cursor column, so a renderer that draws the snapshot left-to-right
    /// shows right-to-left runs in the correct visual order. Called from
    /// [`Terminal::cell_frame_into`](super::Terminal::cell_frame_into), so BOTH
    /// the CPU and GPU renderers (and the `image` introspection capture) get
    /// visual order for free.
    ///
    /// Pure-LTR rows are skipped via a cheap first-RTL-block guard, so frames
    /// with no right-to-left content are byte-identical to the non-BiDi path.
    /// A no-op when BiDi is disabled.
    pub(crate) fn apply_bidi_reorder(&self, frame: &mut crate::render::RenderInput) {
        if self.modes.bidi_mode == BiDiMode::Disabled {
            return;
        }
        for r in 0..frame.cells.len() {
            // Compute the visual->logical permutation for this row. The cheap
            // guard skips the allocation for any row with no codepoint in or
            // after the first RTL block (U+0590); only such rows can reorder.
            let order = {
                let row = &frame.cells[r];
                if !row.iter().any(|c| c.ch >= '\u{0590}') {
                    continue;
                }
                self.bidi_visual_order_cells(row)
            };
            // Identity (e.g. RTL-capable chars that still resolve LTR): skip.
            if order.iter().enumerate().all(|(v, &l)| v == l) {
                continue;
            }
            // Inverse permutation: inv[logical] = visual column.
            let mut inv = vec![0usize; order.len()];
            for (v, &l) in order.iter().enumerate() {
                inv[l] = v;
            }
            // Dense cells: visual[v] = logical[order[v]] (RenderCell is Copy).
            let visual: Vec<super::RenderCell> = order.iter().map(|&l| frame.cells[r][l]).collect();
            frame.cells[r] = visual;
            // Sparse, column-indexed arrays: remap each logical col to visual.
            fn remap_cols<T>(entries: &mut [(usize, T)], inv: &[usize]) {
                for (c, _) in entries.iter_mut() {
                    if *c < inv.len() {
                        *c = inv[*c];
                    }
                }
            }
            remap_cols(&mut frame.clusters[r], &inv);
            remap_cols(&mut frame.combining[r], &inv);
            remap_cols(&mut frame.images[r], &inv);
            // The cursor follows its logical cell to its new visual column.
            if frame.cursor_row == r && frame.cursor_col < inv.len() {
                frame.cursor_col = inv[frame.cursor_col];
            }
        }
    }
}

/// Pure mapping from BiDi config + line scalars to the visual→logical permutation.
///
/// Kept free-standing (not a method) so it is testable without constructing a
/// `Terminal`. `Terminal::bidi_visual_order` is the one-line wrapper over it.
#[must_use]
pub fn compute_visual_order(
    mode: BiDiMode,
    dir: ParagraphDirection,
    scalars: &[char],
) -> Vec<usize> {
    // Disabled, or a pure-LTR line: identity (the common, hot case).
    if mode == BiDiMode::Disabled || !aterm_bidi::has_bidi(scalars) {
        return (0..scalars.len()).collect();
    }
    let base = base_direction(dir, scalars);
    aterm_bidi::reorder_visual_to_logical(scalars, base)
}

/// Cell-level companion to [`compute_visual_order`]: parallel per-cell scalar and
/// wide-continuation slices in, visual→logical CELL permutation out. Kept
/// free-standing so it is testable without constructing a `Terminal`; wide-glyph
/// cell pairs are kept together (see [`aterm_bidi::reorder_cells`]).
#[must_use]
pub fn compute_visual_order_cells(
    mode: BiDiMode,
    dir: ParagraphDirection,
    cell_chars: &[char],
    is_wide_continuation: &[bool],
) -> Vec<usize> {
    if mode == BiDiMode::Disabled || !aterm_bidi::has_bidi(cell_chars) {
        return (0..cell_chars.len()).collect();
    }
    let base = base_direction(dir, cell_chars);
    aterm_bidi::reorder_cells(cell_chars, is_wide_continuation, base)
}

/// Map the engine's [`ParagraphDirection`] onto an `aterm-bidi` `BaseDirection`.
///
/// `AutoRtl` (auto-detect, default RTL when the line has no strong character) has
/// no direct UAX #9 analogue: it resolves to `Auto` when a strong L/R/AL character
/// is present and `Rtl` otherwise, matching its "default to RTL" intent.
fn base_direction(dir: ParagraphDirection, scalars: &[char]) -> aterm_bidi::BaseDirection {
    use aterm_bidi::{BaseDirection, BidiClass};
    match dir {
        ParagraphDirection::Auto => BaseDirection::Auto,
        ParagraphDirection::Ltr => BaseDirection::Ltr,
        ParagraphDirection::Rtl => BaseDirection::Rtl,
        ParagraphDirection::AutoRtl => {
            let has_strong = scalars.iter().any(|&c| {
                matches!(
                    aterm_bidi::bidi_class(c),
                    BidiClass::L | BidiClass::R | BidiClass::AL
                )
            });
            if has_strong {
                BaseDirection::Auto
            } else {
                BaseDirection::Rtl
            }
        }
        // `ParagraphDirection` is #[non_exhaustive]; treat any future variant as
        // auto-detection (the safe, spec-default behavior).
        _ => BaseDirection::Auto,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::Terminal;

    const ALEF: char = '\u{05D0}';
    const BET: char = '\u{05D1}';

    /// End-to-end: a render snapshot of a Hebrew line is reordered into visual
    /// (right-to-left) order by `cell_frame` (which calls `apply_bidi_reorder`),
    /// while a pure-ASCII line is left in logical order.
    #[test]
    fn cell_frame_reorders_rtl_row_visually() {
        let mut term = Terminal::new(4, 8);
        // Default BiDiMode is Implicit, so a fresh terminal reorders RTL.
        assert_ne!(term.modes.bidi_mode, BiDiMode::Disabled);

        // Write three Hebrew letters: logical order ALEF, BET, GIMEL.
        term.process("\u{05D0}\u{05D1}\u{05D2}".as_bytes());
        let frame = term.cell_frame(4, 8);
        let row: Vec<char> = frame.cells[0].iter().take(3).map(|c| c.ch).collect();
        // Visual order is reversed for an RTL run.
        assert_eq!(
            row,
            vec!['\u{05D2}', '\u{05D1}', '\u{05D0}'],
            "Hebrew run must render right-to-left in the snapshot"
        );
    }

    #[test]
    fn cell_frame_leaves_ascii_row_in_logical_order() {
        let mut term = Terminal::new(4, 8);
        term.process(b"abc");
        let frame = term.cell_frame(4, 8);
        let row: Vec<char> = frame.cells[0].iter().take(3).map(|c| c.ch).collect();
        assert_eq!(row, vec!['a', 'b', 'c'], "ASCII stays in logical order");
    }

    fn cv(mode: BiDiMode, dir: ParagraphDirection, s: &str) -> Vec<usize> {
        compute_visual_order(mode, dir, &s.chars().collect::<Vec<_>>())
    }

    #[test]
    fn disabled_is_always_identity() {
        // Even with RTL content, Disabled keeps logical order.
        assert_eq!(
            cv(
                BiDiMode::Disabled,
                ParagraphDirection::Auto,
                "\u{05D0}\u{05D1}"
            ),
            vec![0, 1]
        );
    }

    #[test]
    fn pure_ltr_is_identity_without_invoking_reorder() {
        assert_eq!(
            cv(BiDiMode::Implicit, ParagraphDirection::Auto, "abc"),
            vec![0, 1, 2]
        );
    }

    #[test]
    fn implicit_reorders_rtl() {
        // Hebrew run reverses under implicit auto-detection.
        assert_eq!(
            cv(
                BiDiMode::Implicit,
                ParagraphDirection::Auto,
                "\u{05D0}\u{05D1}"
            ),
            vec![1, 0]
        );
    }

    #[test]
    fn explicit_mode_also_reorders_via_the_bridge() {
        // The bridge reorders for any non-Disabled mode (Explicit included) — the
        // mode distinction (engine-driven vs auto) is handled upstream; here a
        // non-disabled mode means "produce a visual order".
        assert_eq!(
            cv(BiDiMode::Explicit, ParagraphDirection::Rtl, "ab"),
            vec![0, 1]
        );
    }

    #[test]
    fn autortl_defaults_rtl_only_without_strong_chars() {
        // A neutral-only line under AutoRtl uses an RTL base; with a strong char it
        // auto-detects normally.
        assert_eq!(
            base_direction(ParagraphDirection::AutoRtl, &[' ', '.']),
            aterm_bidi::BaseDirection::Rtl
        );
        assert_eq!(
            base_direction(ParagraphDirection::AutoRtl, &['a']),
            aterm_bidi::BaseDirection::Auto
        );
        assert_eq!(
            base_direction(ParagraphDirection::AutoRtl, &[ALEF]),
            aterm_bidi::BaseDirection::Auto
        );
    }

    #[test]
    fn terminal_method_uses_default_implicit_config() {
        // A fresh Terminal defaults to Implicit/Auto, so the method reorders RTL.
        let term = Terminal::new(24, 80);
        assert_eq!(term.bidi_visual_order(&[ALEF, BET]), vec![1, 0]);
        // Plain ASCII stays identity.
        assert_eq!(term.bidi_visual_order(&['x', 'y', 'z']), vec![0, 1, 2]);
    }

    const CJK: char = '\u{4E2D}'; // 中 — a wide (2-cell) glyph

    #[test]
    fn cells_disabled_is_identity() {
        // Disabled mode: identity even with RTL + wide content.
        let chars = [ALEF, CJK, ' '];
        let wide = [false, false, true];
        assert_eq!(
            compute_visual_order_cells(BiDiMode::Disabled, ParagraphDirection::Auto, &chars, &wide),
            vec![0, 1, 2]
        );
    }

    #[test]
    fn cells_implicit_reorders_rtl_keeping_wide_pair() {
        // ALEF (R, 1 cell) + 中 (L, 2 cells) under implicit auto → 中 moves left of
        // the Hebrew letter but its [lead, continuation] cells stay in order.
        let chars = [ALEF, CJK, ' '];
        let wide = [false, false, true];
        assert_eq!(
            compute_visual_order_cells(BiDiMode::Implicit, ParagraphDirection::Auto, &chars, &wide),
            vec![1, 2, 0]
        );
    }

    #[test]
    fn terminal_cell_method_uses_default_config_and_real_cells() {
        // Drive a real engine so the method runs over actual RenderCell rows.
        let mut term = Terminal::new(2, 8);
        term.process("ab".as_bytes());
        let cells = term.render_row(0);
        // Pure ASCII row → identity.
        assert_eq!(
            term.bidi_visual_order_cells(&cells),
            (0..cells.len()).collect::<Vec<_>>()
        );

        let mut rtl = Terminal::new(2, 8);
        rtl.process("\u{05D0}\u{05D1}".as_bytes()); // ALEF BET
        let rcells = rtl.render_row(0);
        // The two Hebrew lead cells reverse; trailing blanks stay in place.
        let order = rtl.bidi_visual_order_cells(&rcells);
        assert_eq!(
            order[0], 1,
            "first visual cell is the 2nd Hebrew letter; got {order:?}"
        );
        assert_eq!(
            order[1], 0,
            "second visual cell is the 1st Hebrew letter; got {order:?}"
        );
        // Still a permutation of all cells.
        let mut seen = order.clone();
        seen.sort_unstable();
        assert_eq!(seen, (0..rcells.len()).collect::<Vec<_>>());
    }
}
