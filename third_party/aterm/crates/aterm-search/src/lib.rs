// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Trigram-indexed search with Bloom filter acceleration.
//!
//! ## Design
//!
//! - Bloom filter for fast negative lookups (O(1) per trigram check)
//! - Trigram index for candidate filtering via posting-list intersection
//! - SparseBitmap (BTreeSet) for line number storage
//! - Generic interfaces for integrating with grid/scrollback providers
//!
//! ## Streaming Search
//!
//! The [`streaming`] module provides memory-bounded streaming search:
//! - Search through content incrementally (row by row)
//! - Memory-bounded results with configurable limits
//! - Multiple filter modes: Literal, Regex, Fuzzy
//! - Navigation with optional wraparound
//!
//! ## Performance
//!
//! | Operation | Time Complexity |
//! |-----------|-----------------|
//! | Negative lookup | O(t) bloom filter checks, t = query trigrams |
//! | Candidate search | O(t) posting-list intersections (SparseBitmap) |
//! | Verified search | O(t + k·L) where k = candidates, L = avg line length |
//! | Index line | O(n) where n = line length |
//!
//! Complexity claims derived from [`SearchIndex::search`] and
//! [`SearchIndex::search_with_positions`]. Bloom filter O(1)
//! per-check bound verified by operation counters in `bloom` module tests.
//!
//! ## Verification
//!
//! - Kani proofs: `no_false_negatives_symbolic`
//! - Tests: `search_with_positions`, `search_reverse_iterator_multiple_matches_per_line`
//! - Fuzz tests: `fuzz/fuzz_targets/search.rs` (in aterm-core)
//! - TLA+ spec: `tla/StreamingSearch.tla`

#![deny(unsafe_op_in_unsafe_fn)]
#![deny(missing_docs)]
#![deny(clippy::all)]

mod bitmap;
mod bloom;
mod grapheme;
mod index;
mod iterators;
pub mod streaming;
mod types;

pub use bloom::BloomFilter;
pub use index::{SearchIndex, SearchOptionsError, DEFAULT_MAX_CACHED_LINES};
pub use types::{SearchDirection, SearchMatch, SearchResults};

#[cfg(test)]
mod tests;

#[cfg(kani)]
mod proofs;

// ffi_kani_gaps removed (#5887): all three harnesses were null-guard proofs.

/// Terminal search that integrates with Grid and Scrollback.
///
/// This provides a unified interface for searching across:
/// - Current visible grid content
/// - Ring buffer scrollback
/// - Tiered scrollback (hot/warm/cold)
///
/// ## Staleness Detection
///
/// Every mutation to the search index (indexing, clearing, re-indexing) bumps
/// an internal generation counter. Consumers that cache search results should
/// snapshot the generation via [`generation()`](Self::generation) before
/// searching and compare it before using the results. A mismatch means the
/// index was mutated and the cached coordinates may be stale.
#[derive(Debug)]
pub struct TerminalSearch {
    /// Search index for all content.
    index: SearchIndex,
    /// Number of lines from scrollback that have been indexed.
    indexed_scrollback_lines: usize,
    /// Monotonically increasing generation counter.
    ///
    /// Bumped on every index mutation (line add, update, clear, invalidation).
    /// Consumers snapshot this before a search and compare before using results
    /// to detect stale match coordinates (#7271).
    generation: u64,
}

impl TerminalSearch {
    /// Create a new terminal search.
    #[must_use]
    pub fn new() -> Self {
        Self {
            index: SearchIndex::new(),
            indexed_scrollback_lines: 0,
            generation: 0,
        }
    }

    /// Create with expected capacity.
    #[must_use]
    pub fn with_capacity(expected_lines: usize) -> Self {
        Self {
            index: SearchIndex::with_capacity(expected_lines),
            indexed_scrollback_lines: 0,
            generation: 0,
        }
    }

    /// Create with expected capacity and an explicit cache cap.
    ///
    /// `max_cached_lines` bounds how many indexed lines are retained before the
    /// oldest are evicted. Use this when the scrollback window the GUI wants
    /// searchable differs from [`DEFAULT_MAX_CACHED_LINES`]. Eviction is
    /// observable via [`results_may_be_incomplete`](Self::results_may_be_incomplete).
    #[must_use]
    pub fn with_capacity_and_max(expected_lines: usize, max_cached_lines: usize) -> Self {
        Self {
            index: SearchIndex::with_capacity_and_max(expected_lines, max_cached_lines),
            indexed_scrollback_lines: 0,
            generation: 0,
        }
    }

    /// Set the maximum number of cached lines before eviction.
    ///
    /// Forwarded to the underlying [`SearchIndex`]. A value of 0 is clamped to
    /// 1. Does not bump the generation counter (no indexed content changes).
    pub fn set_max_cached_lines(&mut self, max: usize) {
        self.index.set_max_cached_lines(max);
    }

    /// The oldest line still retained in the index.
    ///
    /// See [`SearchIndex::lowest_retained_line`]. Matches below this line have
    /// been evicted; the searchable range is `[lowest_retained_line(),
    /// indexed_line_count())`.
    #[must_use]
    pub fn lowest_retained_line(&self) -> usize {
        self.index.lowest_retained_line()
    }

    /// Whether search results may be incomplete due to eviction.
    ///
    /// See [`SearchIndex::results_may_be_incomplete`]. When true, `cmd_search`
    /// should tell the AI results are truncated rather than exhaustive.
    #[must_use]
    pub fn results_may_be_incomplete(&self) -> bool {
        self.index.results_may_be_incomplete()
    }

    /// Get the current generation counter.
    ///
    /// Snapshot this value before a search operation. If the generation has
    /// changed by the time you use the results, the match coordinates may
    /// be stale and should be discarded or re-queried.
    #[must_use]
    #[inline]
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Bump the generation counter (internal helper).
    #[inline]
    fn bump_generation(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }

    /// Index a scrollback line.
    ///
    /// Call this when lines are pushed to scrollback.
    pub fn index_scrollback_line(&mut self, text: &str) {
        self.index.push_line(text);
        self.indexed_scrollback_lines += 1;
        self.bump_generation();
    }

    /// Index multiple scrollback lines.
    pub fn index_scrollback_lines(&mut self, lines: impl IntoIterator<Item = impl AsRef<str>>) {
        for line in lines {
            self.index_scrollback_line(line.as_ref());
        }
    }

    /// Re-index visible grid content.
    ///
    /// Call this to update the index with current grid content.
    /// Pass the visible content as an iterator of (line_index, text).
    pub fn index_visible_content(
        &mut self,
        base_line: usize,
        lines: impl IntoIterator<Item = impl AsRef<str>>,
    ) {
        for (offset, line) in lines.into_iter().enumerate() {
            self.index.index_line(base_line + offset, line.as_ref());
        }
        self.bump_generation();
    }

    /// Notify the search index that grid content has been invalidated.
    ///
    /// Call this when lines are deleted, scrollback is evicted, or the grid
    /// is cleared. This bumps the generation counter so that consumers holding
    /// stale `SearchMatch` coordinates can detect the invalidation.
    ///
    /// This does NOT remove entries from the underlying trigram index. If the
    /// invalidated lines need to be removed from the index, call [`clear()`]
    /// followed by re-indexing.
    pub fn invalidate(&mut self) {
        self.bump_generation();
    }

    /// Check if a query might have matches.
    #[must_use]
    pub fn might_contain(&self, query: &str) -> bool {
        self.index.might_contain(query)
    }

    /// Search for a query string.
    pub fn search(&self, query: &str) -> Vec<SearchMatch> {
        self.index.search_with_positions(query)
    }

    /// Search with options for case sensitivity and regex mode.
    ///
    /// When `case_sensitive` is true and `is_regex` is false, this uses the
    /// trigram-accelerated search path. Otherwise, all cached lines are scanned
    /// directly.
    pub fn search_opts(
        &self,
        query: &str,
        case_sensitive: bool,
        is_regex: bool,
    ) -> Result<Vec<SearchMatch>, SearchOptionsError> {
        self.index
            .search_with_positions_opts(query, case_sensitive, is_regex)
    }

    /// Search with options, returning matches bundled with the eviction signal.
    ///
    /// This is the entry point intended for the GUI's `cmd_search`: it returns
    /// absolute-row [`SearchMatch`]es plus an `incomplete` flag and the oldest
    /// searchable line, so truncated results can be reported to the AI honestly.
    /// See [`SearchIndex::search_results_opts`].
    pub fn search_results_opts(
        &self,
        query: &str,
        case_sensitive: bool,
        is_regex: bool,
    ) -> Result<SearchResults, SearchOptionsError> {
        self.index
            .search_results_opts(query, case_sensitive, is_regex)
    }

    /// Search in the specified direction.
    pub fn search_ordered(&self, query: &str, direction: SearchDirection) -> Vec<SearchMatch> {
        self.index.search_ordered(query, direction)
    }

    /// Find the next match after the given position.
    ///
    /// This uses O(log n) range queries to skip lines before `after_line`,
    /// then iterates with early termination to find the first match.
    pub fn find_next(
        &self,
        query: &str,
        after_line: usize,
        after_col: usize,
    ) -> Option<SearchMatch> {
        // Use optimized range query starting from after_line
        self.index
            .search_from_line(query, after_line)
            .find(|m| m.line > after_line || (m.line == after_line && m.start_col > after_col))
    }

    /// Find the previous match before the given position.
    ///
    /// This uses O(log n) range queries to only search lines before `before_line`,
    /// then iterates with early termination to find the first match.
    pub fn find_prev(
        &self,
        query: &str,
        before_line: usize,
        before_col: usize,
    ) -> Option<SearchMatch> {
        // Include `before_line` itself by using an exclusive upper bound.
        // Saturate at `usize::MAX` to avoid overflow for sentinel/high-bound callers.
        let exclusive_upper = before_line.saturating_add(1);
        self.index
            .search_before_line(query, exclusive_upper)
            .find(|m| m.line < before_line || (m.line == before_line && m.start_col < before_col))
    }

    /// Get the number of indexed lines.
    #[must_use]
    #[doc(hidden)]
    pub fn indexed_line_count(&self) -> usize {
        self.index.len()
    }

    /// Get the number of scrollback lines indexed.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn indexed_scrollback_count(&self) -> usize {
        self.indexed_scrollback_lines
    }

    /// Clear the search index.
    pub fn clear(&mut self) {
        self.index.clear();
        self.indexed_scrollback_lines = 0;
        self.bump_generation();
    }
}

impl Default for TerminalSearch {
    fn default() -> Self {
        Self::new()
    }
}
