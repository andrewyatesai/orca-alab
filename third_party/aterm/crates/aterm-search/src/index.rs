// Copyright 2026 The aterm Authors
// Author: The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors <ayates.com>

//! Core trigram search index with bloom filter acceleration.

use aterm_hash::{FxBuildHasher, FxHashMap};

use crate::bitmap::SparseBitmap;

use super::bloom::BloomFilter;
use super::iterators::{CandidateSource, SearchMatchIterator, SearchMatchReverseIterator};
use super::types::{SearchDirection, SearchMatch, SearchResult, SearchResults};
use crate::grapheme::ColumnMap;

/// Default maximum number of lines to keep in the search index cache.
/// Eviction triggers when cache exceeds this limit, removing the oldest 25%.
///
/// This is the *default* only; callers that need a different bound should use
/// [`SearchIndex::with_max_cached_lines`] (or
/// [`SearchIndex::set_max_cached_lines`]). The default value is unchanged from
/// the original hard-coded constant — behavior is preserved; the cap is now
/// configurable and eviction is now observable (see
/// [`SearchIndex::results_may_be_incomplete`]).
pub const DEFAULT_MAX_CACHED_LINES: usize = 100_000;

// Deterministic test instrumentation for range-query candidate counts.
//
// This tracks how many line IDs are materialized from `bitmap.range(...)` in
// `search_from_line`, allowing scaling tests to verify that `find_next` does
// not depend on prefix matches before the search start line.
#[cfg(test)]
thread_local! {
    static SEARCH_FROM_LINE_CANDIDATES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
fn count_search_from_line_candidates(count: usize) {
    SEARCH_FROM_LINE_CANDIDATES.with(|c| c.set(c.get() + count));
}

/// Search index using trigrams with bloom filter acceleration.
///
/// The index maintains:
/// - A bloom filter for instant negative lookups
/// - A trigram map for candidate line identification
/// - Line content cache for match verification (with capacity-based eviction)
#[derive(Debug)]
pub struct SearchIndex {
    /// Bloom filter for fast negative lookups.
    bloom: BloomFilter,
    /// Trigram -> line numbers mapping.
    trigrams: FxHashMap<[u8; 3], SparseBitmap>,
    /// Cached line content for match verification.
    /// Maps line number to line text. Evicted when exceeding `max_cached_lines`.
    pub(super) lines: FxHashMap<usize, String>,
    /// Cached column maps for search hit lines.
    /// Built at index time and reused across searches to avoid O(G)-per-query
    /// reconstruction (#7373).
    pub(super) column_maps: FxHashMap<usize, ColumnMap>,
    /// Total number of indexed lines.
    line_count: usize,
    /// Next line number to index (for incremental indexing).
    next_line: usize,
    /// Maximum lines to keep in cache before eviction.
    max_cached_lines: usize,
    /// Lowest line number still retained in the index.
    ///
    /// Starts at 0 and advances each time eviction drops the oldest cached
    /// lines. Any match at a line below this watermark has been evicted and can
    /// no longer be returned, even though [`len`](Self::len) (which tracks the
    /// highest indexed line) keeps growing. Exposed via
    /// [`lowest_retained_line`](Self::lowest_retained_line) so callers can tell
    /// the AI which range of scrollback is actually searchable.
    lowest_retained_line: usize,
    /// Whether eviction has ever dropped lines from this index.
    ///
    /// Once true, search results may be incomplete: matches in evicted lines
    /// are silently absent. Surfaced via
    /// [`results_may_be_incomplete`](Self::results_may_be_incomplete) so the
    /// future `cmd_search` can flag truncated results to the AI rather than
    /// presenting them as exhaustive.
    eviction_occurred: bool,
    /// Guards the one-time `aterm_log` warning emitted on first eviction.
    first_eviction_warned: bool,
}

/// Convert a line number to u32 for `SparseBitmap` storage.
///
/// Line numbers are bounded by scrollback limits (max ~1M lines) which fits
/// in u32 (max ~4B). Saturates at `u32::MAX` for defensive safety.
fn line_as_u32(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// Intersect a pre-sorted (ascending by len) slice of posting list references
/// into a single owned bitmap.
///
/// Avoids cloning the smallest list when there are 2+ lists: the first
/// intersection uses `&a & &b` (borrowed on both sides) which constructs the
/// result directly. Only when there is exactly one list does a clone occur,
/// because we need an owned bitmap for downstream consumers (#7375).
fn intersect_posting_lists(sorted_lists: &[&SparseBitmap]) -> SparseBitmap {
    debug_assert!(!sorted_lists.is_empty());
    if sorted_lists.len() == 1 {
        return sorted_lists[0].clone();
    }
    // First pair: borrow-borrow intersection avoids cloning the smallest list.
    let mut result: SparseBitmap = sorted_lists[0] & sorted_lists[1];
    for bitmap in &sorted_lists[2..] {
        result &= *bitmap;
    }
    result
}

impl SearchIndex {
    /// Take (read and reset) `search_from_line` candidate count.
    #[cfg(test)]
    pub(crate) fn take_search_from_line_candidates() -> usize {
        SEARCH_FROM_LINE_CANDIDATES.with(|c| {
            let value = c.get();
            c.set(0);
            value
        })
    }

    #[cfg(test)]
    pub(crate) fn bloom_is_saturated(&self) -> bool {
        self.bloom.is_saturated()
    }

    /// Create a new search index with the default cache cap
    /// ([`DEFAULT_MAX_CACHED_LINES`]).
    #[must_use]
    pub fn new() -> Self {
        Self {
            bloom: BloomFilter::with_capacity(100_000),
            trigrams: FxHashMap::default(),
            lines: FxHashMap::default(),
            column_maps: FxHashMap::default(),
            line_count: 0,
            next_line: 0,
            max_cached_lines: DEFAULT_MAX_CACHED_LINES,
            lowest_retained_line: 0,
            eviction_occurred: false,
            first_eviction_warned: false,
        }
    }

    /// Create a new search index with expected capacity.
    ///
    /// Uses the default cache cap ([`DEFAULT_MAX_CACHED_LINES`]); pair with
    /// [`set_max_cached_lines`](Self::set_max_cached_lines) or use
    /// [`with_capacity_and_max`](Self::with_capacity_and_max) to override it.
    #[must_use]
    pub fn with_capacity(expected_lines: usize) -> Self {
        Self::with_capacity_and_max(expected_lines, DEFAULT_MAX_CACHED_LINES)
    }

    /// Create a new search index with an explicit cache cap.
    ///
    /// `max_cached_lines` is the number of distinct line entries kept before
    /// eviction drops the oldest 25%. A value of 0 is clamped to 1 so the index
    /// can always hold the most recent line. See
    /// [`results_may_be_incomplete`](Self::results_may_be_incomplete) for the
    /// eviction signal.
    #[must_use]
    pub fn with_max_cached_lines(max_cached_lines: usize) -> Self {
        let mut index = Self::new();
        index.set_max_cached_lines(max_cached_lines);
        index
    }

    /// Create a new search index with expected capacity and an explicit cap.
    #[must_use]
    pub fn with_capacity_and_max(expected_lines: usize, max_cached_lines: usize) -> Self {
        Self {
            bloom: BloomFilter::with_capacity(expected_lines.max(1000)),
            trigrams: FxHashMap::with_capacity_and_hasher(expected_lines / 10, FxBuildHasher),
            lines: FxHashMap::with_capacity_and_hasher(expected_lines, FxBuildHasher),
            column_maps: FxHashMap::with_capacity_and_hasher(expected_lines, FxBuildHasher),
            line_count: 0,
            next_line: 0,
            max_cached_lines: max_cached_lines.max(1),
            lowest_retained_line: 0,
            eviction_occurred: false,
            first_eviction_warned: false,
        }
    }

    /// Index a line at a specific line number.
    ///
    /// This overwrites any existing content at that line number.
    pub fn index_line(&mut self, line_num: usize, text: &str) {
        // Remove old trigrams if this line was previously indexed.
        // Use remove() to move the old String out (avoids clone).
        if let Some(old_text) = self.lines.remove(&line_num) {
            self.remove_trigrams(line_num, &old_text);
        }

        let bytes = text.as_bytes();
        let line_u32 = line_as_u32(line_num);

        // Add all trigrams from this line (original case).
        for window in bytes.windows(3) {
            let trigram: [u8; 3] = [window[0], window[1], window[2]];
            self.bloom.insert_bytes(&trigram);
            self.trigrams.entry(trigram).or_default().insert(line_u32);
        }

        // Also insert Unicode-lowercased trigrams for case-insensitive
        // bloom filter and posting-list acceleration (#7273, #7398, #7470).
        // Uses full Unicode lowercasing so non-ASCII characters
        // (e.g., Ä→ä, É→é) are indexed correctly.
        let lowered = text.to_lowercase();
        for window in lowered.as_bytes().windows(3) {
            let trigram: [u8; 3] = [window[0], window[1], window[2]];
            self.bloom.insert_bytes(&trigram);
            self.trigrams.entry(trigram).or_default().insert(line_u32);
        }

        // Cache the line content and precomputed column map (#7373).
        self.lines.insert(line_num, text.to_string());
        self.column_maps.insert(line_num, ColumnMap::new(text));
        self.line_count = self.line_count.max(line_num.saturating_add(1));
        self.next_line = self.next_line.max(line_num.saturating_add(1));

        // Evict oldest cached lines if over capacity
        if self.lines.len() > self.max_cached_lines {
            self.evict_oldest_lines();
        }

        // Rebuild bloom filter if saturated (#7243). When the estimated FPR
        // exceeds 50%, the bloom filter returns true for most queries, making
        // it useless as a negative filter. Rebuild from remaining cached lines
        // to restore its effectiveness.
        if self.bloom.is_saturated() {
            self.rebuild_bloom();
        }
    }

    /// Index a line at the next available line number.
    ///
    /// Returns the assigned line number.
    pub fn push_line(&mut self, text: &str) -> usize {
        let line_num = self.next_line;
        self.index_line(line_num, text);
        line_num
    }

    /// Remove trigrams for a line (internal helper).
    ///
    /// Prunes empty bitmaps from the trigrams map to prevent unbounded growth (#2111).
    fn remove_trigrams(&mut self, line_num: usize, text: &str) {
        let bytes = text.as_bytes();
        let line_u32 = line_as_u32(line_num);

        // Remove original-case trigrams.
        for window in bytes.windows(3) {
            let trigram: [u8; 3] = [window[0], window[1], window[2]];
            if let Some(bitmap) = self.trigrams.get_mut(&trigram) {
                bitmap.remove(line_u32);
                if bitmap.is_empty() {
                    self.trigrams.remove(&trigram);
                }
            }
        }

        // Remove Unicode-lowercased trigrams (#7398, #7470).
        let lowered = text.to_lowercase();
        for window in lowered.as_bytes().windows(3) {
            let trigram: [u8; 3] = [window[0], window[1], window[2]];
            if let Some(bitmap) = self.trigrams.get_mut(&trigram) {
                bitmap.remove(line_u32);
                if bitmap.is_empty() {
                    self.trigrams.remove(&trigram);
                }
            }
        }
    }

    /// Evict the oldest 25% of cached lines when capacity is exceeded.
    ///
    /// Collects and sorts cached line numbers to evict in O(n log n) instead
    /// of scanning linearly through gaps. With sparse line numbers (e.g.,
    /// visible content at line 50000 after scrollback lines 0-99), the
    /// previous approach was O(gap_size) which caused visible stalls. (#7246)
    fn evict_oldest_lines(&mut self) {
        let target = self.max_cached_lines * 3 / 4;
        let to_evict = self.lines.len().saturating_sub(target);
        if to_evict == 0 {
            return;
        }

        // Collect and sort line numbers to find the oldest entries.
        let mut line_nums: Vec<usize> = self.lines.keys().copied().collect();
        line_nums.sort_unstable();

        // Remove the oldest `to_evict` entries.
        let evict_count = to_evict.min(line_nums.len());
        for &line in &line_nums[..evict_count] {
            if let Some(text) = self.lines.remove(&line) {
                self.remove_trigrams(line, &text);
            }
            self.column_maps.remove(&line);
        }

        // Advance the retained-line watermark to the smallest remaining line.
        // Matches below this line are gone and can no longer be returned, so
        // callers must treat results that span this range as incomplete.
        self.lowest_retained_line = line_nums.get(evict_count).copied().unwrap_or(self.next_line);
        self.eviction_occurred = true;

        // Warn once: results are now potentially incomplete for the lifetime of
        // this index. Repeated eviction passes do not re-warn (avoids log spam).
        if !self.first_eviction_warned {
            self.first_eviction_warned = true;
            aterm_log::warn!(
                "search index exceeded {} cached lines; evicting oldest entries — \
                 search results may be incomplete below line {} (oldest indexed line is now {})",
                self.max_cached_lines,
                self.lowest_retained_line,
                self.lowest_retained_line,
            );
        }

        // Rebuild bloom filter from remaining lines (#7270).
        self.rebuild_bloom();
    }

    /// Rebuild the bloom filter sized for the current trigram load.
    ///
    /// The bloom filter stores trigrams, not lines. Rebuilding it with only
    /// `lines.len()` capacity badly underestimates the true insert volume for
    /// wide scrollback lines and causes immediate re-saturation, which in turn
    /// can trigger a rebuild on nearly every indexed line. Use the current
    /// trigram insert count as the rebuild target so the resized filter tracks
    /// actual load rather than line cardinality (#7243).
    fn rebuild_bloom(&mut self) {
        let capacity = self.bloom.item_count().max(self.lines.len()).max(1000);
        self.bloom = BloomFilter::with_capacity(capacity);
        for text in self.lines.values() {
            // Insert original-case trigrams.
            for window in text.as_bytes().windows(3) {
                let trigram: [u8; 3] = [window[0], window[1], window[2]];
                self.bloom.insert_bytes(&trigram);
            }
            // Insert Unicode-lowercased trigrams for case-insensitive
            // bloom filter acceleration (#7273, #7470).
            let lowered = text.to_lowercase();
            for window in lowered.as_bytes().windows(3) {
                let trigram: [u8; 3] = [window[0], window[1], window[2]];
                self.bloom.insert_bytes(&trigram);
            }
        }
    }

    /// Check if a query might have matches (bloom filter check).
    ///
    /// Returns `false` if definitely no matches exist.
    /// Returns `true` if matches are possible (verify with actual search).
    #[must_use]
    pub fn might_contain(&self, query: &str) -> bool {
        let bytes = query.as_bytes();

        // For short queries, we can't use the bloom filter effectively
        if bytes.len() < 3 {
            return true;
        }

        // Check if all query trigrams might exist
        for window in bytes.windows(3) {
            if !self.bloom.might_contain_bytes(window) {
                return false;
            }
        }
        true
    }

    /// Search for a query string.
    ///
    /// Returns line numbers that might contain the query.
    /// Results may include false positives but never false negatives.
    pub fn search(&self, query: &str) -> impl Iterator<Item = u32> + '_ + use<'_> {
        let bytes = query.as_bytes();

        if bytes.len() < 3 {
            // Can't use trigram index for short queries
            // Fall back to returning all lines (caller must verify)
            return SearchResult::All(0..line_as_u32(self.line_count));
        }

        // Quick bloom filter check
        if !self.might_contain(query) {
            return SearchResult::None;
        }

        // Intersect posting lists for all trigrams.
        // Collect references first, then build the result starting from the
        // smallest posting list to minimize clone + intersection cost (#7357).
        let mut posting_lists: Vec<&SparseBitmap> = Vec::new();

        for window in bytes.windows(3) {
            let trigram: [u8; 3] = [window[0], window[1], window[2]];

            if let Some(bitmap) = self.trigrams.get(&trigram) {
                posting_lists.push(bitmap);
            } else {
                // Trigram not found, no matches possible
                return SearchResult::None;
            }
        }

        if posting_lists.is_empty() {
            return SearchResult::None;
        }

        // Sort by size so intersect_posting_lists starts from the smallest.
        posting_lists.sort_unstable_by_key(|b| b.len());
        let result = intersect_posting_lists(&posting_lists);
        SearchResult::Bitmap(Box::new(result.into_iter()))
    }

    /// Search with match verification and position extraction.
    ///
    /// Returns actual matches with column positions.
    /// This verifies candidates against cached line content.
    pub fn search_with_positions(&self, query: &str) -> Vec<SearchMatch> {
        // Empty query returns no matches (prevents infinite loop in find)
        if query.is_empty() {
            return Vec::new();
        }

        let mut matches = Vec::new();

        for line_num in self.search(query) {
            let line_idx = line_num as usize;
            if let Some(text) = self.lines.get(&line_idx) {
                let col_map = self.column_maps.get(&line_idx);
                let mut start = 0;
                while let Some(pos) = text[start..].find(query) {
                    let abs_pos = start + pos;
                    let (start_col, end_col) = if let Some(cm) = col_map {
                        (
                            cm.byte_to_column(abs_pos),
                            cm.byte_to_column(abs_pos + query.len()),
                        )
                    } else {
                        // Fallback: build on the fly if cache was evicted
                        // before the line text (should not happen in practice).
                        let cm = ColumnMap::new(text);
                        (
                            cm.byte_to_column(abs_pos),
                            cm.byte_to_column(abs_pos + query.len()),
                        )
                    };
                    matches.push(SearchMatch::new(line_idx, start_col, end_col));
                    // Advance by one character (not one byte) to stay on a
                    // valid char boundary. This prevents panicking when the
                    // query contains multi-byte UTF-8 characters.
                    start = abs_pos + text[abs_pos..].chars().next().map_or(1, char::len_utf8);
                }
            }
        }

        matches
    }

    /// Search and return matches in the specified direction.
    ///
    /// Returns an iterator over matches sorted by line number.
    pub fn search_ordered(&self, query: &str, direction: SearchDirection) -> Vec<SearchMatch> {
        let mut matches = self.search_with_positions(query);

        match direction {
            SearchDirection::Forward => {
                matches.sort_by_key(|m| (m.line, m.start_col));
            }
            SearchDirection::Backward => {
                matches
                    .sort_by_key(|m| (std::cmp::Reverse(m.line), std::cmp::Reverse(m.start_col)));
            }
        }

        matches
    }

    /// Get the number of indexed lines.
    #[must_use]
    pub fn len(&self) -> usize {
        self.line_count
    }

    /// Returns true if the index is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.line_count == 0
    }

    /// Clear the index.
    ///
    /// Resets the eviction watermark and incomplete-results signal: a cleared
    /// index has not evicted anything, so [`results_may_be_incomplete`] is
    /// false again until the cache cap is next exceeded.
    ///
    /// [`results_may_be_incomplete`]: Self::results_may_be_incomplete
    pub fn clear(&mut self) {
        self.bloom.clear();
        self.trigrams.clear();
        self.lines.clear();
        self.column_maps.clear();
        self.line_count = 0;
        self.next_line = 0;
        self.lowest_retained_line = 0;
        self.eviction_occurred = false;
        self.first_eviction_warned = false;
    }

    /// Set the maximum number of cached lines before eviction.
    ///
    /// A value of 0 is clamped to 1 so the index always retains at least the
    /// most recent line. Lowering the cap below the current cache size does not
    /// retroactively evict — eviction happens on the next [`index_line`] that
    /// exceeds the new cap.
    ///
    /// [`index_line`]: Self::index_line
    pub fn set_max_cached_lines(&mut self, max: usize) {
        self.max_cached_lines = max.max(1);
    }

    /// Current maximum cached-lines cap.
    #[must_use]
    pub fn max_cached_lines(&self) -> usize {
        self.max_cached_lines
    }

    /// The oldest line number still retained in the index.
    ///
    /// Returns 0 until eviction occurs. After eviction this is the lowest line
    /// that can still produce a match; any match in scrollback below this line
    /// has been dropped from the index. Callers (e.g. `cmd_search`) can report
    /// the searchable range `[lowest_retained_line(), len())` to the AI.
    #[must_use]
    pub fn lowest_retained_line(&self) -> usize {
        self.lowest_retained_line
    }

    /// Whether search results may be incomplete due to eviction.
    ///
    /// `false` means every indexed line is still cached and search is
    /// exhaustive over the indexed range. `true` means the cache cap has been
    /// exceeded at least once and the oldest lines were dropped, so matches
    /// below [`lowest_retained_line`](Self::lowest_retained_line) are silently
    /// absent. The future `cmd_search` should pass this through so the AI is
    /// told results are truncated rather than treating them as exhaustive.
    #[must_use]
    pub fn results_may_be_incomplete(&self) -> bool {
        self.eviction_occurred
    }

    /// Get cached line content by line number.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn get_line(&self, line_num: usize) -> Option<&str> {
        self.lines.get(&line_num).map(|s| s.as_str())
    }

    /// Search for matches starting from a given line (O(log n) for first match).
    ///
    /// Returns an iterator over matches in forward order (oldest to newest),
    /// starting from `from_line`. This is efficient for `find_next` operations
    /// as it uses range queries on the trigram index to skip earlier lines.
    ///
    /// # Arguments
    /// * `query` - The search query (must be 3+ chars for trigram indexing)
    /// * `from_line` - Start searching from this line number (inclusive)
    pub(crate) fn search_from_line<'a>(
        &'a self,
        query: &'a str,
        from_line: usize,
    ) -> SearchMatchIterator<'a> {
        let bytes = query.as_bytes();

        let empty = || CandidateSource::Materialized(Vec::new().into_iter());

        // Empty query returns no matches
        if query.is_empty() {
            return SearchMatchIterator::new(self, query, empty());
        }

        if bytes.len() < 3 {
            // Can't use trigram index for short queries — use lazy range
            // instead of collecting all line numbers into a Vec (avoids O(n) alloc)
            let source =
                CandidateSource::Range(line_as_u32(from_line)..line_as_u32(self.line_count));
            return SearchMatchIterator::new(self, query, source);
        }

        // Quick bloom filter check
        if !self.might_contain(query) {
            return SearchMatchIterator::new(self, query, empty());
        }

        // Intersect posting lists for all trigrams.
        // Collect references first, then build the result starting from the
        // smallest posting list to minimize clone + intersection cost (#7357).
        let mut posting_lists: Vec<&SparseBitmap> = Vec::new();

        for window in bytes.windows(3) {
            let trigram: [u8; 3] = [window[0], window[1], window[2]];

            if let Some(bitmap) = self.trigrams.get(&trigram) {
                posting_lists.push(bitmap);
            } else {
                // Trigram not found, no matches possible
                return SearchMatchIterator::new(self, query, empty());
            }
        }

        if posting_lists.is_empty() {
            return SearchMatchIterator::new(self, query, empty());
        }

        // Sort by size so intersect_posting_lists starts from the smallest.
        posting_lists.sort_unstable_by_key(|b| b.len());
        let result = intersect_posting_lists(&posting_lists);

        // Lazy iteration: remove elements below from_line (O(log n))
        // and iterate remaining candidates on demand. Avoids O(k) collect()
        // when only the first few matches are needed (e.g., find_next).
        #[cfg(test)]
        {
            // len() is O(1) on SparseBitmap after remove_range
            let count = result.range(line_as_u32(from_line)..).count();
            count_search_from_line_candidates(count);
        }
        let source = CandidateSource::from_bitmap_forward(result, line_as_u32(from_line));
        SearchMatchIterator::new(self, query, source)
    }

    /// Search for matches up to a given line for backward iteration.
    ///
    /// Returns an iterator over matches in reverse order (newest to oldest),
    /// only considering lines before `before_line`. This is efficient for
    /// `find_prev` operations.
    ///
    /// # Arguments
    /// * `query` - The search query (must be 3+ chars for trigram indexing)
    /// * `before_line` - Only search lines before this line number (exclusive)
    pub(crate) fn search_before_line<'a>(
        &'a self,
        query: &'a str,
        before_line: usize,
    ) -> SearchMatchReverseIterator<'a> {
        let bytes = query.as_bytes();

        let empty = || CandidateSource::Materialized(Vec::new().into_iter());

        // Empty query returns no matches
        if query.is_empty() {
            return SearchMatchReverseIterator::new(self, query, empty());
        }

        if bytes.len() < 3 {
            // Can't use trigram index for short queries — use lazy reversed range
            // instead of collecting all line numbers into a Vec (avoids O(n) alloc)
            let source =
                CandidateSource::RangeRev((0..line_as_u32(before_line.min(self.line_count))).rev());
            return SearchMatchReverseIterator::new(self, query, source);
        }

        // Quick bloom filter check
        if !self.might_contain(query) {
            return SearchMatchReverseIterator::new(self, query, empty());
        }

        // Intersect posting lists for all trigrams.
        // Collect references first, then build the result starting from the
        // smallest posting list to minimize clone + intersection cost (#7357).
        let mut posting_lists: Vec<&SparseBitmap> = Vec::new();

        for window in bytes.windows(3) {
            let trigram: [u8; 3] = [window[0], window[1], window[2]];

            if let Some(bitmap) = self.trigrams.get(&trigram) {
                posting_lists.push(bitmap);
            } else {
                // Trigram not found, no matches possible
                return SearchMatchReverseIterator::new(self, query, empty());
            }
        }

        if posting_lists.is_empty() {
            return SearchMatchReverseIterator::new(self, query, empty());
        }

        // Sort by size so intersect_posting_lists starts from the smallest.
        posting_lists.sort_unstable_by_key(|b| b.len());
        let result = intersect_posting_lists(&posting_lists);

        // Use range query to get only lines before before_line (O(log n) in SparseBitmap)
        let mut candidates: Vec<u32> = result.range(..line_as_u32(before_line)).collect();
        // Reverse in-place: bitmap.range() yields ascending order, we need
        // descending. .reverse() is O(n) vs .sort() which is O(n log n).
        candidates.reverse();
        SearchMatchReverseIterator::new(
            self,
            query,
            CandidateSource::Materialized(candidates.into_iter()),
        )
    }
}

/// Maximum pattern length for regex compilation (bytes).
///
/// Matches the streaming engine's default `max_pattern_len` (1024). Patterns
/// beyond this limit are rejected before compilation to bound CPU cost.
#[allow(dead_code)]
const MAX_REGEX_PATTERN_LEN: usize = 1024;

/// Maximum compiled regex size (bytes) passed to `RegexBuilder::size_limit`.
///
/// Caps NFA/DFA memory to 1 MiB, well below the `regex` crate's 10 MiB
/// default. This bounds compilation time for deeply nested alternations and
/// large repetition counts that could otherwise cause ReDoS via compilation.
#[cfg(feature = "regex")]
const REGEX_SIZE_LIMIT: usize = 1 << 20; // 1 MiB

/// Maximum DFA size (bytes) passed to `RegexBuilder::dfa_size_limit`.
///
/// Caps DFA cache to 1 MiB. The DFA is built lazily during matching, so this
/// bounds per-query memory even for patterns that pass the NFA size gate.
#[cfg(feature = "regex")]
const REGEX_DFA_SIZE_LIMIT: usize = 1 << 20; // 1 MiB

/// Error returned when search options are invalid (e.g., regex feature not enabled).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, aterm_error::Error)]
pub enum SearchOptionsError {
    /// Regex was requested but the feature is not compiled in.
    #[error("regex feature not enabled")]
    RegexNotEnabled,
    /// The regex pattern is invalid.
    #[error("invalid regex: {0}")]
    InvalidRegex(String),
    /// The regex pattern exceeds the maximum allowed length.
    #[error("pattern exceeds maximum length ({MAX_REGEX_PATTERN_LEN} bytes)")]
    PatternTooLong,
}

impl SearchIndex {
    /// Search with match verification, supporting case-insensitive and regex modes.
    ///
    /// When `case_sensitive` is true and `is_regex` is false, this delegates to
    /// the trigram-accelerated `search_with_positions`. Otherwise, it scans all
    /// cached lines directly.
    pub fn search_with_positions_opts(
        &self,
        query: &str,
        case_sensitive: bool,
        is_regex: bool,
    ) -> Result<Vec<SearchMatch>, SearchOptionsError> {
        if query.is_empty() {
            return Ok(Vec::new());
        }

        // Fast path: case-sensitive literal → trigram-accelerated search
        if case_sensitive && !is_regex {
            return Ok(self.search_with_positions(query));
        }

        if is_regex {
            return self.search_regex(query, case_sensitive);
        }

        // Case-insensitive literal search
        Ok(self.search_case_insensitive(query))
    }

    /// Search with options, returning matches bundled with the eviction signal.
    ///
    /// Identical matching to [`search_with_positions_opts`], but wraps the
    /// result in [`SearchResults`] so the caller learns whether eviction may
    /// have dropped matches ([`results_may_be_incomplete`]) and which line is
    /// the oldest still searchable ([`lowest_retained_line`]). This is the
    /// entry point intended for `cmd_search`, which must tell the AI when
    /// results are truncated.
    ///
    /// [`search_with_positions_opts`]: Self::search_with_positions_opts
    /// [`results_may_be_incomplete`]: Self::results_may_be_incomplete
    /// [`lowest_retained_line`]: Self::lowest_retained_line
    pub fn search_results_opts(
        &self,
        query: &str,
        case_sensitive: bool,
        is_regex: bool,
    ) -> Result<SearchResults, SearchOptionsError> {
        let matches = self.search_with_positions_opts(query, case_sensitive, is_regex)?;
        Ok(SearchResults::new(
            matches,
            self.results_may_be_incomplete(),
            self.lowest_retained_line(),
        ))
    }

    /// Regex search across all cached lines.
    fn search_regex(
        &self,
        query: &str,
        case_sensitive: bool,
    ) -> Result<Vec<SearchMatch>, SearchOptionsError> {
        #[cfg(feature = "regex")]
        {
            if query.len() > MAX_REGEX_PATTERN_LEN {
                return Err(SearchOptionsError::PatternTooLong);
            }
            let pattern = if case_sensitive {
                query.to_string()
            } else {
                format!("(?i){query}")
            };
            let re = regex::RegexBuilder::new(&pattern)
                .size_limit(REGEX_SIZE_LIMIT)
                .dfa_size_limit(REGEX_DFA_SIZE_LIMIT)
                .build()
                .map_err(|e| SearchOptionsError::InvalidRegex(e.to_string()))?;
            let mut matches = Vec::new();
            for (&line_num, text) in &self.lines {
                // Use cached column map when available (#7373).
                let fallback;
                let col_map = match self.column_maps.get(&line_num) {
                    Some(cm) => cm,
                    None => {
                        fallback = ColumnMap::new(text);
                        &fallback
                    }
                };
                for cap in re.find_iter(text) {
                    // Skip zero-length matches (e.g. `^`, `\b`, `x*` at
                    // non-matching positions). These produce SearchMatch with
                    // start_col == end_col which causes saturating_sub(1)
                    // overflow in downstream consumers (convert_search_match).
                    if cap.start() == cap.end() {
                        continue;
                    }
                    matches.push(SearchMatch::new(
                        line_num,
                        col_map.byte_to_column(cap.start()),
                        col_map.byte_to_column(cap.end()),
                    ));
                }
            }
            matches.sort_by_key(|m| (m.line, m.start_col));
            Ok(matches)
        }
        #[cfg(not(feature = "regex"))]
        {
            let _ = (query, case_sensitive);
            Err(SearchOptionsError::RegexNotEnabled)
        }
    }

    /// Case-insensitive literal search across all cached lines.
    ///
    /// Uses lowercased trigrams for bloom filter negative filtering before
    /// scanning lines. For queries >= 3 bytes, this rejects lines that
    /// definitely do not contain the query, avoiding the full O(n) scan
    /// for most lines. Part of #7273.
    ///
    /// Uses an ASCII fast path (zero heap allocation) for the common case where
    /// both query and line are pure ASCII. Falls back to a reusable `String`
    /// buffer for non-ASCII lines, avoiding the previous per-line `to_lowercase()`
    /// allocation. See #6726.
    fn search_case_insensitive(&self, query: &str) -> Vec<SearchMatch> {
        use crate::grapheme::LowerByteMap;

        let lower_query = query.to_lowercase();
        let query_bytes = lower_query.as_bytes();
        let query_is_ascii = query.is_ascii();
        let mut matches = Vec::new();
        let mut lower_buf = String::new();

        // Use lowercased trigrams in the posting-list index for candidate
        // filtering (#7398). Since index_line() now stores lowercased trigrams
        // in self.trigrams, we can intersect posting lists to find candidate
        // lines instead of scanning all cached lines.
        // Collect references first, then build from smallest to minimize
        // clone + intersection cost (#7357).
        let candidate_lines: Option<SparseBitmap> = if query_bytes.len() >= 3 {
            let mut posting_lists: Vec<&SparseBitmap> = Vec::new();
            for w in query_bytes.windows(3) {
                let trigram: [u8; 3] = [w[0], w[1], w[2]];
                match self.trigrams.get(&trigram) {
                    Some(bitmap) => posting_lists.push(bitmap),
                    None => return matches, // trigram absent → no matches
                }
            }
            if posting_lists.is_empty() {
                None
            } else {
                posting_lists.sort_unstable_by_key(|b| b.len());
                Some(intersect_posting_lists(&posting_lists))
            }
        } else {
            None // query too short for trigram filtering
        };

        // Iterate only over candidate lines (posting-list intersection) or
        // all lines if the query is too short for trigram filtering.
        let line_iter: Box<dyn Iterator<Item = (&usize, &String)>> =
            if let Some(ref candidates) = candidate_lines {
                Box::new(candidates.iter().filter_map(|line_u32| {
                    let line_num = line_u32 as usize;
                    self.lines.get_key_value(&line_num)
                }))
            } else {
                Box::new(self.lines.iter())
            };

        for (&line_num, text) in line_iter {
            // Use cached column map when available (#7373).
            let fallback;
            let col_map = match self.column_maps.get(&line_num) {
                Some(cm) => cm,
                None => {
                    fallback = ColumnMap::new(text);
                    &fallback
                }
            };

            if query_is_ascii && text.is_ascii() {
                // Fast path: ASCII-only — byte-level comparison, zero allocation.
                let text_bytes = text.as_bytes();
                let qlen = query_bytes.len();
                if qlen == 0 || qlen > text_bytes.len() {
                    continue;
                }
                let mut start = 0;
                while start + qlen <= text_bytes.len() {
                    let matches_here = text_bytes[start..start + qlen]
                        .iter()
                        .zip(query_bytes)
                        .all(|(a, b)| a.to_ascii_lowercase() == *b);
                    if matches_here {
                        matches.push(SearchMatch::new(
                            line_num,
                            col_map.byte_to_column(start),
                            col_map.byte_to_column(start + qlen),
                        ));
                    }
                    start += 1;
                }
            } else {
                // Slow path: non-ASCII — reuse buffer across lines.
                lower_buf.clear();
                lower_buf.extend(text.chars().flat_map(char::to_lowercase));
                let byte_map = LowerByteMap::new(text);
                let mut start = 0;
                while let Some(pos) = lower_buf[start..].find(&lower_query) {
                    let abs_pos = start + pos;
                    let orig_start = byte_map.map_to_original(abs_pos);
                    let orig_end = byte_map.map_to_original(abs_pos + lower_query.len());
                    matches.push(SearchMatch::new(
                        line_num,
                        col_map.byte_to_column(orig_start),
                        col_map.byte_to_column(orig_end),
                    ));
                    start = abs_pos
                        + lower_buf[abs_pos..]
                            .chars()
                            .next()
                            .map_or(1, char::len_utf8);
                }
            }
        }
        matches.sort_by_key(|m| (m.line, m.start_col));
        matches
    }
}

impl Default for SearchIndex {
    fn default() -> Self {
        Self::new()
    }
}
