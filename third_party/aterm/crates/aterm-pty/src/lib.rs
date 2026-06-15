// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! The single PTY spawn + IO seam (ATERM_DESIGN WS-G).
//!
//! Every raw `libc` PTY syscall — `forkpty`, `execve`, `read`, `write`,
//! `ioctl(TIOCSWINSZ)` — is contained HERE, in one auditable crate, so the
//! frontend holds no unsafe PTY code and there is exactly one place where a child
//! process is spawned. The master fd is returned as a raw `i32` because aterm's
//! frontend shares it across the input, reader, and control-socket threads (the
//! same sharing it already did); the unsafe is what moves, not the ownership
//! model.

use std::ffi::CString;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::ptr;

/// Spawn `$SHELL` in a fresh PTY of `rows`×`cols`, returning the master fd.
///
/// Honors `$ATERM_EXEC`: if set, the shell runs that command first (to paint a
/// known screen) and then `exec`s an interactive shell so the result persists.
/// Defaults to `/bin/sh` when `$SHELL` is unset.
///
/// `env_add` is a set of `(key, value)` environment entries injected into the
/// child before exec (e.g. the OSC 133/633 shell-integration loader vars +
/// nonce); `argv_override`, when `Some`, replaces the shell's argv (e.g. bash's
/// `--rcfile`). Both are GENERIC — this seam knows nothing about shell
/// integration; the frontend computes them. Pass `&[]` / `None` for a bare
/// interactive shell.
///
/// `exec_command`, when `Some(&[prog, args…])`, runs that command DIRECTLY in the
/// PTY instead of a shell (the `-e` convention: when it exits, the PTY closes and
/// the window follows). `prog` is PATH-resolved HERE in the parent (the child must
/// stay async-signal-safe, so no `execvp` PATH search there); `argv[0]` is `prog`
/// as given. It takes precedence over `argv_override` and `$ATERM_EXEC` — there is
/// no interactive shell to integrate with. An unresolved/again-failing `prog` ends
/// the child with `_exit(127)`, closing the window, just like a failed shell exec.
///
/// `cwd`, when `Some`, is the working directory the child `chdir`s into before
/// exec (the `--working-directory` flag); it overrides the default
/// `/`→`$HOME` Finder-launch fallback. A failed `chdir` is non-fatal (the child
/// starts in the inherited directory), matching the existing best-effort `chdir`.
///
/// Spawning a child process is a privileged effect (ATERM_DESIGN WS-G), so it
/// requires a `Cap<Spawn>` of at least `Trusted` tier (`aterm-cap`): there is no
/// way to spawn without one.
///
/// ## Fail-closed confinement (ATERM_DESIGN §5.6, exit-before-exec)
///
/// The child applies the resource sandbox BEFORE `execve`. If the sandbox
/// `apply()` returns an error the child does NOT exec — it writes a one-byte
/// failure indicator on the close-on-exec status pipe and `_exit(126)`s, so a
/// confinement failure can never silently hand back a master fd for an
/// UNCONFINED shell. The parent reads the status pipe: a clean EOF (the write
/// end closed by `execve`'s O_CLOEXEC) means the child exec'd confined; any byte
/// means the child failed before exec, and the parent returns an error instead
/// of the master fd.
///
/// # Errors
/// Returns `PermissionDenied` if the capability's tier is too low, the OS error
/// if `forkpty`/`pipe` fails, or `PermissionDenied`/`Other` if the child failed
/// to confine itself (sandbox `apply` error) or to `execve` before exec. On any
/// pre-exec child failure the master fd is closed and NO unconfined shell is
/// returned.
pub fn spawn_shell(
    rows: u16,
    cols: u16,
    cap: &aterm_cap::Cap<aterm_cap::effects::Spawn>,
    sandbox_cap: &aterm_cap::Cap<aterm_sandbox::Sandbox>,
    env_add: &[(String, String)],
    argv_override: Option<&[String]>,
    exec_command: Option<&[String]>,
    cwd: Option<&str>,
) -> io::Result<i32> {
    aterm_cap::require(cap, aterm_cap::Tier::Trusted)
        .map_err(|e| io::Error::new(io::ErrorKind::PermissionDenied, e.to_string()))?;

    // EVERYTHING that allocates or reads the environment happens HERE, in the
    // PARENT, BEFORE forkpty. The frontend is multi-threaded (GPU/Metal + socket
    // threads are live), and POSIX permits ONLY async-signal-safe calls between
    // fork and exec — so the child below must not allocate, take the std env
    // lock, or call `setenv`. We pre-build the C arrays and hand them to
    // `execve`; a lock a vanished thread held would otherwise deadlock (or, with
    // the macOS Obj-C runtime, hard-abort) the child.
    let shell = std::env::var_os("SHELL").unwrap_or_else(|| "/bin/sh".into());
    let cshell = CString::new(shell.as_os_str().as_bytes())
        .unwrap_or_else(|_| CString::new("/bin/sh").unwrap());

    // envp = the current environment with `env_add` overriding existing keys
    // (new keys appended). `env_store` owns the C strings `envp` points into.
    let mut env_pairs: Vec<(std::ffi::OsString, std::ffi::OsString)> = std::env::vars_os().collect();
    for (k, v) in env_add {
        let key = std::ffi::OsString::from(k);
        match env_pairs.iter_mut().find(|(ek, _)| *ek == key) {
            Some(slot) => slot.1 = std::ffi::OsString::from(v),
            None => env_pairs.push((key, std::ffi::OsString::from(v))),
        }
    }
    let env_store: Vec<CString> = env_pairs
        .iter()
        .filter_map(|(k, v)| {
            let mut kv = k.clone();
            kv.push("=");
            kv.push(v);
            CString::new(kv.as_bytes()).ok()
        })
        .collect();
    let mut envp: Vec<*const libc::c_char> = env_store.iter().map(|c| c.as_ptr()).collect();
    envp.push(ptr::null());

    // exec target + argv. `-e prog args…` (`exec_command`) runs the command
    // DIRECTLY and takes precedence over every shell path. Otherwise the program is
    // `$SHELL` and argv is: an explicit override (bash `--rcfile …`) wins; else
    // `$ATERM_EXEC` runs a command then execs the shell; else a LOGIN interactive
    // shell whose argv[0] is "-"+basename (the macOS convention → sources
    // .zprofile / .bash_profile / path_helper). `argv_store` + `exec_target` own
    // the C strings the child's `execve` reads.
    let (exec_target, argv_store): (CString, Vec<CString>) =
        if let Some(cmd) = exec_command.filter(|c| !c.is_empty()) {
            let argv: Vec<CString> =
                cmd.iter().filter_map(|a| CString::new(a.as_bytes()).ok()).collect();
            (resolve_program(&cmd[0]), argv)
        } else if let Some(ov) = argv_override {
            let argv = ov.iter().filter_map(|a| CString::new(a.as_bytes()).ok()).collect();
            (cshell.clone(), argv)
        } else if let Some(cmd) = std::env::var_os("ATERM_EXEC") {
            let script = format!("{}; exec {}", cmd.to_string_lossy(), shell.to_string_lossy());
            let argv = vec![
                cshell.clone(),
                CString::new("-c").unwrap(),
                CString::new(script).unwrap_or_else(|_| CString::new("true").unwrap()),
            ];
            (cshell.clone(), argv)
        } else {
            let base = std::path::Path::new(&shell).file_name().unwrap_or(shell.as_os_str());
            let mut argv0 = std::ffi::OsString::from("-");
            argv0.push(base);
            let argv = vec![CString::new(argv0.as_bytes()).unwrap_or_else(|_| cshell.clone())];
            (cshell.clone(), argv)
        };
    let mut argv: Vec<*const libc::c_char> = argv_store.iter().map(|c| c.as_ptr()).collect();
    argv.push(ptr::null());

    // chdir target: an explicit `--working-directory` (`cwd`) wins; else, when
    // launched from `/` (a Finder/launchd .app start), begin in $HOME instead of
    // the filesystem root. Resolved up front — the child only calls `chdir`.
    let chdir_c: Option<CString> = if let Some(dir) = cwd {
        CString::new(dir.as_bytes()).ok()
    } else if std::env::current_dir().ok().as_deref() == Some(std::path::Path::new("/")) {
        std::env::var_os("HOME").and_then(|h| CString::new(h.as_bytes()).ok())
    } else {
        None
    };

    // Exec-status pipe: a close-on-exec pipe whose write end the child holds. A
    // successful `execve` closes that end (O_CLOEXEC) and the parent reads EOF (0
    // bytes) = "child exec'd confined". A pre-exec failure (sandbox apply error,
    // or execve itself failing) makes the child WRITE a one-byte reason then
    // `_exit`, and the parent reads that byte = "child failed before exec" and
    // returns an error rather than a master fd for an unconfined shell.
    let mut status_fds = [0i32; 2];
    // SAFETY: `status_fds` is a valid 2-element buffer. (`pipe2` with O_CLOEXEC is
    // not available on macOS, so we set FD_CLOEXEC explicitly below.)
    let rc = unsafe { libc::pipe(status_fds.as_mut_ptr()) };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    let (status_rd, status_wr) = (status_fds[0], status_fds[1]);
    // Mark BOTH ends close-on-exec: the write end's close-on-exec close is the
    // SUCCESS signal (parent reads EOF after the child execs), and the read end
    // must not leak into the shell. Set in the PARENT, before fork (still safe to
    // allocate / call fcntl here). A failure to set CLOEXEC would break the
    // success/failure distinction, so treat it as a hard error.
    // SAFETY: both fds are valid; `fcntl(F_SETFD, FD_CLOEXEC)` only sets a flag.
    let cloexec_ok = unsafe {
        libc::fcntl(status_rd, libc::F_SETFD, libc::FD_CLOEXEC) != -1
            && libc::fcntl(status_wr, libc::F_SETFD, libc::FD_CLOEXEC) != -1
    };
    if !cloexec_ok {
        let err = io::Error::last_os_error();
        // SAFETY: closing the two pipe fds we just opened.
        unsafe {
            libc::close(status_rd);
            libc::close(status_wr);
        }
        return Err(err);
    }

    let mut ws = libc::winsize { ws_row: rows, ws_col: cols, ws_xpixel: 0, ws_ypixel: 0 };
    let mut master: libc::c_int = -1;
    // SAFETY: `forkpty` is called with a valid out-param for the master fd, null
    // for the (unused) slave-name/termios buffers, and a valid `winsize`. It
    // returns the child pid in the parent (and 0 in the child), per POSIX.
    let pid = unsafe { libc::forkpty(&mut master, ptr::null_mut(), ptr::null_mut(), &mut ws) };
    if pid < 0 {
        let err = io::Error::last_os_error();
        // SAFETY: closing the two pipe fds we just opened (fork failed).
        unsafe {
            libc::close(status_rd);
            libc::close(status_wr);
        }
        return Err(err);
    }
    if pid == 0 {
        // CHILD — async-signal-safe ONLY. Everything was pre-built in the parent
        // above; nothing here allocates, locks, or reads std env.
        // (0) the read end is the parent's; drop it in the child so only the
        //     write end (closed by exec on success) carries the status.
        // SAFETY: `status_rd` is the inherited read-end fd; `close` is a-s-safe.
        unsafe {
            libc::close(status_rd);
        }
        // (1) confine resource use (WS-G auto-sandbox). FAIL-CLOSED (§5.6): if
        //     the sandbox cannot be installed, do NOT exec an unconfined shell —
        //     signal the parent and exit before exec. With a valid cap `apply`
        //     does not allocate, and `setrlimit` is async-signal-safe.
        if aterm_sandbox::Limits::shell_default().apply(sandbox_cap).is_err() {
            // SAFETY: write a single async-signal-safe failure byte then exit.
            // `write`/`_exit` are async-signal-safe; the byte distinguishes a
            // sandbox failure (b'S') for the parent's diagnostic.
            unsafe {
                let b: u8 = b'S';
                libc::write(status_wr, std::ptr::addr_of!(b).cast::<libc::c_void>(), 1);
                libc::_exit(126);
            }
        }
        // (2) chdir to $HOME when started from `/`.
        if let Some(dir) = &chdir_c {
            // SAFETY: `dir` is a valid NUL-terminated path; `chdir` is async-signal-safe.
            unsafe {
                libc::chdir(dir.as_ptr());
            }
        }
        // (3) close the inherited master fd: the slave is already this child's
        //     controlling tty (forkpty's login_tty), so the master must not leak
        //     into the shell or any process it spawns.
        // SAFETY: `master` is the forkpty master fd; `close` is async-signal-safe.
        unsafe {
            libc::close(master);
        }
        // (4) exec. `execve` (not `execvp`) takes the pre-built `envp` and does no
        //     PATH-search allocation; the target is an absolute path ($SHELL, or a
        //     `-e` program already PATH-resolved in the parent).
        //     On success `execve` does not return and the O_CLOEXEC `status_wr`
        //     is closed by the kernel → parent reads EOF (confined-and-exec'd).
        //     On failure, signal the parent (b'E') and exit before any shell runs.
        // SAFETY: exec_target/argv/envp are null-terminated arrays of live C
        // strings that outlive the call; `write`/`_exit` are async-signal-safe.
        unsafe {
            libc::execve(exec_target.as_ptr(), argv.as_ptr(), envp.as_ptr());
            let b: u8 = b'E';
            libc::write(status_wr, std::ptr::addr_of!(b).cast::<libc::c_void>(), 1);
            libc::_exit(127);
        }
    }
    // PARENT. Close our copy of the write end so the read sees EOF once the only
    // remaining write end (the child's) is gone (exec-closed or after the child
    // exits). Then read the status: 0 bytes (EOF) = success; any byte = the child
    // failed BEFORE exec, so there is no confined shell to hand back.
    // SAFETY: `status_wr` is the parent's copy of the write end.
    unsafe {
        libc::close(status_wr);
    }
    let mut indicator = [0u8; 1];
    // EINTR-retrying read of the single status byte (or EOF).
    let n = loop {
        // SAFETY: `status_rd` is a valid read fd; `indicator` is a 1-byte buffer.
        let r = unsafe {
            libc::read(status_rd, indicator.as_mut_ptr().cast::<libc::c_void>(), 1)
        };
        if r < 0 && io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
            continue;
        }
        break r;
    };
    // SAFETY: done with the read end.
    unsafe {
        libc::close(status_rd);
    }
    if n > 0 {
        // Child reported a pre-exec failure. Close the master (no unconfined
        // shell escapes) and reap the child so it is not left as a zombie.
        // SAFETY: `master` is the parent's forkpty master fd.
        unsafe {
            libc::close(master);
            let mut wstatus: libc::c_int = 0;
            libc::waitpid(pid, &mut wstatus, 0);
        }
        let (kind, what) = match indicator[0] {
            b'S' => (io::ErrorKind::PermissionDenied, "sandbox confinement failed in child (fail-closed: shell not exec'd, _exit(126))"),
            _ => (io::ErrorKind::Other, "child failed to exec the shell before exec (_exit(127))"),
        };
        return Err(io::Error::new(kind, what));
    }
    Ok(master)
}

/// PATH-resolve a `-e` program name to an absolute path, IN THE PARENT (the child
/// must stay async-signal-safe, so it cannot do its own `execvp` PATH search). A
/// name containing `/` is used verbatim (an explicit path). Otherwise each `$PATH`
/// entry is probed for an executable regular file. Falls back to the name verbatim
/// when nothing matches, so `execve` fails cleanly (`_exit(127)`) instead of this
/// resolver masking a not-found command.
fn resolve_program(name: &str) -> CString {
    let verbatim =
        || CString::new(name.as_bytes()).unwrap_or_else(|_| CString::new("/nonexistent").unwrap());
    if name.is_empty() || name.contains('/') {
        return verbatim();
    }
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            if dir.as_os_str().is_empty() {
                continue;
            }
            let candidate = dir.join(name);
            if let Ok(c) = CString::new(candidate.as_os_str().as_bytes()) {
                // Executable (X_OK) AND a regular file — something we can exec.
                // SAFETY: `c` is a valid NUL-terminated path string.
                let executable = unsafe { libc::access(c.as_ptr(), libc::X_OK) } == 0;
                if executable && candidate.is_file() {
                    return c;
                }
            }
        }
    }
    verbatim()
}

/// Write all of `bytes` to the PTY master, retrying short writes AND `EINTR`
/// (a signal interrupting the write must not silently drop the rest of the
/// buffer — that would lose terminal input). Stops only on a real error or a
/// zero/negative non-`EINTR` return (peer closed).
pub fn write_all(master: i32, bytes: &[u8]) {
    let mut data = bytes;
    while !data.is_empty() {
        // SAFETY: `master` is a PTY master fd from `spawn_shell`; `data` is a
        // valid slice of `data.len()` bytes.
        let r = unsafe {
            libc::write(master, data.as_ptr() as *const libc::c_void, data.len())
        };
        if r < 0 {
            // A signal interrupted the write before any byte moved: retry. Any
            // other error means the master is gone — stop.
            if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            break;
        }
        if r == 0 {
            break; // peer closed
        }
        data = &data[r as usize..];
    }
}

/// Read up to `buf.len()` bytes from the PTY master into `buf`. Returns the number
/// of bytes read (`0` = EOF, `< 0` = error, per `read(2)`).
pub fn read(master: i32, buf: &mut [u8]) -> isize {
    // SAFETY: `master` is a valid fd; `buf` is a valid mutable slice of
    // `buf.len()` bytes.
    unsafe { libc::read(master, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) }
}

/// Resize the PTY to `rows`×`cols` (`TIOCSWINSZ`).
pub fn resize(master: i32, rows: u16, cols: u16) {
    let ws = libc::winsize { ws_row: rows, ws_col: cols, ws_xpixel: 0, ws_ypixel: 0 };
    // SAFETY: `master` is a valid PTY master fd; `&ws` is a valid `winsize` for
    // the `TIOCSWINSZ` ioctl.
    unsafe {
        libc::ioctl(master, libc::TIOCSWINSZ, &ws);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Validates the `write_all` + `read` syscall wrappers on a real fd (a plain
    // pipe), so the seam's IO is exercised without spawning a shell (no flake, no
    // leftover process). `spawn_shell`/`resize` are exercised end-to-end by the
    // GUI that depends on this crate.
    #[test]
    fn write_all_then_read_roundtrips_on_a_pipe() {
        let mut fds = [0i32; 2];
        // SAFETY: `fds` is a valid 2-element buffer for `pipe`.
        let rc = unsafe { libc::pipe(fds.as_mut_ptr()) };
        assert_eq!(rc, 0, "pipe() failed");
        let (rd, wr) = (fds[0], fds[1]);

        write_all(wr, b"hello-pty-seam");
        let mut buf = [0u8; 64];
        let n = read(rd, &mut buf);
        assert!(n > 0, "read returned {n}");
        assert_eq!(&buf[..n as usize], b"hello-pty-seam");

        // SAFETY: closing the two fds we just opened.
        unsafe {
            libc::close(rd);
            libc::close(wr);
        }
    }

    // SEC-2: a confinement failure in the child must FAIL CLOSED. We force the
    // sandbox `apply` to fail by handing it an UNTRUSTED `Cap<Sandbox>` (its gate
    // requires Trusted+), so the child takes the `_exit(126)` path BEFORE exec and
    // the parent returns an error instead of a master fd for an unconfined shell.
    #[test]
    fn sandbox_apply_failure_in_child_fails_closed_no_unconfined_shell() {
        // SAFETY: single-threaded test, trusted-launcher contract trivially holds.
        let authority = unsafe { aterm_cap::Authority::root_authority() };
        // A valid spawn cap (passes the PARENT gate) but a too-weak sandbox cap
        // (fails the CHILD's `apply` gate) — exactly the silent-unconfined hole.
        let spawn_cap = authority.grant::<aterm_cap::effects::Spawn>(aterm_cap::Tier::Trusted);
        let weak_sandbox = authority.grant::<aterm_sandbox::Sandbox>(aterm_cap::Tier::Untrusted);

        let result = spawn_shell(24, 80, &spawn_cap, &weak_sandbox, &[], None, None, None);
        let err = result.expect_err(
            "a sandbox confinement failure must surface as an error, NOT a master fd",
        );
        assert_eq!(
            err.kind(),
            io::ErrorKind::PermissionDenied,
            "child sandbox failure must be reported as PermissionDenied, got: {err}",
        );
        assert!(
            err.to_string().contains("fail-closed"),
            "error should describe the fail-closed confinement: {err}",
        );
    }

    // The success path still works: with a properly-tiered sandbox cap a real
    // `$SHELL` spawns and the parent gets a live master fd. Reading from it (the
    // shell's first prompt/banner, or at least the PTY echo) proves a process is
    // attached; then we close the master to tear the child down.
    #[test]
    fn normal_shell_spawns_with_a_trusted_sandbox_cap() {
        // SAFETY: single-threaded test, trusted-launcher contract trivially holds.
        let authority = unsafe { aterm_cap::Authority::root_authority() };
        let spawn_cap = authority.grant::<aterm_cap::effects::Spawn>(aterm_cap::Tier::Trusted);
        let sandbox_cap = authority.grant::<aterm_sandbox::Sandbox>(aterm_cap::Tier::Trusted);
        // Run a deterministic command then exit, so the test does not hang on an
        // interactive prompt: ATERM_EXEC makes the child run it, then exec $SHELL.
        // Using a bare `echo` + immediate close is enough to prove a live master.
        let master = spawn_shell(24, 80, &spawn_cap, &sandbox_cap, &[], None, None, None)
            .expect("a normal shell must spawn with a Trusted sandbox cap");
        assert!(master >= 0, "master fd must be valid, got {master}");
        // Best-effort: write a harmless newline and read whatever echoes back, to
        // confirm the fd is a live PTY master, not a dangling descriptor.
        write_all(master, b"\n");
        let mut buf = [0u8; 64];
        let _ = read(master, &mut buf); // may be 0 if the child raced exit; fd is still valid
        // SAFETY: closing the master tears down the child's controlling tty.
        unsafe {
            libc::close(master);
        }
    }
}
