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
//! A thin `libc` driver (raw mode + poll) over the PROTECTED spawn seam. The
//! shell is launched via [`aterm_pty::spawn_shell`] — cap-gated, `setrlimit`-
//! bounded, fail-closed fork/exec, and OS-sandbox-wrapped when the containment
//! mode demands it (P0) — exactly like `aterm-gui`, NOT raw `forkpty`/`execvp`.
//! Daily-driver essentials are handled: window resize is forwarded (SIGWINCH ->
//! PTY + engine), the loop is signal-robust (EINTR), and aterm exits with the
//! shell's own status.
//!
//! Containment mode is launcher-owned (`ATERM_CONTAINMENT_MODE`, ATERM_DESIGN §5):
//! the default is `User` — no OS sandbox, so the daily-driver shell keeps full
//! network/credential access and behaves as before, now confined by the cap gate +
//! resource limits. `ATERM_CONTAINMENT_MODE=containment` opts into the macOS
//! Seatbelt sandbox (deny network + credential/private-data reads); a malformed
//! value fails CLOSED to Containment.

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

/// Polished `--help` text: synopsis, description, OPTIONS, ENVIRONMENT, EXAMPLES.
/// Mirrors `aterm-gui`'s `parse_cli()` help in tone and layout, scoped to what the
/// daily-driver CLI actually does (transparent passthrough of `$SHELL`).
const HELP: &str = concat!(
    "aterm — a transparent, introspecting terminal\n",
    "\n",
    "Spawns your $SHELL in a PTY and passes I/O through unchanged, so it looks and\n",
    "behaves exactly like your shell — while feeding every output byte into the aterm\n",
    "VT engine for a live, introspectable model. The shell runs through the PROTECTED\n",
    "spawn seam: cap-gated, setrlimit-bounded, fail-closed, and OS-sandbox-wrapped\n",
    "when the containment mode demands it.\n",
    "\n",
    "USAGE:\n",
    "    aterm [OPTIONS]\n",
    "\n",
    "OPTIONS:\n",
    "        --containment <MODE>  Containment mode: master, user, safety, or\n",
    "        --containment=<MODE>  containment (case-insensitive; space or = form).\n",
    "                              Overrides $ATERM_CONTAINMENT_MODE. An invalid value\n",
    "                              fails CLOSED to the most restrictive mode\n",
    "                              (containment).\n",
    "        --sandbox             Shorthand for --containment containment (deny\n",
    "                              network + credential reads via the macOS sandbox).\n",
    "        --no-sandbox          Shorthand for --containment user (no OS sandbox;\n",
    "                              full network/credential access — the default).\n",
    "    -h, --help                Print this help and exit.\n",
    "    -V, --version             Print the version and exit.\n",
    "\n",
    "PRECEDENCE:\n",
    "    explicit flag > $ATERM_CONTAINMENT_MODE > default (user). Among multiple\n",
    "    conflicting containment flags the LAST one wins, e.g.\n",
    "    `--sandbox --containment user` selects user.\n",
    "\n",
    "ENVIRONMENT:\n",
    "    ATERM_CONTAINMENT_MODE    Containment mode (master|user|safety|containment),\n",
    "                              consulted when no --containment flag is given.\n",
    "                              A malformed value fails CLOSED to containment.\n",
    "    ATERM_VERBOSE             If set, print a one-line session summary (bytes\n",
    "                              processed by the VT core) to stderr on exit.\n",
    "\n",
    "EXAMPLES:\n",
    "    aterm                              Start an interactive shell (mode: user).\n",
    "    aterm --sandbox                    Sandboxed shell: no network, no secrets.\n",
    "    aterm --containment master         Full-trust developer mode.\n",
    "    ATERM_CONTAINMENT_MODE=safety aterm  Allowlisted-operations mode via env.\n",
);

/// The outcome of parsing the command line: either print-and-exit (help/version),
/// reject (usage error), or proceed with an optional containment override that the
/// init funnel in `main()` will resolve.
#[derive(Debug, PartialEq, Eq)]
enum CliAction {
    /// `-h`/`--help`: print [`HELP`] to stdout, exit 0.
    Help,
    /// `-V`/`--version`: print the version to stdout, exit 0.
    Version,
    /// A usage error (unknown option, missing `--containment` value): the message
    /// is already framed for stderr; exit 2 without launching a shell.
    Usage(String),
    /// Proceed to launch. `containment` is the raw mode selection (if any) to hand
    /// to the init funnel verbatim via `$ATERM_CONTAINMENT_MODE`; `None` leaves the
    /// env untouched (env value, else default `User`, applies).
    Run { containment: Option<String> },
}

/// Pure argument-decision core (no process exit, no env mutation, no I/O): takes the
/// argv tail and returns a [`CliAction`]. Factored out so the precedence + fail-closed
/// edge cases are unit-testable; [`parse_args`] is the thin effectful wrapper.
///
/// Recognizes `-h`/`--help`, `-V`/`--version`, and the containment selection:
/// `--containment <MODE>` (space form) or `--containment=<MODE>` (`=` form), plus the
/// convenience `--sandbox` (= containment) and `--no-sandbox` (= user). A bare `--`
/// ends option parsing; since `aterm` takes no positional operands, anything after it
/// that begins with `-` is no longer treated as a flag (a trailing non-flag operand is
/// still rejected, as `aterm` accepts none).
///
/// Precedence is deterministic and total:
///   explicit flag  >  $ATERM_CONTAINMENT_MODE  >  default `User`,
/// and among MULTIPLE conflicting flags the LAST one on the line wins (standard
/// last-flag-wins, e.g. `--sandbox --containment user` selects `user`; `--no-sandbox
/// --sandbox` selects sandbox/containment). Only the surviving selection is returned,
/// so the caller hands exactly one value to the single init funnel.
///
/// An INVALID `--containment <value>` is carried through verbatim (NOT validated here),
/// so it reaches `main()`'s `init_mode_from_env` and fails CLOSED to Containment with
/// the identical message as the env path — never a parallel/bypass validation.
fn decide_args<I: Iterator<Item = String>>(args: I) -> CliAction {
    let mut containment: Option<String> = None;
    let mut opts_ended = false; // set by a literal `--`
    let mut args = args;
    while let Some(arg) = args.next() {
        if !opts_ended {
            match arg.as_str() {
                "-h" | "--help" => return CliAction::Help,
                "-V" | "--version" => return CliAction::Version,
                "--" => {
                    opts_ended = true;
                    continue;
                }
                "--containment" => {
                    let Some(val) = args.next() else {
                        return CliAction::Usage(
                            "aterm: --containment requires a mode (try --help)".to_string(),
                        );
                    };
                    containment = Some(val);
                    continue;
                }
                // `=` form: `--containment=<MODE>`. An empty value (`--containment=`)
                // is carried through verbatim so it fails CLOSED in the init funnel,
                // exactly like an invalid mode — never silently ignored.
                _ if arg.starts_with("--containment=") => {
                    containment = Some(arg["--containment=".len()..].to_string());
                    continue;
                }
                "--sandbox" => {
                    containment = Some("containment".to_string());
                    continue;
                }
                "--no-sandbox" => {
                    containment = Some("user".to_string());
                    continue;
                }
                _ => {}
            }
        }
        // Either an unrecognized option, or any operand after `--`: `aterm` accepts
        // no positional operands, so reject with exit-2 usage rather than ignoring it.
        return CliAction::Usage(format!("aterm: unknown option {arg} (try --help)"));
    }
    CliAction::Run { containment }
}

/// Dependency-free argument parser for the daily-driver CLI — the effectful shell
/// around [`decide_args`]: prints help/version and exits 0, prints a usage error and
/// exits 2, or normalizes the containment selection onto `$ATERM_CONTAINMENT_MODE`
/// (so the SINGLE init funnel in `main()` resolves it) and returns.
///
/// With no args (a Finder/.app launch) this is a no-op and a normal interactive shell
/// starts, unchanged.
fn parse_args() {
    match decide_args(std::env::args().skip(1)) {
        CliAction::Help => {
            print!("{HELP}");
            std::process::exit(0);
        }
        CliAction::Version => {
            println!("aterm {}", env!("CARGO_PKG_VERSION"));
            std::process::exit(0);
        }
        CliAction::Usage(msg) => {
            eprintln!("{msg}");
            std::process::exit(2);
        }
        CliAction::Run { containment: None } => {}
        CliAction::Run {
            containment: Some(val),
        } => {
            // Hand the selection to the init funnel by setting the env var it reads:
            // explicit flag thus beats any pre-existing $ATERM_CONTAINMENT_MODE, and a
            // bad value fails CLOSED through the exact same `init_mode_from_env` path.
            // SAFETY: single-threaded startup, before any thread is spawned or any PTY
            // byte flows; this is the trusted launcher establishing the mode.
            unsafe { std::env::set_var("ATERM_CONTAINMENT_MODE", val) };
        }
    }
}

fn main() {
    // CLI first: `--help`/`--version` print and exit before any setup; the
    // containment flags are normalized onto $ATERM_CONTAINMENT_MODE so the single
    // init funnel below resolves them (precedence: explicit flag > env > default).
    // A Finder/.app launch passes no args, so this is a no-op and a normal
    // interactive shell starts.
    parse_args();

    let ws = host_winsize();
    let (rows, cols) = (ws.ws_row, ws.ws_col);

    // P0 — the daily-driver CLI runs the shell through the PROTECTED spawn seam,
    // closing the gap where the shipped binary ran `forkpty`/`execvp` with ZERO
    // confinement while only aterm-gui used the protected path.
    //
    // Containment mode is launcher-owned. Default `User`: no OS sandbox, the shell
    // keeps full network/credential access (byte-for-byte daily behavior) and is
    // confined only by the cap gate + setrlimit. `ATERM_CONTAINMENT_MODE=containment`
    // opts into the macOS Seatbelt sandbox; a MALFORMED value fails CLOSED to
    // Containment (never silently disables confinement).
    let mode = aterm_containment::init_mode_from_env(aterm_containment::ContainmentMode::User)
        .unwrap_or_else(|e| {
            eprintln!("aterm: invalid ATERM_CONTAINMENT_MODE ({e}); failing closed to Containment");
            let _ = aterm_containment::init_mode(aterm_containment::ContainmentMode::Containment);
            aterm_containment::ContainmentMode::Containment
        });

    // Ask the actuator whether the shell may spawn for this mode and, for
    // Containment on macOS, the SBPL profile the spawn must be wrapped in. A `Deny`
    // (or any future variant) fails closed: no unconfined shell.
    let sandbox_wrap: Option<String> = match aterm_containment::decide_spawn(mode) {
        aterm_containment::SpawnDecision::Permit { sbpl, .. } => sbpl,
        other => {
            debug_assert!(matches!(
                other,
                aterm_containment::SpawnDecision::Deny { .. }
            ));
            eprintln!(
                "aterm: containment mode {mode} denies spawning a shell (fail-closed); \
                 refusing to start an unconfined child"
            );
            std::process::exit(1);
        }
    };

    // The SINGLE `unsafe` root-authority mint in this binary (CAP-1): trusted
    // launcher, before any PTY bytes flow. Grants the spawn + sandbox capabilities
    // the protected seam requires.
    // SAFETY: trusted process entry point, reached exactly once before the spawn
    // and before any untrusted PTY input is read.
    let authority = unsafe { aterm_cap::Authority::root_authority() };
    let spawn_cap = authority.grant::<aterm_cap::effects::Spawn>(aterm_cap::Tier::Trusted);
    let sandbox_cap = authority.grant::<aterm_sandbox::Sandbox>(aterm_cap::Tier::Trusted);

    // PROTECTED spawn: cap-gated, setrlimit-bounded in the child before execve,
    // fail-closed, and OS-sandbox-wrapped when `sandbox_wrap` is `Some`. Returns the
    // PTY master fd. The seam itself fails closed if a demanded sandbox wrapper is
    // missing (it refuses to spawn an unsandboxed shell).
    let master = aterm_pty::spawn_shell(
        rows,
        cols,
        &spawn_cap,
        &sandbox_cap,
        &[],  // env_add: none — transparent passthrough
        None, // argv_override
        None, // exec_command — interactive $SHELL
        None, // cwd — inherit
        sandbox_wrap.as_deref(),
    )
    .unwrap_or_else(|e| {
        eprintln!("aterm: protected spawn failed ({e}); refusing to start an unconfined shell");
        std::process::exit(1);
    });

    // PARENT.
    let stdin_is_tty = unsafe { libc::isatty(libc::STDIN_FILENO) } == 1;
    let orig = if stdin_is_tty {
        Some(set_raw(libc::STDIN_FILENO))
    } else {
        None
    };
    // Cast through a function pointer (not a direct fn-item-to-int cast) so the
    // `fn_to_numeric_cast` lint is satisfied while still yielding the address
    // libc::signal expects as its sighandler_t.
    unsafe {
        libc::signal(
            libc::SIGWINCH,
            on_winch as extern "C" fn(libc::c_int) as usize,
        )
    };

    let mut engine = Terminal::new(rows, cols);
    let mut bytes_in: u64 = 0;

    let mut fds = [
        libc::pollfd {
            fd: libc::STDIN_FILENO,
            events: libc::POLLIN,
            revents: 0,
        },
        libc::pollfd {
            fd: master,
            events: libc::POLLIN,
            revents: 0,
        },
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
                libc::read(
                    libc::STDIN_FILENO,
                    buf.as_mut_ptr() as *mut libc::c_void,
                    buf.len(),
                )
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
            let r = unsafe { libc::read(master, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
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
        // The protected spawn returns only the master fd; the shell (or the
        // sandbox-exec wrapper, which exits with the shell's status) is this
        // process's sole direct child, so reap it with `-1` to recover the exit
        // code and avoid a zombie.
        libc::waitpid(-1, &mut status, 0);
    }
    if std::env::var_os("ATERM_VERBOSE").is_some() {
        eprintln!("\r\n[aterm] session ended — engine processed {bytes_in} bytes via the VT core.");
    }
    let code = if libc::WIFEXITED(status) {
        libc::WEXITSTATUS(status)
    } else {
        1
    };
    std::process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{CliAction, decide_args};

    fn decide(args: &[&str]) -> CliAction {
        decide_args(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn no_args_runs_with_no_override() {
        // A Finder/.app launch: no flags → launch, env/default decides the mode.
        assert_eq!(decide(&[]), CliAction::Run { containment: None });
    }

    #[test]
    fn help_and_version_short_and_long() {
        for a in [["-h"], ["--help"]] {
            assert_eq!(decide(&a), CliAction::Help);
        }
        for a in [["-V"], ["--version"]] {
            assert_eq!(decide(&a), CliAction::Version);
        }
    }

    #[test]
    fn containment_space_form() {
        assert_eq!(
            decide(&["--containment", "master"]),
            CliAction::Run {
                containment: Some("master".into())
            }
        );
    }

    #[test]
    fn containment_eq_form() {
        // `=` syntax must be accepted and carried through identically to the space form.
        assert_eq!(
            decide(&["--containment=user"]),
            CliAction::Run {
                containment: Some("user".into())
            }
        );
    }

    #[test]
    fn containment_eq_empty_value_is_carried_through_not_ignored() {
        // `--containment=` → empty string, which fails CLOSED in the init funnel
        // exactly like an invalid mode; it must NOT be silently dropped.
        assert_eq!(
            decide(&["--containment="]),
            CliAction::Run {
                containment: Some(String::new())
            }
        );
    }

    #[test]
    fn containment_missing_value_is_usage_error_not_swallow() {
        // Trailing `--containment` with no value: a usage error (exit 2), never a
        // silent run nor a panic.
        match decide(&["--containment"]) {
            CliAction::Usage(m) => assert!(m.contains("--containment requires a mode"), "{m}"),
            other => panic!("expected Usage, got {other:?}"),
        }
    }

    #[test]
    fn invalid_mode_is_carried_through_for_fail_closed_not_validated_here() {
        // The parser does NOT validate the mode; it hands the garbage to the single
        // init funnel, which fails CLOSED. So decide_args still returns Run(Some(..)).
        assert_eq!(
            decide(&["--containment", "xyz"]),
            CliAction::Run {
                containment: Some("xyz".into())
            }
        );
    }

    #[test]
    fn sandbox_and_no_sandbox_map_to_modes() {
        assert_eq!(
            decide(&["--sandbox"]),
            CliAction::Run {
                containment: Some("containment".into())
            }
        );
        assert_eq!(
            decide(&["--no-sandbox"]),
            CliAction::Run {
                containment: Some("user".into())
            }
        );
    }

    #[test]
    fn conflicting_flags_last_one_wins() {
        // Documented precedence among flags: last on the line wins.
        assert_eq!(
            decide(&["--sandbox", "--containment", "user"]),
            CliAction::Run {
                containment: Some("user".into())
            }
        );
        assert_eq!(
            decide(&["--containment", "user", "--sandbox"]),
            CliAction::Run {
                containment: Some("containment".into())
            }
        );
        assert_eq!(
            decide(&["--no-sandbox", "--sandbox"]),
            CliAction::Run {
                containment: Some("containment".into())
            }
        );
    }

    #[test]
    fn unknown_flag_is_usage_error() {
        match decide(&["--bogus"]) {
            CliAction::Usage(m) => assert!(m.contains("unknown option --bogus"), "{m}"),
            other => panic!("expected Usage, got {other:?}"),
        }
    }

    #[test]
    fn double_dash_ends_options_and_bare_form_runs() {
        // A lone `--` ends option parsing; aterm takes no operands, so a bare `--`
        // is a clean no-op run (not an "unknown option").
        assert_eq!(decide(&["--"]), CliAction::Run { containment: None });
        // Flags BEFORE `--` still apply.
        assert_eq!(
            decide(&["--sandbox", "--"]),
            CliAction::Run {
                containment: Some("containment".into())
            }
        );
    }

    #[test]
    fn operand_after_double_dash_is_rejected() {
        // aterm accepts no positional operands; one after `--` is still a usage error
        // (so a stray path can't be silently swallowed), and a `-`-prefixed token
        // after `--` is treated as that same operand, NOT re-parsed as a flag.
        match decide(&["--", "extra"]) {
            CliAction::Usage(m) => assert!(m.contains("unknown option extra"), "{m}"),
            other => panic!("expected Usage, got {other:?}"),
        }
        match decide(&["--", "--help"]) {
            CliAction::Usage(m) => assert!(m.contains("unknown option --help"), "{m}"),
            other => panic!("expected Usage (post-`--` is an operand), got {other:?}"),
        }
    }
}
