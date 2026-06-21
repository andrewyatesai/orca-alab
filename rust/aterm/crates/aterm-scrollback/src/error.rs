// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Shared error type for scrollback access and storage failures.

/// Errors that can occur when accessing scrollback data.
///
/// Distinguishes between index-out-of-bounds (`Ok(None)`) and actual failures
/// such as I/O or decompression errors.
#[non_exhaustive]
#[derive(Debug, aterm_error::Error)]
pub enum ScrollbackError {
    /// Disk I/O failure when reading from cold tier storage.
    #[error("scrollback I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// Decompression failure (corrupted or invalid compressed data).
    #[error("scrollback decompression error: {0}")]
    Decompression(String),
    /// Block permanently quarantined after repeated decompression failures.
    #[error("scrollback block quarantined: {0} lines inaccessible")]
    Quarantined(usize),
    /// Memory budget enforcement failed after eviction attempts.
    #[error("memory budget enforcement failed: {over_bytes} bytes over budget")]
    EnforcementFailed {
        /// Bytes exceeding the budget after all eviction attempts.
        over_bytes: usize,
    },
}
