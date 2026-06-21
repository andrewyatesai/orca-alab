// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Front-truncation operations for [`WarmTier`].
//!
//! Extracted from `tier.rs` for file-size compliance (#5947).
//! Supports `Scrollback::truncate` which removes oldest lines.
//!
//! Uses `front_offset` for O(1) line removal: instead of decompressing
//! the boundary block to trim a few lines, we advance the offset counter.
//! Blocks are dropped when fully consumed. This mirrors the cold tier's
//! `front_offset` pattern and eliminates LZ4 decompression from the
//! push-truncate hot path.

use super::WarmTier;

impl WarmTier {
    /// Materialize front_offset by trimming the first block in place.
    ///
    /// Called before operations that need the front block to contain only
    /// live (non-consumed) lines: pop_front, push_front (restoring after
    /// failed eviction). O(1) when front_offset == 0.
    ///
    /// If the front block is corrupt and cannot be decompressed, we still
    /// convert it into a logical "surviving suffix" by shrinking its recorded
    /// line count and clearing front_offset. This preserves the remaining live
    /// lines for the normal warm eviction/quarantine flow instead of dropping
    /// them immediately.
    pub(super) fn materialize_front_offset(&mut self) {
        if self.front_offset == 0 || self.blocks.is_empty() {
            return;
        }

        while let Some(front) = self.blocks.front() {
            if self.front_offset < front.line_count() {
                break;
            }

            // Fully consumed block — front_offset already removed these lines
            // from the logical count, so only the compressed storage changes.
            let block = self
                .blocks
                .pop_front()
                .expect("invariant: front block exists while materializing front_offset");
            self.budgeted_bytes = self.budgeted_bytes.saturating_sub(block.compressed_size());
            let bytes_used = self
                .bytes_used
                .get()
                .saturating_sub(block.compressed_size());
            self.bytes_used.set(bytes_used);
            self.front_offset = self.front_offset.saturating_sub(block.line_count());
            self.rebuild_cumulative();
            self.clear_cache();
        }

        if self.front_offset == 0 || self.blocks.is_empty() {
            return;
        }

        let front = &self.blocks[0];
        match front.decompress() {
            Ok(lines) => {
                let surviving = &lines[self.front_offset..];
                let old_size = self.blocks[0].compressed_size();
                let replacement = super::WarmBlock::from_lines(surviving);
                let new_size = replacement.compressed_size();
                self.blocks[0] = replacement;
                self.budgeted_bytes = self.budgeted_bytes.saturating_sub(old_size) + new_size;
                let bytes_used = self.bytes_used.get().saturating_sub(old_size) + new_size;
                self.bytes_used.set(bytes_used);
                self.front_offset = 0;
                self.rebuild_cumulative();
                self.clear_cache();
            }
            Err(_) => {
                // Keep the surviving suffix logically present so eviction can
                // retry and eventually quarantine only the remaining live lines.
                let surviving = self.blocks[0]
                    .line_count()
                    .saturating_sub(self.front_offset);
                self.blocks[0].line_count = surviving;
                self.front_offset = 0;
                self.rebuild_cumulative();
                self.clear_cache();
            }
        }
    }

    /// Remove the oldest `n` lines from the front of the warm tier.
    ///
    /// Advances `front_offset` by `n` and drops any blocks that become fully
    /// consumed. O(1) when no block boundary is crossed; O(blocks_dropped) when
    /// blocks are consumed. No decompression is performed.
    ///
    /// This replaces the previous decompress-trim-recompress approach, making
    /// line-limit enforcement during `push_line` O(1) instead of O(block_size).
    ///
    /// # Panics
    ///
    /// Debug-asserts that `n <= self.line_count`.
    pub(crate) fn truncate_front_lines(&mut self, n: usize) -> Result<(), crate::ScrollbackError> {
        if n == 0 {
            return Ok(());
        }
        debug_assert!(
            n <= self.line_count,
            "truncate_front_lines({n}) exceeds line_count({})",
            self.line_count
        );
        if n > self.line_count {
            aterm_log::warn!(
                "warm truncate_front_lines({n}) exceeds line_count({}), saturating",
                self.line_count
            );
        }

        self.front_offset += n;
        self.line_count = self.line_count.saturating_sub(n);

        // Drop fully consumed front blocks.
        let mut blocks_dropped = 0;
        while let Some(front) = self.blocks.front() {
            if self.front_offset >= front.line_count() {
                let block = self
                    .blocks
                    .pop_front()
                    .expect("invariant: front block exists while truncating warm tier");
                self.front_offset = self.front_offset.saturating_sub(block.line_count());
                self.budgeted_bytes = self.budgeted_bytes.saturating_sub(block.compressed_size());
                let bytes_used = self
                    .bytes_used
                    .get()
                    .saturating_sub(block.compressed_size());
                self.bytes_used.set(bytes_used);
                blocks_dropped += 1;
            } else {
                break;
            }
        }

        if blocks_dropped > 0 {
            // Rebuild cumulative index after removing front blocks.
            self.rebuild_cumulative();
            // Invalidate cache — block indices shifted.
            self.clear_cache();
        }

        Ok(())
    }
}
