//! `orca-ffi` — the stable C ABI between the Orca Rust core and the thin native
//! platform wrappers (SwiftUI on macOS, GTK/winit elsewhere).
//!
//! This first surface exposes the headless terminal (`orca-terminal`): a native
//! UI creates a terminal, feeds it PTY output bytes, and reads back grid rows /
//! cursor for rendering. The matching C declarations are in `include/orca.h`.
//!
//! Safety: this is the FFI boundary, so raw pointers and `CString` round-trips
//! require `unsafe`. All other workspace crates forbid unsafe; it is confined
//! here and each `unsafe fn` documents its contract.

use orca_session::{PtyCommand, TerminalSession};
use orca_terminal::{Color, HeadlessTerminal};
use std::ffi::{CStr, CString};
use std::os::raw::c_char;

/// A terminal color over the ABI: `kind` 0=default, 1=indexed (`index`),
/// 2=truecolor (`r`,`g`,`b`).
#[repr(C)]
pub struct OrcaColor {
    pub kind: u8,
    pub index: u8,
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

/// A grid cell for native rendering: the scalar char + SGR attributes.
#[repr(C)]
pub struct OrcaCell {
    pub ch: u32,
    pub bold: u8,
    pub italic: u8,
    pub underline: u8,
    pub inverse: u8,
    pub fg: OrcaColor,
    pub bg: OrcaColor,
}

impl OrcaColor {
    fn from_color(color: Color) -> Self {
        match color {
            Color::Default => OrcaColor { kind: 0, index: 0, r: 0, g: 0, b: 0 },
            Color::Indexed(index) => OrcaColor { kind: 1, index, r: 0, g: 0, b: 0 },
            Color::Rgb(r, g, b) => OrcaColor { kind: 2, index: 0, r, g, b },
        }
    }
}

impl OrcaCell {
    fn blank() -> Self {
        OrcaCell {
            ch: u32::from(' '),
            bold: 0,
            italic: 0,
            underline: 0,
            inverse: 0,
            fg: OrcaColor::from_color(Color::Default),
            bg: OrcaColor::from_color(Color::Default),
        }
    }
}

/// Crate version as a static C string (caller must NOT free).
#[no_mangle]
pub extern "C" fn orca_ffi_version() -> *const c_char {
    // Embeds a trailing NUL at compile time; 'static lifetime, never freed.
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr() as *const c_char
}

/// Create a headless terminal of `rows` x `cols`. Free with
/// [`orca_terminal_free`].
#[no_mangle]
pub extern "C" fn orca_terminal_new(rows: usize, cols: usize) -> *mut HeadlessTerminal {
    Box::into_raw(Box::new(HeadlessTerminal::new(rows, cols)))
}

/// Free a terminal created by [`orca_terminal_new`].
///
/// # Safety
/// `terminal` must be a pointer returned by `orca_terminal_new` and not already
/// freed. After this call the pointer is dangling.
#[no_mangle]
pub unsafe extern "C" fn orca_terminal_free(terminal: *mut HeadlessTerminal) {
    if !terminal.is_null() {
        drop(Box::from_raw(terminal));
    }
}

/// Feed `len` bytes of PTY output through the parser into the grid.
///
/// # Safety
/// `terminal` must be valid; `bytes` must point to at least `len` readable bytes
/// (or be null with `len == 0`).
#[no_mangle]
pub unsafe extern "C" fn orca_terminal_process(
    terminal: *mut HeadlessTerminal,
    bytes: *const u8,
    len: usize,
) {
    if terminal.is_null() || bytes.is_null() || len == 0 {
        return;
    }
    let terminal = &mut *terminal;
    terminal.process(std::slice::from_raw_parts(bytes, len));
}

/// Return row `row`'s text (trailing blanks trimmed) as a heap C string the
/// caller must release with [`orca_string_free`]. Returns null on a bad pointer.
///
/// # Safety
/// `terminal` must be valid.
#[no_mangle]
pub unsafe extern "C" fn orca_terminal_row_text(
    terminal: *const HeadlessTerminal,
    row: usize,
) -> *mut c_char {
    if terminal.is_null() {
        return std::ptr::null_mut();
    }
    let text = (*terminal).row_text(row);
    match CString::new(text) {
        Ok(cstring) => cstring.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Write the cursor position into `out_row`/`out_col` (either may be null).
///
/// # Safety
/// `terminal` must be valid; `out_row`/`out_col` must be writable or null.
#[no_mangle]
pub unsafe extern "C" fn orca_terminal_cursor(
    terminal: *const HeadlessTerminal,
    out_row: *mut usize,
    out_col: *mut usize,
) {
    if terminal.is_null() {
        return;
    }
    let (row, col) = (*terminal).cursor();
    if !out_row.is_null() {
        *out_row = row;
    }
    if !out_col.is_null() {
        *out_col = col;
    }
}

/// The cell at `(row, col)` with its char + SGR attributes, for native
/// rendering. Out-of-bounds or null → a blank default cell.
///
/// # Safety
/// `terminal` must be valid.
#[no_mangle]
pub unsafe extern "C" fn orca_terminal_cell(
    terminal: *const HeadlessTerminal,
    row: usize,
    col: usize,
) -> OrcaCell {
    if terminal.is_null() {
        return OrcaCell::blank();
    }
    match (*terminal).cell(row, col) {
        Some(cell) => cell_to_orca(cell),
        None => OrcaCell::blank(),
    }
}

fn cell_to_orca(cell: orca_terminal::Cell) -> OrcaCell {
    OrcaCell {
        ch: u32::from(cell.ch),
        bold: cell.attrs.bold as u8,
        italic: cell.attrs.italic as u8,
        underline: cell.attrs.underline as u8,
        inverse: cell.attrs.inverse as u8,
        fg: OrcaColor::from_color(cell.attrs.fg),
        bg: OrcaColor::from_color(cell.attrs.bg),
    }
}

/// Write the grid dimensions into `out_rows`/`out_cols` (either may be null).
///
/// # Safety
/// `terminal` must be valid; out-params writable or null.
#[no_mangle]
pub unsafe extern "C" fn orca_terminal_size(
    terminal: *const HeadlessTerminal,
    out_rows: *mut usize,
    out_cols: *mut usize,
) {
    if terminal.is_null() {
        return;
    }
    let (rows, cols) = (*terminal).size();
    if !out_rows.is_null() {
        *out_rows = rows;
    }
    if !out_cols.is_null() {
        *out_cols = cols;
    }
}

/// Resize the terminal grid.
///
/// # Safety
/// `terminal` must be valid.
#[no_mangle]
pub unsafe extern "C" fn orca_terminal_resize(
    terminal: *mut HeadlessTerminal,
    rows: usize,
    cols: usize,
) {
    if terminal.is_null() {
        return;
    }
    (*terminal).resize(rows, cols);
}

/// Free a string returned by an `orca_*` function (e.g. row text).
///
/// # Safety
/// `string` must be a pointer returned by this library and not already freed.
#[no_mangle]
pub unsafe extern "C" fn orca_string_free(string: *mut c_char) {
    if !string.is_null() {
        drop(CString::from_raw(string));
    }
}

// ───────────────────────── Live terminal session ─────────────────────────
// PTY + headless terminal: spawn a command, output streams into the grid.

/// Spawn `program` with `args` (`argc` entries) in a PTY of `rows`×`cols`; the
/// child's output streams into the session's terminal. Free with
/// `orca_session_free`. Null on spawn failure.
///
/// # Safety
/// `program` is a valid C string; `args` points to `argc` C-string pointers
/// (may be null when `argc == 0`).
#[no_mangle]
pub unsafe extern "C" fn orca_session_spawn(
    program: *const c_char,
    args: *const *const c_char,
    argc: usize,
    rows: usize,
    cols: usize,
) -> *mut TerminalSession {
    if program.is_null() {
        return std::ptr::null_mut();
    }
    let program = CStr::from_ptr(program).to_string_lossy().into_owned();
    let mut arg_vec = Vec::with_capacity(argc);
    if !args.is_null() {
        for i in 0..argc {
            let arg = *args.add(i);
            if !arg.is_null() {
                arg_vec.push(CStr::from_ptr(arg).to_string_lossy().into_owned());
            }
        }
    }
    let command = PtyCommand { program, args: arg_vec, cwd: None, env: Vec::new() };
    match TerminalSession::spawn(&command, rows as u16, cols as u16) {
        Ok(session) => Box::into_raw(Box::new(session)),
        Err(_) => std::ptr::null_mut(),
    }
}

/// # Safety
/// `session` must be from `orca_session_spawn` and not already freed.
#[no_mangle]
pub unsafe extern "C" fn orca_session_free(session: *mut TerminalSession) {
    if !session.is_null() {
        drop(Box::from_raw(session));
    }
}

/// Wait for the child to exit and all output to drain.
///
/// # Safety
/// `session` must be valid.
#[no_mangle]
pub unsafe extern "C" fn orca_session_wait(session: *mut TerminalSession) {
    if !session.is_null() {
        (*session).wait();
    }
}

/// Send input bytes to the session's PTY.
///
/// # Safety
/// `session` must be valid; `bytes` has `len` readable bytes (or null/len 0).
#[no_mangle]
pub unsafe extern "C" fn orca_session_write(session: *const TerminalSession, bytes: *const u8, len: usize) {
    if session.is_null() || bytes.is_null() || len == 0 {
        return;
    }
    let _ = (*session).write(std::slice::from_raw_parts(bytes, len));
}

/// # Safety
/// `session` must be valid.
#[no_mangle]
pub unsafe extern "C" fn orca_session_resize(session: *const TerminalSession, rows: usize, cols: usize) {
    if session.is_null() {
        return;
    }
    let _ = (*session).resize(rows as u16, cols as u16);
}

/// # Safety
/// `session` must be valid; out-params writable or null.
#[no_mangle]
pub unsafe extern "C" fn orca_session_size(session: *const TerminalSession, out_rows: *mut usize, out_cols: *mut usize) {
    if session.is_null() {
        return;
    }
    let (rows, cols) = (*session).size();
    if !out_rows.is_null() {
        *out_rows = rows;
    }
    if !out_cols.is_null() {
        *out_cols = cols;
    }
}

/// # Safety
/// `session` must be valid; out-params writable or null.
#[no_mangle]
pub unsafe extern "C" fn orca_session_cursor(session: *const TerminalSession, out_row: *mut usize, out_col: *mut usize) {
    if session.is_null() {
        return;
    }
    let (row, col) = (*session).cursor();
    if !out_row.is_null() {
        *out_row = row;
    }
    if !out_col.is_null() {
        *out_col = col;
    }
}

/// Row text; free with `orca_string_free`.
///
/// # Safety
/// `session` must be valid.
#[no_mangle]
pub unsafe extern "C" fn orca_session_row_text(session: *const TerminalSession, row: usize) -> *mut c_char {
    if session.is_null() {
        return std::ptr::null_mut();
    }
    match CString::new((*session).row_text(row)) {
        Ok(cstring) => cstring.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Cell for native rendering.
///
/// # Safety
/// `session` must be valid.
#[no_mangle]
pub unsafe extern "C" fn orca_session_cell(session: *const TerminalSession, row: usize, col: usize) -> OrcaCell {
    if session.is_null() {
        return OrcaCell::blank();
    }
    match (*session).cell(row, col) {
        Some(cell) => cell_to_orca(cell),
        None => OrcaCell::blank(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CStr;

    /// Round-trip the C ABI exactly as a native wrapper would call it.
    #[test]
    fn terminal_ffi_round_trip() {
        unsafe {
            let term = orca_terminal_new(24, 80);
            assert!(!term.is_null());

            let input = b"hi\r\nthere";
            orca_terminal_process(term, input.as_ptr(), input.len());

            let row0 = orca_terminal_row_text(term, 0);
            let row1 = orca_terminal_row_text(term, 1);
            assert_eq!(CStr::from_ptr(row0).to_str().unwrap(), "hi");
            assert_eq!(CStr::from_ptr(row1).to_str().unwrap(), "there");
            orca_string_free(row0);
            orca_string_free(row1);

            let mut row = 0usize;
            let mut col = 0usize;
            orca_terminal_cursor(term, &mut row, &mut col);
            assert_eq!((row, col), (1, 5));

            orca_terminal_resize(term, 40, 100);
            // null out-params must be tolerated
            orca_terminal_cursor(term, std::ptr::null_mut(), std::ptr::null_mut());

            orca_terminal_free(term);
        }
    }

    #[test]
    fn cell_and_size_expose_rendering_data() {
        unsafe {
            let term = orca_terminal_new(24, 80);
            let input = b"\x1b[1;38;2;10;20;30mX";
            orca_terminal_process(term, input.as_ptr(), input.len());

            let cell = orca_terminal_cell(term, 0, 0);
            assert_eq!(cell.ch, u32::from('X'));
            assert_eq!(cell.bold, 1);
            assert_eq!(cell.fg.kind, 2); // truecolor
            assert_eq!((cell.fg.r, cell.fg.g, cell.fg.b), (10, 20, 30));

            let mut rows = 0usize;
            let mut cols = 0usize;
            orca_terminal_size(term, &mut rows, &mut cols);
            assert_eq!((rows, cols), (24, 80));

            // Out-of-bounds → blank default cell.
            let blank = orca_terminal_cell(term, 1000, 1000);
            assert_eq!(blank.ch, u32::from(' '));
            assert_eq!(blank.fg.kind, 0);

            orca_terminal_free(term);
        }
    }

    #[test]
    fn null_pointers_are_tolerated() {
        unsafe {
            orca_terminal_process(std::ptr::null_mut(), std::ptr::null(), 0);
            assert!(orca_terminal_row_text(std::ptr::null(), 0).is_null());
            orca_terminal_free(std::ptr::null_mut());
            orca_string_free(std::ptr::null_mut());
        }
    }

    #[test]
    #[cfg(unix)]
    fn session_ffi_spawns_pty_and_renders_grid() {
        unsafe {
            let program = CString::new("/bin/sh").unwrap();
            let arg_c = CString::new("-c").unwrap();
            let arg_script = CString::new("printf live-ffi-session").unwrap();
            let args = [arg_c.as_ptr(), arg_script.as_ptr()];
            let session = orca_session_spawn(program.as_ptr(), args.as_ptr(), 2, 24, 80);
            assert!(!session.is_null());

            orca_session_wait(session); // child exits, output drains
            let row = orca_session_row_text(session, 0);
            let text = CStr::from_ptr(row).to_str().unwrap().to_owned();
            orca_string_free(row);
            assert!(text.contains("live-ffi-session"), "got: {text:?}");

            orca_session_free(session);
        }
    }

    #[test]
    fn version_is_a_valid_c_string() {
        unsafe {
            let version = CStr::from_ptr(orca_ffi_version()).to_str().unwrap();
            assert_eq!(version, env!("CARGO_PKG_VERSION"));
        }
    }
}
