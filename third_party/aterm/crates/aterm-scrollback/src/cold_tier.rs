// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0

//! In-memory cold tier: compressed pages.
//!
//! Pages are compressed with zstd when the `zstd` feature is enabled, and with
//! the warm tier's LZ4 codec otherwise (the default headless build). The codec
//! is fixed at compile time, so pages are always decodable in the same build.
//!
//! For disk-backed cold storage, see `DiskColdTier` (the `disk-tier` feature).

use super::line::{Line, deserialize_lines};
use super::tier::WarmBlock;
use std::cell::RefCell;
use std::collections::VecDeque;

#[cfg(test)]
thread_local! {
    static COLD_FIND_PAGE_STEPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
fn count_cold_find_page_step() {
    COLD_FIND_PAGE_STEPS.with(|c| c.set(c.get() + 1));
}

#[cfg(test)]
fn take_cold_find_page_steps() -> usize {
    COLD_FIND_PAGE_STEPS.with(|c| {
        let value = c.get();
        c.set(0);
        value
    })
}

/// A cold page (compressed with the cold codec: zstd, or LZ4 by default).
///
/// Stored in memory. For disk-backed storage, see `DiskColdTier`.
#[derive(Debug, Clone)]
struct ColdPage {
    /// Cold-codec compressed data (re-compressed from the LZ4 warm block).
    compressed: Vec<u8>,
    /// Number of lines in page.
    line_count: usize,
}

impl ColdPage {
    /// Create a cold page from a warm block.
    fn from_warm_block(block: &WarmBlock) -> Result<Self, super::ScrollbackError> {
        let (compressed, line_count) = block.to_cold_compressed()?;

        Ok(Self {
            compressed,
            line_count,
        })
    }

    /// Decompress and get all lines.
    fn decompress(&self) -> Result<Vec<Line>, super::ScrollbackError> {
        let decompressed = super::decode_cold_bounded(&self.compressed)?;
        Ok(deserialize_lines(&decompressed))
    }
}

/// Cold tier: Zstd compressed pages (in-memory).
///
/// For disk-backed cold storage, use [`DiskColdTier`](super::DiskColdTier).
/// Uses cumulative line counts for O(log P) page lookup and caches the
/// last decompressed page to avoid redundant Zstd decompression.
///
/// The `front_offset` field enables O(1) line-limit enforcement: instead of
/// decompressing the boundary page to remove a few oldest lines, we simply
/// advance the offset. The first page is dropped when fully consumed.
#[derive(Debug)]
pub(crate) struct ColdTier {
    /// Compressed pages (VecDeque for O(1) pop_front during eviction).
    pages: VecDeque<ColdPage>,
    /// Total available line count (excludes consumed lines from front_offset).
    line_count: usize,
    /// Cumulative line counts: `cumulative[i]` = total *physical* lines in pages `0..=i`.
    /// Unchanged by front_offset — get_line adjusts indices before lookup.
    cumulative_lines: Vec<usize>,
    /// Cache of last decompressed page: `(page_index, lines)`.
    last_page_cache: RefCell<Option<(usize, Vec<Line>)>>,
    /// Running total for `compressed_size()`.
    bytes_used: usize,
    /// Lines logically consumed from the first page. Avoids decompression
    /// during line-limit truncation — pages are dropped when fully consumed.
    front_offset: usize,
}

impl ColdTier {
    /// Create a new cold tier.
    #[must_use]
    pub(crate) fn new() -> Self {
        Self {
            pages: VecDeque::new(),
            line_count: 0,
            cumulative_lines: Vec::new(),
            last_page_cache: RefCell::new(None),
            bytes_used: 0,
            front_offset: 0,
        }
    }

    /// Get the total number of lines.
    #[must_use]
    #[inline]
    pub(crate) fn line_count(&self) -> usize {
        self.line_count
    }

    /// Get the number of pages.
    #[cfg(test)]
    #[must_use]
    #[inline]
    pub(crate) fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Push a warm block (re-compresses with Zstd).
    ///
    /// Returns the number of lines accepted. On decompression/re-compression
    /// failure, logs a warning, drops the block, and returns 0. This preserves
    /// the fire-and-forget semantics of the in-memory eviction path while
    /// allowing callers to adjust their line counts.
    pub(crate) fn push_block(&mut self, block: &WarmBlock) -> usize {
        match ColdPage::from_warm_block(block) {
            Ok(page) => {
                let accepted = page.line_count;
                self.bytes_used += page.compressed.len();
                self.line_count = self.line_count.saturating_add(accepted);
                let cumulative = self
                    .cumulative_lines
                    .last()
                    .copied()
                    .unwrap_or(0)
                    .saturating_add(accepted);
                self.cumulative_lines.push(cumulative);
                self.pages.push_back(page);
                accepted
            }
            Err(e) => {
                aterm_log::warn!("cold_tier::push_block: dropping block due to error: {e}");
                0
            }
        }
    }

    /// Get a line by index (0 = oldest available line, accounting for front_offset).
    ///
    /// Uses O(log P) binary search on cumulative line counts and caches the
    /// last decompressed page to avoid redundant Zstd decompression.
    ///
    /// Returns `Ok(None)` for out-of-bounds, `Err` for decompression failures.
    pub(crate) fn get_line(&self, idx: usize) -> Result<Option<Line>, super::ScrollbackError> {
        if idx >= self.line_count {
            return Ok(None);
        }

        // Translate logical index (0 = oldest available) to physical index
        // (0 = first line in first page, including consumed lines).
        let physical_idx = idx + self.front_offset;

        let Some(page_idx) = self.find_page(physical_idx) else {
            return Err(super::ScrollbackError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "in-range line index {idx} (physical {physical_idx}) has no backing cold page"
                ),
            )));
        };
        let page_start = if page_idx == 0 {
            0
        } else {
            self.cumulative_lines[page_idx - 1]
        };
        let line_in_page = physical_idx - page_start;

        // Check cache first.
        {
            let cache = self.last_page_cache.borrow();
            if let Some((cached_idx, ref lines)) = *cache
                && cached_idx == page_idx
            {
                let Some(line) = lines.get(line_in_page).cloned() else {
                    return Err(super::ScrollbackError::Io(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!(
                            "cold page {page_idx} missing line offset {line_in_page} for index {idx}"
                        ),
                    )));
                };
                return Ok(Some(line));
            }
        }

        // Decompress and cache.
        let Some(page) = self.pages.get(page_idx) else {
            return Err(super::ScrollbackError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("cold page index {page_idx} out of range"),
            )));
        };
        let lines = page.decompress()?;
        let Some(line) = lines.get(line_in_page).cloned() else {
            return Err(super::ScrollbackError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "cold page {page_idx} missing line offset {line_in_page} after decompression"
                ),
            )));
        };
        *self.last_page_cache.borrow_mut() = Some((page_idx, lines));
        Ok(Some(line))
    }

    /// Find the page containing the given line index via binary search.
    fn find_page(&self, line_idx: usize) -> Option<usize> {
        #[cfg(test)]
        let counter = count_cold_find_page_step as fn();
        #[cfg(not(test))]
        let counter = || {};
        match super::binary_search_counted(&self.cumulative_lines, line_idx + 1, counter) {
            Ok(idx) => Some(idx),
            Err(idx) => {
                if idx < self.cumulative_lines.len() {
                    Some(idx)
                } else {
                    None
                }
            }
        }
    }

    /// Remove the oldest page (FIFO eviction).
    ///
    /// Returns the number of *logical* lines evicted (excluding already-consumed
    /// lines from front_offset), or 0 if empty.
    /// Production code uses `pop_front_batch`/`evict_bytes` for O(P) bulk eviction.
    #[cfg(test)]
    pub(crate) fn pop_front(&mut self) -> usize {
        let Some(page) = self.pages.pop_front() else {
            return 0;
        };
        let physical_lines = page.line_count;
        let logical_lines = physical_lines.saturating_sub(self.front_offset);
        self.bytes_used = self.bytes_used.saturating_sub(page.compressed.len());
        self.line_count = self.line_count.saturating_sub(logical_lines);
        self.front_offset = 0; // New front page starts fresh.

        // Remove first cumulative entry and adjust remaining values.
        self.cumulative_lines.remove(0);
        for c in &mut self.cumulative_lines {
            *c = c.saturating_sub(physical_lines);
        }

        // Invalidate cache — page indices shifted.
        *self.last_page_cache.borrow_mut() = None;

        logical_lines
    }

    /// Remove the oldest `k` pages in a single batch.
    ///
    /// Returns total *logical* lines evicted (the first page's count is
    /// adjusted for `front_offset`). O(P) where P is the page count,
    /// compared to O(k*P) when calling `pop_front()` k times.
    pub(crate) fn pop_front_batch(&mut self, k: usize) -> usize {
        if k == 0 || self.pages.is_empty() {
            return 0;
        }
        let k = k.min(self.pages.len());

        // Drain evicted pages and sum their line counts.
        let mut evicted_lines = 0;
        let mut evicted_bytes = 0;
        for (i, page) in self.pages.drain(..k).enumerate() {
            let logical = if i == 0 {
                page.line_count.saturating_sub(self.front_offset)
            } else {
                page.line_count
            };
            evicted_lines += logical;
            evicted_bytes += page.compressed.len();
        }
        self.bytes_used = self.bytes_used.saturating_sub(evicted_bytes);
        self.line_count = self.line_count.saturating_sub(evicted_lines);
        self.front_offset = 0; // New front page starts fresh.

        // Rebuild cumulative_lines: drop first k entries, adjust remainder.
        if k >= self.cumulative_lines.len() {
            self.cumulative_lines.clear();
        } else {
            let offset = self.cumulative_lines[k - 1];
            self.cumulative_lines.drain(..k);
            for c in &mut self.cumulative_lines {
                *c = c.saturating_sub(offset);
            }
        }

        // Invalidate cache — page indices shifted.
        *self.last_page_cache.borrow_mut() = None;

        evicted_lines
    }

    /// Evict oldest pages until at least `target_bytes` of compressed memory is freed.
    ///
    /// Returns total lines evicted. Counts pages to evict first, then batch-removes
    /// them in O(P) total instead of the O(k*P) cost of repeated `pop_front()`.
    pub(crate) fn evict_bytes(&mut self, target_bytes: usize) -> usize {
        if target_bytes == 0 || self.pages.is_empty() {
            return 0;
        }

        // Count pages needed to free target_bytes.
        let mut bytes_freed = 0;
        let mut pages_to_evict = 0;
        for page in &self.pages {
            if bytes_freed >= target_bytes {
                break;
            }
            bytes_freed += page.compressed.len();
            pages_to_evict += 1;
        }

        self.pop_front_batch(pages_to_evict)
    }

    /// Remove the oldest `n` logical lines from the front of the cold tier.
    ///
    /// Advances `front_offset` by `n` and drops any pages that become fully
    /// consumed. O(1) when no page boundary is crossed; O(pages_dropped) when
    /// pages are consumed. No decompression is performed.
    ///
    /// # Panics
    ///
    /// Debug-asserts that `n <= self.line_count`.
    pub(crate) fn truncate_front_lines(&mut self, n: usize) {
        if n == 0 {
            return;
        }
        debug_assert!(
            n <= self.line_count,
            "truncate_front_lines({n}) exceeds line_count({})",
            self.line_count
        );
        if n > self.line_count {
            aterm_log::warn!(
                "truncate_front_lines({n}) exceeds line_count({}), saturating",
                self.line_count
            );
        }

        self.front_offset += n;
        self.line_count = self.line_count.saturating_sub(n);

        // Drop fully consumed pages from the front.
        let mut pages_dropped = 0;
        while let Some(front) = self.pages.front() {
            if self.front_offset >= front.line_count {
                let page = self
                    .pages
                    .pop_front()
                    .expect("invariant: front exists after while-let guard");
                self.front_offset = self.front_offset.saturating_sub(page.line_count);
                self.bytes_used = self.bytes_used.saturating_sub(page.compressed.len());
                pages_dropped += 1;
            } else {
                break;
            }
        }

        if pages_dropped > 0 {
            // Rebuild cumulative index: drop first `pages_dropped` entries, adjust remainder.
            if pages_dropped >= self.cumulative_lines.len() {
                self.cumulative_lines.clear();
            } else {
                let physical_offset = self.cumulative_lines[pages_dropped - 1];
                self.cumulative_lines.drain(..pages_dropped);
                for c in &mut self.cumulative_lines {
                    *c = c.saturating_sub(physical_offset);
                }
            }
            // Invalidate cache — page indices shifted.
            *self.last_page_cache.borrow_mut() = None;
        }
    }

    // Back-removal methods (pre_validate_truncate_back, truncate_back_lines)
    // are in cold_tier_back_removal.rs.

    /// Clear all pages.
    pub(crate) fn clear(&mut self) {
        self.pages.clear();
        self.line_count = 0;
        self.cumulative_lines.clear();
        *self.last_page_cache.borrow_mut() = None;
        self.bytes_used = 0;
        self.front_offset = 0;
    }

    /// Get total compressed size (for stats).
    #[must_use]
    pub(crate) fn compressed_size(&self) -> usize {
        self.bytes_used
    }

    #[cfg(any(test, debug_assertions))]
    #[must_use]
    pub(crate) fn recompute_compressed_size(&self) -> usize {
        self.pages.iter().map(|p| p.compressed.len()).sum()
    }
}

impl Default for ColdTier {
    fn default() -> Self {
        Self::new()
    }
}

#[path = "cold_tier_back_removal.rs"]
mod back_removal;

#[cfg(test)]
#[path = "cold_tier_tests.rs"]
mod tests;
