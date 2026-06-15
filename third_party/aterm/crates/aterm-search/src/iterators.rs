// Copyright 2026 The aterm Authors
// Author: The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors <ayates.com>

//! Lazy match iterators for forward and reverse search navigation.

use std::ops::Range;

use crate::bitmap::SparseBitmap;

use super::index::SearchIndex;
use super::types::SearchMatch;
use crate::grapheme::ColumnMap;

/// Source of candidate line numbers for search iteration.
///
/// For short queries (< 3 chars), uses a lazy range to avoid O(n) allocation.
/// For trigram queries, uses either a lazy owned bitmap iterator (forward)
/// or a materialized reverse-sorted Vec (backward, since SparseBitmap lacks
/// reverse iteration).
pub(super) enum CandidateSource {
    /// Materialized candidate list (used for reverse search and empty sources).
    Materialized(std::vec::IntoIter<u32>),
    /// Lazy owned bitmap iterator (trigram forward search).
    /// Avoids O(k) collect() when only the first few matches are needed.
    /// Boxed to avoid large enum variant inflating all other variants.
    BitmapOwned(Box<crate::bitmap::SparseBitmapIntoIter>),
    /// Lazy ascending range of line numbers (for short-query forward search).
    Range(Range<u32>),
    /// Lazy descending range of line numbers (for short-query backward search).
    RangeRev(std::iter::Rev<Range<u32>>),
}

impl CandidateSource {
    /// Create a lazy forward source from a bitmap starting at `from_line`.
    ///
    /// Removes elements below `from_line` (O(log n) in SparseBitmap) and
    /// returns an owned iterator that yields candidates lazily.
    pub(super) fn from_bitmap_forward(mut bitmap: SparseBitmap, from_line: u32) -> Self {
        bitmap.remove_range(..from_line);
        Self::BitmapOwned(Box::new(bitmap.into_iter()))
    }

    /// Get the next candidate line number.
    fn next_candidate(&mut self) -> Option<u32> {
        match self {
            Self::Materialized(iter) => iter.next(),
            Self::BitmapOwned(iter) => iter.next(),
            Self::Range(range) => range.next(),
            Self::RangeRev(rev) => rev.next(),
        }
    }
}

/// Lazy iterator over search matches with early termination support.
///
/// This iterator yields matches one at a time without collecting all matches
/// first. Combined with range queries on the underlying bitmap, this enables
/// O(log n) search for find_next/find_prev operations.
pub(crate) struct SearchMatchIterator<'a> {
    /// The search index.
    index: &'a SearchIndex,
    /// The query string.
    query: &'a str,
    /// Candidate line numbers source.
    candidates: CandidateSource,
    /// Current line's matches (buffered for multiple matches per line).
    current_line_matches: Vec<SearchMatch>,
    /// Index into current_line_matches.
    current_match_idx: usize,
}

impl<'a> SearchMatchIterator<'a> {
    /// Create a new match iterator from a candidate source.
    pub(super) fn new(index: &'a SearchIndex, query: &'a str, candidates: CandidateSource) -> Self {
        Self {
            index,
            query,
            candidates,
            current_line_matches: Vec::new(),
            current_match_idx: 0,
        }
    }

    /// Find all matches in a single line.
    fn find_matches_in_line(&self, line_num: usize) -> Vec<SearchMatch> {
        let mut matches = Vec::new();
        if let Some(text) = self.index.lines.get(&line_num) {
            // Use cached column map when available (#7373).
            let fallback;
            let col_map = match self.index.column_maps.get(&line_num) {
                Some(cm) => cm,
                None => {
                    fallback = ColumnMap::new(text);
                    &fallback
                }
            };
            let mut start = 0;
            while let Some(pos) = text[start..].find(self.query) {
                let abs_pos = start + pos;
                let start_col = col_map.byte_to_column(abs_pos);
                let end_col = col_map.byte_to_column(abs_pos + self.query.len());
                matches.push(SearchMatch::new(line_num, start_col, end_col));
                // Advance by one character (not one byte) to stay on a
                // valid char boundary for multi-byte UTF-8 queries.
                start = abs_pos + text[abs_pos..].chars().next().map_or(1, char::len_utf8);
            }
        }
        matches
    }
}

impl Iterator for SearchMatchIterator<'_> {
    type Item = SearchMatch;

    fn next(&mut self) -> Option<Self::Item> {
        // Return buffered match if available
        if self.current_match_idx < self.current_line_matches.len() {
            let m = self.current_line_matches[self.current_match_idx].clone();
            self.current_match_idx += 1;
            return Some(m);
        }

        // Get next candidate line and find matches
        while let Some(line_u32) = self.candidates.next_candidate() {
            let line_num = line_u32 as usize;
            self.current_line_matches = self.find_matches_in_line(line_num);
            self.current_match_idx = 0;

            if !self.current_line_matches.is_empty() {
                let m = self.current_line_matches[0].clone();
                self.current_match_idx = 1;
                return Some(m);
            }
            // No matches in this line (false positive from trigram), try next
        }
        None
    }
}

/// Reverse iterator over search matches.
///
/// Yields matches in reverse order (newest to oldest, right to left).
pub(crate) struct SearchMatchReverseIterator<'a> {
    /// The search index.
    index: &'a SearchIndex,
    /// The query string.
    query: &'a str,
    /// Candidate line numbers source (yields in descending order).
    candidates: CandidateSource,
    /// Current line's matches (in reverse column order).
    current_line_matches: Vec<SearchMatch>,
    /// Index into current_line_matches.
    current_match_idx: usize,
}

impl<'a> SearchMatchReverseIterator<'a> {
    /// Create a new reverse match iterator.
    pub(super) fn new(index: &'a SearchIndex, query: &'a str, candidates: CandidateSource) -> Self {
        Self {
            index,
            query,
            candidates,
            current_line_matches: Vec::new(),
            current_match_idx: 0,
        }
    }

    /// Find all matches in a single line, sorted by column descending.
    fn find_matches_in_line(&self, line_num: usize) -> Vec<SearchMatch> {
        let mut matches = Vec::new();
        if let Some(text) = self.index.lines.get(&line_num) {
            // Use cached column map when available (#7373).
            let fallback;
            let col_map = match self.index.column_maps.get(&line_num) {
                Some(cm) => cm,
                None => {
                    fallback = ColumnMap::new(text);
                    &fallback
                }
            };
            let mut start = 0;
            while let Some(pos) = text[start..].find(self.query) {
                let abs_pos = start + pos;
                let start_col = col_map.byte_to_column(abs_pos);
                let end_col = col_map.byte_to_column(abs_pos + self.query.len());
                matches.push(SearchMatch::new(line_num, start_col, end_col));
                // Advance by one character (not one byte) to stay on a
                // valid char boundary for multi-byte UTF-8 queries.
                start = abs_pos + text[abs_pos..].chars().next().map_or(1, char::len_utf8);
            }
        }
        // Reverse so we iterate from right to left
        matches.reverse();
        matches
    }
}

impl Iterator for SearchMatchReverseIterator<'_> {
    type Item = SearchMatch;

    fn next(&mut self) -> Option<Self::Item> {
        // Return buffered match if available
        if self.current_match_idx < self.current_line_matches.len() {
            let m = self.current_line_matches[self.current_match_idx].clone();
            self.current_match_idx += 1;
            return Some(m);
        }

        // Get next candidate line and find matches
        while let Some(line_u32) = self.candidates.next_candidate() {
            let line_num = line_u32 as usize;
            self.current_line_matches = self.find_matches_in_line(line_num);
            self.current_match_idx = 0;

            if !self.current_line_matches.is_empty() {
                let m = self.current_line_matches[0].clone();
                self.current_match_idx = 1;
                return Some(m);
            }
        }
        None
    }
}
