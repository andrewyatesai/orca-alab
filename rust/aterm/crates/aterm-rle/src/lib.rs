// Copyright 2026 The aterm Authors
// Author: The aterm Authors
// SPDX-License-Identifier: Apache-2.0

#![deny(unsafe_op_in_unsafe_fn)]
#![deny(missing_docs)]
#![deny(clippy::all)]

//! Run-Length Encoding (RLE) for cell attributes.
//!
//! This crate provides RLE compression for terminal cell attributes based on
//! Windows Terminal's `til/rle.h` pattern. Attributes are compressed into runs
//! of consecutive cells with identical style.
//!
//! ## Design
//!
//! Terminal output often has runs of cells with identical attributes (e.g., a
//! prompt in one color, then text in another). RLE compression exploits this
//! by storing `(style, count)` pairs instead of per-cell styles.
//!
//! ## Architecture
//!
//! ```text
//! Row Storage (before RLE):
//! [Cell0][Cell1][Cell2][Cell3][Cell4][Cell5][Cell6][Cell7]
//!  Bold   Bold   Bold   Bold  Normal Normal Normal Normal
//!
//! Row Storage (with RLE attributes):
//! Characters: [C0][C1][C2][C3][C4][C5][C6][C7]
//! Attributes: [(Bold, 4), (Normal, 4)]
//! ```
//!
//! ## References
//!
//! - Windows Terminal: `src/inc/til/rle.h`
//! - Ghostty: Style ID indirection in `page.zig`

use std::fmt;
use std::ops::Index;

#[cfg(test)]
mod iteration_counter {
    use std::cell::Cell;

    thread_local! {
        static RUN_ITERATIONS: Cell<usize> = const { Cell::new(0) };
    }

    pub(super) fn count_run_iteration() {
        RUN_ITERATIONS.with(|counter| counter.set(counter.get() + 1));
    }

    pub(super) fn reset_run_iterations() {
        RUN_ITERATIONS.with(|counter| counter.set(0));
    }

    pub(super) fn take_run_iterations() -> usize {
        RUN_ITERATIONS.with(|counter| {
            let value = counter.get();
            counter.set(0);
            value
        })
    }
}

#[cfg(test)]
use iteration_counter::{count_run_iteration, reset_run_iterations, take_run_iterations};

#[cfg(not(test))]
#[inline]
fn count_run_iteration() {}

/// A run of cells with identical attributes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Run<T> {
    /// The attribute value for this run.
    pub value: T,
    /// Number of consecutive cells with this attribute.
    pub length: u32,
}

/// Error returned when an RLE operation would exceed `u32::MAX` capacity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RleCapacityError {
    /// How many elements were requested.
    pub requested: u32,
    /// How many elements could actually fit.
    pub available: u32,
}

impl fmt::Display for RleCapacityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "RLE capacity exceeded: requested {} but only {} available",
            self.requested, self.available
        )
    }
}

impl std::error::Error for RleCapacityError {}

impl<T: Default> Default for Run<T> {
    fn default() -> Self {
        Self {
            value: T::default(),
            length: 0,
        }
    }
}

/// Run-Length Encoded sequence of attributes.
///
/// Stores a sequence of attributes as runs of consecutive identical values.
/// Provides O(log runs) random access via binary search on cached prefix sums,
/// and efficient range operations.
///
/// # Type Parameters
///
/// - `T`: The attribute type (must be `Copy + PartialEq + Default`)
#[derive(Debug, Clone)]
pub struct Rle<T> {
    /// The runs in order.
    runs: Vec<Run<T>>,
    /// Total length (sum of all run lengths).
    total_length: u32,
    /// Cached prefix sums for O(log n) binary search in `find_run`.
    /// `prefix_sums[i]` = sum of `runs[0..i].length` (start offset of run `i`).
    /// Empty when invalidated; rebuilt lazily on the next `find_run` call.
    prefix_sums: Vec<u32>,
}

impl<T: Copy + PartialEq + Default> Default for Rle<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Copy + PartialEq + Default> Rle<T> {
    /// Create an empty RLE sequence.
    #[must_use]
    pub fn new() -> Self {
        Self {
            runs: Vec::new(),
            total_length: 0,
            prefix_sums: Vec::new(),
        }
    }

    /// Create an RLE sequence with a single value repeated `length` times.
    #[must_use]
    pub fn with_value(value: T, length: u32) -> Self {
        if length == 0 {
            return Self::new();
        }
        Self {
            runs: vec![Run { value, length }],
            total_length: length,
            prefix_sums: Vec::new(),
        }
    }

    /// Create an RLE sequence from an iterator of values.
    ///
    /// Prefer using `.collect::<Rle<T>>()` or `Rle::from_iter(...)` via the
    /// standard `FromIterator` trait.
    fn from_iter_inner<I: IntoIterator<Item = T>>(iter: I) -> Self {
        let mut rle = Self::new();
        for value in iter {
            rle.push(value);
        }
        rle
    }

    /// Returns how many more elements can be added before reaching `u32::MAX`.
    #[must_use]
    #[inline]
    pub fn remaining_capacity(&self) -> u32 {
        u32::MAX - self.total_length
    }

    #[inline]
    fn checked_run_length_sum(lhs: u32, rhs: u32) -> u32 {
        lhs.checked_add(rhs)
            .expect("Rle invariant violated: run lengths exceed u32::MAX")
    }

    /// Get the total length of the sequence.
    #[must_use]
    #[inline]
    pub fn len(&self) -> u32 {
        self.total_length
    }

    /// Check if the sequence is empty.
    #[must_use]
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.total_length == 0
    }

    /// Get the number of runs.
    #[must_use]
    #[inline]
    pub fn run_count(&self) -> usize {
        self.runs.len()
    }

    /// Get a reference to the runs.
    #[must_use]
    #[inline]
    pub fn runs(&self) -> &[Run<T>] {
        &self.runs
    }

    /// Clear the sequence.
    pub fn clear(&mut self) {
        self.runs.clear();
        self.total_length = 0;
        self.prefix_sums.clear();
    }

    /// Push a single value onto the end.
    ///
    /// If the sequence is already at `u32::MAX` capacity, this is a silent
    /// no-op. Use [`try_push`](Self::try_push) to detect overflow.
    pub fn push(&mut self, value: T) {
        self.extend_with(value, 1);
    }

    /// Push a single value, returning an error if at capacity.
    ///
    /// Unlike [`push`](Self::push), this rejects the operation instead of
    /// silently dropping the element.
    pub fn try_push(&mut self, value: T) -> Result<(), RleCapacityError> {
        self.try_extend_with(value, 1)
    }

    /// Extend with multiple copies of the same value.
    ///
    /// If `count` exceeds [`remaining_capacity`](Self::remaining_capacity),
    /// the count is silently clamped to the available space. The invariant
    /// `total_length == sum(run.length)` is always preserved.
    ///
    /// Use [`try_extend_with`](Self::try_extend_with) to reject partial
    /// insertions.
    pub fn extend_with(&mut self, value: T, count: u32) {
        let count = count.min(self.remaining_capacity());
        self.extend_with_unclamped(value, count);
    }

    /// Extend with multiple copies, returning an error if the full count
    /// cannot fit.
    ///
    /// Unlike [`extend_with`](Self::extend_with), this rejects the entire
    /// operation rather than silently clamping.
    pub fn try_extend_with(&mut self, value: T, count: u32) -> Result<(), RleCapacityError> {
        let available = self.remaining_capacity();
        if count > available {
            return Err(RleCapacityError {
                requested: count,
                available,
            });
        }
        self.extend_with_unclamped(value, count);
        Ok(())
    }

    /// Inner extend that assumes `count <= remaining_capacity()`.
    fn extend_with_unclamped(&mut self, value: T, count: u32) {
        if count == 0 {
            return;
        }
        if let Some(last) = self.runs.last_mut()
            && last.value == value
        {
            last.length = Self::checked_run_length_sum(last.length, count);
            self.total_length = Self::checked_run_length_sum(self.total_length, count);
            // Prefix sums still valid — no new run boundaries.
            return;
        }
        let offset = self.total_length;
        self.runs.push(Run {
            value,
            length: count,
        });
        self.total_length = Self::checked_run_length_sum(self.total_length, count);
        if self.prefix_sums.len() == self.runs.len() - 1 {
            self.prefix_sums.push(offset);
        }
    }

    /// Get the value at a specific index (safe alternative to `Index`).
    ///
    /// This is the non-panicking alternative to the `Index` trait implementation.
    /// Returns `None` if index is out of bounds instead of panicking.
    #[must_use]
    pub fn get(&self, index: u32) -> Option<T> {
        if index >= self.total_length {
            return None;
        }
        let (run_idx, _) = self.find_run(index)?;
        Some(self.runs[run_idx].value)
    }

    /// Set the value at a specific index.
    ///
    /// Returns `false` if index is out of bounds.
    pub fn set(&mut self, index: u32, value: T) -> bool {
        if index >= self.total_length {
            return false;
        }

        let Some((run_idx, offset_in_run)) = self.find_run(index) else {
            return false;
        };

        let run = &self.runs[run_idx];
        if run.value == value {
            // Value already matches
            return true;
        }

        // Need to split the run
        self.split_and_set(run_idx, offset_in_run, value);
        self.rebuild_prefix_sums();
        true
    }

    /// Set a range of values to the same attribute.
    ///
    /// This is the most efficient way to update multiple cells.
    pub fn set_range(&mut self, start: u32, end: u32, value: T) {
        if start >= end || start >= self.total_length {
            return;
        }
        let end = end.min(self.total_length);

        // Fast path: entire sequence
        if start == 0 && end == self.total_length {
            self.runs.clear();
            self.runs.push(Run {
                value,
                length: self.total_length,
            });
            self.rebuild_prefix_sums();
            return;
        }

        // Find start and end runs
        let Some((start_run_idx, start_offset)) = self.find_run(start) else {
            return;
        };

        // The end-1 index gives us the last cell to modify
        let Some((end_run_idx, end_offset)) = self.find_run(end - 1) else {
            return;
        };

        // Simple case: same run
        if start_run_idx == end_run_idx {
            let run = &self.runs[start_run_idx];
            if run.value == value {
                return; // Already the correct value
            }
            self.split_range_single_run(start_run_idx, start_offset, end_offset + 1, value);
            self.rebuild_prefix_sums();
            return;
        }

        // Complex case: spans multiple runs
        self.split_range_multi_run(
            start_run_idx,
            start_offset,
            end_run_idx,
            end_offset + 1,
            value,
        );
        self.rebuild_prefix_sums();
    }

    /// Resize the sequence, extending with default value or truncating.
    pub fn resize(&mut self, new_length: u32) {
        if new_length == self.total_length {
            return;
        }

        if new_length == 0 {
            self.clear();
            return;
        }

        if new_length > self.total_length {
            // Extend with default
            self.extend_with(T::default(), new_length - self.total_length);
        } else {
            // Truncate
            self.truncate(new_length);
        }
    }

    /// Resize, extending with a specific value.
    pub fn resize_with(&mut self, new_length: u32, value: T) {
        if new_length == self.total_length {
            return;
        }

        if new_length == 0 {
            self.clear();
            return;
        }

        if new_length > self.total_length {
            self.extend_with(value, new_length - self.total_length);
        } else {
            self.truncate(new_length);
        }
    }

    /// Iterate over all values (expanded).
    pub fn iter(&self) -> RleIter<'_, T> {
        RleIter {
            runs: &self.runs,
            run_idx: 0,
            offset_in_run: 0,
        }
    }
}

/// Index trait implementation for `Rle<T>`.
///
/// # Panics
///
/// Panics if `index` is out of bounds. For a non-panicking alternative,
/// use [`Rle::get`] which returns `Option<T>`.
#[allow(
    clippy::expect_used,
    reason = "Index trait contract: panic on out-of-bounds is standard behavior"
)]
impl<T: Copy + PartialEq + Default> Index<u32> for Rle<T> {
    type Output = T;

    fn index(&self, index: u32) -> &Self::Output {
        let (run_idx, _) = self.find_run(index).expect("index out of bounds");
        &self.runs[run_idx].value
    }
}

impl<'a, T: Copy + PartialEq + Default> IntoIterator for &'a Rle<T> {
    type Item = T;
    type IntoIter = RleIter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// Iterator over expanded RLE values.
pub struct RleIter<'a, T> {
    runs: &'a [Run<T>],
    run_idx: usize,
    offset_in_run: u32,
}

impl<T: Copy> Iterator for RleIter<'_, T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.run_idx >= self.runs.len() {
            return None;
        }

        let run = &self.runs[self.run_idx];
        let value = run.value;

        self.offset_in_run += 1;
        if self.offset_in_run >= run.length {
            self.run_idx += 1;
            self.offset_in_run = 0;
        }

        Some(value)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let mut remaining = 0usize;
        for run in &self.runs[self.run_idx..] {
            remaining += run.length as usize;
        }
        remaining = remaining.saturating_sub(self.offset_in_run as usize);
        (remaining, Some(remaining))
    }
}

impl<T: Copy> ExactSizeIterator for RleIter<'_, T> {}

impl<T: Copy + PartialEq + Default> FromIterator<T> for Rle<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        Self::from_iter_inner(iter)
    }
}

mod mutations;

#[cfg(test)]
mod tests;

#[cfg(kani)]
mod proofs;

#[cfg(kani)]
mod proofs_saturation;

#[cfg(kani)]
#[path = "compact_proofs_tests.rs"]
mod proofs_compact;
