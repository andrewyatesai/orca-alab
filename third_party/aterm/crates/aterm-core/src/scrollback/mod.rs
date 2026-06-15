// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0

//! Compatibility shim — re-exports from the `aterm-scrollback` crate.
//!
//! Domain logic now lives in `crates/aterm-scrollback/`. This module provides
//! backward-compatible `crate::scrollback::*` paths for the rest of aterm-core.
//!
//! Extracted as part of the monolith split (#2341).

// Explicit re-exports from aterm-scrollback (#2753).
// Keep this list explicit (no wildcard) so new public symbols in aterm-scrollback
// do not become part of aterm_core::scrollback::* without review.
pub use aterm_scrollback::{
    CellAttrs, DiskBackedScrollback, DiskBackedScrollbackConfig, HyperlinkSpan, Line, Rle,
    Scrollback, ScrollbackIter, ScrollbackRevIter, ScrollbackStorage, WatermarkLevel,
};

#[cfg(fuzzing)]
pub use aterm_scrollback::{deserialize_lines, serialize_lines};
