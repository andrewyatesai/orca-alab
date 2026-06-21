// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Search surface for aterm-core.
//!
//! Pure search logic lives in `aterm-search`.
//!
//! `Scrollback*` adapters live in `aterm-scrollback`, where those types are
//! local and can satisfy Rust's orphan rules.

pub use aterm_search::streaming;
pub use aterm_search::{BloomFilter, SearchDirection, SearchIndex, SearchMatch, TerminalSearch};

// SearchContent impl for Grid moved to aterm-grid (#6554, orphan rules).
