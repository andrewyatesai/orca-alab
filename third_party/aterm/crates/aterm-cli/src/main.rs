// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! `aterm` — a transparent, introspecting terminal (U1).
//!
//! It spawns your `$SHELL` in a PTY and passes I/O through **unchanged**, so it
//! looks and behaves exactly like your shell — while feeding every output byte
//! into the aterm VT engine, which builds the live, introspectable model the
//! kernel (and an intelligence) reads. No re-rendering: the host terminal draws
//! the bytes; the engine runs alongside.
//!
//! A thin `libc` driver (forkpty + raw mode + poll) with no heavyweight deps.
//! Daily-driver essentials are handled: window resize is forwarded (SIGWINCH ->
//! PTY + engine), the loop is signal-robust (EINTR), and aterm exits with the
//! shell's own status. The capability/sandbox layer (§5) and the native read
//! API land in later slices.

use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};

use aterm_core::terminal::Terminal;

/// Set by the SIGWINCH handler; drained in the main loop.
static GOT_WINCH: AtomicBool = AtomicBool::new(false);

extern "C" fn on_winch(_sig: libc::c_int) {
    GOT_WINCH.store(true, Ordering::Relaxed);
}

/// Ask the controlling terminal for its size; fall back to 24x80.
fn host_winsize() -> libc::winsize {
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    let ok = unsafe { libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut ws) } == 0;
    if !ok || ws.ws_row == 0 || ws.ws_col == 0 {
        ws.ws_row = 24;
        ws.ws_col = 80;
    }
    ws
}

fn set_raw(fd: libc::c_int) -> libc::termios {
    unsafe {
        let mut t: libc::termios = std::mem::zeroed();
        libc::tcgetattr(fd, &mut t);
        let orig = t;
        libc::cfmakeraw(&mut t);
        libc::tcsetattr(fd, libc::TCSANOW, &t);
        orig
    }
}

fn restore(fd: libc::c_int, t: &libc::termios) {
    unsafe {
        libc::tcsetattr(fd, libc::TCSANOW, t);
    }
}

fn write_all(fd: libc::c_int, mut data: &[u8]) {
    while !data.is_empty() {
        let r = unsafe { libc::write(fd, data.as_ptr() as *const libc::c_void, data.len()) };
        if r <= 0 {
            if std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            break;
        }
        data = &data[r as usize..];
    }
}

fn eintr() -> bool {
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR)
}

fn main() {
    let mut ws = host_winsize();
    let (rows, cols) = (ws.ws_row, ws.ws_col);

    let mut master: libc::c_int = -1;
    let pid =
        unsafe { libc::forkpty(&mut master, ptr::null_mut(), ptr::null_mut(), &mut ws) };
    if pid < 0 {
        eprintln!("aterm: forkpty failed");
        std::process::exit(1);
    }
    if pid == 0 {
        // CHILD — exec the user's shell.
        let shell = std::env::var_os("SHELL").unwrap_or_else(|| "/bin/sh".into());
        let cshell =
            CString::new(shell.as_bytes()).unwrap_or_else(|_| CString::new("/bin/sh").unwrap());
        let argv = [cshell.as_ptr(), ptr::null()];
        unsafe {
            libc::execvp(cshell.as_ptr(), argv.as_ptr());
            libc::_exit(127);
        }
    }

    // PARENT.
    let stdin_is_tty = unsafe { libc::isatty(libc::STDIN_FILENO) } == 1;
    let orig = if stdin_is_tty { Some(set_raw(libc::STDIN_FILENO)) } else { None };
    // Cast through a function pointer (not a direct fn-item-to-int cast) so the
    // `fn_to_numeric_cast` lint is satisfied while still yielding the address
    // libc::signal expects as its sighandler_t.
    unsafe { libc::signal(libc::SIGWINCH, on_winch as extern "C" fn(libc::c_int) as usize) };

    let mut engine = Terminal::new(rows, cols);
    let mut bytes_in: u64 = 0;

    let mut fds = [
        libc::pollfd { fd: libc::STDIN_FILENO, events: libc::POLLIN, revents: 0 },
        libc::pollfd { fd: master, events: libc::POLLIN, revents: 0 },
    ];
    let mut buf = [0u8; 8192];

    loop {
        // Apply a pending resize before blocking: tell the PTY (so full-screen
        // apps reflow) and the engine model.
        if GOT_WINCH.swap(false, Ordering::Relaxed) {
            let mut nws = host_winsize();
            unsafe { libc::ioctl(master, libc::TIOCSWINSZ, &mut nws) };
            engine.resize(nws.ws_row, nws.ws_col);
        }

        let n = unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) };
        if n < 0 {
            if eintr() {
                continue; // a signal (e.g. SIGWINCH) — loop and apply it
            }
            break;
        }

        // host keystrokes -> the shell.
        if fds[0].revents & libc::POLLIN != 0 {
            let r = unsafe {
                libc::read(libc::STDIN_FILENO, buf.as_mut_ptr() as *mut libc::c_void, buf.len())
            };
            if r < 0 && eintr() {
                // retry next iteration
            } else if r <= 0 {
                fds[0].fd = -1; // input closed; let the shell run to its own exit
            } else {
                write_all(master, &buf[..r as usize]);
            }
        }

        // shell output -> host terminal (passthrough) AND the engine (model).
        if fds[1].revents & (libc::POLLIN | libc::POLLHUP) != 0 {
            let r =
                unsafe { libc::read(master, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
            if r < 0 && eintr() {
                continue;
            }
            if r <= 0 {
                break; // shell exited / PTY closed
            }
            let out = &buf[..r as usize];
            write_all(libc::STDOUT_FILENO, out);
            engine.process(out);
            bytes_in += out.len() as u64;
        }
    }

    if let Some(t) = orig {
        restore(libc::STDIN_FILENO, &t);
    }
    let mut status = 0;
    unsafe {
        libc::close(master);
        libc::waitpid(pid, &mut status, 0);
    }
    if std::env::var_os("ATERM_VERBOSE").is_some() {
        eprintln!("\r\n[aterm] session ended — engine processed {bytes_in} bytes via the VT core.");
    }
    let code = if libc::WIFEXITED(status) { libc::WEXITSTATUS(status) } else { 1 };
    std::process::exit(code);
}
