// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
// Author: Andrew Yates

//! P1.0b: O(1) reuse of the full-content search index.
//!
//! `cmd_search` historically rebuilt a fresh [`TerminalSearch`] over the ENTIRE
//! retained scrollback + visible rows on every socket call (~459 ms at 50k
//! lines — ~100× the cost of the actual query). The index is a *pure function*
//! of `(which grid is active, that grid's content)`, while the per-query pattern
//! is independent, so the same index can be reused across queries until the
//! content changes.
//!
//! [`Terminal::indexed_search`] caches the built index keyed by
//! `(modes.alternate_screen, content_seq())` and rebuilds only on a key miss.
//! The rebuild reproduces the EXACT indexing `cmd_search` used to do inline —
//! history lines keyed at `oldest_absolute_row()`, then visible rows keyed at
//! `oldest + scrollback` — so search RESULTS (matches, order, absolute-row
//! numbers, INCOMPLETE/eviction semantics) are byte-identical to the old path.
//!
//! ## Why the key cannot go stale
//!
//! The indexed set is the text of every retained addressable line plus each
//! line's absolute-row key. Both inputs are captured by the cache key:
//!
//! - **Line text & set membership** — changes only when cells are written, the
//!   screen scrolls content into scrollback, lines are erased, or the grid is
//!   reflowed/resized. Every such path bumps the active grid's `content_gen`
//!   (forwarded by [`Terminal::content_seq`]).
//! - **Absolute-row keys** — derived from `oldest_absolute_row()` (and
//!   `scrollback_lines()` / `rows()`). `oldest_absolute_row()` advances only when
//!   content scrolls off (a `scroll_up`, which bumps `content_gen`); resize
//!   changes `rows`/`scrollback` via a reflow that also bumps `content_gen`.
//! - **Active grid (main vs. alternate screen)** — swapping screens changes the
//!   whole indexed buffer, captured by the `alt_screen` key component. (The two
//!   grids keep independent `content_gen` counters, so the boolean disambiguates
//!   the otherwise-shared sequence space.)
//!
//! A pure viewport / `display_offset` scroll deliberately does NOT change the
//! retained set (the index always covers ALL retained lines, not the visible
//! page) and correctly does NOT bump `content_seq()`, so the cache is reused —
//! the desired O(1) win — without going stale.

use super::Terminal;
use crate::search::TerminalSearch;

/// A cached search index plus the cache key it was built for.
///
/// See [`Terminal::indexed_search`]. Stored in `Terminal::search_index`.
pub(crate) struct CachedSearchIndex {
    /// Whether the alternate screen was active when this index was built. Part
    /// of the cache key: the alt grid and main grid keep independent
    /// `content_gen` counters, so a screen swap that happens to land on the same
    /// sequence value must still invalidate.
    alt_screen: bool,
    /// The active grid's `content_seq()` at build time. Bumps on every content
    /// mutation, so a mismatch means the indexed text/keys may have changed.
    content_gen: u64,
    /// The fully built index over scrollback + visible rows (keyed by absolute
    /// row). Reused verbatim while the key matches.
    index: TerminalSearch,
}

impl Terminal {
    /// Return a full-content search index, reusing the cache when the active
    /// grid's content is unchanged (P1.0b — the O(1) win).
    ///
    /// The returned index covers EVERY still-retained addressable line keyed by
    /// ABSOLUTE row — scrollback history `0..scrollback_lines` at absolute
    /// `oldest + i`, then visible rows `0..rows` at absolute
    /// `oldest + scrollback + r` — so each
    /// [`SearchMatch::line`](crate::search::SearchMatch) is already an absolute
    /// row. Run the per-query pattern with
    /// `indexed_search(...).search_results_opts(pat, case, regex)`.
    ///
    /// On a cache hit (key `(alternate_screen, content_seq())` unchanged) this
    /// returns the cached index WITHOUT rebuilding. On a miss it rebuilds the
    /// index identically to the legacy inline `cmd_search` indexing — producing
    /// byte-identical results — then caches it under the new key.
    ///
    /// `&mut self` is required because the cache lives on the terminal; the
    /// returned `&TerminalSearch` is immutable so callers cannot mutate cached
    /// coordinates.
    pub fn indexed_search(&mut self) -> &TerminalSearch {
        let key_alt = self.modes.alternate_screen;
        let key_gen = self.content_seq();

        let hit = match &self.search_index {
            Some(cached) => cached.alt_screen == key_alt && cached.content_gen == key_gen,
            None => false,
        };

        if !hit {
            let index = self.build_search_index();
            self.search_index = Some(CachedSearchIndex {
                alt_screen: key_alt,
                content_gen: key_gen,
                index,
            });
            self.search_index_rebuilds = self.search_index_rebuilds.wrapping_add(1);
        }

        // `self.search_index` is `Some` here: it was `Some` on a hit and we just
        // assigned it on a miss.
        &self
            .search_index
            .as_ref()
            .expect("search_index populated above")
            .index
    }

    /// Build a fresh full-content index over the active grid.
    ///
    /// This is the EXACT indexing `cmd_search` performed inline before P1.0b —
    /// kept here (where the grid + `get_line_text` + `TerminalSearch` all live)
    /// so the cached and uncached paths are guaranteed identical. Any change to
    /// the line set, ordering, or absolute-row keys here would change search
    /// results, so it must mirror the legacy loop exactly.
    fn build_search_index(&self) -> TerminalSearch {
        let grid = &self.grid;
        let oldest = grid.oldest_absolute_row();
        let scrollback = grid.scrollback_lines();
        let rows = self.rows();

        let mut search = TerminalSearch::new();

        // Scrollback history 0..scrollback → absolute oldest + i. Line text via
        // `get_history_line` (the same source the legacy loop used).
        let history: Vec<String> = (0..scrollback)
            .map(|i| {
                grid.get_history_line(i)
                    .map(|l| l.to_string())
                    .unwrap_or_default()
            })
            .collect();
        let hist_base = usize::try_from(oldest).unwrap_or(usize::MAX);
        search.index_visible_content(hist_base, &history);

        // Visible rows 0..rows → absolute oldest + scrollback + r. Combining-aware
        // `get_line_text` so accents / ZWJ clusters survive (FIDELITY I-1).
        // `rows` is a u16, so `i32::from` is lossless (mirrors the legacy
        // `r as i32` where `r` was a u16-bounded usize).
        let visible: Vec<String> = (0..rows)
            .map(|r| self.get_line_text(i32::from(r), None).unwrap_or_default())
            .collect();
        let vis_base = hist_base.saturating_add(scrollback);
        search.index_visible_content(vis_base, &visible);

        search
    }

    /// Number of full search-index REBUILDS (cache misses) performed so far.
    ///
    /// Monotonic; advances by one each time [`indexed_search`](Self::indexed_search)
    /// rebuilds the index (content changed or first call) and never on a reuse.
    /// A repeat query with no intervening content change leaves this unchanged —
    /// the observable signature of the O(1) cache hit. Introspection only.
    #[must_use]
    #[inline]
    pub fn search_index_rebuilds(&self) -> u64 {
        self.search_index_rebuilds
    }
}

#[cfg(test)]
mod tests {
    use super::Terminal;

    /// Build the index the legacy (uncached) way for a behavior-identity
    /// reference. Mirrors what `cmd_search` used to do inline AND what
    /// `build_search_index` does now — so any divergence between the cached
    /// path and "rebuild every time" surfaces as a result mismatch.
    fn legacy_results(t: &Terminal, pat: &str) -> Vec<(usize, usize, usize)> {
        use crate::search::TerminalSearch;
        let grid = t.grid();
        let oldest = grid.oldest_absolute_row();
        let scrollback = grid.scrollback_lines();
        let rows = t.rows() as usize;
        let mut search = TerminalSearch::new();
        let history: Vec<String> = (0..scrollback)
            .map(|i| {
                grid.get_history_line(i)
                    .map(|l| l.to_string())
                    .unwrap_or_default()
            })
            .collect();
        let hist_base = usize::try_from(oldest).unwrap_or(usize::MAX);
        search.index_visible_content(hist_base, &history);
        let visible: Vec<String> = (0..rows)
            .map(|r| t.get_line_text(r as i32, None).unwrap_or_default())
            .collect();
        let vis_base = hist_base.saturating_add(scrollback);
        search.index_visible_content(vis_base, &visible);
        let res = search
            .search_results_opts(pat, false, false)
            .expect("search ok");
        res.matches
            .iter()
            .map(|m| (m.line, m.start_col, m.len()))
            .collect()
    }

    fn cached_results(t: &mut Terminal, pat: &str) -> Vec<(usize, usize, usize)> {
        let res = t
            .indexed_search()
            .search_results_opts(pat, false, false)
            .expect("search ok");
        res.matches
            .iter()
            .map(|m| (m.line, m.start_col, m.len()))
            .collect()
    }

    /// Two consecutive identical searches with NO content change between them
    /// return IDENTICAL results, and the second REUSES the cache (no rebuild) —
    /// the O(1) win. Then a search AFTER a content write rebuilds and reflects
    /// the new content. Throughout, the cached results equal the legacy
    /// (rebuild-every-time) results: behavior-identity is non-negotiable.
    #[test]
    fn repeat_search_reuses_cache_write_invalidates() {
        let mut t = Terminal::new(6, 40);
        // Push a needle off-screen into scrollback, plus filler.
        t.process(b"NEEDLE_alpha\r\n");
        for i in 0..20 {
            t.process(format!("filler line {i}\r\n").as_bytes());
        }

        // First search: a cache MISS -> exactly one rebuild.
        let before = t.search_index_rebuilds();
        let r1 = cached_results(&mut t, "NEEDLE_alpha");
        assert_eq!(
            t.search_index_rebuilds(),
            before + 1,
            "first search must rebuild the index once"
        );
        assert_eq!(
            r1,
            legacy_results(&t, "NEEDLE_alpha"),
            "results must match the legacy index"
        );
        assert_eq!(r1.len(), 1, "the scrolled-off needle is found exactly once");

        // Second IDENTICAL search, no content change -> cache HIT, NO rebuild,
        // byte-identical results.
        let rebuilds_after_first = t.search_index_rebuilds();
        let r2 = cached_results(&mut t, "NEEDLE_alpha");
        assert_eq!(
            t.search_index_rebuilds(),
            rebuilds_after_first,
            "the repeat search must REUSE the cache (no rebuild) — the O(1) win"
        );
        assert_eq!(
            r1, r2,
            "reused-cache results must be identical to the first search"
        );

        // A DIFFERENT pattern still reuses the SAME index (the per-query pattern
        // is independent of the indexed content) — still no rebuild.
        let r_filler = cached_results(&mut t, "filler");
        assert_eq!(
            t.search_index_rebuilds(),
            rebuilds_after_first,
            "a different pattern on unchanged content must NOT rebuild"
        );
        assert_eq!(r_filler, legacy_results(&t, "filler"));
        assert!(r_filler.len() >= 2, "many filler rows match");

        // Now WRITE new content: the next search must REBUILD and reflect it.
        t.process(b"NEEDLE_beta later\r\n");
        let rebuilds_before_write_search = t.search_index_rebuilds();
        let r_beta = cached_results(&mut t, "NEEDLE_beta");
        assert_eq!(
            t.search_index_rebuilds(),
            rebuilds_before_write_search + 1,
            "a search after a content write must rebuild the index"
        );
        assert_eq!(r_beta, legacy_results(&t, "NEEDLE_beta"));
        assert_eq!(r_beta.len(), 1, "the freshly written needle is found");

        // And the original needle is still found post-rebuild (content retained).
        assert_eq!(cached_results(&mut t, "NEEDLE_alpha"), r1);
    }

    /// A pure viewport scroll (display_offset) must NOT bump `content_seq`, so it
    /// must NOT invalidate the cache — the index already covers ALL retained
    /// lines regardless of which page is visible. Results stay identical.
    #[test]
    fn viewport_scroll_reuses_cache() {
        let mut t = Terminal::new(6, 40);
        t.process(b"NEEDLE_alpha\r\n");
        for i in 0..20 {
            t.process(format!("filler line {i}\r\n").as_bytes());
        }
        let r1 = cached_results(&mut t, "NEEDLE_alpha");
        let rebuilds = t.search_index_rebuilds();
        let gen_before = t.content_seq();

        // Scroll the viewport up into scrollback (content unchanged).
        t.grid_mut().scroll_display(5);
        assert_eq!(
            t.content_seq(),
            gen_before,
            "a pure viewport scroll must NOT bump content_seq"
        );

        let r2 = cached_results(&mut t, "NEEDLE_alpha");
        assert_eq!(
            t.search_index_rebuilds(),
            rebuilds,
            "a pure viewport scroll must NOT invalidate the search cache"
        );
        assert_eq!(r1, r2, "results identical across a viewport scroll");
    }
}
