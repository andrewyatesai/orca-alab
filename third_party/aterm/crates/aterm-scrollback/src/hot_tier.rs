// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Hot tier: uncompressed lines in VecDeque.
//!
//! Instant access, no decompression overhead.

use super::line::Line;
use std::collections::VecDeque;

/// Hot tier: uncompressed lines in RAM.
///
/// Uses VecDeque for efficient front/back operations.
#[derive(Debug)]
pub(crate) struct HotTier {
    /// Lines stored uncompressed.
    lines: VecDeque<Line>,
    /// Running total for `memory_used()` (diagnostic: includes struct overhead).
    bytes_used: usize,
    /// Reclaimable line storage only (budget enforcement). Excludes struct overhead.
    budgeted_bytes: usize,
}

impl HotTier {
    /// Create a new hot tier.
    #[must_use]
    pub(crate) fn new() -> Self {
        Self {
            lines: VecDeque::new(),
            bytes_used: std::mem::size_of::<Self>(),
            budgeted_bytes: 0,
        }
    }

    /// Get the number of lines.
    #[must_use]
    #[inline]
    pub(crate) fn len(&self) -> usize {
        self.lines.len()
    }

    /// Push a line to the back.
    #[inline]
    pub(crate) fn push(&mut self, line: Line) {
        let mem = line.memory_used();
        self.bytes_used += mem;
        self.budgeted_bytes += mem;
        self.lines.push_back(line);
    }

    /// Get a line by index (0 = oldest).
    ///
    /// Returns a reference — no clone. Callers that need ownership should
    /// clone explicitly or use `Cow::into_owned()` at the tier-dispatch level.
    #[must_use]
    pub(crate) fn get(&self, idx: usize) -> Option<&Line> {
        self.lines.get(idx)
    }

    /// Take n lines from the front.
    pub(crate) fn take_front(&mut self, n: usize) -> Vec<Line> {
        let n = n.min(self.lines.len());
        let mut result = Vec::with_capacity(n);
        for _ in 0..n {
            if let Some(line) = self.lines.pop_front() {
                let mem = line.memory_used();
                self.bytes_used = self.bytes_used.saturating_sub(mem);
                self.budgeted_bytes = self.budgeted_bytes.saturating_sub(mem);
                result.push(line);
            }
        }
        result
    }

    /// Truncate to keep only the last n lines.
    pub(crate) fn truncate_front(&mut self, n: usize) {
        while self.lines.len() > n {
            if let Some(line) = self.lines.pop_front() {
                let mem = line.memory_used();
                self.bytes_used = self.bytes_used.saturating_sub(mem);
                self.budgeted_bytes = self.budgeted_bytes.saturating_sub(mem);
            }
        }
    }

    /// Remove the `n` most recent lines (from the back).
    ///
    /// Used by Kitty CSI +T unscroll: recovered lines must be removed
    /// from scrollback after being placed back into the visible grid.
    pub(crate) fn truncate_back(&mut self, n: usize) {
        for _ in 0..n.min(self.lines.len()) {
            if let Some(line) = self.lines.pop_back() {
                let mem = line.memory_used();
                self.bytes_used = self.bytes_used.saturating_sub(mem);
                self.budgeted_bytes = self.budgeted_bytes.saturating_sub(mem);
            }
        }
    }

    /// Clear all lines.
    pub(crate) fn clear(&mut self) {
        self.lines.clear();
        self.bytes_used = std::mem::size_of::<Self>();
        self.budgeted_bytes = 0;
    }

    /// Calculate memory used (diagnostic: includes struct overhead).
    #[must_use]
    pub(crate) fn memory_used(&self) -> usize {
        self.bytes_used
    }

    /// Reclaimable line storage bytes (budget enforcement only).
    #[must_use]
    #[inline]
    pub(crate) fn budgeted_bytes(&self) -> usize {
        self.budgeted_bytes
    }

    #[cfg(any(test, debug_assertions))]
    #[must_use]
    pub(crate) fn recompute_memory_used(&self) -> usize {
        let base = std::mem::size_of::<Self>();
        let lines_mem: usize = self.lines.iter().map(Line::memory_used).sum();
        base + lines_mem
    }

    #[cfg(any(test, debug_assertions))]
    #[must_use]
    pub(crate) fn recompute_budgeted_bytes(&self) -> usize {
        self.lines.iter().map(Line::memory_used).sum()
    }
}

impl Default for HotTier {
    fn default() -> Self {
        Self::new()
    }
}
