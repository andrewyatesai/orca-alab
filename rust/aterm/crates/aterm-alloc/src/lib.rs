// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0

//! Inline-optimized collections for aterm.
//!
//! Zero external dependencies. Provides:
//!
//! - [`SmallVec<T, N>`] — inline storage for up to N elements, heap fallback.
//!   Replaces `smallvec::SmallVec<[T; N]>`.
//! - [`ArrayVec<T, N>`] — fixed-capacity inline-only storage, no heap allocation.
//!   Replaces `arrayvec::ArrayVec<T, N>`.
//!
//! ## Capacities used in aterm
//!
//! **SmallVec:** 2 (combining chars, hyperlinks, placements), 4 (deferred cols,
//! combining marks), 8 (extra collection keys), 16 (color palette overrides),
//! 96 (cell vertices).
//!
//! **ArrayVec:** 4 (intermediates), 16 (CSI params), 32 (OSC params).

#![deny(clippy::all)]
#![deny(unsafe_op_in_unsafe_fn)]

mod array_vec;
mod small_vec;

pub use array_vec::ArrayVec;
pub use small_vec::SmallVec;
