// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Tests for the scrollback module.
//!
//! Split from monolithic tests.rs (#5931) into focused submodules.

use super::*;

mod basic;
mod decompression;
mod line_limit;
mod memory_budget;
mod repaired_trimmed;
mod threading;
mod truncation;

// Performance proofs extracted to stay under 1000-line limit.
#[path = "../performance_tests.rs"]
mod performance_tests;
