// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors
//
//! End-to-end scrollback CONTENT integrity under load.
//!
//! The scrollback unit tests cover error propagation (`None` on a corrupt/failed
//! backend) and the lz4 layer has its own round-trip + fuzz tests, but nothing
//! drove the full engine path — emit many lines → scroll them off into history
//! (through the hot ring and, past its capacity, the lz4 cold tier) → read them
//! back BY INDEX — and asserted the exact text survived. This pins that: a
//! corruption or off-by-one in the ring / materialization / cold-tier round-trip
//! (a class that has produced real bugs here) now fails a test instead of
//! silently mangling a user's history.

use aterm_core::terminal::Terminal;

#[test]
fn scrollback_preserves_line_content_in_order() {
    // Distinctly-numbered lines so each is self-identifying. The default
    // scrollback limit is 100k, so all 5000 are retained (no eviction) and
    // history index 0 == oldest == the first emitted line, contiguous upward.
    let mut term = Terminal::new(24, 80);
    const N: usize = 5000;
    let mut input = Vec::with_capacity(N * 8);
    for i in 0..N {
        input.extend_from_slice(format!("L{i}\r\n").as_bytes());
    }
    term.process(&input);

    let h = term.grid().scrollback_lines();
    // Nearly all of them scrolled off the 24-row screen into history.
    assert!(h >= N - 40, "expected ~{} history lines, got {h}", N - 24);

    // Read a spread of indices — including the oldest (0, likely cold-tiered)
    // and the newest history line (h-1) — and assert the exact content.
    for k in [0usize, 1, 100, 2500, h - 1] {
        let line = term
            .grid()
            .get_history_line(k)
            .unwrap_or_else(|| panic!("history line {k} missing (history len {h})"));
        assert_eq!(
            line.to_string().trim_end(),
            format!("L{k}"),
            "history line {k} content corrupted (history len {h})"
        );
    }
}
