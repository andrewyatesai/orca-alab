// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Shared test utilities for the vi_mode module.

use std::collections::HashSet;

use aterm_types::BufferAccess;

/// Mock grid for vi_mode tests.
///
/// Each row is a vector of characters, initialized to spaces. Use
/// [`with_line`](MockGrid::with_line) to set row content. Implements
/// [`BufferAccess`] so it can be passed to navigation and word functions.
pub(crate) struct MockGrid {
    rows: u16,
    cols: u16,
    display_offset: i32,
    total_lines: i32,
    content: Vec<Vec<char>>,
    wrapped_lines: HashSet<usize>,
}

impl MockGrid {
    pub(crate) fn new(rows: u16, cols: u16) -> Self {
        let content = vec![vec![' '; cols as usize]; rows as usize];
        Self {
            rows,
            cols,
            display_offset: 0,
            total_lines: 0,
            content,
            wrapped_lines: HashSet::new(),
        }
    }

    pub(crate) fn with_scrollback(mut self, total_lines: i32) -> Self {
        self.total_lines = total_lines;
        self
    }

    pub(crate) fn with_display_offset(mut self, offset: i32) -> Self {
        self.display_offset = offset;
        self
    }

    /// Mark a row as a soft-wrapped continuation of the previous row.
    pub(crate) fn with_wrapped(mut self, row: usize) -> Self {
        self.wrapped_lines.insert(row);
        self
    }

    pub(crate) fn with_line(mut self, row: usize, text: &str) -> Self {
        let chars: Vec<char> = text.chars().collect();
        if row < self.content.len() {
            for (i, &ch) in chars.iter().enumerate() {
                if i < self.cols as usize {
                    self.content[row][i] = ch;
                }
            }
        }
        self
    }
}

impl BufferAccess for MockGrid {
    fn char_at(&self, line: i32, col: u16) -> Option<char> {
        if line < 0 || line >= i32::from(self.rows) {
            return None;
        }
        self.content
            .get(line as usize)
            .and_then(|r| r.get(col as usize))
            .copied()
    }

    fn line_len(&self, line: i32) -> u16 {
        if line < 0 || line >= i32::from(self.rows) {
            return 0;
        }
        let row = &self.content[line as usize];
        row.iter()
            .rposition(|&c| c != ' ')
            .map_or(0, |i| (i + 1) as u16)
    }

    fn total_lines(&self) -> i32 {
        self.total_lines
    }

    fn visible_rows(&self) -> u16 {
        self.rows
    }

    fn cols(&self) -> u16 {
        self.cols
    }

    fn line_text(&self, line: i32) -> Option<String> {
        if line < 0 || line >= i32::from(self.rows) {
            return None;
        }
        Some(self.content[line as usize].iter().collect())
    }

    fn display_offset(&self) -> i32 {
        self.display_offset
    }

    fn is_line_wrapped(&self, line: i32) -> bool {
        if line < 0 {
            return false;
        }
        self.wrapped_lines.contains(&(line as usize))
    }
}
