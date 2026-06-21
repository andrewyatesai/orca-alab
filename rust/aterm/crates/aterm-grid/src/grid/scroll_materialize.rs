// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Full-fidelity scrollback materialization (#4216).
//!
//! Converts scrollback [`Line`]s into [`MaterializedRow`]s that bundle
//! cells with supplementary [`CellExtra`] data for hyperlinks, complex
//! characters, and RGB colors.  The bridge renderer queries these extras
//! the same way it queries visible-area `CellExtras`.

use std::sync::Arc;

use aterm_hash::FxHashMap;
use aterm_scrollback::{CellAttrs, Line};

use crate::CellExtra;
use crate::PackedColor;

/// A scrollback row materialized for rendering with full fidelity.
///
/// Bundles cells with supplementary [`CellExtra`] data for columns that need
/// hyperlinks, complex characters, or RGB colors.  This allows scrollback
/// cells to be rendered identically to visible-area cells.
///
/// The bridge's `RenderableCellIterator` calls [`get_extra`](Self::get_extra)
/// for scrollback cells using the same code path it uses for visible cells.
#[derive(Debug, Clone, Default)]
pub struct MaterializedRow {
    /// The cells for this row (one per column).
    pub cells: Vec<super::Cell>,
    /// Sparse extras for columns that have hyperlinks, complex chars, or RGB.
    extras: FxHashMap<u16, CellExtra>,
}

impl MaterializedRow {
    /// Look up extras for a column (mirrors `CellExtras::get`).
    #[must_use]
    #[inline]
    pub fn get_extra(&self, col: u16) -> Option<&CellExtra> {
        self.extras.get(&col)
    }

    /// Whether this row has no occupied columns.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Compute the effective row length (last occupied column + 1).
    ///
    /// Accounts for wide characters, wide continuations, complex chars,
    /// and combining marks — matching the semantics of `Row::len` for
    /// visible-area rows.
    #[must_use]
    #[allow(clippy::cast_possible_truncation)] // cells.len() bounded by terminal width (≤ u16::MAX)
    pub(crate) fn len(&self) -> u16 {
        self.cells
            .iter()
            .enumerate()
            .rposition(|(idx, cell)| {
                let col = idx as u16;
                cell.char() != ' '
                    || cell.is_wide()
                    || cell.is_wide_continuation()
                    || self.get_extra(col).is_some_and(|extra| {
                        extra.complex_char().is_some() || !extra.combining().is_empty()
                    })
            })
            .and_then(|idx| u16::try_from(idx + 1).ok())
            .unwrap_or(0)
    }
}

/// Materialize a scrollback [`Line`] into a [`MaterializedRow`] with full
/// fidelity.
///
/// Preserves all data by populating [`CellExtra`] entries for columns
/// that need them (hyperlinks, complex chars, RGB colors).
///
/// ## What's restored
///
/// - **Hyperlinks** from `Line::hyperlinks()` → `CellExtra::set_hyperlink`
/// - **Non-BMP characters** (emoji, math symbols) → `CellExtra::set_complex_char`
/// - **ZWJ sequences** (family emoji, flag emoji) → `CellExtra::set_complex_char`
/// - **Combining marks** (diacritics) → `CellExtra::push_combining`
/// - **RGB foreground** (0x01_RRGGBB in CellAttrs) → `CellExtra::set_fg_rgb`
/// - **RGB background** → `CellExtra::set_bg_rgb`
#[must_use]
pub fn materialize_from_line(line: &Line, cols: u16) -> MaterializedRow {
    use crate::{Cell, CellFlags};

    let mut row = MaterializedRow {
        cells: vec![Cell::default(); cols as usize],
        extras: FxHashMap::default(),
    };

    let Some(text) = line.as_str() else {
        return row;
    };

    let mut byte_idx: usize = 0;
    let mut char_idx: usize = 0;
    let mut col: u16 = 0;

    while byte_idx < text.len() && col < cols {
        // byte_idx is always at a char boundary (advanced by char_indices).
        let c = text[byte_idx..]
            .chars()
            .next()
            .expect("invariant: byte_idx < text.len()");

        // Skip orphan zero-width characters at the start of text.
        let base_width = aterm_grapheme::char_width(c);
        if base_width == 0 {
            byte_idx += c.len_utf8();
            char_idx += 1;
            continue;
        }

        let unit_byte_start = byte_idx;
        let unit_char_start = char_idx;
        let chars_consumed = advance_grapheme_unit(text, &mut byte_idx);
        char_idx += chars_consumed;
        let unit_str = &text[unit_byte_start..byte_idx];

        let attrs = line.get_attr(unit_char_start);
        let flags = CellFlags::from_bits(attrs.flags);
        let is_wide = base_width >= 2;
        let is_complex = chars_consumed > 1 || c as u32 > super::Cell::MAX_DIRECT_CODEPOINT;

        let fg = PackedColor(attrs.fg);
        let bg = PackedColor(attrs.bg);

        let prev_col = col;
        col = place_cell(
            &mut row, col, cols, c, unit_str, fg, bg, flags, is_wide, is_complex,
        );

        // Store RGB colors in extras for the cell we just placed.
        // Skip if place_cell didn't advance col (wide char dropped at last column).
        if col > prev_col {
            store_rgb_extras(&mut row.extras, prev_col, cols, &attrs);
        }
    }

    // Restore hyperlinks from Line into extras.
    restore_hyperlinks(&mut row.extras, line, cols);

    row
}

/// Advance `byte_idx` past the current grapheme unit (one cell's worth of
/// characters) in `text`.
///
/// Returns the number of characters consumed, which callers use to maintain
/// a parallel character index for `Line::get_attr`.
///
/// Consumes the base character plus any following zero-width chars
/// (combining marks, variation selectors) and ZWJ-joined characters.
/// Callers use `&text[start..*byte_idx]` to access the consumed `&str`
/// slice without heap allocation (#5949).
///
/// Used by both `materialize_from_line` and `fill_row_from_line` to ensure
/// consistent grapheme handling when recovering content from scrollback.
pub(crate) fn advance_grapheme_unit(text: &str, byte_idx: &mut usize) -> usize {
    let remaining = &text[*byte_idx..];
    let mut iter = remaining.char_indices();
    let Some((_, c)) = iter.next() else {
        return 0;
    };

    let mut end = c.len_utf8();
    let mut char_count: usize = 1;
    let mut last_was_zwj = c == '\u{200D}';

    for (offset, next_c) in iter {
        let next_width = aterm_grapheme::char_width(next_c);

        if next_c == '\u{200D}' {
            end = offset + next_c.len_utf8();
            char_count += 1;
            last_was_zwj = true;
        } else if next_width == 0 || last_was_zwj {
            // Zero-width chars (combining marks, variation selectors)
            // or the visible char after a ZWJ — both join the current unit.
            end = offset + next_c.len_utf8();
            char_count += 1;
            last_was_zwj = false;
        } else {
            break;
        }
    }

    *byte_idx += end;
    char_count
}

/// Place a cell (complex, wide, or normal) into the materialized row.
///
/// Accepts `unit_str` as a `&str` slice borrowed from the source text,
/// avoiding heap allocation entirely (#5949). `Arc::from(unit_str)` is
/// used directly for complex characters that need storage.
///
/// Returns the new column position after placement.
#[allow(clippy::too_many_arguments)]
fn place_cell(
    row: &mut MaterializedRow,
    col: u16,
    cols: u16,
    c: char,
    unit_str: &str,
    fg: PackedColor,
    bg: PackedColor,
    flags: crate::CellFlags,
    is_wide: bool,
    is_complex: bool,
) -> u16 {
    use crate::{Cell, CellFlags};

    let cell_flags = if is_wide {
        flags.union(CellFlags::WIDE)
    } else {
        flags
    };

    if is_complex {
        let mut cell = Cell::with_style(' ', fg, bg, cell_flags);
        cell.set_overflow_index(0);

        row.extras
            .entry(col)
            .or_default()
            .set_complex_char(Some(Arc::from(unit_str)));

        if is_wide && col + 1 < cols {
            row.cells[col as usize] = cell;
            row.cells[(col + 1) as usize] =
                Cell::with_style(' ', fg, bg, CellFlags::WIDE_CONTINUATION);
            col.saturating_add(2)
        } else if !is_wide {
            row.cells[col as usize] = cell;
            col.saturating_add(1)
        } else {
            col // wide at last column — drop
        }
    } else if is_wide {
        if col + 1 < cols {
            row.cells[col as usize] = Cell::with_style(c, fg, bg, cell_flags);
            row.cells[(col + 1) as usize] =
                Cell::with_style(' ', fg, bg, CellFlags::WIDE_CONTINUATION);
            col.saturating_add(2)
        } else {
            col
        }
    } else {
        row.cells[col as usize] = Cell::with_style(c, fg, bg, cell_flags);
        col.saturating_add(1)
    }
}

/// Store RGB color data from CellAttrs into extras.
fn store_rgb_extras(
    extras: &mut FxHashMap<u16, CellExtra>,
    placed_col: u16,
    cols: u16,
    attrs: &CellAttrs,
) {
    if placed_col >= cols {
        return;
    }
    let fg_is_rgb = (attrs.fg >> 24) == 0x01;
    let bg_is_rgb = (attrs.bg >> 24) == 0x01;
    if !fg_is_rgb && !bg_is_rgb {
        return;
    }
    let extra = extras.entry(placed_col).or_default();
    if fg_is_rgb {
        let r = ((attrs.fg >> 16) & 0xFF) as u8;
        let g = ((attrs.fg >> 8) & 0xFF) as u8;
        let b = (attrs.fg & 0xFF) as u8;
        extra.set_fg_rgb(Some([r, g, b]));
    }
    if bg_is_rgb {
        let r = ((attrs.bg >> 16) & 0xFF) as u8;
        let g = ((attrs.bg >> 8) & 0xFF) as u8;
        let b = (attrs.bg & 0xFF) as u8;
        extra.set_bg_rgb(Some([r, g, b]));
    }
}

/// Restore hyperlinks from Line into extras.
fn restore_hyperlinks(extras: &mut FxHashMap<u16, CellExtra>, line: &Line, cols: u16) {
    if let Some(spans) = line.hyperlinks() {
        for span in spans {
            for hcol in span.start_col..span.end_col.min(cols) {
                let extra = extras.entry(hcol).or_default();
                extra.set_hyperlink(Some(span.url.clone()));
                extra.set_hyperlink_id(span.id.clone());
            }
        }
    }
}
