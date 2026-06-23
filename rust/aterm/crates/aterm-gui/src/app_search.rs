// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! Cmd-F find overlay: the in-progress find state (`SearchState`), the scan-depth
//! bound (`MAX_SEARCH_HISTORY`), the pure matcher (`find_line_matches`), and the
//! `App`-side enter/recompute/apply/step/exit methods (an inherent-impl split).

use aterm_core::selection::{SelectionSide, SelectionType};
use aterm_search::TerminalSearch;

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

impl SearchState {
    /// Pure next/previous-match cursor step with wraparound (no window/render
    /// side effects). `forward` advances toward later matches; both directions
    /// wrap. A no-op when there are no matches. This is the testable core of
    /// [`App::search_step`], which calls it and then re-applies the highlight.
    pub(crate) fn step(&mut self, forward: bool) {
        let n = self.matches.len();
        if n == 0 {
            return;
        }
        self.current = if forward {
            (self.current + 1) % n
        } else {
            (self.current + n - 1) % n
        };
    }

    /// The currently-highlighted match, or `None` when the find has no matches.
    pub(crate) fn current_match(&self) -> Option<(i32, u16, u16)> {
        self.matches.get(self.current).copied()
    }
}

/// How many scrollback lines back a Cmd-F find scans (plus the live screen). Bounds
/// per-keystroke cost and keeps the scan in the fast hot/warm tiers, not the disk
/// tier. Deep-history search beyond this is a follow-up.
pub(crate) const MAX_SEARCH_HISTORY: i32 = 5000;

/// Case-insensitive matches of `query` in each `(row, text)` line (rows are
/// selection coords: 0..rows = live screen, negative = scrollback). Returns
/// `(row, start_col, end_col)` (INCLUSIVE columns) per match, in the order the
/// lines are given. Column = char index (v1: one column per char; wide chars not
/// adjusted). Empty query / no match → empty. Pure, so it is unit-testable.
///
/// Backed by the shared [`aterm_search`] engine (trigram + Bloom): the lines are
/// indexed sequentially into a throwaway [`TerminalSearch`], searched
/// case-insensitively (the default literal path), and the engine's absolute line
/// numbers are mapped back to the caller's selection rows. We do NOT reimplement
/// the matcher here — this is the GUI-coordinate adapter over the engine.
pub(crate) fn find_line_matches(lines: &[(i32, String)], query: &str) -> Vec<(i32, u16, u16)> {
    if query.is_empty() {
        return Vec::new();
    }
    // Index each line at its sequential position; `idx` (0-based from the first
    // line given) is the engine's absolute line number and maps straight back to
    // `lines[idx].0` — the selection row.
    let mut engine = TerminalSearch::with_capacity(lines.len());
    engine.index_visible_content(0, lines.iter().map(|(_, text)| text.as_str()));
    // Default literal, case-insensitive (case_sensitive = false, is_regex =
    // false) — matches the prior hand-rolled scan's semantics. The engine only
    // errors on a bad regex, which this non-regex path never produces.
    let matches = engine.search_opts(query, false, false).unwrap_or_default();
    let mut out: Vec<(i32, u16, u16)> = matches
        .into_iter()
        .filter_map(|m| {
            let row = lines.get(m.line).map(|(r, _)| *r)?;
            // SearchMatch end_col is EXCLUSIVE; the selection wants an INCLUSIVE
            // end. A non-empty match always has end_col > start_col.
            let start = u16::try_from(m.start_col).ok()?;
            let end = u16::try_from(m.end_col.saturating_sub(1)).ok()?;
            Some((row, start, end))
        })
        .collect();
    // The engine groups matches by line; re-sort into the line order the caller
    // gave (top-to-bottom over scrollback→live), with intra-line left-to-right,
    // so next/prev navigation reads in visual order.
    out.sort_by_key(|&(row, start, _)| (row, start));
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
                s.current_match(),
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
            s.step(forward);
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

#[cfg(test)]
mod tests {
    use super::{SearchState, find_line_matches};

    /// A known multi-line buffer with three `foo` matches across two rows
    /// (scrollback row -1 and live rows 0/1), in selection coordinates. Drives
    /// the find-state machine that the GUI plumbs (`SearchState` + `step`)
    /// headlessly — no window, no renderer.
    fn buffer() -> Vec<(i32, String)> {
        vec![
            (-1, "foo in scrollback".to_string()), // match at col 0
            (0, "a foo and a Foo".to_string()),    // matches at col 2 and col 12 (case-insensitive)
            (1, "no match here".to_string()),
        ]
    }

    /// open -> query -> the FULL highlight set is every match, in top-to-bottom
    /// then left-to-right order, with INCLUSIVE end columns; an empty / unmatched
    /// query yields nothing.
    #[test]
    fn find_state_open_query_highlight_set() {
        let lines = buffer();
        // Empty query => no matches (find bar open but nothing typed yet).
        assert!(find_line_matches(&lines, "").is_empty());
        // Case-insensitive, all matches, ordered (row, start_col).
        let all = find_line_matches(&lines, "foo");
        assert_eq!(all, vec![(-1, 0, 2), (0, 2, 4), (0, 12, 14)]);
        // A query with no hit => empty highlight set.
        assert!(find_line_matches(&lines, "zzz").is_empty());
    }

    /// open -> query -> next/next/next (wrap) and prev (wrap) walks the current
    /// match through the highlight set with correct wraparound, and the current
    /// offset matches the engine-derived set at every step.
    #[test]
    fn find_state_next_prev_wraparound() {
        let mut s = SearchState::default();
        s.query = "foo".to_string();
        s.matches = find_line_matches(&buffer(), &s.query);
        s.current = 0;
        let n = s.matches.len();
        assert_eq!(n, 3);

        // Starts on the first (top-most) match.
        assert_eq!(s.current_match(), Some((-1, 0, 2)));
        // next -> second, next -> third.
        s.step(true);
        assert_eq!(s.current_match(), Some((0, 2, 4)));
        s.step(true);
        assert_eq!(s.current_match(), Some((0, 12, 14)));
        // next off the end wraps to the first.
        s.step(true);
        assert_eq!(s.current, 0);
        assert_eq!(s.current_match(), Some((-1, 0, 2)));
        // prev off the front wraps to the last (Shift-Enter from the top).
        s.step(false);
        assert_eq!(s.current, n - 1);
        assert_eq!(s.current_match(), Some((0, 12, 14)));
        // prev -> middle, prev -> first.
        s.step(false);
        assert_eq!(s.current_match(), Some((0, 2, 4)));
        s.step(false);
        assert_eq!(s.current_match(), Some((-1, 0, 2)));
    }

    /// With no matches the cursor step is a no-op (Enter/Shift-Enter do nothing),
    /// and there is no current match to highlight.
    #[test]
    fn find_state_step_no_matches_is_noop() {
        let mut s = SearchState::default();
        s.query = "zzz".to_string();
        s.matches = find_line_matches(&buffer(), &s.query);
        assert!(s.matches.is_empty());
        assert_eq!(s.current_match(), None);
        s.step(true);
        assert_eq!(s.current, 0);
        s.step(false);
        assert_eq!(s.current, 0);
        assert_eq!(s.current_match(), None);
    }
}
