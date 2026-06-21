// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Unsafe path boundary tests (MIRI-exercised).
//!
//! Tests here directly exercise `unsafe { get_unchecked* }` paths
//! in `write_char_styled` and `write_wide_char` with edge indices.

use super::super::*;
use super::make_row;

mod boundaries;
mod memory_safety;
mod overwrite;
mod unchecked;
