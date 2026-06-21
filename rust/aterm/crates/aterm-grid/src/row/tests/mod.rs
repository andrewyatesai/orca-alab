// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

use super::*;

mod algorithm_audit;
mod basic_ops;
mod operations;
mod style_id;
mod unsafe_boundary;
mod wide_char_fixup;

/// Shared test helper: create a Row with the given column count.
fn make_row(cols: u16) -> (PageStore, Row) {
    let mut pages = PageStore::new();
    // SAFETY: Test rows never outlive their local `pages` owner.
    let row = unsafe { Row::new(cols, &mut pages) };
    (pages, row)
}
