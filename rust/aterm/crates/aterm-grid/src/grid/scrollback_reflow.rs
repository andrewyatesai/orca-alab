// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Off-screen scrollback reflow: rewrap history [`Line`]s on a width change.
//!
//! The visible-grid reflow in [`super::reflow`] only rewraps the on-screen
//! rows; the off-screen scrollback (tiered storage, lazy buffer, and ring-
//! buffer scrollback rows) lives outside that window. Without this step a
//! resize would silently drop ALL history (#7906). This rewraps the full
//! history at the logical-line level — joining soft-wrapped runs, then
//! re-splitting by DISPLAY WIDTH to the new column count — preserving text,
//! per-character attributes, and hyperlinks in both directions.

use aterm_rle::Rle;
use aterm_scrollback::{CellAttrs, HyperlinkSpan, Line};

use super::Grid;
use super::scroll_convert::DeferredLine;

impl Grid {
    /// Extract the ENTIRE off-screen scrollback as logical [`Line`]s (oldest
    /// first) and remove it from the grid, leaving only the visible rows.
    ///
    /// Order matches `get_history_line`: tiered scrollback, then the lazy
    /// buffer, then ring-buffer scrollback. After this returns the grid has no
    /// scrollback (tiered cleared, lazy drained, ring scrollback rows dropped),
    /// so the visible-grid reflow runs against a clean history that this
    /// function's rewrapped output is later prepended to.
    pub(super) fn take_scrollback_lines(&mut self) -> Vec<Line> {
        // Tiered + lazy: materialize then clear. drain_lazy_buffer pushes lazy
        // lines into tiered, so a single tiered sweep then covers both.
        self.drain_lazy_buffer();
        let mut lines = Vec::new();
        if let Some(scrollback) = self.storage.scrollback.as_mut() {
            let count = scrollback.line_count();
            lines.reserve(count);
            for i in 0..count {
                match scrollback.get_line(i) {
                    Ok(Some(line)) => lines.push(line.into_owned()),
                    // A decode failure must not silently truncate older history:
                    // keep a blank placeholder so indices/ordering stay sane.
                    Ok(None) | Err(_) => lines.push(Line::new()),
                }
            }
            if let Err(error) = scrollback.clear() {
                aterm_log::warn!("scrollback clear during reflow failed: {error}");
            }
        }

        // Ring-buffer scrollback: the rows preceding the visible window. The
        // caller has already drained the lazy buffer (above), so all deferred
        // lines are accounted for. Linearize so logical order == Vec order.
        let ring_scrollback = self.storage.ring_buffer_scrollback();
        if ring_scrollback > 0 {
            let ring_head = self.storage.ring_head;
            if ring_head != 0 {
                self.storage.rows.rotate_left(ring_head);
                self.storage.ring_head = 0;
            }
            lines.reserve(ring_scrollback);
            for i in 0..ring_scrollback {
                let extras = self
                    .storage
                    .ring_history_extras(i)
                    .cloned()
                    .unwrap_or_default();
                lines.push(Self::row_to_line_with_stored_extras(
                    &self.storage.rows[i],
                    &extras,
                ));
            }
            // Drop the scrollback rows; keep only the visible window.
            self.storage.rows.drain(..ring_scrollback);
            self.storage.ring_extras.clear();
            self.storage.total_lines = self.storage.rows.len();
        }

        lines
    }

    /// Push rewrapped scrollback [`Line`]s back into history as the FRONT (oldest)
    /// of the scrollback, ahead of any overflow the visible-grid reflow produced.
    ///
    /// Lines go to the lazy buffer when tiered scrollback is attached (the
    /// normal path; they drain to tiered storage which honors its line limit),
    /// otherwise they are converted to ring-buffer scrollback rows up to the
    /// configured `max_scrollback` cap (older lines beyond the cap are evicted —
    /// the correct, configured behavior).
    pub(super) fn restore_reflowed_scrollback(&mut self, lines: Vec<Line>, new_cols: u16) {
        if lines.is_empty() {
            return;
        }
        if self.storage.scrollback.is_some() {
            // Front of the lazy buffer = oldest. The visible-grid reflow's
            // overflow was pushed to the back of the lazy buffer afterwards, so
            // prepend here to keep [old scrollback | reflow overflow] order.
            let mut deferred: Vec<DeferredLine> = Vec::with_capacity(lines.len());
            for line in &lines {
                deferred.push(DeferredLine::from_line(line));
            }
            self.storage.lazy_buffer.prepend(deferred);
            self.drain_lazy_buffer();
        } else {
            self.prepend_ring_scrollback_lines(lines, new_cols);
        }
    }

    /// Convert reflowed scrollback [`Line`]s into ring-buffer scrollback rows
    /// and prepend them ahead of the visible window, honoring `max_scrollback`.
    fn prepend_ring_scrollback_lines(&mut self, lines: Vec<Line>, new_cols: u16) {
        // Cap to the ring's scrollback budget: only the newest lines fit when
        // history exceeds the configured limit (oldest evicted — correct).
        let cap = self.storage.max_scrollback;
        let skip = lines.len().saturating_sub(cap);
        let kept = &lines[skip..];
        if kept.is_empty() {
            return;
        }

        // Linearize so we can splice scrollback rows at the front (index 0).
        let ring_head = self.storage.ring_head;
        if ring_head != 0 {
            self.storage.rows.rotate_left(ring_head);
            self.storage.ring_head = 0;
        }

        // Build the scrollback rows at the new width, capturing extras keyed by
        // their ring-scrollback index for ring_extras (front = oldest).
        let mut new_rows = Vec::with_capacity(kept.len());
        let mut new_extras = Vec::with_capacity(kept.len());
        for line in kept {
            let (row, extras) = self.build_scrollback_row(line, new_cols);
            new_rows.push(row);
            new_extras.push(if extras.is_empty() {
                None
            } else {
                Some(Box::new(extras))
            });
        }

        let added = new_rows.len();
        // Splice the scrollback rows in front of the (linearized) visible rows.
        let visible: Vec<_> = std::mem::take(&mut self.storage.rows);
        new_rows.extend(visible);
        self.storage.rows = new_rows;
        for (i, extra) in new_extras.into_iter().enumerate() {
            self.storage.ring_extras.insert(i, extra);
        }
        self.storage.total_lines = self.storage.rows.len();
        self.storage.absolute_row_counter =
            self.storage.absolute_row_counter.saturating_add(added as u64);
    }

    /// Build a single scrollback [`Row`](crate::Row) plus its preserved extras
    /// from a [`Line`] at the new width, reusing the unscroll fill path.
    fn build_scrollback_row(
        &mut self,
        line: &Line,
        new_cols: u16,
    ) -> (crate::Row, super::ScrolledRowExtras) {
        // SAFETY: the row is moved into `self.storage.rows` by the caller, which
        // owns `self.storage.pages` for at least as long as the row lives.
        let mut row = unsafe { crate::Row::new(new_cols, &mut self.storage.pages) };
        let extras = super::scroll_fill::fill_row_into(&mut row, line, new_cols, self.styles());
        (row, extras)
    }
}

/// Maximum display columns a logical line may span before we stop accumulating,
/// guarding against pathological inputs (`MAX_GRID_COLS` * a large row count).
const MAX_LOGICAL_WIDTH: usize = crate::MAX_GRID_COLS as usize * crate::MAX_GRID_ROWS as usize;

/// One display-cell's worth of content extracted from a source [`Line`].
struct Unit<'a> {
    /// The grapheme text for the cell (base char plus combining marks / ZWJ).
    text: &'a str,
    /// Attributes for the cell.
    attrs: CellAttrs,
    /// Hyperlink (url, id) covering this cell, if any.
    link: Option<(std::sync::Arc<str>, Option<std::sync::Arc<str>>)>,
    /// Display width (1 or 2).
    width: u16,
}

/// Rewrap a sequence of scrollback [`Line`]s to `new_cols`, preserving logical
/// line breaks (hard newlines) and content. Soft-wrapped runs (each line after
/// the first in a run carries the wrapped flag) are joined, then re-split by
/// display width. O(total cells); allocation is bounded per logical line.
#[must_use]
pub(super) fn reflow_scrollback_lines(lines: &[Line], new_cols: u16) -> Vec<Line> {
    let new_cols = new_cols.max(1);
    let mut out: Vec<Line> = Vec::with_capacity(lines.len());
    let mut i = 0;
    while i < lines.len() {
        // A logical line = lines[i] (non-wrapped or first) plus following
        // lines whose wrapped flag marks them as soft continuations.
        let start = i;
        i += 1;
        while i < lines.len() && lines[i].is_wrapped() {
            i += 1;
        }
        emit_logical_line(&lines[start..i], new_cols, &mut out);
    }
    out
}

/// Flatten a logical-line run into display-cell units, then re-split to
/// `new_cols`-wide output [`Line`]s (first not wrapped, rest wrapped).
fn emit_logical_line(run: &[Line], new_cols: u16, out: &mut Vec<Line>) {
    let mut units: Vec<Unit<'_>> = Vec::new();
    for line in run {
        collect_units(line, &mut units);
        if units.len() >= MAX_LOGICAL_WIDTH {
            break;
        }
    }

    if units.is_empty() {
        // Preserve a blank logical line (e.g. an empty hard-newline row).
        out.push(Line::new());
        return;
    }

    let cols = new_cols as usize;
    let mut col = 0usize;
    let mut seg_start = 0usize;
    let mut first = true;
    let mut idx = 0usize;
    while idx < units.len() {
        let w = units[idx].width as usize;
        // A wide char that would straddle the right edge wraps to the next row.
        if col + w > cols && col > 0 {
            out.push(build_line(&units[seg_start..idx], !first));
            first = false;
            seg_start = idx;
            col = 0;
        }
        col += w;
        idx += 1;
    }
    out.push(build_line(&units[seg_start..], !first));
}

/// Decompose a [`Line`] into per-display-cell units (text + attrs + hyperlink).
fn collect_units<'a>(line: &'a Line, units: &mut Vec<Unit<'a>>) {
    let Some(text) = line.as_str() else {
        return;
    };
    let mut byte_idx = 0usize;
    let mut char_idx = 0usize;
    let mut col: u16 = 0;
    while byte_idx < text.len() {
        let c = text[byte_idx..]
            .chars()
            .next()
            .expect("invariant: byte_idx < text.len()");
        let base_width = aterm_grapheme::char_width(c);
        if base_width == 0 {
            // Orphan zero-width char with no base; skip (matches materialize).
            byte_idx += c.len_utf8();
            char_idx += 1;
            continue;
        }
        let unit_byte_start = byte_idx;
        let unit_char_start = char_idx;
        let chars_consumed =
            super::scroll_materialize::advance_grapheme_unit(text, &mut byte_idx);
        char_idx += chars_consumed;
        let width = if base_width >= 2 { 2 } else { 1 };
        let link = line.get_hyperlink_span(col).map(|s| (s.url.clone(), s.id.clone()));
        units.push(Unit {
            text: &text[unit_byte_start..byte_idx],
            attrs: line.get_attr(unit_char_start),
            link,
            width,
        });
        col = col.saturating_add(width);
        if units.len() >= MAX_LOGICAL_WIDTH {
            break;
        }
    }
}

/// Build an output [`Line`] from a slice of display-cell units.
fn build_line(units: &[Unit<'_>], wrapped: bool) -> Line {
    let mut text = String::new();
    let mut attrs_rle: Rle<CellAttrs> = Rle::new();
    let mut spans: Vec<HyperlinkSpan> = Vec::new();
    // Coalesce consecutive cells sharing a hyperlink (url ptr + id) into spans.
    let mut open: Option<(u16, std::sync::Arc<str>, Option<std::sync::Arc<str>>)> = None;
    let mut col: u16 = 0;

    for unit in units {
        let char_count = unit.text.chars().count();
        text.push_str(unit.text);
        for _ in 0..char_count {
            attrs_rle.push(unit.attrs);
        }
        match (&open, &unit.link) {
            (None, Some((url, id))) => open = Some((col, url.clone(), id.clone())),
            (Some((_, ourl, oid)), Some((url, id)))
                if std::sync::Arc::ptr_eq(ourl, url) && oid == id => {}
            (Some((start, ourl, oid)), next) => {
                spans.push(HyperlinkSpan::with_id(*start, col, ourl.clone(), oid.clone()));
                open = next.as_ref().map(|(u, i)| (col, u.clone(), i.clone()));
            }
            (None, None) => {}
        }
        col = col.saturating_add(unit.width);
    }
    if let Some((start, url, id)) = open {
        spans.push(HyperlinkSpan::with_id(start, col, url, id));
    }

    let mut line = Line::with_hyperlinks(&text, attrs_rle, spans);
    line.set_wrapped(wrapped);
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    fn styled(text: &str, wrapped: bool) -> Line {
        let mut l = Line::from(text);
        l.set_wrapped(wrapped);
        l
    }

    #[test]
    fn rewrap_shrink_splits_logical_line() {
        // One logical line "ABCDEFGHIJ" at width 10 -> width 4 = 3 rows.
        let lines = vec![styled("ABCDEFGHIJ", false)];
        let out = reflow_scrollback_lines(&lines, 4);
        let texts: Vec<_> = out.iter().map(|l| l.as_str().unwrap().to_string()).collect();
        assert_eq!(texts, vec!["ABCD", "EFGH", "IJ"]);
        assert!(!out[0].is_wrapped());
        assert!(out[1].is_wrapped());
        assert!(out[2].is_wrapped());
    }

    #[test]
    fn rewrap_grow_merges_soft_wrapped_run() {
        // "ABCD" + wrapped "EFGH" + wrapped "IJ" -> width 20 = one row.
        let lines = vec![
            styled("ABCD", false),
            styled("EFGH", true),
            styled("IJ", true),
        ];
        let out = reflow_scrollback_lines(&lines, 20);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].as_str().unwrap(), "ABCDEFGHIJ");
        assert!(!out[0].is_wrapped());
    }

    #[test]
    fn rewrap_preserves_hard_newlines() {
        let lines = vec![styled("ABC", false), styled("DEF", false)];
        let out = reflow_scrollback_lines(&lines, 20);
        let texts: Vec<_> = out.iter().map(|l| l.as_str().unwrap().to_string()).collect();
        assert_eq!(texts, vec!["ABC", "DEF"]);
        assert!(!out[0].is_wrapped());
        assert!(!out[1].is_wrapped());
    }

    #[test]
    fn rewrap_round_trip_is_content_stable() {
        let original = vec![styled("The quick brown fox jumps", false)];
        let narrow = reflow_scrollback_lines(&original, 7);
        let wide = reflow_scrollback_lines(&narrow, 40);
        assert_eq!(wide.len(), 1);
        assert_eq!(wide[0].as_str().unwrap(), "The quick brown fox jumps");
    }

    #[test]
    fn rewrap_blank_logical_line_survives() {
        let lines = vec![styled("", false)];
        let out = reflow_scrollback_lines(&lines, 10);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].as_str().unwrap_or(""), "");
    }

    #[test]
    fn rewrap_wide_char_not_split_across_rows() {
        // Two wide chars (width 2 each) + width 3 = needs >= 4 cols to hold one.
        let lines = vec![styled("世界", false)];
        let out = reflow_scrollback_lines(&lines, 3);
        // width 3: first wide char fits (2), second would straddle -> wraps.
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].as_str().unwrap(), "世");
        assert_eq!(out[1].as_str().unwrap(), "界");
        assert!(out[1].is_wrapped());
    }
}
