// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

use super::super::content::SearchContent;
use super::super::error::SearchError;
use super::super::types::{FilterMode, NavigationDirection, SearchState, StreamingMatch};
use super::StreamingSearch;

#[cfg(kani)]
fn kani_retain<T: Clone>(items: &mut Vec<T>, mut keep: impl FnMut(&T) -> bool) {
    let mut retained = Vec::new();
    let mut index = 0;
    while index < items.len() {
        let item = items[index].clone();
        if keep(&item) {
            retained.push(item);
        }
        index += 1;
    }
    *items = retained;
}

impl StreamingSearch {
    /// Compile a regex pattern, prepending `(?i)` when case-insensitive mode
    /// is active. Without this, Regex mode ignores the `case_sensitive` config.
    #[cfg(feature = "regex")]
    fn compile_regex(&self, pattern: &str) -> Result<regex::Regex, SearchError> {
        let effective = if self.config.case_sensitive {
            pattern.to_string()
        } else {
            format!("(?i){pattern}")
        };
        regex::RegexBuilder::new(&effective)
            .size_limit(1 << 20) // 1 MiB NFA limit
            .dfa_size_limit(1 << 20) // 1 MiB DFA limit
            .build()
            .map_err(|e| SearchError::InvalidRegex(e.to_string()))
    }

    #[inline]
    fn restart_search_from_beginning(&mut self) {
        self.state = SearchState::Searching;
        self.results.clear();
        self.seen_positions.clear();
        self.current_index = 0;
        self.scan_progress = 0;
        self.total_matches = 0;
        self.bump_generation();
    }

    /// Start a new search with the given pattern and mode.
    ///
    /// Corresponds to TLA+ `StartSearch` action.
    pub fn start_search(&mut self, pattern: &str, mode: FilterMode) -> Result<(), SearchError> {
        if pattern.is_empty() {
            return Err(SearchError::EmptyPattern);
        }

        if pattern.len() > self.config.max_pattern_len {
            return Err(SearchError::PatternTooLong);
        }

        // Compile regex if needed (with case-sensitivity from config)
        #[cfg(feature = "regex")]
        if mode == FilterMode::Regex {
            self.compiled_regex = Some(self.compile_regex(pattern)?);
        }

        self.pattern = pattern.to_string();
        self.filter_mode = mode;
        self.state = SearchState::Searching;
        self.results.clear();
        self.seen_positions.clear();
        self.current_index = 0;
        self.scan_progress = 0;
        self.total_matches = 0;
        self.bump_generation();

        Ok(())
    }

    /// Update the pattern incrementally (as user types).
    ///
    /// Corresponds to TLA+ `UpdatePattern` action.
    pub fn update_pattern(&mut self, new_pattern: &str) -> Result<(), SearchError> {
        if !matches!(
            self.state,
            SearchState::Searching | SearchState::HasResults | SearchState::NoResults
        ) {
            return Err(SearchError::InvalidState);
        }

        if new_pattern == self.pattern {
            return Ok(()); // No change
        }

        if new_pattern.len() > self.config.max_pattern_len {
            return Err(SearchError::PatternTooLong);
        }

        if new_pattern.is_empty() {
            // Pattern cleared - reset to idle
            self.pattern.clear();
            self.state = SearchState::Idle;
            self.results.clear();
            self.seen_positions.clear();
            self.current_index = 0;
            self.scan_progress = -1;
            self.total_matches = 0;
            self.bump_generation();
            #[cfg(feature = "regex")]
            {
                self.compiled_regex = None;
            }
        } else {
            // Pattern changed - restart search
            // Validate regex before mutating state (failed ops shouldn't have side effects)
            #[cfg(feature = "regex")]
            let compiled = if self.filter_mode == FilterMode::Regex {
                Some(self.compile_regex(new_pattern)?)
            } else {
                None
            };

            self.pattern = new_pattern.to_string();
            #[cfg(feature = "regex")]
            {
                self.compiled_regex = compiled;
            }
            self.state = SearchState::Searching;
            self.results.clear();
            self.seen_positions.clear();
            self.current_index = 0;
            self.scan_progress = 0;
            self.total_matches = 0;
            self.bump_generation();
        }

        Ok(())
    }

    /// Scan a single row for matches.
    ///
    /// Corresponds to TLA+ `ScanRow` action.
    /// Returns the number of matches found in this row.
    pub fn scan_row(&mut self, row: usize, text: &str, max_rows: usize) -> usize {
        if self.state != SearchState::Searching {
            return 0;
        }

        // Row count bounded by terminal dimensions — fits in isize
        let row_isize = isize::try_from(row).unwrap_or(isize::MAX);
        if self.scan_progress != row_isize {
            return 0;
        }

        if row >= max_rows {
            return 0;
        }

        // Find matches in this row
        let matches = self.find_matches_in_row(row, text);
        let match_count = matches.len();

        // Add matches (respecting memory bound)
        for m in matches {
            let pos_key = (m.row, m.start_col);

            // INV-SEARCH-4: No duplicate results
            if self.seen_positions.contains(&pos_key) {
                // Skip duplicates
            } else if self.results.len() >= self.config.max_results {
                // INV-SEARCH-3: Memory bounded - at capacity, count but don't store
                self.total_matches += 1;
            } else {
                self.record_seen_position(pos_key);
                self.results.push(m);
                self.total_matches += 1;
            }
        }

        // Advance scan progress — row count bounded by terminal dimensions
        self.scan_progress = isize::try_from(row + 1).unwrap_or(isize::MAX);

        // Check if scan is complete
        if row + 1 >= max_rows {
            self.complete_search();
        }

        match_count
    }

    /// Scan all content in one call.
    ///
    /// Convenience method that scans all rows from a content provider.
    /// Joins consecutive wrapped rows into logical lines so that search
    /// queries spanning wrap boundaries can match (#7471).
    /// Note: Requires `&mut C` because disk-backed scrollback uses an LRU cache.
    pub fn scan_all<C: SearchContent>(&mut self, content: &mut C) {
        let max_rows = content.row_count();

        while self.state == SearchState::Searching {
            // scan_progress >= 0 during Searching state — try_from is lossless
            let row = usize::try_from(self.scan_progress).unwrap_or(usize::MAX);
            if row >= max_rows {
                self.complete_search();
                break;
            }

            if let Some(text) = content.get_row_text(row) {
                // Join consecutive wrapped continuation rows into a single
                // logical line so queries spanning the wrap boundary match.
                let mut logical_end = row + 1;
                while logical_end < max_rows && content.is_row_wrapped(logical_end) {
                    logical_end += 1;
                }

                if logical_end > row + 1 {
                    // This row starts a logical line that spans multiple grid rows.
                    // Track per-row column widths so we can remap match
                    // coordinates from the joined text back to physical
                    // (row, column) pairs (#7572).
                    let mut row_widths: Vec<usize> = Vec::with_capacity(logical_end - row);
                    let col_map_first = crate::grapheme::ColumnMap::new(&text);
                    row_widths.push(col_map_first.total_columns());
                    let mut joined = text;
                    for cont_row in (row + 1)..logical_end {
                        if let Some(cont_text) = content.get_row_text(cont_row) {
                            let cont_cols =
                                crate::grapheme::ColumnMap::new(&cont_text).total_columns();
                            row_widths.push(cont_cols);
                            joined.push_str(&cont_text);
                        } else {
                            row_widths.push(0);
                        }
                    }

                    // Find matches in the joined text, then remap coordinates.
                    let raw_matches = self.find_matches_in_row(row, &joined);
                    for m in raw_matches {
                        // Determine which physical row this match starts on.
                        let mut remaining_col = m.start_col;
                        let mut phys_row = row;
                        for (i, &width) in row_widths.iter().enumerate() {
                            if width == 0 || remaining_col < width {
                                phys_row = row + i;
                                break;
                            }
                            remaining_col -= width;
                            if i + 1 == row_widths.len() {
                                phys_row = row + i;
                            }
                        }
                        let phys_start_col = remaining_col;

                        // Compute end column on the same physical row.
                        // Clamp to the row width to prevent exceeding physical width.
                        let this_row_width = row_widths.get(phys_row - row).copied().unwrap_or(0);
                        let phys_end_col = if phys_start_col + m.match_len > this_row_width {
                            this_row_width
                        } else {
                            phys_start_col + m.match_len
                        };

                        let adjusted = StreamingMatch::new(phys_row, phys_start_col, phys_end_col);
                        // Filter zero-length after remapping.
                        if adjusted.match_len == 0 {
                            continue;
                        }
                        let pos_key = (adjusted.row, adjusted.start_col);
                        if self.seen_positions.contains(&pos_key) {
                            continue;
                        }
                        if self.results.len() >= self.config.max_results {
                            self.total_matches += 1;
                        } else {
                            self.record_seen_position(pos_key);
                            self.results.push(adjusted);
                            self.total_matches += 1;
                        }
                    }

                    // Advance scan progress past all wrapped rows.
                    self.scan_progress = isize::try_from(logical_end).unwrap_or(isize::MAX);
                    if logical_end >= max_rows {
                        self.complete_search();
                    }
                } else {
                    self.scan_row(row, &text, max_rows);
                }
            } else {
                // Skip missing rows — row count bounded by terminal dimensions
                self.scan_progress = isize::try_from(row + 1).unwrap_or(isize::MAX);
                if row + 1 >= max_rows {
                    self.complete_search();
                }
            }
        }
    }

    /// Complete the search scan.
    ///
    /// Corresponds to TLA+ `CompleteSearch` action.
    fn complete_search(&mut self) {
        if self.state != SearchState::Searching {
            return;
        }

        if self.results.is_empty() {
            self.state = SearchState::NoResults;
            self.current_index = 0;
        } else {
            self.state = SearchState::HasResults;
            self.current_index = 1;
        }

        self.scan_progress = -1;
    }

    /// Cancel the current search.
    ///
    /// Corresponds to TLA+ `CancelSearch` action.
    pub fn cancel(&mut self) {
        if !matches!(
            self.state,
            SearchState::Searching | SearchState::HasResults | SearchState::NoResults
        ) {
            return;
        }

        self.state = SearchState::Idle;
        self.pattern.clear();
        self.results.clear();
        self.seen_positions.clear();
        self.current_index = 0;
        self.scan_progress = -1;
        self.total_matches = 0;
        self.bump_generation();
        #[cfg(feature = "regex")]
        {
            self.compiled_regex = None;
        }
    }

    // ========================================================================
    // Navigation Operations
    // ========================================================================

    /// Navigate to the next match.
    ///
    /// Corresponds to TLA+ `NextMatch` action.
    pub fn next_match(&mut self) {
        if self.state != SearchState::HasResults || self.results.is_empty() {
            return;
        }

        self.current_index = Self::next_index(
            self.current_index,
            self.results.len(),
            NavigationDirection::Forward,
            self.config.wrap_enabled,
        );
    }

    /// Navigate to the previous match.
    ///
    /// Corresponds to TLA+ `PrevMatch` action.
    pub fn prev_match(&mut self) {
        if self.state != SearchState::HasResults || self.results.is_empty() {
            return;
        }

        self.current_index = Self::next_index(
            self.current_index,
            self.results.len(),
            NavigationDirection::Backward,
            self.config.wrap_enabled,
        );
    }

    /// Jump to a specific match index (1-based).
    ///
    /// Corresponds to TLA+ `JumpToMatch` action.
    pub fn jump_to_match(&mut self, index: usize) {
        if self.state != SearchState::HasResults {
            return;
        }

        if index >= 1 && index <= self.results.len() {
            self.current_index = index;
        }
    }

    /// Calculate next index with wraparound (from TLA+ NextIndex).
    pub(crate) fn next_index(
        idx: usize,
        len: usize,
        dir: NavigationDirection,
        wrap: bool,
    ) -> usize {
        if len == 0 {
            return 0;
        }

        match dir {
            NavigationDirection::Forward => {
                if idx >= len {
                    if wrap { 1 } else { idx }
                } else {
                    idx + 1
                }
            }
            NavigationDirection::Backward => {
                if idx <= 1 {
                    if wrap { len } else { idx }
                } else {
                    idx - 1
                }
            }
        }
    }

    // ========================================================================
    // Configuration Operations
    // ========================================================================

    /// Toggle wrap-around navigation.
    pub fn toggle_wrap(&mut self) {
        self.config.wrap_enabled = !self.config.wrap_enabled;
    }

    /// Toggle case sensitivity.
    ///
    /// Note: Changing case sensitivity requires re-search and regex recompilation.
    pub fn toggle_case_sensitive(&mut self) {
        self.config.case_sensitive = !self.config.case_sensitive;

        // Re-search if we have a pattern
        if !self.pattern.is_empty()
            && matches!(
                self.state,
                SearchState::Searching | SearchState::HasResults | SearchState::NoResults
            )
        {
            // Recompile regex with updated case-sensitivity flag
            #[cfg(feature = "regex")]
            if self.filter_mode == FilterMode::Regex
                && let Ok(re) = self.compile_regex(&self.pattern.clone())
            {
                self.compiled_regex = Some(re);
            }
            self.restart_search_from_beginning();
        }
    }

    /// Toggle highlight all matches.
    pub fn toggle_highlight_all(&mut self) {
        self.config.highlight_all = !self.config.highlight_all;
    }

    /// Set the filter mode.
    ///
    /// Note: Changing mode requires re-search.
    pub fn set_filter_mode(&mut self, mode: FilterMode) -> Result<(), SearchError> {
        if mode == self.filter_mode {
            return Ok(());
        }

        // Compile new regex if switching to regex mode (with case-sensitivity)
        #[cfg(feature = "regex")]
        if mode == FilterMode::Regex && !self.pattern.is_empty() {
            self.compiled_regex = Some(self.compile_regex(&self.pattern.clone())?);
        }

        self.filter_mode = mode;

        // Re-search if we have a pattern
        if !self.pattern.is_empty()
            && matches!(
                self.state,
                SearchState::Searching | SearchState::HasResults | SearchState::NoResults
            )
        {
            self.restart_search_from_beginning();
        }

        Ok(())
    }

    // ========================================================================
    // Content Change Handling
    // ========================================================================

    /// Handle new content added to terminal.
    ///
    /// Corresponds to TLA+ `ContentAdded` action.
    /// When in `NoResults` state and new content matches the query,
    /// transitions to `HasResults` so the user sees updated match count.
    pub fn content_added(&mut self, row: usize, text: &str) {
        if !matches!(self.state, SearchState::HasResults | SearchState::NoResults) {
            return;
        }

        // Find new matches
        let matches = self.find_matches_in_row(row, text);

        let prev_len = self.results.len();

        for m in matches {
            let pos_key = (m.row, m.start_col);

            // Skip duplicates
            if self.seen_positions.contains(&pos_key) {
                continue;
            }

            // Skip if at capacity
            if self.results.len() >= self.config.max_results {
                self.total_matches += 1;
                continue;
            }

            self.record_seen_position(pos_key);
            self.results.push(m);
            self.total_matches += 1;
        }

        // Transition NoResults → HasResults when matches are found
        if self.state == SearchState::NoResults && !self.results.is_empty() {
            self.state = SearchState::HasResults;
        }

        // Bump generation when result set changes so UI consumers detect staleness.
        if self.results.len() != prev_len {
            // Re-sort to maintain row-order for next/prev navigation.
            // Appended matches may be for rows earlier than the scan frontier.
            self.results.sort_unstable_by_key(|m| (m.row, m.start_col));
            self.bump_generation();
        }

        // Set current index if this is the first result
        if self.current_index == 0 && !self.results.is_empty() {
            self.current_index = 1;
        }
    }

    /// Handle in-place modification of a terminal row.
    ///
    /// When a row is overwritten (e.g., by cursor movement and re-print, CSI
    /// erase-in-line, or shell prompt redraw) while a search is active, any
    /// previously recorded matches for that row become stale. This method
    /// removes those stale matches, re-scans the row with `new_text`, and
    /// updates the result set accordingly.
    ///
    /// Works in all active search states:
    /// - `Searching`: only acts on already-scanned rows (row < scan_progress).
    /// - `HasResults` / `NoResults`: removes old matches and re-scans.
    ///
    /// Fixes #7244.
    pub fn content_modified(&mut self, row: usize, new_text: &str) {
        match self.state {
            SearchState::Searching => {
                // Only already-scanned rows can have stale matches.
                let row_isize = isize::try_from(row).unwrap_or(isize::MAX);
                if row_isize >= self.scan_progress {
                    return; // Will be scanned normally when scan_progress reaches it.
                }
            }
            SearchState::HasResults | SearchState::NoResults => {}
            SearchState::Idle => return,
        }

        let prev_len = self.results.len();

        // --- Remove stale matches for this row ---
        let removed_count = self.results.iter().filter(|m| m.row == row).count();

        #[cfg(kani)]
        kani_retain(&mut self.results, |m| m.row != row);
        #[cfg(not(kani))]
        self.results.retain(|m| m.row != row);

        // Purge dedup entries for the row so re-scan can re-insert.
        #[cfg(kani)]
        kani_retain(&mut self.seen_positions, |(r, _)| *r != row);
        #[cfg(not(kani))]
        self.seen_positions.retain(|(r, _)| *r != row);

        // Adjust total_matches by the number of removed stored results.
        // (Matches that were counted but not stored are lost; this is
        // acceptable because the row content they referred to is gone.)
        self.total_matches = self.total_matches.saturating_sub(removed_count);

        // --- Re-scan the row with new content ---
        let new_matches = self.find_matches_in_row(row, new_text);

        for m in new_matches {
            let pos_key = (m.row, m.start_col);

            if self.seen_positions.contains(&pos_key) {
                continue;
            }

            if self.results.len() >= self.config.max_results {
                self.total_matches += 1;
                continue;
            }

            self.record_seen_position(pos_key);
            self.results.push(m);
            self.total_matches += 1;
        }

        // Bump generation if the result set changed.
        if self.results.len() != prev_len || removed_count > 0 {
            self.bump_generation();
        }

        // Re-sort to maintain row-order after retain + append.
        // Must sort when results were added OR removed, not just removed —
        // appended matches from a modified row may sort before existing matches
        // from later rows, breaking navigation order (#7472).
        if !self.results.is_empty() && (removed_count > 0 || self.results.len() != prev_len) {
            self.results.sort_unstable_by_key(|m| (m.row, m.start_col));
        }

        // Fix up state and current_index.
        if self.results.is_empty() {
            if matches!(self.state, SearchState::HasResults | SearchState::NoResults) {
                self.state = SearchState::NoResults;
            }
            self.current_index = 0;
        } else {
            if self.state == SearchState::NoResults {
                self.state = SearchState::HasResults;
            }
            if self.current_index == 0 {
                self.current_index = 1;
            } else if self.current_index > self.results.len() {
                self.current_index = self.results.len();
            }
        }
    }

    /// Handle a full content reflow (e.g., terminal resize that rewraps lines).
    ///
    /// All existing match coordinates become stale because row indices change
    /// during reflow. If a search is active (searching or has results), this
    /// clears the results and restarts the scan from the beginning so matches
    /// are recomputed against the new layout. If no search is active, this is
    /// a no-op.
    ///
    /// Fixes #7271.
    pub fn content_reflowed(&mut self) {
        match self.state {
            SearchState::Searching | SearchState::HasResults | SearchState::NoResults => {
                self.restart_search_from_beginning();
            }
            SearchState::Idle => {}
        }
    }

    /// Handle content being invalidated (scrolled out, cleared).
    ///
    /// Corresponds to TLA+ `ContentInvalidated` action.
    pub fn content_invalidated(&mut self, from_row: usize, to_row: usize) {
        if !matches!(self.state, SearchState::HasResults | SearchState::NoResults) {
            return;
        }

        let prev_len = self.results.len();

        // Remove results in the invalidated range
        #[cfg(kani)]
        kani_retain(&mut self.results, |m| m.row < from_row || m.row > to_row);
        #[cfg(not(kani))]
        self.results.retain(|m| m.row < from_row || m.row > to_row);

        // Update dedup set
        #[cfg(kani)]
        kani_retain(&mut self.seen_positions, |(row, _)| {
            *row < from_row || *row > to_row
        });
        #[cfg(not(kani))]
        self.seen_positions
            .retain(|(row, _)| *row < from_row || *row > to_row);

        // Update total_matches to reflect removed results.
        // (Matches that were counted but not stored are lost; this is
        // acceptable because the row content they referred to is gone.)
        let removed_count = prev_len - self.results.len();
        self.total_matches = self.total_matches.saturating_sub(removed_count);

        // Bump generation if any results were actually removed
        if removed_count > 0 {
            self.bump_generation();
        }

        if self.results.is_empty() {
            self.state = SearchState::NoResults;
            self.current_index = 0;
        } else if self.current_index > self.results.len() {
            self.current_index = self.results.len();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::error::SearchError;
    use super::super::super::types::{
        FilterMode, NavigationDirection, SearchState, StreamingSearchConfig,
    };
    use super::super::StreamingSearch;

    /// Helper: create engine, start search, scan rows, return engine in HasResults state.
    fn engine_with_matches(pattern: &str, rows: &[&str]) -> StreamingSearch {
        let mut engine = StreamingSearch::new();
        engine.start_search(pattern, FilterMode::Literal).unwrap();
        let max_rows = rows.len();
        for (i, text) in rows.iter().enumerate() {
            engine.scan_row(i, text, max_rows);
        }
        engine
    }

    // ====================================================================
    // Start search / basic scanning
    // ====================================================================

    #[test]
    fn test_start_search_transitions_to_searching() {
        let mut engine = StreamingSearch::new();
        assert_eq!(engine.state(), SearchState::Idle);

        engine.start_search("needle", FilterMode::Literal).unwrap();
        assert_eq!(engine.state(), SearchState::Searching);
        assert_eq!(engine.pattern(), "needle");
        assert_eq!(engine.filter_mode(), FilterMode::Literal);
        assert_eq!(engine.scan_progress(), 0);
    }

    #[test]
    fn test_start_search_empty_pattern_returns_error() {
        let mut engine = StreamingSearch::new();
        let result = engine.start_search("", FilterMode::Literal);
        assert_eq!(result, Err(SearchError::EmptyPattern));
        assert_eq!(engine.state(), SearchState::Idle);
    }

    #[test]
    fn test_start_search_pattern_too_long() {
        let config = StreamingSearchConfig {
            max_pattern_len: 10,
            ..StreamingSearchConfig::default()
        };
        let mut engine = StreamingSearch::with_config(config);
        let long_pattern = "a".repeat(11);
        let result = engine.start_search(&long_pattern, FilterMode::Literal);
        assert_eq!(result, Err(SearchError::PatternTooLong));
    }

    #[test]
    fn test_scan_rows_finds_matches_and_completes() {
        let engine = engine_with_matches("hello", &["hello world", "goodbye", "hello again"]);
        assert_eq!(engine.state(), SearchState::HasResults);
        assert_eq!(engine.result_count(), 2);
        assert_eq!(engine.total_matches(), 2);
        assert_eq!(engine.current_index(), 1);
    }

    #[test]
    fn test_scan_no_matches_transitions_to_no_results() {
        let engine = engine_with_matches("xyz", &["hello world", "goodbye"]);
        assert_eq!(engine.state(), SearchState::NoResults);
        assert_eq!(engine.result_count(), 0);
        assert_eq!(engine.current_index(), 0);
    }

    #[test]
    fn test_scan_multiple_matches_same_line() {
        let engine = engine_with_matches("ab", &["ab cd ab ef ab"]);
        assert_eq!(engine.state(), SearchState::HasResults);
        assert_eq!(engine.result_count(), 3);
        let results = engine.results();
        assert_eq!(results[0].start_col, 0);
        assert_eq!(results[1].start_col, 6);
        assert_eq!(results[2].start_col, 12);
    }

    #[test]
    fn test_match_positions_across_lines() {
        let engine = engine_with_matches("test", &["test line 0", "no match", "test line 2"]);
        assert_eq!(engine.result_count(), 2);
        let results = engine.results();
        assert_eq!(results[0].row, 0);
        assert_eq!(results[0].start_col, 0);
        assert_eq!(results[1].row, 2);
        assert_eq!(results[1].start_col, 0);
    }

    // ====================================================================
    // Navigation: next/prev match cycling
    // ====================================================================

    #[test]
    fn test_next_match_cycles_forward() {
        let mut engine = engine_with_matches("x", &["x", "x", "x"]);
        assert_eq!(engine.current_index(), 1);

        engine.next_match();
        assert_eq!(engine.current_index(), 2);

        engine.next_match();
        assert_eq!(engine.current_index(), 3);

        // Wrap around (wrap_enabled is true by default)
        engine.next_match();
        assert_eq!(engine.current_index(), 1);
    }

    #[test]
    fn test_prev_match_cycles_backward() {
        let mut engine = engine_with_matches("x", &["x", "x", "x"]);
        assert_eq!(engine.current_index(), 1);

        // Wrap backward from first
        engine.prev_match();
        assert_eq!(engine.current_index(), 3);

        engine.prev_match();
        assert_eq!(engine.current_index(), 2);

        engine.prev_match();
        assert_eq!(engine.current_index(), 1);
    }

    #[test]
    fn test_next_match_no_wrap() {
        let config = StreamingSearchConfig {
            wrap_enabled: false,
            ..StreamingSearchConfig::default()
        };
        let mut engine = StreamingSearch::with_config(config);
        engine.start_search("x", FilterMode::Literal).unwrap();
        engine.scan_row(0, "x", 2);
        engine.scan_row(1, "x", 2);
        assert_eq!(engine.state(), SearchState::HasResults);
        assert_eq!(engine.current_index(), 1);

        engine.next_match();
        assert_eq!(engine.current_index(), 2);

        // Should stay at end when wrap is disabled
        engine.next_match();
        assert_eq!(engine.current_index(), 2);
    }

    #[test]
    fn test_prev_match_no_wrap() {
        let config = StreamingSearchConfig {
            wrap_enabled: false,
            ..StreamingSearchConfig::default()
        };
        let mut engine = StreamingSearch::with_config(config);
        engine.start_search("x", FilterMode::Literal).unwrap();
        engine.scan_row(0, "x", 1);
        assert_eq!(engine.current_index(), 1);

        // Should stay at 1 when wrap is disabled
        engine.prev_match();
        assert_eq!(engine.current_index(), 1);
    }

    #[test]
    fn test_next_match_noop_when_no_results() {
        let mut engine = engine_with_matches("xyz", &["hello"]);
        assert_eq!(engine.state(), SearchState::NoResults);
        engine.next_match();
        assert_eq!(engine.current_index(), 0);
    }

    #[test]
    fn test_prev_match_noop_when_idle() {
        let mut engine = StreamingSearch::new();
        engine.prev_match();
        assert_eq!(engine.current_index(), 0);
    }

    #[test]
    fn test_jump_to_match_valid_index() {
        let mut engine = engine_with_matches("x", &["x", "x", "x"]);
        engine.jump_to_match(3);
        assert_eq!(engine.current_index(), 3);

        engine.jump_to_match(1);
        assert_eq!(engine.current_index(), 1);
    }

    #[test]
    fn test_jump_to_match_invalid_index_noop() {
        let mut engine = engine_with_matches("x", &["x", "x"]);
        engine.jump_to_match(0); // 0 is invalid (1-based)
        assert_eq!(engine.current_index(), 1);

        engine.jump_to_match(3); // out of range
        assert_eq!(engine.current_index(), 1);
    }

    // ====================================================================
    // next_index pure function
    // ====================================================================

    #[test]
    fn test_next_index_forward_wrap() {
        assert_eq!(
            StreamingSearch::next_index(3, 3, NavigationDirection::Forward, true),
            1
        );
    }

    #[test]
    fn test_next_index_forward_no_wrap() {
        assert_eq!(
            StreamingSearch::next_index(3, 3, NavigationDirection::Forward, false),
            3
        );
    }

    #[test]
    fn test_next_index_backward_wrap() {
        assert_eq!(
            StreamingSearch::next_index(1, 5, NavigationDirection::Backward, true),
            5
        );
    }

    #[test]
    fn test_next_index_backward_no_wrap() {
        assert_eq!(
            StreamingSearch::next_index(1, 5, NavigationDirection::Backward, false),
            1
        );
    }

    #[test]
    fn test_next_index_empty_len_returns_zero() {
        assert_eq!(
            StreamingSearch::next_index(0, 0, NavigationDirection::Forward, true),
            0
        );
        assert_eq!(
            StreamingSearch::next_index(0, 0, NavigationDirection::Backward, true),
            0
        );
    }

    // ====================================================================
    // Pattern update
    // ====================================================================

    #[test]
    fn test_update_pattern_restarts_search() {
        let mut engine = engine_with_matches("hello", &["hello world"]);
        assert_eq!(engine.state(), SearchState::HasResults);

        engine.update_pattern("world").unwrap();
        assert_eq!(engine.state(), SearchState::Searching);
        assert_eq!(engine.pattern(), "world");
        assert_eq!(engine.result_count(), 0);
        assert_eq!(engine.scan_progress(), 0);
    }

    #[test]
    fn test_update_pattern_to_empty_resets_to_idle() {
        let mut engine = engine_with_matches("hello", &["hello world"]);
        engine.update_pattern("").unwrap();
        assert_eq!(engine.state(), SearchState::Idle);
        assert_eq!(engine.pattern(), "");
        assert_eq!(engine.result_count(), 0);
    }

    #[test]
    fn test_update_pattern_same_is_noop() {
        let mut engine = engine_with_matches("hello", &["hello world"]);
        let gen_before = engine.generation();
        engine.update_pattern("hello").unwrap();
        assert_eq!(
            engine.generation(),
            gen_before,
            "same pattern should not bump generation"
        );
    }

    #[test]
    fn test_update_pattern_idle_returns_error() {
        let mut engine = StreamingSearch::new();
        let result = engine.update_pattern("test");
        assert_eq!(result, Err(SearchError::InvalidState));
    }

    #[test]
    fn test_update_pattern_too_long() {
        let config = StreamingSearchConfig {
            max_pattern_len: 5,
            ..StreamingSearchConfig::default()
        };
        let mut engine = StreamingSearch::with_config(config);
        engine.start_search("abc", FilterMode::Literal).unwrap();
        let result = engine.update_pattern("toolong");
        assert_eq!(result, Err(SearchError::PatternTooLong));
    }

    // ====================================================================
    // Cancel
    // ====================================================================

    #[test]
    fn test_cancel_resets_to_idle() {
        let mut engine = engine_with_matches("hello", &["hello world"]);
        assert_eq!(engine.state(), SearchState::HasResults);

        engine.cancel();
        assert_eq!(engine.state(), SearchState::Idle);
        assert_eq!(engine.pattern(), "");
        assert_eq!(engine.result_count(), 0);
        assert_eq!(engine.current_index(), 0);
        assert_eq!(engine.scan_progress(), -1);
    }

    #[test]
    fn test_cancel_noop_when_idle() {
        let mut engine = StreamingSearch::new();
        let gen_before = engine.generation();
        engine.cancel();
        assert_eq!(engine.state(), SearchState::Idle);
        assert_eq!(engine.generation(), gen_before);
    }

    // ====================================================================
    // Content changes: add, modify, invalidate, reflow
    // ====================================================================

    #[test]
    fn test_content_added_appends_matches() {
        let mut engine = engine_with_matches("needle", &["needle here"]);
        assert_eq!(engine.result_count(), 1);

        engine.content_added(5, "another needle");
        assert_eq!(engine.result_count(), 2);
        assert_eq!(engine.total_matches(), 2);
    }

    #[test]
    fn test_content_added_transitions_no_results_to_has_results() {
        let mut engine = engine_with_matches("xyz", &["no match"]);
        assert_eq!(engine.state(), SearchState::NoResults);

        engine.content_added(1, "xyz found");
        assert_eq!(engine.state(), SearchState::HasResults);
        assert_eq!(engine.result_count(), 1);
        assert_eq!(engine.current_index(), 1);
    }

    #[test]
    fn test_content_added_noop_when_idle() {
        let mut engine = StreamingSearch::new();
        let gen_before = engine.generation();
        engine.content_added(0, "needle");
        assert_eq!(engine.generation(), gen_before);
        assert_eq!(engine.result_count(), 0);
    }

    #[test]
    fn test_content_invalidated_removes_matches() {
        let mut engine =
            engine_with_matches("test", &["test 0", "test 1", "test 2", "test 3", "test 4"]);
        assert_eq!(engine.result_count(), 5);

        // Invalidate rows 1 through 3
        engine.content_invalidated(1, 3);
        assert_eq!(engine.result_count(), 2);
        let rows: Vec<usize> = engine.results().iter().map(|m| m.row).collect();
        assert!(rows.contains(&0));
        assert!(rows.contains(&4));
    }

    #[test]
    fn test_content_invalidated_all_transitions_to_no_results() {
        let mut engine = engine_with_matches("test", &["test 0", "test 1"]);
        engine.content_invalidated(0, 1);
        assert_eq!(engine.state(), SearchState::NoResults);
        assert_eq!(engine.current_index(), 0);
    }

    #[test]
    fn test_content_invalidated_clamps_current_index() {
        let mut engine = engine_with_matches("test", &["test 0", "test 1", "test 2"]);
        engine.jump_to_match(3); // current is now 3
        assert_eq!(engine.current_index(), 3);

        // Remove the last match
        engine.content_invalidated(2, 2);
        assert_eq!(engine.result_count(), 2);
        assert_eq!(engine.current_index(), 2); // clamped
    }

    #[test]
    fn test_content_modified_replaces_matches() {
        let mut engine = engine_with_matches("needle", &["needle here", "no match"]);
        assert_eq!(engine.result_count(), 1);

        // Modify row 0 to remove the match
        engine.content_modified(0, "nothing here");
        assert_eq!(engine.result_count(), 0);
        assert_eq!(engine.state(), SearchState::NoResults);
    }

    #[test]
    fn test_content_modified_adds_new_match() {
        let mut engine = engine_with_matches("needle", &["no match"]);
        assert_eq!(engine.state(), SearchState::NoResults);

        engine.content_modified(0, "needle found");
        assert_eq!(engine.state(), SearchState::HasResults);
        assert_eq!(engine.result_count(), 1);
    }

    #[test]
    fn test_content_reflowed_restarts_search() {
        let mut engine = engine_with_matches("test", &["test here"]);
        let gen_before = engine.generation();

        engine.content_reflowed();
        assert_eq!(engine.state(), SearchState::Searching);
        assert_eq!(engine.result_count(), 0);
        assert_eq!(engine.scan_progress(), 0);
        assert!(engine.generation() > gen_before);
    }

    #[test]
    fn test_content_reflowed_noop_when_idle() {
        let mut engine = StreamingSearch::new();
        let gen_before = engine.generation();
        engine.content_reflowed();
        assert_eq!(engine.state(), SearchState::Idle);
        assert_eq!(engine.generation(), gen_before);
    }

    // ====================================================================
    // Configuration toggles
    // ====================================================================

    #[test]
    fn test_toggle_wrap() {
        let mut engine = StreamingSearch::new();
        assert!(engine.wrap_enabled());
        engine.toggle_wrap();
        assert!(!engine.wrap_enabled());
        engine.toggle_wrap();
        assert!(engine.wrap_enabled());
    }

    #[test]
    fn test_toggle_highlight_all() {
        let mut engine = StreamingSearch::new();
        assert!(engine.highlight_all());
        engine.toggle_highlight_all();
        assert!(!engine.highlight_all());
    }

    #[test]
    fn test_toggle_case_sensitive_restarts_search() {
        let mut engine = engine_with_matches("hello", &["hello world"]);
        assert_eq!(engine.state(), SearchState::HasResults);

        engine.toggle_case_sensitive();
        assert_eq!(engine.state(), SearchState::Searching);
        assert_eq!(engine.result_count(), 0);
    }

    #[test]
    fn test_set_filter_mode_restarts_search() {
        let mut engine = engine_with_matches("hello", &["hello world"]);
        assert_eq!(engine.filter_mode(), FilterMode::Literal);

        engine.set_filter_mode(FilterMode::Fuzzy).unwrap();
        assert_eq!(engine.filter_mode(), FilterMode::Fuzzy);
        assert_eq!(engine.state(), SearchState::Searching);
    }

    #[test]
    fn test_set_filter_mode_same_is_noop() {
        let mut engine = engine_with_matches("hello", &["hello world"]);
        let gen_before = engine.generation();
        engine.set_filter_mode(FilterMode::Literal).unwrap();
        assert_eq!(engine.generation(), gen_before);
    }

    // ====================================================================
    // Generation counter
    // ====================================================================

    #[test]
    fn test_generation_bumps_on_start_search() {
        let mut engine = StreamingSearch::new();
        assert_eq!(engine.generation(), 0);
        engine.start_search("test", FilterMode::Literal).unwrap();
        assert_eq!(engine.generation(), 1);
    }

    #[test]
    fn test_generation_bumps_on_cancel() {
        let mut engine = StreamingSearch::new();
        engine.start_search("test", FilterMode::Literal).unwrap();
        let gen_before = engine.generation();
        engine.cancel();
        assert_eq!(engine.generation(), gen_before + 1);
    }

    // ====================================================================
    // Invariant verification after operations
    // ====================================================================

    #[test]
    fn test_all_invariants_hold_after_full_lifecycle() {
        let mut engine = engine_with_matches("test", &["test one", "no match", "test two"]);
        assert!(engine.verify_all_invariants());

        engine.next_match();
        assert!(engine.verify_all_invariants());

        engine.content_added(5, "test added");
        assert!(engine.verify_all_invariants());

        engine.content_invalidated(0, 0);
        assert!(engine.verify_all_invariants());

        engine.cancel();
        assert!(engine.verify_all_invariants());
    }

    // ====================================================================
    // Memory bound
    // ====================================================================

    #[test]
    fn test_memory_bound_respected() {
        let config = StreamingSearchConfig {
            max_results: 3,
            ..StreamingSearchConfig::default()
        };
        let mut engine = StreamingSearch::with_config(config);
        engine.start_search("x", FilterMode::Literal).unwrap();
        for i in 0..10 {
            engine.scan_row(i, "x", 10);
        }
        assert_eq!(engine.result_count(), 3);
        assert_eq!(engine.total_matches(), 10);
        assert!(engine.verify_memory_bounded());
    }

    // ====================================================================
    // Duplicate prevention
    // ====================================================================

    #[test]
    fn test_no_duplicate_results_after_content_added() {
        let mut engine = engine_with_matches("needle", &["needle here"]);
        // Re-add same row content
        engine.content_added(0, "needle here");
        assert!(engine.verify_no_duplicates());
    }
}
