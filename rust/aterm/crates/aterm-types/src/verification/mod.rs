// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Shared verification helpers used across extracted crates.
//!
//! - `stubs`: Kani-friendly replacements for `HashMap`, `HashSet`, `Instant`, `VecDeque`
//!
//! FFI pointer lifecycle tracking (`ffi_free_tracker`, `terminal_handle_tracker`)
//! lives in `aterm-ffi-types` (#3353).

/// Verification-friendly container stubs for Kani proofs.
pub mod stubs;
