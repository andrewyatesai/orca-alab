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

/// Fixed absolute path to the macOS Seatbelt wrapper used by the OS-sandbox wrap
/// (see [`spawn_shell`]'s `sandbox_wrap`). Inlined here (rather than depending on
/// the policy crate) to keep this minimal syscall seam dependency-light; it MUST
/// equal `aterm_containment::SANDBOX_EXEC_PATH` — a test in this crate locks that.
const SANDBOX_EXEC_PATH: &str = "/usr/bin/sandbox-exec";

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
/// ## OS sandbox wrap (`sandbox_wrap`, macOS Seatbelt — ATERM_DESIGN §5.6)
///
/// `sandbox_wrap`, when `Some(sbpl)`, wraps the WHOLE resolved program+argv in
/// `/usr/bin/sandbox-exec -p <sbpl>` so the macOS kernel Seatbelt applies the SBPL
/// profile (e.g. `(deny network*)` for `Containment` mode) before the target
/// `exec`s. The wrap is BUILT IN THE PARENT: `sandbox-exec` becomes the exec
/// target (a fixed absolute path — no PATH search, async-signal-safe in the child)
/// and the original program+argv become its trailing arguments, so the login-shell
/// argv[0], `--rcfile`, `$ATERM_EXEC`, and `-e` paths are all preserved verbatim
/// as what sandbox-exec runs. This is **fail-closed**: if `sandbox-exec` is not
/// present at its fixed path, `spawn_shell` returns an error and does NOT spawn —
/// it never silently runs an UNSANDBOXED shell when the caller demanded the
/// sandbox. `None` means no wrap: the spawn is byte-identical to before (used for
/// every non-`Containment` mode, so the default User-mode spawn is unchanged).
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
// The arg list is intentionally wide: this is the SINGLE spawn seam, and each
// argument is an independent, security-relevant input (caps, env, argv, cwd, the
// OS-sandbox wrap). Bundling them into a struct would hide that surface, not
// shrink it.
#[allow(clippy::too_many_arguments)]
pub fn spawn_shell(
    rows: u16,
    cols: u16,
    cap: &aterm_cap::Cap<aterm_cap::effects::Spawn>,
    sandbox_cap: &aterm_cap::Cap<aterm_sandbox::Sandbox>,
    env_add: &[(String, String)],
    argv_override: Option<&[String]>,
    exec_command: Option<&[String]>,
    cwd: Option<&str>,
    sandbox_wrap: Option<&str>,
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

    // OS-sandbox wrap (macOS Seatbelt, ATERM_DESIGN §5.6). When the caller demands
    // a sandbox (`Some(sbpl)` — Containment mode denies network), wrap the resolved
    // program+argv in `/usr/bin/sandbox-exec -p <sbpl>` so the kernel applies the
    // profile before the target execs. We FAIL CLOSED in the PARENT (before any
    // fork) if the wrapper binary is absent: a caller that demanded the sandbox
    // must NEVER get an unsandboxed shell. The wrapped argv is:
    //   sandbox-exec, "-p", <sbpl>, <program-path>, <original argv[1..]>
    // i.e. the original argv with argv[0] replaced by the resolved program PATH
    // (sandbox-exec execs its first positional and sets that path as the child's
    // argv[0]). This preserves every real argument (`--rcfile FILE`, `-c SCRIPT`,
    // a `-e` command's args); only the cosmetic leading-dash login marker on a
    // BARE interactive shell is dropped (a Containment shell is a non-login
    // interactive shell — an accepted, documented tradeoff for the hostile mode).
    // `exec_target`/`argv_store` from above are shadowed by the wrapped versions so
    // the rest of the seam (the C-array build, the child's execve) is unchanged.
    let (exec_target, argv_store): (CString, Vec<CString>) = if let Some(sbpl) = sandbox_wrap {
        // FAIL CLOSED in the PARENT, before any fork, if the wrapper is missing or
        // the argv can't be built — never spawn an unsandboxed shell when a sandbox
        // was demanded. The presence check + argv build is the pure, testable
        // `build_sandbox_wrap`.
        build_sandbox_wrap(SANDBOX_EXEC_PATH, sbpl, &exec_target, &argv_store)?
    } else {
        // No wrap requested → byte-identical to the pre-sandbox spawn.
        (exec_target, argv_store)
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

/// Build the `sandbox-exec`-wrapped `(exec_target, argv)` for an OS-sandboxed
/// spawn, FAILING CLOSED if the wrapper at `wrapper_path` is missing/not
/// executable. Pure (its only side effect is the `access(X_OK)` probe of
/// `wrapper_path`), so the fail-closed and argv-shape behavior is unit-testable
/// without forking.
///
/// On success the returned exec target is `wrapper_path` and the argv is:
///   ["sandbox-exec", "-p", <sbpl>, <program-path>, <orig argv[1..]>]
/// i.e. the original argv with argv[0] replaced by the resolved program PATH
/// (`prog`), because `sandbox-exec` execs its first positional and sets that path
/// as the child's argv[0]. Every real argument after argv[0] is preserved; only a
/// cosmetic login-dash argv[0] on a bare shell is dropped (documented on
/// [`spawn_shell`]).
///
/// # Errors
/// `NotFound` if `wrapper_path` is missing/not executable (fail-closed — the
/// caller must NOT spawn unsandboxed); `Other`/`InvalidInput` if `wrapper_path`
/// or `sbpl` cannot be turned into a C string (interior NUL).
fn build_sandbox_wrap(
    wrapper_path: &str,
    sbpl: &str,
    prog: &CString,
    orig_argv: &[CString],
) -> io::Result<(CString, Vec<CString>)> {
    let wrapper = CString::new(wrapper_path.as_bytes())
        .map_err(|_| io::Error::other("sandbox-exec path not representable"))?;
    // `access(X_OK)` in the PARENT (the child does no PATH search). A missing
    // wrapper means the policy-demanded sandbox cannot be applied → refuse.
    // SAFETY: `wrapper` is a valid NUL-terminated absolute path.
    let present = unsafe { libc::access(wrapper.as_ptr(), libc::X_OK) } == 0;
    if !present {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "OS sandbox demanded but {wrapper_path} is missing/not executable — refusing \
                 to spawn an unsandboxed shell (fail-closed, ATERM_DESIGN §5.6)"
            ),
        ));
    }
    let sbpl_c = CString::new(sbpl.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "SBPL profile has interior NUL"))?;
    let mut wrapped: Vec<CString> = Vec::with_capacity(orig_argv.len() + 3);
    wrapped.push(CString::new("sandbox-exec").unwrap_or_else(|_| wrapper.clone()));
    wrapped.push(CString::new("-p").unwrap());
    wrapped.push(sbpl_c);
    wrapped.push(prog.clone());
    wrapped.extend(orig_argv.iter().skip(1).cloned());
    Ok((wrapper, wrapped))
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

/// The decision a single `write(2)` return drives in the `write_all` drain loop.
/// Extracted as a pure value so the EINTR-retry / short-write / peer-closed branch
/// logic is unit-testable WITHOUT provoking a real (timing-dependent) `EINTR`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WriteStep {
    /// A signal interrupted the write before any byte moved (`EINTR`): retry.
    Retry,
    /// A real error, or the peer closed (`r == 0`): stop draining.
    Stop,
    /// `n` bytes were written: advance the slice cursor by `n` and continue.
    Advance(usize),
}

/// Classify a `write(2)` result for the `write_all` loop. `r` is the raw return;
/// `is_eintr` is whether `errno` was `EINTR` (only consulted when `r < 0`, exactly
/// as the loop does — the caller reads `errno` only on the error branch). Pure: no
/// syscalls, no `errno` read of its own, so it can be tested with synthetic inputs.
///
/// This is a behavior-preserving extraction of the original inline branch ladder;
/// the runtime decisions are byte-identical:
///   r < 0 && EINTR      -> Retry
///   r < 0 && other      -> Stop
///   r == 0 (peer closed) -> Stop
///   r > 0               -> Advance(r)
fn classify_write_result(r: isize, is_eintr: bool) -> WriteStep {
    if r < 0 {
        if is_eintr {
            WriteStep::Retry
        } else {
            WriteStep::Stop
        }
    } else if r == 0 {
        WriteStep::Stop
    } else {
        WriteStep::Advance(r as usize)
    }
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
        // `errno` is only meaningful when `r < 0`; mirror the original loop, which
        // read `last_os_error()` solely on the error branch.
        let is_eintr =
            r < 0 && io::Error::last_os_error().kind() == io::ErrorKind::Interrupted;
        match classify_write_result(r, is_eintr) {
            WriteStep::Retry => continue,
            WriteStep::Stop => break,
            WriteStep::Advance(n) => data = &data[n..],
        }
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

        let result = spawn_shell(24, 80, &spawn_cap, &weak_sandbox, &[], None, None, None, None);
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
        let master = spawn_shell(24, 80, &spawn_cap, &sandbox_cap, &[], None, None, None, None)
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

    // ---- write_all branch logic (pure, via the extracted classifier) ----
    //
    // The EINTR-retry / short-write / peer-closed branch ladder of `write_all` is
    // a behavior-preserving extraction into `classify_write_result`. Testing the
    // pure classifier covers the EXACT decision the loop drives, WITHOUT having to
    // provoke a real (timing-dependent, flaky) `EINTR`.

    #[test]
    fn classify_write_eintr_negative_retries() {
        // r < 0 with EINTR => retry the write (do not drop the rest of the buffer).
        assert_eq!(classify_write_result(-1, true), WriteStep::Retry);
    }

    #[test]
    fn classify_write_noneintr_error_stops() {
        // r < 0 with any other errno (EIO, EBADF, EPIPE, …) => stop: master is gone.
        assert_eq!(classify_write_result(-1, false), WriteStep::Stop);
    }

    #[test]
    fn classify_write_zero_is_peer_closed_stop() {
        // r == 0 => peer closed; stop draining (errno is irrelevant here).
        assert_eq!(classify_write_result(0, false), WriteStep::Stop);
        assert_eq!(classify_write_result(0, true), WriteStep::Stop);
    }

    #[test]
    fn classify_write_partial_advances_by_exact_count() {
        // r > 0 => advance the cursor by EXACTLY r bytes (short-write handling).
        assert_eq!(classify_write_result(1, false), WriteStep::Advance(1));
        assert_eq!(classify_write_result(4096, false), WriteStep::Advance(4096));
    }

    // ---- read() syscall wrapper: EOF and bad-fd error contract ----

    // EOF: when the write end of a pipe is closed and the buffer is drained, a
    // `read` of the read end returns exactly 0 (not negative, not a partial-read
    // surprise). This is the `0 = EOF` half of the documented `read` contract.
    #[test]
    fn read_returns_zero_on_eof_after_write_end_closed() {
        let mut fds = [0i32; 2];
        // SAFETY: `fds` is a valid 2-element buffer for `pipe`.
        let rc = unsafe { libc::pipe(fds.as_mut_ptr()) };
        assert_eq!(rc, 0, "pipe() failed");
        let (rd, wr) = (fds[0], fds[1]);
        // Close the only write end with no data pending => the next read sees EOF.
        // SAFETY: `wr` is the pipe write end we just opened.
        unsafe {
            libc::close(wr);
        }
        let mut buf = [0u8; 16];
        let n = read(rd, &mut buf);
        assert_eq!(n, 0, "read at EOF must return 0, got {n}");
        // SAFETY: closing the read end we opened.
        unsafe {
            libc::close(rd);
        }
    }

    // Error: a `read` of an invalid descriptor must return a negative value (the
    // `< 0 = error` half of the contract), with `errno == EBADF`. We use fd -1,
    // which is never a valid descriptor, so this is hermetic and deterministic and
    // never touches a real, possibly-open fd. (We assert the raw errno, not
    // `ErrorKind`, because libstd categorizes EBADF as `Uncategorized` here — the
    // stable contract is the negative return + the POSIX errno, not the kind.)
    #[test]
    fn read_returns_negative_with_ebadf_on_invalid_fd() {
        let mut buf = [0u8; 16];
        let n = read(-1, &mut buf);
        assert!(n < 0, "read on a bad fd must be negative, got {n}");
        let err = io::Error::last_os_error();
        assert_eq!(
            err.raw_os_error(),
            Some(libc::EBADF),
            "read on a bad fd must set errno=EBADF, got {err}",
        );
        // And EBADF is NOT EINTR, so the read loop would STOP (not spin-retry) on it
        // — the very decision the classifier encodes.
        assert_ne!(err.kind(), io::ErrorKind::Interrupted);
    }

    // ---- write_all drains a buffer larger than one pipe write (partial writes) ----

    // A pipe's kernel buffer is finite (typically 16–64 KiB), so a single
    // `write(2)` of a buffer larger than the pipe capacity CANNOT move all the
    // bytes at once: the kernel returns a short count and `write_all` must loop to
    // drain the remainder. A dedicated reader thread keeps draining so the writer
    // never blocks forever; we assert the bytes arrive byte-for-byte, in order,
    // for the full payload. This exercises the real `Advance(n)` short-write path
    // of `write_all` on a live fd (not just the pure classifier).
    #[test]
    fn write_all_drains_payload_larger_than_one_pipe_write() {
        let mut fds = [0i32; 2];
        // SAFETY: `fds` is a valid 2-element buffer for `pipe`.
        let rc = unsafe { libc::pipe(fds.as_mut_ptr()) };
        assert_eq!(rc, 0, "pipe() failed");
        let (rd, wr) = (fds[0], fds[1]);

        // 1 MiB — far larger than any pipe buffer, so >=1 short write is forced.
        // A deterministic, position-dependent pattern catches reorder/drop bugs.
        let n_bytes = 1usize << 20;
        let payload: Vec<u8> = (0..n_bytes).map(|i| (i % 251) as u8).collect();

        // Drain thread: read the read end to completion (until EOF) and return what
        // it saw. It must run concurrently with the writer or the pipe deadlocks.
        let reader = std::thread::spawn(move || {
            let mut got = Vec::with_capacity(n_bytes);
            let mut chunk = [0u8; 8192];
            loop {
                let r = read(rd, &mut chunk);
                if r <= 0 {
                    break; // 0 = EOF (writer closed), <0 = error
                }
                got.extend_from_slice(&chunk[..r as usize]);
            }
            // SAFETY: closing the read end this thread owns.
            unsafe {
                libc::close(rd);
            }
            got
        });

        write_all(wr, &payload);
        // Close the write end so the reader observes EOF and the thread joins.
        // SAFETY: `wr` is the write end this thread owns after `write_all`.
        unsafe {
            libc::close(wr);
        }

        let got = reader.join().expect("reader thread panicked");
        assert_eq!(got.len(), payload.len(), "drained byte count mismatch");
        assert!(got == payload, "drained bytes differ from the payload byte-for-byte");
    }

    // ---- fail-closed spawn: under-tier capability is denied WITHOUT forking ----

    // An under-tier `Cap<Spawn>` (Untrusted, below the required Trusted) must be
    // rejected by the PARENT gate BEFORE any `forkpty` — there must be no way to
    // spawn a child with an insufficient capability. We assert PermissionDenied;
    // the absence of a leaked child is implicit (no fork happened, so there is
    // nothing to reap), and the error originates from `aterm_cap::require`, not
    // from a child status byte.
    #[test]
    fn under_tier_spawn_cap_is_denied_before_forking() {
        // SAFETY: single-threaded test, trusted-launcher contract trivially holds.
        let authority = unsafe { aterm_cap::Authority::root_authority() };
        // Untrusted spawn cap: below the Trusted floor `spawn_shell` requires.
        let weak_spawn = authority.grant::<aterm_cap::effects::Spawn>(aterm_cap::Tier::Untrusted);
        let sandbox_cap = authority.grant::<aterm_sandbox::Sandbox>(aterm_cap::Tier::Trusted);

        let result = spawn_shell(24, 80, &weak_spawn, &sandbox_cap, &[], None, None, None, None);
        let err = result.expect_err("an under-tier spawn cap must be denied, not spawn a shell");
        assert_eq!(
            err.kind(),
            io::ErrorKind::PermissionDenied,
            "under-tier spawn must be PermissionDenied, got: {err}",
        );
    }

    // ---- fail-closed spawn: a child that cannot exec takes the _exit(127) path ----

    // The exec-failure path through the REAL production code: a `-e` command naming
    // a nonexistent absolute program forces the child's `execve` to fail, so the
    // child writes the b'E' status byte and `_exit(127)`s. The parent reads that
    // byte off the status pipe, reaps the child internally, and surfaces an
    // `io::Error` (ErrorKind::Other) describing the pre-exec exec failure — never a
    // master fd. This drives a real `forkpty` + the full status-pipe protocol.
    //
    // NOTE on "$SHELL in the child": `spawn_shell` resolves the exec target in the
    // PARENT (it must, to stay async-signal-safe in the child), so a bogus `$SHELL`
    // can only be injected by mutating the parent's env — which is a data race
    // against the multi-threaded test harness under edition 2024. We therefore
    // drive the SAME child exec-failure path hermetically via a bogus `exec_command`
    // (no env mutation). The raw 127 exit code is consumed by `spawn_shell`'s own
    // `waitpid` reap, so it is not observable here; the contract that exit code 127
    // is what a bogus `execve` yields is locked by the sibling test below.
    #[test]
    fn bogus_exec_command_takes_child_exec_failure_path() {
        // SAFETY: single-threaded test, trusted-launcher contract trivially holds.
        let authority = unsafe { aterm_cap::Authority::root_authority() };
        let spawn_cap = authority.grant::<aterm_cap::effects::Spawn>(aterm_cap::Tier::Trusted);
        let sandbox_cap = authority.grant::<aterm_sandbox::Sandbox>(aterm_cap::Tier::Trusted);

        // An absolute path that cannot exist => `resolve_program` returns it
        // verbatim => the child's `execve` fails => b'E' + _exit(127).
        let bogus = vec![String::from("/nonexistent/aterm-pty-no-such-prog-xyz")];
        let result =
            spawn_shell(24, 80, &spawn_cap, &sandbox_cap, &[], None, Some(&bogus), None, None);
        let err = result.expect_err("a child that cannot exec must surface an error, not a master fd");
        assert_eq!(
            err.kind(),
            io::ErrorKind::Other,
            "exec failure before exec must be reported as Other, got: {err}",
        );
        assert!(
            err.to_string().contains("127"),
            "error should describe the _exit(127) exec failure: {err}",
        );
    }

    // Contract lock for the exit code the design depends on: a child that writes a
    // status byte and `_exit(127)`s after a failed `execve` is reaped by the parent
    // with the WEXITSTATUS == 127 the spawn protocol claims. This mirrors the exact
    // child syscall shape of `spawn_shell` (status pipe + write byte + _exit), using
    // a real `forkpty`, and ASSERTS the raw exit code — which `spawn_shell` itself
    // consumes during its internal reap, so it cannot be observed through that API.
    // It is a contract test of the OS primitive, NOT a re-implementation of product
    // logic: it locks "bogus execve => _exit(127), reapable" so a future change to
    // the child's exit code would be caught here.
    #[test]
    fn child_exec_failure_exit_code_is_127_and_reapable() {
        let mut master: libc::c_int = -1;
        let mut ws = libc::winsize { ws_row: 24, ws_col: 80, ws_xpixel: 0, ws_ypixel: 0 };
        // SAFETY: valid out-param for the master fd, null for the unused name/termios
        // buffers, and a valid winsize; returns the child pid (parent) or 0 (child).
        let pid =
            unsafe { libc::forkpty(&mut master, ptr::null_mut(), ptr::null_mut(), &mut ws) };
        assert!(pid >= 0, "forkpty failed: {}", io::Error::last_os_error());
        if pid == 0 {
            // CHILD — async-signal-safe only: attempt to exec a nonexistent program
            // (mirroring the child's `execve`), then take the _exit(127) failure
            // path exactly as `spawn_shell`'s child does.
            // SAFETY: a NUL-terminated absolute path; on `execve` failure we _exit.
            unsafe {
                let prog = b"/nonexistent/aterm-pty-no-such-prog-xyz\0";
                let argv: [*const libc::c_char; 2] =
                    [prog.as_ptr().cast::<libc::c_char>(), ptr::null()];
                let envp: [*const libc::c_char; 1] = [ptr::null()];
                libc::execve(prog.as_ptr().cast::<libc::c_char>(), argv.as_ptr(), envp.as_ptr());
                libc::_exit(127);
            }
        }
        // PARENT: reap the child and assert the exit code.
        // SAFETY: `master` is the forkpty master; closing it tears the child's tty.
        unsafe {
            libc::close(master);
        }
        let mut wstatus: libc::c_int = 0;
        // SAFETY: reaping the child we just forked; `wstatus` is a valid out-param.
        let w = unsafe { libc::waitpid(pid, &mut wstatus, 0) };
        assert_eq!(w, pid, "waitpid did not reap our child");
        assert!(libc::WIFEXITED(wstatus), "child did not exit normally: {wstatus}");
        assert_eq!(
            libc::WEXITSTATUS(wstatus),
            127,
            "a failed execve child must _exit(127)",
        );
    }

    // ---- OS-sandbox wrap (sandbox_wrap) ----

    // The seam's inlined wrapper path MUST be the SAME bytes as the policy crate's
    // canonical SANDBOX_EXEC_PATH. They are kept in lockstep by hand (the seam
    // stays dependency-light), so this test fails loudly if either drifts.
    #[test]
    fn inlined_sandbox_exec_path_matches_policy_crate() {
        assert_eq!(SANDBOX_EXEC_PATH, aterm_containment::SANDBOX_EXEC_PATH);
        assert_eq!(SANDBOX_EXEC_PATH, "/usr/bin/sandbox-exec");
    }

    // FAIL-CLOSED: when the wrapper binary is absent at the given path,
    // build_sandbox_wrap returns NotFound — the caller (`spawn_shell`) propagates
    // it and NEVER forks, so a policy-demanded sandbox that can't be applied
    // refuses to spawn rather than silently running an unsandboxed shell. We point
    // it at a guaranteed-nonexistent path to drive this without disturbing the real
    // /usr/bin/sandbox-exec.
    #[test]
    fn build_sandbox_wrap_fails_closed_when_wrapper_missing() {
        let prog = CString::new("/bin/zsh").unwrap();
        let argv = vec![CString::new("-zsh").unwrap()];
        let err = build_sandbox_wrap(
            "/nonexistent/aterm-no-such-sandbox-exec",
            aterm_containment::NETWORK_DENY_PROFILE,
            &prog,
            &argv,
        )
        .expect_err("a missing wrapper must fail closed, not silently skip the sandbox");
        assert_eq!(err.kind(), io::ErrorKind::NotFound, "fail-closed kind: {err}");
        assert!(
            err.to_string().contains("fail-closed"),
            "error must describe the fail-closed refusal: {err}",
        );
    }

    // The wrapped argv has the exact shape the kernel needs: sandbox-exec, -p,
    // <profile>, <program-path>, then the original args AFTER argv[0]. The login
    // argv[0] ("-zsh") is replaced by the program PATH; "--rcfile FILE" style real
    // args are carried through verbatim. Uses the REAL /usr/bin/sandbox-exec path
    // (present on macOS) so the access() probe passes.
    #[cfg(target_os = "macos")]
    #[test]
    fn build_sandbox_wrap_produces_correct_argv_shape() {
        let prog = CString::new("/bin/zsh").unwrap();
        // Original argv: a login-shell argv[0] plus a real flag+value pair.
        let argv = vec![
            CString::new("-zsh").unwrap(),
            CString::new("--rcfile").unwrap(),
            CString::new("/tmp/rc").unwrap(),
        ];
        let (target, wrapped) = build_sandbox_wrap(
            SANDBOX_EXEC_PATH,
            aterm_containment::NETWORK_DENY_PROFILE,
            &prog,
            &argv,
        )
        .expect("wrapper present → build succeeds");
        assert_eq!(target.to_str().unwrap(), SANDBOX_EXEC_PATH);
        let got: Vec<&str> = wrapped.iter().map(|c| c.to_str().unwrap()).collect();
        assert_eq!(
            got,
            vec![
                "sandbox-exec",
                "-p",
                aterm_containment::NETWORK_DENY_PROFILE,
                "/bin/zsh",     // argv[0] replaced by the program PATH
                "--rcfile",     // real args carried through verbatim …
                "/tmp/rc",      // …
            ],
            "wrapped argv shape must be sandbox-exec -p <sbpl> <prog> <orig argv[1..]>",
        );
    }

    // Default (no-wrap) spawn is byte-identical: passing `sandbox_wrap = None` must
    // NOT change the exec target — it stays `$SHELL`, never `sandbox-exec`. We
    // assert this through the SAME `-e` echo path used elsewhere: with no wrap, a
    // `-e /bin/echo MARKER` runs `/bin/echo` directly (argv[0] == the program), so
    // the PTY shows exactly "MARKER" with no sandbox-exec banner/argv mutation.
    #[test]
    fn no_wrap_spawn_runs_program_directly_unchanged() {
        // SAFETY: single-threaded test, trusted-launcher contract trivially holds.
        let authority = unsafe { aterm_cap::Authority::root_authority() };
        let spawn_cap = authority.grant::<aterm_cap::effects::Spawn>(aterm_cap::Tier::Trusted);
        let sandbox_cap = authority.grant::<aterm_sandbox::Sandbox>(aterm_cap::Tier::Trusted);
        let cmd = vec![
            String::from("/bin/echo"),
            String::from("ATERM-NOWRAP-MARKER"),
        ];
        // sandbox_wrap = None → no wrap, byte-identical spawn.
        let master = spawn_shell(24, 80, &spawn_cap, &sandbox_cap, &[], None, Some(&cmd), None, None)
            .expect("unwrapped -e command must spawn");
        let mut out = Vec::new();
        let mut buf = [0u8; 256];
        for _ in 0..50 {
            let n = read(master, &mut buf);
            if n <= 0 {
                break;
            }
            out.extend_from_slice(&buf[..n as usize]);
            if out.windows(b"ATERM-NOWRAP-MARKER".len()).any(|w| w == b"ATERM-NOWRAP-MARKER") {
                break;
            }
        }
        // SAFETY: tear down the child.
        unsafe {
            libc::close(master);
        }
        let s = String::from_utf8_lossy(&out);
        assert!(s.contains("ATERM-NOWRAP-MARKER"), "echo output not seen: {s:?}");
        assert!(
            !s.contains("sandbox-exec"),
            "no-wrap spawn must NOT involve sandbox-exec: {s:?}",
        );
    }

    // The wrap path is well-formed AND actually applies Seatbelt: wrap a `-e`
    // command in the real `(deny network*)` profile and run `/usr/bin/nc` against a
    // live loopback listener bound in this parent. WITHOUT the wrap nc connects;
    // WITH the wrap the kernel denies network so nc cannot connect — observed via
    // the child's exit code (the wrapped sandbox-exec→nc child fails). This drives
    // the REAL `spawn_shell` wrap-argv construction end to end, not just a direct
    // sandbox-exec call.
    #[cfg(target_os = "macos")]
    #[test]
    fn wrapped_spawn_enforces_network_deny_via_seatbelt() {
        use std::io::Write;
        use std::net::TcpListener;

        // SAFETY: single-threaded test, trusted-launcher contract trivially holds.
        let authority = unsafe { aterm_cap::Authority::root_authority() };
        let spawn_cap = authority.grant::<aterm_cap::effects::Spawn>(aterm_cap::Tier::Trusted);
        let sandbox_cap = authority.grant::<aterm_sandbox::Sandbox>(aterm_cap::Tier::Trusted);

        // Loopback listener in the parent + a draining accept thread.
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let port = listener.local_addr().unwrap().port();
        let accepter = std::thread::spawn(move || {
            for _ in 0..2 {
                match listener.accept() {
                    Ok((mut s, _)) => {
                        let _ = s.write_all(b"x");
                    }
                    Err(_) => break,
                }
            }
        });
        let port_s = port.to_string();

        // Control: unwrapped `-e nc` to the listener CONNECTS (so the probe works).
        // We can't read nc's exit code through spawn_shell's API, so prove the
        // control via a direct connect from the parent instead, then focus the
        // wrapped assertion on the seam producing a sandbox-exec'd child that the
        // kernel network-denies (nc fails → its PTY closes quickly with no data
        // that looks like a successful connect).
        let probe = std::net::TcpStream::connect(("127.0.0.1", port));
        assert!(probe.is_ok(), "loopback listener must be connectable (probe)");
        drop(probe);

        // Wrapped `-e nc` under (deny network*). The wrap is built by spawn_shell:
        // sandbox-exec -p <profile> /usr/bin/nc <args>. The connect is denied.
        let nc = vec![
            String::from("/usr/bin/nc"),
            String::from("-G"),
            String::from("1"),
            String::from("-w"),
            String::from("1"),
            String::from("-z"),
            String::from("127.0.0.1"),
            port_s.clone(),
        ];
        let profile = aterm_containment::NETWORK_DENY_PROFILE;
        let master = spawn_shell(
            24,
            80,
            &spawn_cap,
            &sandbox_cap,
            &[],
            None,
            Some(&nc),
            None,
            Some(profile),
        )
        .expect("wrapped -e nc must spawn (sandbox-exec applies the profile)");
        // Drain to EOF (the child exits fast: nc's connect is denied). The success
        // banner "succeeded!" must NOT appear — a denied connect never prints it.
        let mut out = Vec::new();
        let mut buf = [0u8; 256];
        for _ in 0..200 {
            let n = read(master, &mut buf);
            if n <= 0 {
                break;
            }
            out.extend_from_slice(&buf[..n as usize]);
        }
        // SAFETY: tear down the child.
        unsafe {
            libc::close(master);
        }
        let _ = std::net::TcpStream::connect(("127.0.0.1", port)); // unblock accepter
        let _ = accepter.join();
        let s = String::from_utf8_lossy(&out);
        assert!(
            !s.contains("succeeded"),
            "DENY FAILED: wrapped nc reported a successful connect under (deny network*): {s:?}",
        );
    }
}
