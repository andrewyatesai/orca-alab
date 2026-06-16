//! Trust safety harness for the terminal engine's cursor/grid arithmetic.
//!
//! The differential fuzzer (tools/conformance) finds BEHAVIOUR bugs (engine vs
//! xterm). Trust finds the complementary class — SAFETY bugs that *crash* the
//! daemon: integer overflow on cursor/param arithmetic and out-of-bounds grid
//! indexing. Every `+`/`-`/index below is a Trust Level-0 obligation; Trust
//! flags any that can overflow or panic on adversarial PTY input.
//!
//!   tcargo trust check tools/trust-terminal/cursor_arithmetic.rs
//!
//! The `_unsafe` variants are deliberate: Trust reports their arithmetic as a
//! can-panic obligation (`mir_assert::Overflow`). The clamped variants are the
//! shape real engine code must use (saturating arithmetic + bounds checks).
#![cfg_attr(trust_verify, feature(register_tool))]
#![cfg_attr(trust_verify, register_tool(trust))]

pub const ROWS: usize = 24;
pub const COLS: usize = 80;

/// CUD/VPR/print advance — raw `+` overflows on a near-MAX column (Trust flags it).
pub fn advance_unsafe(col: usize, n: usize) -> usize {
    col + n
}

/// Safe advance: saturating add, clamped to the last column.
pub fn advance_clamped(col: usize, n: usize) -> usize {
    let next = col.saturating_add(n);
    if next >= COLS { COLS - 1 } else { next }
}

/// Grid write with no bounds check — out-of-bounds panic if the cursor escaped
/// the grid (the exact crash an unclamped cursor causes). Trust flags the index.
pub fn write_unsafe(grid: &mut [[char; COLS]; ROWS], row: usize, col: usize, ch: char) {
    grid[row][col] = ch;
}

/// Safe grid write: bounds-checked, provably panic-free.
pub fn write_clamped(grid: &mut [[char; COLS]; ROWS], row: usize, col: usize, ch: char) {
    if row < ROWS && col < COLS {
        grid[row][col] = ch;
    }
}

fn main() {
    let mut g = [[' '; COLS]; ROWS];
    write_clamped(&mut g, 1, advance_clamped(0, 1), 'x');
}
