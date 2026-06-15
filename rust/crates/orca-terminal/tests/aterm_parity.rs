//! Parity contract: the aterm-backed `HeadlessTerminal` reproduces every
//! behaviour the previous `vte`-based engine guaranteed through Orca's public
//! surface (the `@xterm/headless` replacement), and upgrades correctness where
//! the old engine was a deliberate subset.
//!
//! Each `parity_*` case is an input/observation pair the old engine's own unit
//! suite asserted; they must hold byte-for-byte on the new engine so `orca-ffi`,
//! `orca-session`, and the native shells are unaffected by the swap. The
//! `upgrade_*` cases exercise behaviour aterm adds (alt-screen, cursor
//! addressing, erase) that the vte subset did not model.

use orca_terminal::{Cell, CellAttrs, Color, HeadlessTerminal, MouseTracking};

fn drive(input: &str) -> HeadlessTerminal {
    let mut term = HeadlessTerminal::new(24, 80);
    term.process_str(input);
    term
}

// ─────────────────────────── parity (must match old engine) ──────────────────

#[test]
fn parity_text_and_crlf_layout() {
    let term = drive("hello\r\nworld");
    assert_eq!(term.row_text(0), "hello");
    assert_eq!(term.row_text(1), "world");
    assert_eq!(term.cursor(), (1, 5));
}

#[test]
fn parity_carriage_return_overwrites() {
    assert_eq!(drive("abc\rX").row_text(0), "Xbc");
}

#[test]
fn parity_backspace_moves_cursor_back() {
    assert_eq!(drive("abc\x08X").row_text(0), "abX");
}

#[test]
fn parity_osc7_cwd_percent_decoded() {
    let term = drive("\x1b]7;file:///Users/me/my%20repo\x07");
    assert_eq!(term.cwd(), Some("/Users/me/my repo"));
}

#[test]
fn parity_sgr_indexed_bold_and_reset() {
    let term = drive("\x1b[1;31mE\x1b[0mN");
    assert_eq!(
        term.cell(0, 0),
        Some(Cell {
            ch: 'E',
            attrs: CellAttrs { bold: true, fg: Color::Indexed(1), ..Default::default() },
        })
    );
    assert_eq!(term.cell(0, 1).unwrap().attrs, CellAttrs::default());
}

#[test]
fn parity_sgr_256_truecolor_and_bright() {
    let term = drive("\x1b[38;5;200mA\x1b[48;2;10;20;30mB\x1b[92mC");
    assert_eq!(term.cell(0, 0).unwrap().attrs.fg, Color::Indexed(200));
    assert_eq!(term.cell(0, 1).unwrap().attrs.bg, Color::Rgb(10, 20, 30));
    assert_eq!(term.cell(0, 2).unwrap().attrs.fg, Color::Indexed(10)); // bright green = 8 + 2
}

#[test]
fn parity_decset_mouse_modes() {
    let mut term = HeadlessTerminal::new(4, 10);
    assert_eq!(term.mouse_tracking(), MouseTracking::None);
    term.process_str("\x1b[?1000h");
    assert_eq!(term.mouse_tracking(), MouseTracking::Normal);
    term.process_str("\x1b[?1003h");
    assert_eq!(term.mouse_tracking(), MouseTracking::Any);
    term.process_str("\x1b[?1006h");
    assert!(term.sgr_mouse());
    term.process_str("\x1b[?1016h");
    assert!(term.sgr_pixels());
}

#[test]
fn parity_snapshot_round_trip() {
    let term = drive("\x1b]7;file:///srv/app\x07first\r\nsecond");
    let snap = term.capture();
    let restored = HeadlessTerminal::from_snapshot(&snap);
    assert_eq!(restored.capture(), snap);
    assert_eq!(restored.row_text(1), "second");
    assert_eq!(restored.cursor(), (1, 6));
    assert_eq!(restored.cwd(), Some("/srv/app"));
}

// ─────────────────────────── upgrades (new on aterm) ─────────────────────────

#[test]
fn upgrade_cursor_addressing_cup() {
    // Absolute cursor positioning (CUP) — not modelled by the vte subset.
    let mut term = HeadlessTerminal::new(5, 10);
    term.process_str("\x1b[3;4Hxy");
    assert_eq!(term.cursor(), (2, 5));
    assert_eq!(term.row_text(2), "   xy");
}

#[test]
fn upgrade_erase_in_line() {
    // EL (erase to end of line) — aterm honours it; the old subset ignored it.
    let mut term = HeadlessTerminal::new(2, 10);
    term.process_str("abcdef\r\x1b[3C\x1b[0K");
    assert_eq!(term.row_text(0), "abc");
}

#[test]
fn upgrade_alt_screen_isolated() {
    // Alternate screen buffer (1049): the main grid is preserved underneath.
    let mut term = HeadlessTerminal::new(3, 10);
    term.process_str("main\x1b[?1049h");
    term.process_str("\x1b[2J\x1b[Halt");
    assert_eq!(term.row_text(0), "alt");
    term.process_str("\x1b[?1049l");
    assert_eq!(term.row_text(0), "main");
}
