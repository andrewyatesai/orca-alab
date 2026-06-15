// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Accounting and watermark maintenance helpers for [`Scrollback`].

use super::{Scrollback, WatermarkLevel, threshold_bytes};

impl Scrollback {
    /// Configure watermark thresholds as percentages (0-100) of the memory budget.
    pub fn set_watermark_thresholds(&mut self, yellow_percent: usize, red_percent: usize) {
        let yellow = yellow_percent.clamp(1, 100);
        let red = red_percent.clamp(yellow, 100);
        let exit = yellow / 2;

        self.yellow_threshold = threshold_bytes(yellow, self.memory_budget);
        self.yellow_exit_threshold = threshold_bytes(exit.max(1), self.memory_budget);
        self.red_threshold = threshold_bytes(red, self.memory_budget);
        self.watermark_level = WatermarkLevel::Green;
        self.update_watermark_level();
    }

    /// Update both diagnostic and budget aggregates from per-tier counters.
    pub(crate) fn sync_accounting(&mut self) {
        self.bytes_used =
            self.hot.memory_used() + self.warm.memory_used() + self.cold.compressed_size();
        self.budgeted_bytes =
            self.hot.budgeted_bytes() + self.warm.budgeted_bytes() + self.cold.compressed_size();
        self.update_watermark_level();
    }

    /// Recompute watermark level from current `budgeted_bytes` vs thresholds.
    #[inline]
    pub(crate) fn update_watermark_level(&mut self) {
        self.watermark_level = super::recompute_watermark(
            self.watermark_level,
            self.budgeted_bytes,
            self.red_threshold,
            self.yellow_threshold,
            self.yellow_exit_threshold,
        );
    }

    #[cfg(any(test, debug_assertions))]
    pub(crate) fn recompute_total_memory_used(&self) -> usize {
        self.hot.recompute_memory_used()
            + self.warm.recompute_memory_used()
            + self.cold.recompute_compressed_size()
    }

    #[cfg(any(test, debug_assertions))]
    pub(crate) fn recompute_budgeted_bytes(&self) -> usize {
        self.hot.recompute_budgeted_bytes()
            + self.warm.recompute_budgeted_bytes()
            + self.cold.recompute_compressed_size()
    }

    pub(crate) fn assert_bytes_used_invariant(&self) {
        #[cfg(any(test, debug_assertions))]
        {
            debug_assert_eq!(
                self.bytes_used,
                self.recompute_total_memory_used(),
                "scrollback bytes_used counter drift",
            );
            debug_assert_eq!(
                self.budgeted_bytes,
                self.recompute_budgeted_bytes(),
                "scrollback budgeted_bytes counter drift",
            );
            let tier_line_count = self.hot.len() + self.warm.line_count() + self.cold.line_count();
            debug_assert_eq!(
                self.line_count, tier_line_count,
                "scrollback line_count drift: aggregate={} but tiers sum={}",
                self.line_count, tier_line_count,
            );
        }
    }
}
