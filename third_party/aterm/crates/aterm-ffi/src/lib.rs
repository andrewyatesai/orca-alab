// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! C ABI embedding surface for the aterm engine (ATERM_DESIGN WS-D).
//!
//! The whole point of aterm is reliable screen reading; this exposes that to any
//! language with a C FFI as the canonical `feed bytes -> read the screen` loop,
//! over an opaque engine handle:
//!
//! ```c
//! AtermEngine* e = aterm_engine_new(24, 80);
//! aterm_engine_feed(e, input, input_len);
//! char* screen = aterm_engine_visible_content(e);   // NUL-terminated UTF-8
//! /* ... use screen ... */
//! aterm_string_free(screen);
//! aterm_engine_free(e);
//! ```
//!
//! All `unsafe` lives behind these wrappers; each function null-checks its handle.
//! Strings returned by this library are owned by the caller and MUST be released
//! with [`aterm_string_free`]; engines with [`aterm_engine_free`].

use std::ffi::{c_char, CString};

use aterm_core::terminal::Terminal;

/// Opaque engine handle. The C side treats `*mut Terminal` as `AtermEngine*`.
pub use aterm_core::terminal::Terminal as Engine;

/// Create a new engine of `rows`×`cols`. Free with [`aterm_engine_free`].
#[unsafe(no_mangle)]
pub extern "C" fn aterm_engine_new(rows: u16, cols: u16) -> *mut Terminal {
    Box::into_raw(Box::new(Terminal::new(rows, cols)))
}

/// Feed `len` VT bytes at `ptr` to the engine. No-op on a null handle or pointer.
///
/// # Safety
/// `engine` must be a live handle from [`aterm_engine_new`]; `ptr` must point to
/// at least `len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aterm_engine_feed(engine: *mut Terminal, ptr: *const u8, len: usize) {
    if engine.is_null() || ptr.is_null() {
        return;
    }
    // SAFETY: caller contract — `engine` is a live handle, `ptr`/`len` a valid
    // readable region.
    let term = unsafe { &mut *engine };
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    term.process(bytes);
}

/// Resize the engine grid. No-op on a null handle.
///
/// # Safety
/// `engine` must be a live handle from [`aterm_engine_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aterm_engine_resize(engine: *mut Terminal, rows: u16, cols: u16) {
    if engine.is_null() {
        return;
    }
    // SAFETY: caller contract — live handle.
    unsafe { &mut *engine }.resize(rows, cols);
}

/// The visible screen as a newly-allocated NUL-terminated UTF-8 C string. Free
/// with [`aterm_string_free`]. Returns null on a null handle.
///
/// # Safety
/// `engine` must be a live handle from [`aterm_engine_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aterm_engine_visible_content(engine: *const Terminal) -> *mut c_char {
    if engine.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: caller contract — live handle (shared read).
    let s = unsafe { &*engine }.visible_content();
    to_c_string(s)
}

/// Row `row`'s text as a newly-allocated NUL-terminated UTF-8 C string. Free with
/// [`aterm_string_free`]. Returns null on a null handle or out-of-range row.
///
/// # Safety
/// `engine` must be a live handle from [`aterm_engine_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aterm_engine_row_text(engine: *const Terminal, row: usize) -> *mut c_char {
    if engine.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: caller contract — live handle (shared read).
    match unsafe { &*engine }.row_text(row) {
        Some(s) => to_c_string(s),
        None => std::ptr::null_mut(),
    }
}

/// Free a string previously returned by this library. No-op on null.
///
/// # Safety
/// `s` must be a pointer returned by one of this library's string functions and
/// not already freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aterm_string_free(s: *mut c_char) {
    if !s.is_null() {
        // SAFETY: caller contract — `s` came from `CString::into_raw` here.
        drop(unsafe { CString::from_raw(s) });
    }
}

/// Free an engine handle. No-op on null.
///
/// # Safety
/// `engine` must be a handle from [`aterm_engine_new`] and not already freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aterm_engine_free(engine: *mut Terminal) {
    if !engine.is_null() {
        // SAFETY: caller contract — `engine` came from `Box::into_raw` here.
        drop(unsafe { Box::from_raw(engine) });
    }
}

/// Convert an owned `String` to a heap C string. Interior NULs (impossible in a C
/// string) are stripped so the conversion never fails; the screen text is UTF-8.
fn to_c_string(s: String) -> *mut c_char {
    let cleaned = if s.as_bytes().contains(&0) { s.replace('\0', "") } else { s };
    CString::new(cleaned).map_or(std::ptr::null_mut(), CString::into_raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Exercises the whole C ABI round-trip from Rust: create -> feed an escape
    // sequence + text -> read the screen -> free. This is exactly what a C
    // embedder does, so passing it validates the surface end-to-end.
    #[test]
    fn feed_then_read_screen_roundtrips() {
        unsafe {
            let eng = aterm_engine_new(24, 80);
            assert!(!eng.is_null());

            // SGR bold + plain text; the engine should put "helloworld" on row 0.
            let input = b"hello\x1b[1mworld";
            aterm_engine_feed(eng, input.as_ptr(), input.len());

            let screen = aterm_engine_visible_content(eng);
            assert!(!screen.is_null());
            let text = std::ffi::CStr::from_ptr(screen).to_string_lossy().into_owned();
            assert!(text.contains("hello"), "screen was: {text:?}");
            assert!(text.contains("world"), "screen was: {text:?}");
            aterm_string_free(screen);

            let row0 = aterm_engine_row_text(eng, 0);
            assert!(!row0.is_null());
            let r0 = std::ffi::CStr::from_ptr(row0).to_string_lossy().into_owned();
            assert!(r0.contains("helloworld"), "row 0 was: {r0:?}");
            aterm_string_free(row0);

            // Out-of-range row -> null, not a crash.
            assert!(aterm_engine_row_text(eng, 9999).is_null());

            aterm_engine_free(eng);
        }
    }

    #[test]
    fn null_handles_are_safe() {
        unsafe {
            aterm_engine_feed(std::ptr::null_mut(), b"x".as_ptr(), 1); // no-op
            aterm_engine_resize(std::ptr::null_mut(), 1, 1); // no-op
            assert!(aterm_engine_visible_content(std::ptr::null()).is_null());
            aterm_string_free(std::ptr::null_mut()); // no-op
            aterm_engine_free(std::ptr::null_mut()); // no-op
        }
    }
}
