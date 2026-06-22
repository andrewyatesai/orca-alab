// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! Cmd-F find overlay: the in-progress find state (`SearchState`), the scan-depth
//! bound (`MAX_SEARCH_HISTORY`), the pure matcher (`find_line_matches`), and the
//! `App`-side enter/recompute/apply/step/exit methods (an inherent-impl split).

use aterm_core::selection::{SelectionSide, SelectionType};

use crate::{App, term_lock};

/// In-progress Cmd-F find over the live screen + recent scrollback. Matches are
/// `(row, start_col, end_col)` in SELECTION coordinates (0..rows = live screen,
/// negative = scrollback); the current one is highlighted by setting the text
/// selection (the existing overlay — no renderer change) and scrolled into view.
#[derive(Default)]
pub(crate) struct SearchState {
    pub(crate) query: String,
    pub(crate) matches: Vec<(i32, u16, u16)>,
    pub(crate) current: usize,
}

/// How many scrollback lines back a Cmd-F find scans (plus the live screen). Bounds
/// per-keystroke cost and keeps the scan in the fast hot/warm tiers, not the disk
/// tier. Deep-history search beyond this is a follow-up.
pub(crate) const MAX_SEARCH_HISTORY: i32 = 5000;

/// Case-insensitive, non-overlapping matches of `query` in each `(row, text)`
/// line (rows are selection coords: 0..rows = live screen, negative = scrollback).
/// Returns `(row, start_col, end_col)` (INCLUSIVE columns) per match, in the order
/// the lines are given. Column = char index (v1: one column per char; wide chars
/// not adjusted). Empty query / no match → empty. Pure, so it is unit-testable.
pub(crate) fn find_line_matches(lines: &[(i32, String)], query: &str) -> Vec<(i32, u16, u16)> {
    let q: Vec<char> = query.to_lowercase().chars().collect();
    if q.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (row, text) in lines {
        let hay: Vec<char> = text.to_lowercase().chars().collect();
        if hay.len() < q.len() {
            continue;
        }
        let mut i = 0;
        while i + q.len() <= hay.len() {
            if hay[i..i + q.len()] == q[..] {
                out.push((*row, i as u16, (i + q.len() - 1) as u16));
                i += q.len(); // non-overlapping
            } else {
                i += 1;
            }
        }
    }
    out
}

impl App {
    /// Enter (or refresh) Cmd-F find mode.
    pub(crate) fn search_enter(&mut self) {
        if let Some(ws) = self.front_mut()
            && ws.search.is_none()
        {
            ws.search = Some(SearchState::default());
        }
        self.search_recompute();
    }

    /// Re-run the find for the current query over the live screen + recent
    /// scrollback, then show the first match. Snaps the viewport to the bottom
    /// first so `get_line_text` rows are stable selection coordinates (0..rows =
    /// live, negative = scrollback); the lines are gathered oldest→newest so match
    /// order reads top-to-bottom.
    pub(crate) fn search_recompute(&mut self) {
        let search_history_lines = self.search_history_lines;
        let Some(ws) = self.front_mut() else { return };
        let query = match &ws.search {
            Some(s) => s.query.clone(),
            None => return,
        };
        let matches = if query.is_empty() {
            Vec::new()
        } else {
            let rows = i32::from(ws.rows);
            let mut term = term_lock(&ws.term);
            term.scroll_to_bottom(); // display_offset = 0 → stable coords
            // Scrollback (negative rows) oldest→newest, bounded; then the live screen.
            let mut hist: Vec<(i32, String)> = Vec::new();
            let mut r = -1;
            while r >= -search_history_lines {
                match term.get_line_text(r, None) {
                    Some(t) => hist.push((r, t)),
                    None => break, // past the top of history
                }
                r -= 1;
            }
            hist.reverse();
            for r in 0..rows {
                hist.push((r, term.get_line_text(r, None).unwrap_or_default()));
            }
            drop(term);
            find_line_matches(&hist, &query)
        };
        if let Some(s) = ws.search.as_mut() {
            s.matches = matches;
            s.current = 0;
        }
        self.search_apply_current();
    }

    /// Highlight the current match via the text selection (the existing overlay —
    /// no renderer change), scroll it into view, and show the find state in the
    /// window title.
    pub(crate) fn search_apply_current(&mut self) {
        let Some(ws) = self.front() else { return };
        let (query, mat, idx, total) = match &ws.search {
            Some(s) => (
                s.query.clone(),
                s.matches.get(s.current).copied(),
                s.current,
                s.matches.len(),
            ),
            None => return,
        };
        {
            let mut term = term_lock(&ws.term);
            term.scroll_to_bottom(); // reset to display_offset = 0 (stable coords)
            let sel = term.text_selection_mut();
            sel.clear();
            if let Some((row, c0, c1)) = mat {
                sel.start_selection(row, c0, SelectionSide::Left, SelectionType::Simple);
                sel.update_selection(row, c1, SelectionSide::Right);
                // A scrollback match (row < 0) is scrolled up to the top visible row.
                if row < 0 {
                    term.scroll_display(-row);
                }
            }
        }
        let title = if query.is_empty() {
            "aterm — find:".to_string()
        } else if total == 0 {
            format!("aterm — find: {query} (no matches)")
        } else {
            format!("aterm — find: {query} ({}/{total})", idx + 1)
        };
        if let Some(w) = &ws.os_window {
            w.set_title(&title);
            w.request_redraw();
        }
    }

    /// Cycle to the next (`forward`) / previous match, wrapping.
    pub(crate) fn search_step(&mut self, forward: bool) {
        if let Some(ws) = self.front_mut()
            && let Some(s) = ws.search.as_mut()
        {
            let n = s.matches.len();
            if n == 0 {
                return;
            }
            s.current = if forward {
                (s.current + 1) % n
            } else {
                (s.current + n - 1) % n
            };
        }
        self.search_apply_current();
    }

    /// Leave find mode: clear the highlight + restore the title.
    pub(crate) fn search_exit(&mut self) {
        let Some(ws) = self.front_mut() else { return };
        ws.search = None;
        term_lock(&ws.term).text_selection_mut().clear();
        if let Some(w) = &ws.os_window {
            w.set_title("aterm");
            w.request_redraw();
        }
    }
}
