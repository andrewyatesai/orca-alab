// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! Async-signal-safe capture of native fatal signals (M6 "CRASH-CORE").
//!
//! The panic hook in [`crate::logging`] only fires for Rust unwinds. Native
//! fatal signals — `SIGSEGV`, `SIGABRT`, `SIGBUS`, `SIGILL`, `SIGFPE` — never
//! run the panic machinery: they jump straight to the kernel-installed
//! disposition and the process is gone, with nothing on disk for a windowed
//! app whose stderr nobody reads. This module installs a `sigaction`(2)
//! handler that drops a crash *marker* before the process dies, so an
//! after-the-fact "did aterm take a signal, and which one?" question has an
//! artifact to point at.
//!
//! ASYNC-SIGNAL-SAFETY (the whole reason this is its own module):
//! a signal handler may interrupt the program at *any* instruction, including
//! the middle of `malloc`, a `Mutex` critical section, or libc's own buffers.
//! It may therefore call ONLY async-signal-safe functions (POSIX
//! `signal-safety(7)`): here that is `write(2)`, `sigaction(2)`, and
//! `raise(3)`. Everything that is NOT signal-safe — resolving the marker path,
//! `open`ing it, allocating, `format!` — is done ONCE AT INSTALL TIME and the
//! results are parked in `static`s. The handler itself:
//!   * does no allocation (the integer is formatted into a stack buffer with a
//!     hand-rolled, allocation-free decimal helper),
//!   * takes no lock,
//!   * calls no `format!`/`String`/`println!`,
//!   * writes a pre-built banner + the signal number to `STDERR_FILENO` and to
//!     a pre-opened marker fd, then
//!   * restores the signal's default disposition and `raise()`s it, so the
//!     process still core-dumps / exits with the conventional status.

/// Arm async-signal-safe fatal-signal capture (`SIGSEGV`/`SIGABRT`/`SIGBUS`/
/// `SIGILL`/`SIGFPE`). Call alongside the panic hook so both crash paths are
/// covered. No-op on non-unix targets so the call site stays portable.
pub fn install_signal_handlers() {
    #[cfg(unix)]
    imp::install_signal_handlers();
}

#[cfg(unix)]
mod imp {
    use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

    /// Fatal signals we trap. Each bypasses the Rust panic hook entirely, so
    /// without this handler they leave no on-disk trace for a windowed app.
    const FATAL_SIGNALS: [i32; 5] = [
        libc::SIGSEGV,
        libc::SIGABRT,
        libc::SIGBUS,
        libc::SIGILL,
        libc::SIGFPE,
    ];

    /// Static, pre-built banner halves written around the formatted signal
    /// number. Keeping these as `&[u8]` constants means the handler does no
    /// string work at signal time — it just `write(2)`s known bytes. The
    /// suffix's leading byte is a space; `\u{2014}` is the em dash (UTF-8
    /// `E2 80 94`).
    const BANNER_PREFIX: &[u8] = b"aterm: fatal signal ";
    const BANNER_SUFFIX: &[u8] = " \u{2014} crash marker written\n".as_bytes();

    /// fd of the pre-opened crash-marker file, or `-1` when none was opened
    /// (no writable private dir at install time). `AtomicI32` so the handler
    /// reads it without a lock; written once during `install`.
    static MARKER_FD: AtomicI32 = AtomicI32::new(-1);

    /// Tripped once we have armed the `sigaction` handlers, so a second
    /// `install` call (defensive) does not re-open the marker fd.
    static ARMED: AtomicBool = AtomicBool::new(false);

    /// Maximum decimal digits a `u32` ever needs (`4294967295`). A signal
    /// number is far smaller, but sizing for `u32` keeps the helper reusable
    /// and the buffer trivially large enough.
    const U32_MAX_DIGITS: usize = 10;

    /// Format `value` as decimal ASCII into the tail of `buf`, returning the
    /// filled sub-slice. ALLOCATION-FREE and async-signal-safe: it only writes
    /// bytes into the caller's stack buffer, so it is callable from the signal
    /// handler. Digits are produced least-significant-first from the end of the
    /// buffer, then the populated tail slice is returned. Zero yields `"0"`.
    fn fmt_u32_decimal(value: u32, buf: &mut [u8; U32_MAX_DIGITS]) -> &[u8] {
        let mut n = value;
        // Index just past the last byte; we fill backwards from here.
        let mut pos = buf.len();
        loop {
            pos -= 1;
            buf[pos] = b'0' + (n % 10) as u8;
            n /= 10;
            if n == 0 {
                break;
            }
        }
        &buf[pos..]
    }

    /// Async-signal-safe `write(2)` of `bytes` to `fd`, looping over short and
    /// `EINTR`-interrupted writes. Returns once everything is written or a
    /// non-retryable error is seen. Errors are swallowed: a crash handler that
    /// cannot write its marker must still fall through to re-raising the signal.
    fn write_all_signal_safe(fd: i32, bytes: &[u8]) {
        if fd < 0 {
            return;
        }
        let mut off = 0usize;
        while off < bytes.len() {
            // SAFETY: `bytes[off..]` is a live slice for the duration of the
            // call; `fd` is checked non-negative above. `write` is on the POSIX
            // async-signal-safe list, so calling it from a signal handler is
            // sound. A short (`n >= 0`) or error (`n < 0`) return is handled
            // below; we never deref the return as a pointer.
            let n = unsafe {
                libc::write(
                    fd,
                    bytes[off..].as_ptr().cast::<libc::c_void>(),
                    bytes.len() - off,
                )
            };
            if n > 0 {
                off += n as usize;
                continue;
            }
            // n == 0 (nothing written) or n < 0 (error). Retry only on EINTR;
            // otherwise give up so we still re-raise the signal. `errno` is a
            // thread-local read, which is async-signal-safe.
            if n < 0 {
                // SAFETY: `errno_location()` returns a valid, thread-local
                // `*mut c_int`; reading it on this thread is sound and
                // async-signal-safe.
                let err = unsafe { *errno_location() };
                if err == libc::EINTR {
                    continue;
                }
            }
            break;
        }
    }

    /// Pointer to the calling thread's `errno` (`__error` on macOS/BSD,
    /// `__errno_location` elsewhere). Both are async-signal-safe.
    ///
    /// # Safety
    /// The returned pointer is valid for the calling thread only; deref it on
    /// that thread.
    #[inline]
    unsafe fn errno_location() -> *mut libc::c_int {
        #[cfg(target_os = "macos")]
        {
            // SAFETY: `__error` returns this thread's errno address.
            unsafe { libc::__error() }
        }
        #[cfg(not(target_os = "macos"))]
        {
            // SAFETY: `__errno_location` returns this thread's errno address.
            unsafe { libc::__errno_location() }
        }
    }

    /// The fatal-signal handler. Installed without `SA_SIGINFO`, so it receives
    /// only the signal number. EVERYTHING here is async-signal-safe: a stack
    /// buffer, the allocation-free decimal helper, `write(2)`, `sigaction(2)`
    /// to restore the default disposition, and `raise(3)`.
    extern "C" fn handle_fatal_signal(sig: libc::c_int) {
        // Format the (non-negative) signal number with no allocation.
        let mut digits = [0u8; U32_MAX_DIGITS];
        let num = fmt_u32_decimal(sig.max(0) as u32, &mut digits);

        // Banner -> STDERR, then the same banner -> the pre-opened marker fd.
        let marker_fd = MARKER_FD.load(Ordering::Relaxed);
        for fd in [libc::STDERR_FILENO, marker_fd] {
            write_all_signal_safe(fd, BANNER_PREFIX);
            write_all_signal_safe(fd, num);
            write_all_signal_safe(fd, BANNER_SUFFIX);
        }

        // Restore the default disposition for THIS signal and re-raise it, so
        // the process dies the conventional way (core dump / exit status),
        // exactly as if we had never trapped it.
        //
        // SAFETY: `act` is a fully-zeroed, correctly-sized `sigaction` with
        // `sa_sigaction = SIG_DFL`; `sigaction`/`raise` are async-signal-safe.
        // We ignore the return values: if restoring fails we still `raise`,
        // and a handler must not unwind, so there is nothing to propagate.
        unsafe {
            let mut act: libc::sigaction = std::mem::zeroed();
            act.sa_sigaction = libc::SIG_DFL;
            libc::sigaction(sig, &act, std::ptr::null_mut());
            libc::raise(sig);
        }
    }

    /// Open the crash-marker file `0600` (truncating any prior marker from this
    /// pid) and return its raw fd, or `-1` when no private dir is available.
    /// Done AT INSTALL TIME — `open`/path work is not signal-safe.
    fn open_marker_fd() -> i32 {
        use std::os::unix::fs::OpenOptionsExt;
        use std::os::unix::io::IntoRawFd;
        let Some(dir) = crate::logging::log_dir() else {
            return -1;
        };
        let path = dir.join(format!("crash-signal-{}.log", std::process::id()));
        let mut opts = std::fs::OpenOptions::new();
        opts.create(true).write(true).truncate(true).mode(0o600);
        match opts.open(&path) {
            // Leak the `File` into a raw fd: the marker must outlive this
            // function and stay open for the whole process so the handler can
            // write to it. The OS reclaims it at exit.
            Ok(f) => f.into_raw_fd(),
            Err(_) => -1,
        }
    }

    /// Arm async-signal-safe capture of the fatal signals. Idempotent: the
    /// marker fd is opened (and handlers installed) only on the first call.
    pub fn install_signal_handlers() {
        if ARMED.swap(true, Ordering::SeqCst) {
            return; // already armed
        }
        // Pre-open the marker fd now (NOT in the handler — `open` is unsafe in
        // a signal context). A `-1` simply means the handler writes to stderr
        // only.
        MARKER_FD.store(open_marker_fd(), Ordering::SeqCst);

        for &sig in &FATAL_SIGNALS {
            // SAFETY: `act` is a zeroed `sigaction` with a valid function
            // pointer in `sa_sigaction`, an empty (zeroed, then `sigemptyset`)
            // signal mask, and standard flags. `sigaction` is called outside
            // any signal context (install time) with a correctly-shaped struct,
            // so the call is sound. We ignore the result — a failure to arm one
            // signal must not abort startup of the terminal.
            unsafe {
                let mut act: libc::sigaction = std::mem::zeroed();
                // The `sa_sigaction` field is the function-pointer slot; libc
                // types it as `usize`, so cast via a thin pointer (not a direct
                // fn-item-to-int cast, which lints).
                act.sa_sigaction = handle_fatal_signal as *const () as usize;
                // SA_RESTART so syscalls we trap *out of* are restarted where
                // possible. SA_NODEFER is intentionally NOT set, so the signal
                // stays masked while our handler runs.
                act.sa_flags = libc::SA_RESTART;
                libc::sigemptyset(&mut act.sa_mask);
                libc::sigaction(sig, &act, std::ptr::null_mut());
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// Render `value` via the allocation-free helper into a `String` for
        /// easy comparison against `to_string`.
        fn render(value: u32) -> String {
            let mut buf = [0u8; U32_MAX_DIGITS];
            let bytes = fmt_u32_decimal(value, &mut buf);
            String::from_utf8(bytes.to_vec()).unwrap()
        }

        #[test]
        fn decimal_formatter_matches_std_for_many_values() {
            // Exhaustive small range + boundaries + the widest u32 cases.
            for v in 0u32..2000 {
                assert_eq!(render(v), v.to_string(), "mismatch at {v}");
            }
            for v in [
                0,
                1,
                9,
                10,
                11,
                99,
                100,
                255, // max u8 — bigger than any signal number
                999,
                1000,
                65_535,
                1_000_000,
                4_294_967_294,
                u32::MAX, // 4294967295 — the widest 10-digit case
            ] {
                assert_eq!(render(v), v.to_string(), "mismatch at {v}");
            }
        }

        #[test]
        fn decimal_formatter_handles_zero_without_dropping_the_digit() {
            assert_eq!(render(0), "0");
        }

        #[test]
        fn every_fatal_signal_number_formats_within_the_buffer() {
            for &sig in &FATAL_SIGNALS {
                let mut buf = [0u8; U32_MAX_DIGITS];
                let bytes = fmt_u32_decimal(sig as u32, &mut buf);
                assert_eq!(
                    bytes,
                    sig.to_string().as_bytes(),
                    "signal {sig} formatted wrong"
                );
            }
        }

        #[test]
        fn banner_bytes_are_the_expected_marker_text() {
            assert_eq!(BANNER_PREFIX, b"aterm: fatal signal ");
            // Suffix is a leading space + em dash (U+2014, UTF-8 E2 80 94) +
            // text + newline.
            assert_eq!(
                BANNER_SUFFIX,
                &[
                    b' ', 0xE2, 0x80, 0x94, b' ', b'c', b'r', b'a', b's', b'h', b' ', b'm', b'a',
                    b'r', b'k', b'e', b'r', b' ', b'w', b'r', b'i', b't', b't', b'e', b'n', b'\n',
                ]
            );
            // The assembled banner around a sample signal number reads
            // correctly end to end.
            let mut buf = [0u8; U32_MAX_DIGITS];
            let num = fmt_u32_decimal(libc::SIGSEGV as u32, &mut buf);
            let mut line = Vec::new();
            line.extend_from_slice(BANNER_PREFIX);
            line.extend_from_slice(num);
            line.extend_from_slice(BANNER_SUFFIX);
            let text = String::from_utf8(line).unwrap();
            assert_eq!(
                text,
                format!(
                    "aterm: fatal signal {} \u{2014} crash marker written\n",
                    libc::SIGSEGV
                )
            );
        }

        #[test]
        fn install_is_idempotent_and_arms_a_handler_for_sigsegv() {
            // Calling twice must not panic and must not re-open the marker.
            install_signal_handlers();
            install_signal_handlers();

            // Read back the current SIGSEGV disposition with a null `act`; a
            // handler should now be installed (neither SIG_DFL nor SIG_IGN).
            // We do NOT raise the signal.
            //
            // SAFETY: `old` is a zeroed, correctly-sized `sigaction`; passing a
            // null `act` makes `sigaction` a pure query of the current
            // disposition, which it writes into `old`. Called at test (non-
            // signal) time.
            let installed = unsafe {
                let mut old: libc::sigaction = std::mem::zeroed();
                let rc = libc::sigaction(libc::SIGSEGV, std::ptr::null(), &mut old);
                rc == 0 && old.sa_sigaction != libc::SIG_DFL && old.sa_sigaction != libc::SIG_IGN
            };
            assert!(
                installed,
                "SIGSEGV should have a custom handler after install"
            );
        }
    }
}
