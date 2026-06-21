// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Hyperlink span type for scrollback lines.
//!
//! Represents a contiguous range of columns that share an OSC 8 hyperlink URL.
//! Used to preserve hyperlinks when lines scroll from the visible grid into
//! scrollback storage.

use std::sync::Arc;

/// Hyperlink span within a line.
///
/// Represents a contiguous range of columns that share a hyperlink URL.
/// Used to preserve OSC 8 hyperlinks when lines scroll into scrollback.
///
/// ## Memory Layout
///
/// ```text
/// ┌─────────────────────────────────────────────────────────────────┐
/// │ start_col: u16 (2 bytes) - Start column (inclusive)             │
/// │ end_col: u16 (2 bytes) - End column (exclusive)                 │
/// │ url: Arc<str> (16 bytes) - Shared reference to URL              │
/// │ id: Option<Arc<str>> (16 bytes) - OSC 8 id parameter            │
/// └─────────────────────────────────────────────────────────────────┘
/// Total: 36 bytes per span
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HyperlinkSpan {
    /// Start column (inclusive).
    pub start_col: u16,
    /// End column (exclusive).
    pub end_col: u16,
    /// URL (shared for efficiency when the same URL spans multiple positions).
    pub url: Arc<str>,
    /// OSC 8 `id=` parameter (shared). `None` if the hyperlink had no explicit ID.
    pub id: Option<Arc<str>>,
}

impl HyperlinkSpan {
    /// Create a new hyperlink span (no explicit ID).
    #[must_use]
    pub fn new(start_col: u16, end_col: u16, url: Arc<str>) -> Self {
        Self {
            start_col,
            end_col,
            url,
            id: None,
        }
    }

    /// Create a new hyperlink span with an explicit OSC 8 ID.
    #[must_use]
    pub fn with_id(start_col: u16, end_col: u16, url: Arc<str>, id: Option<Arc<str>>) -> Self {
        Self {
            start_col,
            end_col,
            url,
            id,
        }
    }

    /// Check if a column is within this span.
    ///
    /// ENSURES: result == (col >= self.start_col && col < self.end_col)
    #[inline]
    #[must_use]
    pub fn contains(&self, col: u16) -> bool {
        col >= self.start_col && col < self.end_col
    }

    /// Get the span width in columns.
    #[inline]
    #[must_use]
    pub fn width(&self) -> u16 {
        self.end_col.saturating_sub(self.start_col)
    }

    /// Serialized size in bytes (for memory accounting).
    ///
    /// V3 format: start_col (u16) + end_col (u16) + url_len (u32) + url + id_len (u32) + id
    #[must_use]
    pub fn serialized_size(&self) -> usize {
        2 + 2 + 4 + self.url.len() + 4 + self.id.as_ref().map_or(0, |id| id.len())
    }
}
