// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Regression tests for keyboard encoding internals (write_u32).

use super::write_u32;

/// Regression test: write_u32 with values >= 2,000,000,000 must not
/// infinite-loop or produce incorrect output.
///
/// Bug #2775: The original implementation used `u32` for the divisor.
/// When val >= 2B, `divisor * 10` overflowed u32, causing an infinite
/// loop in the digit-extraction while loop. Fix: use u64 for divisor.
#[test]
fn write_u32_two_billion_boundary() {
    let mut buf = Vec::new();
    write_u32(&mut buf, 2_000_000_000);
    assert_eq!(buf, b"2000000000");
}

#[test]
fn write_u32_max() {
    let mut buf = Vec::new();
    write_u32(&mut buf, u32::MAX); // 4294967295
    assert_eq!(buf, b"4294967295");
}

#[test]
fn write_u32_zero() {
    let mut buf = Vec::new();
    write_u32(&mut buf, 0);
    assert_eq!(buf, b"0");
}

#[test]
fn write_u32_small_values() {
    let mut buf = Vec::new();
    write_u32(&mut buf, 1);
    assert_eq!(buf, b"1");

    buf.clear();
    write_u32(&mut buf, 97); // 'a' codepoint
    assert_eq!(buf, b"97");

    buf.clear();
    write_u32(&mut buf, 999_999_999);
    assert_eq!(buf, b"999999999");
}
