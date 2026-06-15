// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! `aterm-ctl` — a tiny, dependency-free client for the aterm introspection
//! control socket (protocol v1).
//!
//! The aterm engine optionally exposes a Unix-domain control socket whose
//! protocol is newline-delimited text: the client sends one request line
//! (`"VERB [args...]\n"`) and reads exactly one response. This binary frames a
//! single request from the command line, prints the relevant part of the
//! response to stdout, and maps the protocol's `OK`/`ERR` outcome onto the
//! process exit status.
//!
//! It uses only `std` (notably [`std::os::unix::net::UnixStream`]) plus the
//! workspace's platform-free `aterm-types` engine crate, which carries the
//! socket naming/discovery decisions shared with the server; there are no
//! external dependencies.
//!
//! # Usage
//!
//! ```text
//! aterm-ctl [--sock PATH | --pid PID] <verb> [args...]
//! ```
//!
//! The socket path is resolved as: `--sock PATH` if given, else `--pid PID`
//! (a specific instance's `<dir>/aterm-<PID>.sock`), else the
//! `$ATERM_CONTROL_SOCK` environment variable (whose `0`/`off` disable
//! keywords are honoured as on the server), else the per-user default
//! `<dir>/aterm.sock` where `<dir>` is `$XDG_RUNTIME_DIR/aterm` (when set) or
//! `~/Library/Application Support/aterm` (macOS). The default is a symlink
//! the server atomically points at the newest instance's `aterm-<pid>.sock`,
//! so the flagless flow reaches a live instance. This matches the server's
//! resolution exactly.
//!
//! ## Authentication (transparent)
//!
//! The server is access-controlled by default: it accepts only same-uid peers
//! and requires a per-launch capability token. This client reads that token
//! from the socket's sibling token file — the matching `aterm-<pid>.token`
//! for a per-instance socket (resolved through the `latest` symlink), else
//! `aterm.token` — and sends `AUTH <hex>\n` as the FIRST line of every
//! connection, before the verb. Normal same-user usage is therefore unchanged
//! — there is no flag and no prompt. If the token file is unreadable
//! (different user, or aterm not running) the connection is refused by the
//! server with `ERR auth`.
//!
//! ## Verbs
//!
//! * `text`            — print the visible screen, one row per line.
//! * `cursor`          — print `OK <row> <col> <visible> <style>` (`<style>`
//!   is the DECSCUSR style, lowercase: `blinking_block`, `steady_block`,
//!   `blinking_underline`, `steady_underline`, `blinking_bar`, `steady_bar`,
//!   `hidden`, `hollow_block`).
//! * `cell <r> <c>`    — print `OK <codepoint> <fg> <bg>`.
//! * `search <pat>`    — print one `"<row> <col> <len>"` line per match.
//! * `send <text>`     — write `<text>` to the PTY (trailing literal `\n` ⇒ CR).
//! * `key <name>`      — send a named key (`enter`, `tab`, `up`, …) to the PTY.
//! * `image [path]`    — render the screen to a PNG; print `OK <w> <h> <path>`.
//!   WYSIWYG: includes cursor blink phase / unfocused-hollow override (headless
//!   sessions are always deterministic); use `cursor` for phase-independent state.
//! * `resize <r> <c>`  — resize the engine + PTY (each dimension 1..=4096;
//!   out-of-range requests get `ERR out of range`).
//! * `select <r1> <c1> <r2> <c2>` — select from cell `(r1,c1)` to `(r2,c2)`,
//!   both endpoint cells inclusive (live-screen coords; negative rows reach
//!   into scrollback). `select clear` clears the selection.
//! * `select word <r> <c>` — word-select the cell via the engine's builtin
//!   smart-selection rules (URLs/paths/words; a whitespace cell selects just
//!   itself) — the double-click gesture.
//! * `select line <r>` — select the full line of row `r` (triple-click).
//! * `select block <r1> <c1> <r2> <c2>` — rectangular selection, the two
//!   cells as inclusive corners (alt-drag).
//! * `select extend <r> <c>` — extend the existing selection so `(r,c)` is
//!   its new inclusive endpoint (shift-click); errors with no selection.
//! * `selection`       — print the selected text, one line per selected row.
//! * `copy`            — copy the selection to the system clipboard
//!   (`pbcopy`); print `OK <byte-count>` (`OK 0` when nothing is selected).
//!
//! For `text`, `search`, `modes`, and `selection` the response is `"OK <n>\n"`
//! followed by `<n>` data lines, and those data lines are what gets printed.
//! For every other verb the single `OK …` status line itself is printed. An
//! `ERR …` response is written to stderr and yields exit code 1; so does any
//! connection failure.

use std::env;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use aterm_types::control_socket::{self, SocketDirective};

/// Environment variable consulted for the socket path when `--sock`/`--pid`
/// are absent. `0`/`off` mean the server runs without a socket.
const SOCK_ENV: &str = "ATERM_CONTROL_SOCK";

/// Environment kill switch: set (truthy) means the server has no socket.
const NO_SOCK_ENV: &str = "ATERM_NO_CONTROL_SOCK";

/// Socket filename inside the per-user directory — the server-maintained
/// `latest` symlink to the newest instance's socket.
const SOCK_FILE: &str = control_socket::LATEST_SOCK_FILE;

/// Resolve the per-user directory holding the control socket + token, matching
/// the server's `control_auth::socket_dir` logic exactly:
/// `$XDG_RUNTIME_DIR/aterm` when set, else `~/Library/Application Support/aterm`.
fn socket_dir() -> Option<PathBuf> {
    if let Some(xdg) = env::var_os("XDG_RUNTIME_DIR") {
        return Some(PathBuf::from(xdg).join("aterm"));
    }
    let home = env::var_os("HOME")?;
    Some(
        PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("aterm"),
    )
}

/// The default socket path: `<socket_dir>/aterm.sock`. `None` only when neither
/// `XDG_RUNTIME_DIR` nor `HOME` is set.
fn default_sock_path() -> Option<String> {
    Some(socket_dir()?.join(SOCK_FILE).to_string_lossy().into_owned())
}

/// Resolve the socket path from the flags and the environment values. Flags
/// win over the environment; `--pid` targets one instance's
/// `<dir>/aterm-<pid>.sock` directly. The env interpretation (explicit path
/// vs `0`/`off` disable keywords vs per-instance default) is the engine's
/// [`control_socket::socket_directive`], identical to the server's.
fn resolve_path(
    sock: Option<String>,
    pid: Option<u32>,
    env_sock: Option<String>,
    env_no_sock: Option<String>,
) -> io::Result<String> {
    let no_dir = || {
        io::Error::new(
            io::ErrorKind::NotFound,
            "cannot resolve control socket: set --sock, $ATERM_CONTROL_SOCK, \
             or $XDG_RUNTIME_DIR/$HOME",
        )
    };
    if sock.is_some() && pid.is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "--sock and --pid are mutually exclusive",
        ));
    }
    if let Some(pid) = pid {
        let dir = socket_dir().ok_or_else(no_dir)?;
        return Ok(dir
            .join(control_socket::instance_sock_name(pid))
            .to_string_lossy()
            .into_owned());
    }
    if let Some(s) = sock {
        return Ok(s);
    }
    match control_socket::socket_directive(env_sock.as_deref(), env_no_sock.as_deref()) {
        SocketDirective::Disabled => Err(io::Error::new(
            io::ErrorKind::NotFound,
            "the control socket is disabled in this environment \
             ($ATERM_CONTROL_SOCK=0/off or $ATERM_NO_CONTROL_SOCK)",
        )),
        SocketDirective::Explicit(p) => Ok(p),
        SocketDirective::PerInstance => default_sock_path().ok_or_else(no_dir),
    }
}

/// Read the per-launch capability token sitting beside the socket at `path`.
/// A per-instance socket (reached directly or through the `latest` symlink)
/// pairs with its `aterm-<pid>.token`; anything else falls back to the
/// sibling `aterm.token`. Returns `None` if unreadable (e.g. a different
/// user, or aterm not running); the connection is then attempted without an
/// `AUTH` line and the server refuses it with `ERR auth`.
fn read_token_for(path: &str) -> Option<String> {
    let p = Path::new(path);
    let dir = p.parent()?;
    let sock_name = std::fs::read_link(p)
        .ok()
        .and_then(|t| t.file_name().map(std::ffi::OsStr::to_os_string))
        .unwrap_or(p.file_name()?.to_os_string());
    let token_name = control_socket::token_name_for_sock(&sock_name.to_string_lossy());
    let raw = std::fs::read_to_string(dir.join(token_name)).ok()?;
    let t = raw.trim().to_string();
    if t.is_empty() { None } else { Some(t) }
}

fn main() -> ExitCode {
    match real_main() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("aterm-ctl: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Parse arguments, frame the request, talk to the server, and print the reply.
///
/// Returns the process exit code on a completed exchange (`SUCCESS` for `OK`,
/// `FAILURE` for an `ERR`/unexpected status), or an [`io::Error`] for usage or
/// connection problems (surfaced on stderr by [`main`]).
fn real_main() -> io::Result<ExitCode> {
    // Flag parsing stops at the first positional argument: everything from the
    // verb onward is part of the request, so a literal "--sock" inside e.g. a
    // `send`/`search` payload is never mistaken for our own flag.
    let mut args = env::args().skip(1);
    let mut sock: Option<String> = None;
    let mut pid: Option<u32> = None;
    let mut request_parts: Vec<String> = Vec::new();

    let parse_pid = |v: &str| {
        v.parse::<u32>().map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidInput, "--pid requires a numeric PID")
        })
    };
    while let Some(arg) = args.next() {
        if arg == "--sock" {
            sock = Some(args.next().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "--sock requires a PATH")
            })?);
        } else if let Some(p) = arg.strip_prefix("--sock=") {
            sock = Some(p.to_string());
        } else if arg == "--pid" {
            let v = args.next().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "--pid requires a PID")
            })?;
            pid = Some(parse_pid(&v)?);
        } else if let Some(v) = arg.strip_prefix("--pid=") {
            pid = Some(parse_pid(v)?);
        } else {
            // First positional is the verb; the remainder is its argument list.
            request_parts.push(arg);
            request_parts.extend(args.by_ref());
            break;
        }
    }

    let verb = match request_parts.first() {
        Some(v) => v.clone(),
        None => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "usage: aterm-ctl [--sock PATH | --pid PID] <verb> [args...]",
            ));
        }
    };

    let path = resolve_path(sock, pid, env::var(SOCK_ENV).ok(), env::var(NO_SOCK_ENV).ok())?;

    // One request per line: "VERB [args...]\n". Args are joined with single
    // spaces; for `send`/`search` this reconstructs the free-form rest-of-line
    // payload (modulo collapsed inter-arg whitespace).
    let request = format!("{}\n", request_parts.join(" "));

    exchange(&path, &request, &verb)
}

/// Connect to `path`, AUTHENTICATE, send `request`, and print the response for
/// `verb`.
///
/// The server requires `AUTH <hex>\n` as the first line of every connection.
/// We read the token from the socket's sibling `aterm.token` and send it
/// transparently; only then do we send the actual request line. There is no
/// server response to the `AUTH` line itself (it is consumed silently on
/// success), so the first line we read back is the response to `request`.
fn exchange(path: &str, request: &str, verb: &str) -> io::Result<ExitCode> {
    let stream = UnixStream::connect(path)
        .map_err(|e| io::Error::new(e.kind(), format!("connect {path}: {e}")))?;

    // `&UnixStream` implements both `Read` and `Write`, so the two borrows can
    // coexist: send the auth line + request, then buffer-read the response.
    if let Some(token) = read_token_for(path) {
        (&stream).write_all(format!("AUTH {token}\n").as_bytes())?;
    }
    (&stream).write_all(request.as_bytes())?;
    (&stream).flush()?;

    let mut reader = BufReader::new(&stream);
    let mut status_line = String::new();
    if reader.read_line(&mut status_line)? == 0 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "server closed the connection without responding",
        ));
    }
    let status_line = status_line.trim_end_matches(['\r', '\n']);

    let mut tokens = status_line.splitn(2, ' ');
    let status = tokens.next().unwrap_or("");
    let tail = tokens.next().unwrap_or("");

    if status != "OK" {
        // "ERR <msg>", "ERR", or any unexpected reply: report and fail.
        eprintln!("aterm-ctl: {status_line}");
        return Ok(ExitCode::FAILURE);
    }

    // `text`, `search`, `modes`, and `selection` stream `<n>` follow-up data
    // lines after "OK <n>"; those lines are the payload the user cares about.
    // Every other verb's meaning lives entirely in the status line, so we echo
    // that instead. (Gated by verb, not by "tail is an int", because
    // `lines`/`feed`/`signal`/`copy` legitimately answer with "OK <int>" and
    // have NO follow-up lines.)
    if verb == "text" || verb == "search" || verb == "modes" || verb == "selection" || verb == "blocks" || verb == "blocktext" {
        let count: usize = tail.trim().parse().map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("malformed response header: {status_line:?}"),
            )
        })?;
        let stdout = io::stdout();
        let mut out = stdout.lock();
        for _ in 0..count {
            let mut line = String::new();
            if reader.read_line(&mut line)? == 0 {
                break; // server hung up early; print what we have.
            }
            // Normalize the line ending; preserve the row's content verbatim.
            let line = line.strip_suffix('\n').unwrap_or(&line);
            let line = line.strip_suffix('\r').unwrap_or(line);
            writeln!(out, "{line}")?;
        }
    } else {
        println!("{status_line}");
    }

    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_refuses_both_sock_and_pid() {
        let err = resolve_path(Some("/tmp/a.sock".into()), Some(7), None, None).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn resolve_pid_targets_that_instance_socket() {
        let path = resolve_path(None, Some(42), None, None).expect("per-user dir");
        assert!(path.ends_with("/aterm-42.sock"), "got {path}");
    }

    #[test]
    fn resolve_flag_beats_environment() {
        let path =
            resolve_path(Some("/tmp/a.sock".into()), None, Some("/elsewhere.sock".into()), None)
                .unwrap();
        assert_eq!(path, "/tmp/a.sock");
    }

    #[test]
    fn resolve_honours_environment_disable_keywords() {
        for (env_sock, env_kill) in
            [(Some("0"), None), (Some("off"), None), (None, Some("1"))]
        {
            let err = resolve_path(
                None,
                None,
                env_sock.map(String::from),
                env_kill.map(String::from),
            )
            .unwrap_err();
            assert_eq!(err.kind(), io::ErrorKind::NotFound);
        }
        // ...but an explicit path value passes straight through.
        let path = resolve_path(None, None, Some("/tmp/x.sock".into()), None).unwrap();
        assert_eq!(path, "/tmp/x.sock");
    }
}
