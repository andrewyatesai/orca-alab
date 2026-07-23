//! E-5 (federated search): the headless/daemon search entry over a
//! `HeadlessTerminal`'s text-only scrollback + visible grid.
//!
//! This is the fork's daemon-side matching kernel for FEDERATED-SEARCH-DESIGN
//! §2.2/§2.3: warm daemon sessions are searched in-memory, cold/parked content
//! is replayed through a transient `HeadlessTerminal` (ANSI stripped by the
//! headless parse — never a TS regex strip) and then searched the same way.
//! Only match SUMMARIES leave this module — callers ship them over the daemon
//! socket, never the scrollback text itself. Query semantics mirror the wasm
//! find bar: literal by default, explicit case toggle, regex compiled here with
//! invalid patterns treated as zero matches.

use crate::headless::HeadlessTerminal;
use regex::RegexBuilder;

/// Query options carried on the wire (`caseSensitive` / `regex`), matching the
/// pane find bar's engine call. Smart-case is resolved by the CALLER (the
/// federation controller) into an explicit `case_sensitive` value.
#[derive(Clone, Copy, Debug, Default)]
pub struct SearchOptions {
    pub case_sensitive: bool,
    pub regex: bool,
}

/// One match, summary-only: NO surrounding scrollback crosses the socket
/// beyond the matched line's text (the fed result model's `snippet`, produced
/// source-side). `abs_row` is 0-based from the OLDEST retained history row;
/// visible-grid rows continue after history (`abs_row = scrollback_len + row`).
/// `col`/`len` are char offsets into `line` (text-only rows have no wide-cell
/// column identity — the renderer treats daemon rows as approximate anyway).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchSummary {
    pub abs_row: usize,
    pub col: usize,
    pub len: usize,
    pub line: String,
}

/// A search result: newest-first summaries (capped), the TRUE total, and the
/// fed design's per-source truncation honesty flag.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchOutcome {
    pub matches: Vec<MatchSummary>,
    pub total: usize,
    pub incomplete: bool,
}

/// Regex compiled-size bound (ReDoS hygiene, mirroring the engine's bounded
/// compile): a hostile pattern fails to compile and yields zero matches.
const REGEX_SIZE_LIMIT: usize = 1 << 20;

/// The compiled query: literal (with optional case folding) or regex.
/// Invalid/oversized regex compiles to `None` == zero matches, matching the
/// wasm engine's contract for the pane find bar.
enum Matcher {
    Literal { needle: String, fold_case: bool },
    Regex(regex::Regex),
    Never,
}

impl Matcher {
    fn build(query: &str, opts: SearchOptions) -> Self {
        if query.is_empty() {
            return Matcher::Never;
        }
        if opts.regex {
            return match RegexBuilder::new(query)
                .case_insensitive(!opts.case_sensitive)
                .size_limit(REGEX_SIZE_LIMIT)
                .build()
            {
                Ok(re) => Matcher::Regex(re),
                Err(_) => Matcher::Never,
            };
        }
        let fold_case = !opts.case_sensitive;
        Matcher::Literal {
            needle: if fold_case { query.to_lowercase() } else { query.to_string() },
            fold_case,
        }
    }

    /// All non-overlapping match spans in `line`, as CHAR (offset, len) pairs.
    fn spans(&self, line: &str) -> Vec<(usize, usize)> {
        match self {
            Matcher::Never => Vec::new(),
            Matcher::Regex(re) => re
                .find_iter(line)
                // Why char offsets: byte offsets would desync col/len from the
                // renderer's per-char row model on any non-ASCII line.
                .map(|m| {
                    let col = line[..m.start()].chars().count();
                    let len = line[m.start()..m.end()].chars().count();
                    (col, len)
                })
                // Zero-width regex matches (e.g. `a*`) carry no span identity; drop them.
                .filter(|(_, len)| *len > 0)
                .collect(),
            Matcher::Literal { needle, fold_case } => {
                let haystack = if *fold_case { line.to_lowercase() } else { line.to_string() };
                let needle_chars = needle.chars().count();
                let mut spans = Vec::new();
                let mut from = 0;
                while let Some(rel) = haystack[from..].find(needle.as_str()) {
                    let start = from + rel;
                    spans.push((haystack[..start].chars().count(), needle_chars));
                    from = start + needle.len().max(1);
                }
                spans
            }
        }
    }
}

impl HeadlessTerminal {
    /// E-5: search the retained history + visible grid, newest row first, and
    /// return at most `max_matches` summaries with the true `total` and an
    /// `incomplete` flag when the cap truncated. `cutoff_row` (the federated
    /// depth-extension contract, fed design §2.3) keeps only matches at rows
    /// STRICTLY OLDER than the given absolute row — the controller passes the
    /// live pane's oldest retained row so daemon results never double-report.
    ///
    /// `&mut self` because retention is settled first (same contract as
    /// `serialize_ansi`): staged compress-offload residue would otherwise make
    /// `abs_row` disagree with what a snapshot observer sees.
    pub fn search_scrollback(
        &mut self,
        query: &str,
        opts: SearchOptions,
        max_matches: usize,
        cutoff_row: Option<usize>,
    ) -> SearchOutcome {
        self.settle_scrollback();
        let matcher = Matcher::build(query, opts);
        if matches!(matcher, Matcher::Never) {
            return SearchOutcome::default();
        }
        let history = self.scrollback_len();
        let (rows, _) = self.size();
        let total_rows = history + rows;
        let newest_considered = cutoff_row.map_or(total_rows, |c| c.min(total_rows));
        let mut out = SearchOutcome::default();
        // Newest → oldest so the cap keeps the newest matches (the find bar's
        // select-last convention); `total` still counts every match honestly.
        for abs in (0..newest_considered).rev() {
            let line = self.abs_row_text(abs, history);
            for (col, len) in matcher.spans(&line) {
                out.total += 1;
                if out.matches.len() < max_matches {
                    out.matches.push(MatchSummary { abs_row: abs, col, len, line: line.clone() });
                }
            }
        }
        out.incomplete = out.total > out.matches.len();
        out
    }

    /// E-5 context window (`searchContext` RPC): up to `before` lines above and
    /// `after` lines below `abs_row`, plus the row itself. Returns the lines and
    /// the absolute row of the first returned line, clamped to retained content.
    pub fn search_context(
        &mut self,
        abs_row: usize,
        before: usize,
        after: usize,
    ) -> (Vec<String>, usize) {
        self.settle_scrollback();
        let history = self.scrollback_len();
        let (rows, _) = self.size();
        let total_rows = history + rows;
        if total_rows == 0 || abs_row >= total_rows {
            return (Vec::new(), 0);
        }
        let first = abs_row.saturating_sub(before);
        let last = (abs_row + after).min(total_rows - 1);
        let lines = (first..=last).map(|abs| self.abs_row_text(abs, history)).collect();
        (lines, first)
    }

    /// Row text at an absolute row spanning history then the visible grid.
    fn abs_row_text(&self, abs: usize, history: usize) -> String {
        if abs < history {
            self.scrollback_row_text(abs)
        } else {
            self.row_text(abs - history)
        }
    }
}

/// E-5's transient-replay path for COLD sessions (persisted checkpoint+log) and
/// parked snapshots: feed the stored ANSI through a fresh headless parse (the
/// policy-mandated Rust strip), then run the same kernel. The caller owns
/// caching the returned terminal keyed by checkpoint generation so repeat
/// queries skip the replay (fed design §2.3).
pub fn replay_for_search(
    rows: usize,
    cols: usize,
    scrollback_limit: usize,
    chunks: impl IntoIterator<Item = impl AsRef<[u8]>>,
) -> HeadlessTerminal {
    let mut term = HeadlessTerminal::with_scrollback(rows, cols, scrollback_limit);
    for chunk in chunks {
        term.process(chunk.as_ref());
    }
    term
}

#[cfg(test)]
mod tests {
    use super::*;

    fn term_with_lines(lines: &[&str]) -> HeadlessTerminal {
        let mut term = HeadlessTerminal::with_scrollback(4, 40, 1000);
        for (i, line) in lines.iter().enumerate() {
            if i > 0 {
                term.process_str("\r\n");
            }
            term.process_str(line);
        }
        term
    }

    #[test]
    fn literal_search_finds_matches_across_history_and_grid() {
        let mut term = term_with_lines(&[
            "alpha needle one",
            "nothing here",
            "beta needle two",
            "gamma",
            "delta",
            "needle in grid",
        ]);
        // 6 lines on a 4-row grid → 2 history rows, 4 visible rows.
        assert_eq!(term.scrollback_len(), 2);
        let out = term.search_scrollback("needle", SearchOptions::default(), 50, None);
        assert_eq!(out.total, 3);
        assert!(!out.incomplete);
        // Newest first: visible row 3 ("needle in grid") → abs 5; visible row 0
        // ("beta needle two") → abs 2; history row 0 ("alpha needle one") → abs 0.
        let rows: Vec<usize> = out.matches.iter().map(|m| m.abs_row).collect();
        assert_eq!(rows, vec![5, 2, 0]);
        assert_eq!(out.matches[0].line, "needle in grid");
        assert_eq!(out.matches[0].col, 0);
        assert_eq!(out.matches[0].len, 6);
        assert_eq!(out.matches[2].col, 6);
    }

    #[test]
    fn case_insensitive_by_default_case_sensitive_on_request() {
        let mut term = term_with_lines(&["Mixed CASE line", "mixed case line"]);
        let insensitive = term.search_scrollback("mixed case", SearchOptions::default(), 10, None);
        assert_eq!(insensitive.total, 2);
        let sensitive = term.search_scrollback(
            "mixed case",
            SearchOptions { case_sensitive: true, regex: false },
            10,
            None,
        );
        assert_eq!(sensitive.total, 1);
        assert_eq!(sensitive.matches[0].line, "mixed case line");
    }

    #[test]
    fn regex_search_matches_and_invalid_regex_is_zero_matches() {
        let mut term = term_with_lines(&["error: code 137", "ok: code 0", "error: code 9"]);
        let out = term.search_scrollback(
            r"error: code \d+",
            SearchOptions { case_sensitive: false, regex: true },
            10,
            None,
        );
        assert_eq!(out.total, 2);
        assert_eq!(out.matches[0].line, "error: code 9");
        // Invalid pattern → zero matches, never an error (wasm parity).
        let bad = term.search_scrollback(
            r"error: (unclosed",
            SearchOptions { case_sensitive: false, regex: true },
            10,
            None,
        );
        assert_eq!(bad, SearchOutcome::default());
    }

    #[test]
    fn zero_width_regex_matches_are_dropped() {
        let mut term = term_with_lines(&["aaa bbb"]);
        let out = term.search_scrollback(
            "a*",
            SearchOptions { case_sensitive: false, regex: true },
            10,
            None,
        );
        // Only the real "aaa" run matches; empty positions carry no span.
        assert_eq!(out.total, 1);
        assert_eq!(out.matches[0].len, 3);
    }

    #[test]
    fn max_matches_caps_newest_first_and_flags_incomplete() {
        let mut term = HeadlessTerminal::with_scrollback(2, 20, 1000);
        for i in 0..10 {
            term.process_str(&format!("hit {i}\r\n"));
        }
        let out = term.search_scrollback("hit", SearchOptions::default(), 3, None);
        assert_eq!(out.matches.len(), 3);
        assert_eq!(out.total, 10);
        assert!(out.incomplete);
        // The kept 3 are the NEWEST rows.
        assert!(out.matches[0].abs_row > out.matches[2].abs_row);
        assert_eq!(out.matches[0].line, "hit 9");
    }

    #[test]
    fn cutoff_row_keeps_only_strictly_older_rows() {
        let mut term = HeadlessTerminal::with_scrollback(2, 20, 1000);
        for i in 0..6 {
            term.process_str(&format!("hit {i}\r\n"));
        }
        let all = term.search_scrollback("hit", SearchOptions::default(), 50, None);
        let oldest_live = 3;
        let out = term.search_scrollback("hit", SearchOptions::default(), 50, Some(oldest_live));
        assert!(out.total < all.total);
        assert!(out.matches.iter().all(|m| m.abs_row < oldest_live), "matches: {:?}", out.matches);
    }

    #[test]
    fn empty_query_matches_nothing() {
        let mut term = term_with_lines(&["anything"]);
        assert_eq!(term.search_scrollback("", SearchOptions::default(), 10, None), SearchOutcome::default());
    }

    #[test]
    fn non_ascii_spans_are_char_offsets() {
        let mut term = term_with_lines(&["日本語 needle 語"]);
        let out = term.search_scrollback("needle", SearchOptions::default(), 10, None);
        assert_eq!(out.total, 1);
        // "日本語 " = 4 chars before the match (byte offset would be 10).
        assert_eq!(out.matches[0].col, 4);
        assert_eq!(out.matches[0].len, 6);
    }

    #[test]
    fn ansi_colored_output_is_matched_as_plain_text() {
        // The headless parse IS the ANSI strip: colored output matches its text.
        let mut term = HeadlessTerminal::with_scrollback(2, 40, 100);
        term.process_str("\x1b[1;31mred alert\x1b[0m\r\nplain\r\n");
        let out = term.search_scrollback("red alert", SearchOptions::default(), 10, None);
        assert_eq!(out.total, 1);
        assert_eq!(out.matches[0].line, "red alert");
    }

    #[test]
    fn search_context_returns_window_and_first_row() {
        let mut term = HeadlessTerminal::with_scrollback(2, 20, 1000);
        for i in 0..8 {
            term.process_str(&format!("line {i}\r\n"));
        }
        let (lines, first) = term.search_context(3, 2, 2);
        assert_eq!(first, 1);
        assert_eq!(lines, vec!["line 1", "line 2", "line 3", "line 4", "line 5"]);
        // Clamped at the top.
        let (top, top_first) = term.search_context(1, 5, 0);
        assert_eq!(top_first, 0);
        assert_eq!(top, vec!["line 0", "line 1"]);
        // Out of range → empty.
        let (none, _) = term.search_context(10_000, 2, 2);
        assert!(none.is_empty());
    }

    #[test]
    fn replay_for_search_strips_ansi_via_headless_parse() {
        // Cold/parked groundwork: persisted ANSI replays into searchable text.
        let stored = "\x1b[32mgreen needle\x1b[0m\r\nsecond line\r\n";
        let mut term = replay_for_search(4, 40, 1000, [stored.as_bytes()]);
        let out = term.search_scrollback("green needle", SearchOptions::default(), 10, None);
        assert_eq!(out.total, 1);
        assert_eq!(out.matches[0].line, "green needle");
    }

    #[test]
    fn search_settles_compress_offload_residue_first() {
        // Staged offload residue must not skew abs_row identity vs snapshot
        // observation (the serialize_ansi settle contract).
        let mut term = HeadlessTerminal::with_scrollback(2, 20, 1500);
        term.set_compress_offload_active(true);
        for i in 0..2000 {
            term.process_str(&format!("L{i}\r\n"));
        }
        let out = term.search_scrollback("L1999", SearchOptions::default(), 10, None);
        assert_eq!(out.total, 1);
        assert_eq!(term.scrollback_len(), 1500, "search observation settled retention");
    }
}
