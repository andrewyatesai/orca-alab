// Copyright 2026 The aterm Authors
// Author: The aterm Authors
// SPDX-License-Identifier: Apache-2.0

//! Internal mutation helpers for RLE sequences.
//!
//! Contains the split, compact, find, and truncate operations used by
//! the public `set`, `set_range`, and `resize` methods in `lib.rs`.

use super::{Rle, Run, count_run_iteration};

impl<T: Copy + PartialEq + Default> Rle<T> {
    /// Rebuild prefix sums from current runs.
    pub(super) fn rebuild_prefix_sums(&mut self) {
        self.prefix_sums.clear();
        self.prefix_sums.reserve(self.runs.len());
        let mut acc = 0u32;
        for run in &self.runs {
            self.prefix_sums.push(acc);
            acc = Self::checked_run_length_sum(acc, run.length);
        }
    }

    /// Truncate to a specific length.
    pub(super) fn truncate(&mut self, new_length: u32) {
        if new_length >= self.total_length {
            return;
        }

        if new_length == 0 {
            self.clear();
            return;
        }

        // Find the run containing the new end
        let mut accumulated = 0u32;
        for (i, run) in self.runs.iter_mut().enumerate() {
            let next_accumulated = Self::checked_run_length_sum(accumulated, run.length);
            if next_accumulated >= new_length {
                // This run contains the new end
                let keep_in_run = new_length - accumulated;
                run.length = keep_in_run;
                self.runs.truncate(i + 1);
                self.total_length = new_length;
                self.rebuild_prefix_sums();
                return;
            }
            accumulated = next_accumulated;
        }
    }

    /// Find the run containing an index.
    ///
    /// Uses O(log n) binary search when prefix sums are cached,
    /// falls back to O(n) linear scan otherwise.
    /// Returns `(run_index, offset_within_run)`.
    pub(super) fn find_run(&self, index: u32) -> Option<(usize, u32)> {
        if self.prefix_sums.len() == self.runs.len() && !self.runs.is_empty() {
            return self.find_run_binary(index);
        }
        self.find_run_linear(index)
    }

    /// Binary search on cached prefix sums.
    fn find_run_binary(&self, index: u32) -> Option<(usize, u32)> {
        // prefix_sums[i] = start offset of run i.
        // Find the largest i where prefix_sums[i] <= index.
        // partition_point returns the first i where prefix_sums[i] > index.
        let pos = self.prefix_sums.partition_point(|&start| start <= index);
        if pos == 0 {
            return None;
        }
        let run_idx = pos - 1;
        // Count a single "iteration" for tests; no-op in non-test builds.
        count_run_iteration();
        let offset = index - self.prefix_sums[run_idx];
        if offset < self.runs[run_idx].length {
            Some((run_idx, offset))
        } else {
            None
        }
    }

    /// Linear scan fallback when prefix sums are not cached.
    fn find_run_linear(&self, index: u32) -> Option<(usize, u32)> {
        let mut accumulated = 0u32;
        for (i, run) in self.runs.iter().enumerate() {
            count_run_iteration();
            let next_accumulated = Self::checked_run_length_sum(accumulated, run.length);
            if next_accumulated > index {
                return Some((i, index - accumulated));
            }
            accumulated = next_accumulated;
        }
        None
    }

    /// Split a single run to set a value at a specific offset.
    pub(super) fn split_and_set(&mut self, run_idx: usize, offset: u32, value: T) {
        let run = &self.runs[run_idx];
        let run_len = run.length;
        let old_value = run.value;

        if run_len == 1 {
            // Simple case: run of length 1
            self.runs[run_idx].value = value;
            self.compact_around(run_idx);
            return;
        }

        if offset == 0 {
            // At start of run
            self.runs[run_idx].length -= 1;
            self.runs.insert(run_idx, Run { value, length: 1 });
            self.compact_around(run_idx);
        } else if offset == run_len - 1 {
            // At end of run
            self.runs[run_idx].length -= 1;
            self.runs.insert(run_idx + 1, Run { value, length: 1 });
            self.compact_around(run_idx + 1);
        } else {
            // In middle - split into 3
            let after_len = run_len - offset - 1;
            self.runs[run_idx].length = offset;
            self.runs.insert(run_idx + 1, Run { value, length: 1 });
            self.runs.insert(
                run_idx + 2,
                Run {
                    value: old_value,
                    length: after_len,
                },
            );
        }
    }

    /// Split a single run to set a range to a new value.
    pub(super) fn split_range_single_run(
        &mut self,
        run_idx: usize,
        start_offset: u32,
        end_offset: u32,
        value: T,
    ) {
        let run = &self.runs[run_idx];
        let run_len = run.length;
        let old_value = run.value;
        let range_len = end_offset - start_offset;

        if start_offset == 0 && end_offset >= run_len {
            // Replace entire run
            self.runs[run_idx].value = value;
            self.compact_around(run_idx);
            return;
        }

        let mut new_runs = Vec::with_capacity(3);

        // Before part
        if start_offset > 0 {
            new_runs.push(Run {
                value: old_value,
                length: start_offset,
            });
        }

        // Replaced part
        new_runs.push(Run {
            value,
            length: range_len,
        });

        // After part
        if end_offset < run_len {
            new_runs.push(Run {
                value: old_value,
                length: run_len - end_offset,
            });
        }

        // Replace the run with new runs
        self.runs.splice(run_idx..=run_idx, new_runs);
        self.compact();
    }

    /// Split multiple runs to set a range to a new value.
    pub(super) fn split_range_multi_run(
        &mut self,
        start_run_idx: usize,
        start_offset: u32,
        end_run_idx: usize,
        end_offset: u32,
        value: T,
    ) {
        // Calculate total length of the range
        let mut range_len = 0u32;
        for i in start_run_idx..=end_run_idx {
            count_run_iteration();
            let run = &self.runs[i];
            let segment_len = if i == start_run_idx {
                run.length - start_offset
            } else if i == end_run_idx {
                end_offset
            } else {
                run.length
            };
            range_len = Self::checked_run_length_sum(range_len, segment_len);
        }

        let mut new_runs = Vec::new();

        // Before part from start run
        let start_run = &self.runs[start_run_idx];
        if start_offset > 0 {
            new_runs.push(Run {
                value: start_run.value,
                length: start_offset,
            });
        }

        // The new range
        new_runs.push(Run {
            value,
            length: range_len,
        });

        // After part from end run
        let end_run = &self.runs[end_run_idx];
        if end_offset < end_run.length {
            new_runs.push(Run {
                value: end_run.value,
                length: end_run.length - end_offset,
            });
        }

        // Replace the runs
        self.runs.splice(start_run_idx..=end_run_idx, new_runs);
        self.compact();
    }

    /// Compact adjacent runs with the same value.
    pub(crate) fn compact(&mut self) {
        if self.runs.len() <= 1 {
            return;
        }

        let mut write = 0;
        for read in 1..self.runs.len() {
            count_run_iteration();
            if self.runs[write].value == self.runs[read].value {
                self.runs[write].length = self.runs[write]
                    .length
                    .checked_add(self.runs[read].length)
                    .expect("Rle invariant violated: merged run exceeds u32::MAX");
            } else {
                write += 1;
                if write != read {
                    self.runs[write] = self.runs[read];
                }
            }
        }
        self.runs.truncate(write + 1);
    }

    /// Compact around a specific index.
    pub(crate) fn compact_around(&mut self, idx: usize) {
        // Merge with previous
        if idx > 0 && self.runs[idx - 1].value == self.runs[idx].value {
            self.runs[idx - 1].length = self.runs[idx - 1]
                .length
                .checked_add(self.runs[idx].length)
                .expect("Rle invariant violated: merged run exceeds u32::MAX");
            self.runs.remove(idx);
            // Check if we need to merge with next (now at idx-1)
            if idx < self.runs.len() && self.runs[idx - 1].value == self.runs[idx].value {
                self.runs[idx - 1].length = self.runs[idx - 1]
                    .length
                    .checked_add(self.runs[idx].length)
                    .expect("Rle invariant violated: merged run exceeds u32::MAX");
                self.runs.remove(idx);
            }
            return;
        }

        // Merge with next
        if idx + 1 < self.runs.len() && self.runs[idx].value == self.runs[idx + 1].value {
            self.runs[idx].length = self.runs[idx]
                .length
                .checked_add(self.runs[idx + 1].length)
                .expect("Rle invariant violated: merged run exceeds u32::MAX");
            self.runs.remove(idx + 1);
        }
    }
}
