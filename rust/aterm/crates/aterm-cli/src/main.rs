// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Andrew Yates

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
    "    aterm [OPTIONS]            Start an interactive shell (the default).\n",
    "    aterm <SUBCOMMAND>         Print diagnostics and exit (see SUBCOMMANDS).\n",
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
    "SUBCOMMANDS (print info and exit; no shell is spawned):\n",
    "    show-config               Print aterm's effective runtime configuration.\n",
    "    validate-config           Validate the config; exit non-zero on error.\n",
    "    explain-config            Explain how aterm resolves its configuration.\n",
    "    doctor                    Pre-flight health check; exit non-zero on a problem.\n",
    "    list-fonts                List available font families.\n",
    "    show-face <family>        Show metrics for a font family.\n",
    "    list-themes               List the built-in colour schemes.\n",
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
    "    aterm --sandbox                    Containment mode (macOS: deny network +\n",
    "                                       secret-dir read; Linux today: rlimit +\n",
    "                                       capability gate only — prints a notice).\n",
    "    aterm --containment master         Full-trust developer mode.\n",
    "    ATERM_CONTAINMENT_MODE=safety aterm  Allowlisted-operations mode via env.\n",
);

/// Diagnostic subcommands (CLI-DIAG): `aterm <name>` prints introspection about
/// aterm's own configuration/environment and exits 0 WITHOUT spawning a shell.
///
/// Implemented: config introspection (`show-config` / `validate-config` /
/// `explain-config`), a `doctor` pre-flight health check, and the read-only
/// enumerators `list-fonts` / `show-face` / `list-themes` (backed by aterm-render +
/// aterm-types). `list-keybinds` is deliberately NOT here: keybindings are an
/// aterm-gui concept; the transparent passthrough binary has no keymap, so it would
/// belong in aterm-gui, not a false affordance here.
///
/// This is the SINGLE source of truth: [`diag_report`] must handle every entry
/// AND [`HELP`] must advertise every entry — both enforced by the
/// `diag_commands_advertised_and_dispatchable` gate, so a subcommand can never
/// ship undocumented or unimplemented.
const DIAG_COMMANDS: &[(&str, &str)] = &[
    (
        "show-config",
        "Print aterm's effective runtime configuration.",
    ),
    (
        "validate-config",
        "Validate the effective config; exit non-zero on error.",
    ),
    (
        "explain-config",
        "Explain how aterm resolves its configuration.",
    ),
    (
        "doctor",
        "Run an aggregate pre-flight health check; exit non-zero on any problem.",
    ),
    ("list-fonts", "List available font families (one per line)."),
    (
        "show-face",
        "Show metrics for a font family (usage: aterm show-face <family>).",
    ),
    (
        "list-themes",
        "List the built-in colour schemes and their descriptions.",
    ),
];

/// Build the `(report, exit_code)` for diagnostic subcommand `cmd` (with an
/// optional positional `arg`, e.g. the family for `show-face`), or `None` if `cmd`
/// is not a registry command. Read-only: consults env / the controlling terminal /
/// the system font + theme registries WITHOUT actuating containment or spawning a
/// shell, so it is safe to run anywhere and unit-testable. A non-zero code
/// (`validate-config`/`doctor`/`show-face` on bad input) makes them scriptable.
fn diag_report(cmd: &str, arg: Option<&str>) -> Option<(String, i32)> {
    match cmd {
        "show-config" => Some((show_config_report(), 0)),
        "validate-config" => Some(validate_config_report()),
        "explain-config" => Some((explain_config_report(), 0)),
        "doctor" => Some(doctor_report()),
        "list-fonts" => Some((list_fonts_report(), 0)),
        "show-face" => Some(show_face_report(arg)),
        "list-themes" => Some((list_themes_report(), 0)),
        _ => None,
    }
}

/// `aterm show-config` — aterm's effective runtime configuration as stable
/// `key=value` lines (one per line, scriptable). Reports the raw inputs the
/// launcher resolves the containment mode from (env value + the fail-closed
/// default), NOT an actuated mode — `show-config` never actuates or spawns.
fn show_config_report() -> String {
    let ws = host_winsize();
    let env = |k: &str| std::env::var(k).unwrap_or_default();
    let or = |s: String, dflt: &str| if s.is_empty() { dflt.to_string() } else { s };
    let mut out = String::new();
    out.push_str(&format!("version={}\n", env!("CARGO_PKG_VERSION")));
    out.push_str(&format!("shell={}\n", or(env("SHELL"), "(unset)")));
    out.push_str(&format!("term={}\n", or(env("TERM"), "(unset)")));
    out.push_str(&format!("rows={}\n", ws.ws_row));
    out.push_str(&format!("cols={}\n", ws.ws_col));
    out.push_str(&format!(
        "containment_mode_env={}\n",
        or(env("ATERM_CONTAINMENT_MODE"), "(unset)")
    ));
    out.push_str("containment_default=user\n");
    out.push_str(&format!(
        "verbose={}\n",
        if std::env::var_os("ATERM_VERBOSE").is_some() {
            "on"
        } else {
            "off"
        }
    ));
    out
}

/// `aterm validate-config` — validate the effective configuration WITHOUT actuating
/// anything, then exit (0 = valid, non-zero = invalid). Today the one fail-closed
/// knob is the containment mode: a malformed `$ATERM_CONTAINMENT_MODE` would force
/// aterm to the most restrictive mode at launch, so surface it here instead. Reads
/// the env, then delegates to the pure [`validate_containment_value`]. Scriptable:
/// `aterm validate-config && aterm`.
fn validate_config_report() -> (String, i32) {
    validate_containment_value(std::env::var("ATERM_CONTAINMENT_MODE").ok().as_deref())
}

/// Pure core of `validate-config` (env-free, so it is deterministically testable):
/// validate a containment-mode selection. `None` = unset (the default `user`
/// applies — valid). Parsing is the SAME public `ContainmentMode::FromStr` the
/// launcher uses, so this can never disagree with the real init funnel.
fn validate_containment_value(v: Option<&str>) -> (String, i32) {
    match v {
        None => (
            "OK: ATERM_CONTAINMENT_MODE unset (default: user)\n".to_string(),
            0,
        ),
        Some(s) => match s.parse::<aterm_containment::ContainmentMode>() {
            Ok(mode) => (
                format!("OK: containment_mode={mode} (ATERM_CONTAINMENT_MODE={s:?})\n"),
                0,
            ),
            Err(e) => (format!("ERR: {e}\n"), 1),
        },
    }
}

/// `aterm explain-config` — explain how aterm resolves its configuration: the
/// precedence rule, the containment modes (least → most capability), and the
/// environment variables consulted. Read-only static reference text.
fn explain_config_report() -> String {
    let mut out = String::new();
    out.push_str("aterm configuration resolution\n\n");
    out.push_str("Containment mode precedence (most-specific wins):\n");
    out.push_str("  1. --containment <mode> / --sandbox / --no-sandbox (CLI flag)\n");
    out.push_str("  2. $ATERM_CONTAINMENT_MODE (environment)\n");
    out.push_str("  3. default: user\n");
    out.push_str(
        "  A malformed value fails CLOSED to the most restrictive mode (containment).\n\n",
    );
    out.push_str("Containment modes (least → most capability):\n");
    out.push_str(
        "  containment  Hostile-agent: OS-enforced network + credential denial (macOS Seatbelt).\n",
    );
    out.push_str("  safety       Reduced capability: allowlisted operations only.\n");
    out.push_str("  user         Normal usage: standard safeguards (the default).\n");
    out.push_str("  master       Full trust: developer mode.\n\n");
    out.push_str("Environment variables:\n");
    out.push_str(
        "  ATERM_CONTAINMENT_MODE  containment mode when no --containment flag is given.\n",
    );
    out.push_str("  ATERM_VERBOSE           print a one-line session summary to stderr on exit.\n");
    out
}

/// `aterm list-fonts` — available font families (file stems), one per line, sorted
/// and deduplicated for scriptable output. Data: [`aterm_render::list_fonts`].
fn list_fonts_report() -> String {
    let fonts = aterm_render::list_fonts();
    if fonts.is_empty() {
        return "(no fonts found)\n".to_string();
    }
    let mut out = String::new();
    for f in fonts {
        out.push_str(&f);
        out.push('\n');
    }
    out
}

/// `aterm show-face <family>` — the resolved path + cell metrics for a font family
/// as stable `key=value` lines. Exit 1 (usage / not-found) when `family` is absent
/// or unresolvable. Data: [`aterm_render::face_info`].
fn show_face_report(family: Option<&str>) -> (String, i32) {
    let Some(family) = family else {
        return ("ERR: usage: aterm show-face <family>\n".to_string(), 1);
    };
    match aterm_render::face_info(family) {
        Some(info) => (
            format!(
                "family={family}\npath={}\ncell_width={}\ncell_height={}\nbaseline={}\nglyph_count={}\n",
                info.path, info.cell_width, info.cell_height, info.baseline, info.glyph_count
            ),
            0,
        ),
        None => (
            format!("ERR: could not resolve or load font family {family:?}\n"),
            1,
        ),
    }
}

/// `aterm list-themes` — the built-in colour schemes + one-line descriptions.
/// Data: [`aterm_types::scheme::builtin_themes`].
fn list_themes_report() -> String {
    let mut out = String::from("Built-in colour schemes:\n\n");
    for (name, desc) in aterm_types::scheme::builtin_themes() {
        out.push_str(&format!("{name:<18} {desc}\n"));
    }
    out
}

/// `aterm doctor` — an aggregate pre-flight health check (containment validity,
/// $SHELL set+executable, stdout is a tty, plus version/size). Reads env/fs/tty,
/// then delegates to the pure [`doctor_checks`]. Exit 0 = all pass; non-zero = a
/// problem. Scriptable: `aterm doctor && aterm`.
fn doctor_report() -> (String, i32) {
    let shell = std::env::var("SHELL").ok();
    let shell_exec = shell.as_deref().is_some_and(shell_is_executable);
    let containment = std::env::var("ATERM_CONTAINMENT_MODE").ok();
    let is_tty = unsafe { libc::isatty(libc::STDOUT_FILENO) } == 1;
    let ws = host_winsize();
    doctor_checks(
        shell.as_deref(),
        shell_exec,
        containment.as_deref(),
        is_tty,
        ws.ws_row,
        ws.ws_col,
    )
}

/// Whether `path` exists and is executable (`X_OK`). Conservative: any `access(2)`
/// error (missing, not executable, embedded NUL) → `false` — a non-runnable shell
/// is a failed check.
fn shell_is_executable(path: &str) -> bool {
    let Ok(c) = std::ffi::CString::new(path) else {
        return false;
    };
    unsafe { libc::access(c.as_ptr(), libc::X_OK) == 0 }
}

/// Pure core of `doctor` (all state is an input, so it is deterministically
/// testable): aggregate the pre-flight checks into a stable report + exit code
/// (0 = all pass, 1 = any fail). Containment validity reuses
/// [`validate_containment_value`], so `doctor` can't disagree with the launcher.
fn doctor_checks(
    shell: Option<&str>,
    shell_executable: bool,
    containment: Option<&str>,
    is_tty: bool,
    rows: u16,
    cols: u16,
) -> (String, i32) {
    let (cont_detail, cont_code) = validate_containment_value(containment);
    let cont_ok = cont_code == 0;

    let (shell_ok, shell_detail) = match shell {
        None => (false, "shell: $SHELL unset".to_string()),
        Some("") => (false, "shell: $SHELL is empty".to_string()),
        Some(p) if shell_executable => (true, format!("shell: {p} (executable)")),
        Some(p) => (false, format!("shell: {p} (not executable or missing)")),
    };

    let (tty_ok, tty_detail) = if is_tty {
        (true, "tty: stdout is a terminal".to_string())
    } else {
        (
            false,
            "tty: stdout is not a terminal (headless/piped)".to_string(),
        )
    };

    let all_ok = cont_ok && shell_ok && tty_ok;
    let mark = |ok: bool| if ok { "ok" } else { "FAIL" };

    let mut out = String::new();
    out.push_str(&format!("containment: {}\n", mark(cont_ok)));
    out.push_str(&format!("shell:       {}\n", mark(shell_ok)));
    out.push_str(&format!("tty:         {}\n", mark(tty_ok)));
    out.push('\n');
    // Strip validate-config's `OK:`/`ERR:` prefix so every detail line shares the
    // uniform `key: …` shape (a single `^key:` grep then matches all of them).
    let cont_trim = cont_detail.trim_end();
    let cont_body = cont_trim
        .strip_prefix("OK: ")
        .or_else(|| cont_trim.strip_prefix("ERR: "))
        .unwrap_or(cont_trim);
    out.push_str(&format!("containment: {cont_body}\n"));
    out.push_str(&shell_detail);
    out.push('\n');
    out.push_str(&tty_detail);
    out.push('\n');
    out.push_str(&format!(
        "version: {} ({cols}x{rows})\n\n",
        env!("CARGO_PKG_VERSION")
    ));
    out.push_str(if all_ok {
        "health: OK — aterm is ready to run\n"
    } else {
        "health: FAIL — one or more checks did not pass\n"
    });
    (out, i32::from(!all_ok))
}

/// The outcome of parsing the command line: either print-and-exit (help/version),
/// reject (usage error), or proceed with an optional containment override that the
/// init funnel in `main()` will resolve.
#[derive(Debug, PartialEq, Eq)]
enum CliAction {
    /// `-h`/`--help`: print [`HELP`] to stdout, exit 0.
    Help,
    /// `-V`/`--version`: print the version to stdout, exit 0.
    Version,
    /// A diagnostic subcommand (e.g. `show-config`, or `show-face <family>`): print
    /// introspection and exit WITHOUT spawning a shell. `cmd` is a registry-validated
    /// name (one of [`DIAG_COMMANDS`]); `arg` is the optional positional operand
    /// after it (the family for `show-face`).
    Diag { cmd: String, arg: Option<String> },
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
    let mut args = args.peekable();
    // A leading diagnostic subcommand (`aterm show-config`) dispatches BEFORE any
    // flag parsing — git-style: the subcommand is the first operand. It prints
    // introspection and exits without spawning a shell. Anything else (flags, or an
    // unknown first operand) falls through to the existing option parser.
    if let Some(first) = args.peek()
        && DIAG_COMMANDS
            .iter()
            .any(|(name, _)| *name == first.as_str())
    {
        let cmd = args.next().expect("peeked Some");
        // The optional positional operand after the subcommand (e.g. the family for
        // `show-face <family>`). Subcommands that take none simply ignore it.
        let arg = args.next();
        return CliAction::Diag { cmd, arg };
    }
    let mut containment: Option<String> = None;
    let mut opts_ended = false; // set by a literal `--`
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
        CliAction::Diag { cmd, arg } => {
            // `decide_args` only emits registry names, so `diag_report` is Some.
            match diag_report(&cmd, arg.as_deref()) {
                // Success goes to stdout; a non-zero result (e.g. validate-config on
                // a bad mode) goes to stderr and sets the exit code, so it scripts.
                Some((report, 0)) => {
                    print!("{report}");
                    std::process::exit(0);
                }
                Some((report, code)) => {
                    eprint!("{report}");
                    std::process::exit(code);
                }
                None => {
                    eprintln!("aterm: unknown subcommand {cmd} (try --help)");
                    std::process::exit(2);
                }
            }
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

    // HONESTY (mirrors aterm-gui's startup line): in a CONFINEMENT mode whose OS
    // sandbox is not actuated on this platform — today every non-macOS target, which
    // has no Landlock/seccomp lane yet — say so on stderr. Otherwise a user invoking
    // `aterm --sandbox` to cage a (possibly hostile) agent shell would be silently
    // unprotected and uninformed: only the rlimit + capability gate apply, NOT the
    // network/filesystem confinement the mode name implies.
    if !aterm_containment::os_sandbox_actuated()
        && matches!(
            mode,
            aterm_containment::ContainmentMode::Containment
                | aterm_containment::ContainmentMode::Safety
        )
    {
        eprintln!(
            "aterm: containment mode {mode}: OS sandbox NOT actuated on this platform \
             (rlimits + capability gate only; NO network/filesystem confinement). \
             See aterm-containment::actuator."
        );
    }

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
    use super::{
        CliAction, DIAG_COMMANDS, HELP, decide_args, diag_report, doctor_checks,
        explain_config_report, list_fonts_report, list_themes_report, show_face_report,
        validate_containment_value,
    };

    fn decide(args: &[&str]) -> CliAction {
        decide_args(args.iter().map(|s| s.to_string()))
    }

    fn diag(cmd: &str, arg: Option<&str>) -> CliAction {
        CliAction::Diag {
            cmd: cmd.to_string(),
            arg: arg.map(str::to_string),
        }
    }

    #[test]
    fn diag_subcommand_recognized_as_first_operand() {
        // `aterm show-config` → Diag, dispatched before flag parsing / shell spawn.
        assert_eq!(decide(&["show-config"]), diag("show-config", None));
        // A positional operand after the subcommand is captured as `arg`
        // (`aterm show-face Menlo`); commands that take none simply ignore it.
        assert_eq!(
            decide(&["show-face", "Menlo"]),
            diag("show-face", Some("Menlo"))
        );
    }

    #[test]
    fn diag_subcommand_only_as_first_operand_not_after_flags() {
        // A subcommand name after a flag is NOT a subcommand (git-style: first only),
        // so it falls through to the option parser and is rejected as unknown.
        assert!(matches!(
            decide(&["--no-sandbox", "show-config"]),
            CliAction::Usage(_)
        ));
    }

    /// THE CLI-DIAG gate (`cli-subcommands-advertised`): every registry command must
    /// be BOTH advertised in `--help` AND dispatchable — so a new subcommand cannot
    /// ship undocumented or unimplemented (the test fails if either regresses).
    #[test]
    fn diag_commands_advertised_and_dispatchable() {
        assert!(!DIAG_COMMANDS.is_empty(), "registry must not be empty");
        for (name, _desc) in DIAG_COMMANDS {
            // Advertised as its OWN SUBCOMMANDS line (a leading token), not merely a
            // loose substring — so a future name that is a substring of another
            // can't pass vacuously.
            assert!(
                HELP.lines().any(|l| l.trim_start().starts_with(name)),
                "subcommand {name:?} is not advertised as a --help line"
            );
            assert!(
                diag_report(name, None).is_some(),
                "subcommand {name:?} is in the registry but has no dispatch"
            );
            // And it must actually be recognized by the parser.
            assert_eq!(decide(&[name]), diag(name, None));
        }
        // A non-registry name is NOT dispatchable.
        assert!(diag_report("definitely-not-a-command", None).is_none());
    }

    #[test]
    fn show_config_report_has_stable_keys() {
        let (r, code) = diag_report("show-config", None).expect("show-config dispatches");
        assert_eq!(code, 0, "show-config always succeeds");
        for key in [
            "version=",
            "shell=",
            "term=",
            "rows=",
            "cols=",
            "containment_mode_env=",
            "containment_default=user",
            "verbose=",
        ] {
            assert!(
                r.contains(key),
                "show-config missing {key:?}\n--- report ---\n{r}"
            );
        }
        // Stable, scriptable shape: every non-empty line is `key=value`.
        for line in r.lines().filter(|l| !l.is_empty()) {
            assert!(line.contains('='), "non key=value line: {line:?}");
        }
    }

    #[test]
    fn validate_config_exit_codes() {
        // Unset → valid (default user applies), exit 0.
        let (msg, code) = validate_containment_value(None);
        assert_eq!(code, 0, "{msg}");
        assert!(msg.starts_with("OK"), "{msg}");

        // Every accepted mode (case-insensitive) is valid, exit 0.
        for m in ["master", "user", "safety", "containment", "MASTER", "User"] {
            let (msg, code) = validate_containment_value(Some(m));
            assert_eq!(code, 0, "mode {m:?} should be valid: {msg}");
            assert!(msg.starts_with("OK"), "{msg}");
        }

        // Bad / empty values → invalid, exit 1, with a message naming the modes.
        for bad in ["bogus", "", "use", "containmnt"] {
            let (msg, code) = validate_containment_value(Some(bad));
            assert_eq!(code, 1, "value {bad:?} should be invalid: {msg}");
            assert!(msg.starts_with("ERR"), "{msg}");
            assert!(
                msg.contains("master") && msg.contains("containment"),
                "error must name the accepted modes: {msg}"
            );
        }
    }

    #[test]
    fn explain_config_names_modes_and_precedence() {
        let r = explain_config_report();
        for needle in [
            "precedence",
            "ATERM_CONTAINMENT_MODE",
            "master",
            "user",
            "safety",
            "containment",
            "fails CLOSED",
        ] {
            assert!(r.contains(needle), "explain-config missing {needle:?}\n{r}");
        }
    }

    #[test]
    fn show_face_requires_and_validates_family() {
        // No family → usage error, exit 1.
        let (msg, code) = show_face_report(None);
        assert_eq!(code, 1);
        assert!(msg.contains("usage"), "{msg}");
        // An unresolvable family → not-found error, exit 1 (the positive path is
        // covered by aterm-render's face_info test against a real system font).
        let (msg, code) = show_face_report(Some("definitely-not-a-real-font-xyzzy"));
        assert_eq!(code, 1);
        assert!(msg.starts_with("ERR"), "{msg}");
    }

    #[test]
    fn doctor_checks_pass_and_flag_each_failure() {
        // /bin/sh is executable on every POSIX host; all checks pass.
        let (r, code) = doctor_checks(Some("/bin/sh"), true, Some("user"), true, 24, 80);
        assert_eq!(code, 0, "{r}");
        assert!(r.contains("health: OK"), "{r}");
        assert!(
            r.contains("(executable)") && r.contains("stdout is a terminal"),
            "{r}"
        );
        assert!(r.contains("version: ") && r.contains("80x24"), "{r}");

        // Shell not executable → fail.
        let (r, code) = doctor_checks(Some("/no/such/shell"), false, Some("user"), true, 24, 80);
        assert_eq!(code, 1);
        assert!(
            r.contains("health: FAIL") && r.contains("not executable or missing"),
            "{r}"
        );

        // $SHELL unset → fail.
        let (r, code) = doctor_checks(None, false, Some("user"), true, 24, 80);
        assert_eq!(code, 1);
        assert!(r.contains("$SHELL unset"), "{r}");

        // No tty (headless/piped) → fail.
        let (r, code) = doctor_checks(Some("/bin/sh"), true, None, false, 24, 80);
        assert_eq!(code, 1);
        assert!(r.contains("health: FAIL") && r.contains("headless"), "{r}");

        // Bad containment mode → fail (reuses validate-config's verdict).
        let (r, code) = doctor_checks(Some("/bin/sh"), true, Some("bogus"), true, 24, 80);
        assert_eq!(code, 1);
        assert!(
            r.contains("health: FAIL") && r.contains("invalid containment mode"),
            "{r}"
        );

        // The detail block is uniform `key: …` lines (no stray `OK:`/`ERR:` prefix).
        let (r, _) = doctor_checks(Some("/bin/sh"), true, Some("user"), true, 24, 80);
        for key in ["containment: ", "shell: ", "tty: ", "version: "] {
            assert!(
                r.lines().any(|l| l.starts_with(key)),
                "doctor detail missing a {key:?} line\n{r}"
            );
        }
    }

    #[test]
    fn show_face_success_path_emits_metrics() {
        // Pick any real, resolvable font and assert the key=value metrics shape.
        // Skips cleanly on a host with no resolvable fonts.
        let Some(family) = aterm_render::list_fonts()
            .into_iter()
            .find(|f| aterm_render::face_info(f).is_some())
        else {
            eprintln!("SKIP: no resolvable system font");
            return;
        };
        let (r, code) = show_face_report(Some(&family));
        assert_eq!(code, 0, "{r}");
        for key in [
            "family=",
            "path=",
            "cell_width=",
            "cell_height=",
            "baseline=",
            "glyph_count=",
        ] {
            assert!(r.contains(key), "show-face missing {key:?}\n{r}");
        }
    }

    #[test]
    fn list_fonts_report_shape() {
        let r = list_fonts_report();
        // Either the sentinel, or a newline-terminated list of non-empty lines that
        // matches the enumeration exactly (deterministic, scriptable).
        if r == "(no fonts found)\n" {
            return;
        }
        assert!(r.ends_with('\n'), "must be newline-terminated");
        let lines: Vec<&str> = r.lines().collect();
        assert!(lines.iter().all(|l| !l.is_empty()), "no empty lines");
        assert_eq!(
            lines,
            aterm_render::list_fonts(),
            "must mirror list_fonts()"
        );
    }

    #[test]
    fn list_themes_includes_default_and_named() {
        let r = list_themes_report();
        for name in ["Default", "Dracula", "Nord", "Solarized Dark"] {
            assert!(r.contains(name), "list-themes missing {name:?}\n{r}");
        }
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
