// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Private `Grid` state pods.
//!
//! These keep `Grid` as the stable public facade while separating storage,
//! cursor/region, and presentation concerns.

mod scrollback;
mod storage;

pub use crate::{GridCursorState, GridPresentationState};
pub use storage::GridStorage;
