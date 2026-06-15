// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0

//! Property-based tests for aterm-core.
//!
//! Physical files relocated to test_support/proptest/ (Part of #6814).
//! These tests still exercise crate-private seams and remain owned by aterm-core.

#[path = "../../../test_support/proptest/scrollback.rs"]
mod scrollback;
