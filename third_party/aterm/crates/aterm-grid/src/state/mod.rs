// Copyright 2026 The aterm Authors
// Author: The aterm Authors
// SPDX-License-Identifier: Apache-2.0

//! Grid state pods: presentation and cursor/region state.

mod cursor;
mod presentation;

pub use cursor::GridCursorState;
pub use presentation::GridPresentationState;
