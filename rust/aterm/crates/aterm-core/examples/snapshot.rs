// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Differential-conformance snapshot tool for aterm.
//!
//! Reads a raw ANSI byte stream from stdin, feeds it to a 24x80 `Terminal`,
//! and prints the visible grid as exactly 24 lines (one per row), each row's
//! visible text with trailing whitespace stripped. This is the IDENTICAL
//! snapshot format emitted by the xterm.js side of the harness, so the two
//! grids can be diffed byte-for-byte.

use std::io::Read;

use aterm_core::terminal::Terminal;

fn main() {
    // Read ALL stdin bytes (raw).
    let mut bytes = Vec::new();
    std::io::stdin()
        .read_to_end(&mut bytes)
        .expect("failed to read stdin");

    // 24 rows x 80 cols.
    let mut term = Terminal::new(24, 80);
    term.process(&bytes);

    // Emit exactly 24 lines, one per grid row, trailing whitespace stripped.
    for row in 0..24u16 {
        let line = term.grid().row_text(row).unwrap_or_default();
        println!("{}", line.trim_end());
    }
}
