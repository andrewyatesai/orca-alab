// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! `aterm-nest` — build an N-deep stack of nested aterms and run a command in the
//! DEEPEST one, proving the recursive-stacking feature from the command line.
//!
//! Each level is a real headless `aterm-gui` (engine + control socket, no window).
//! Level 0 is spawned directly; every deeper level is spawned by driving the level
//! above it over ITS OWN control socket — so authority is exercised one hop at a
//! time, by each level's owner, never borrowed transitively (the confused-deputy
//! boundary the proxy enforces and the Trust `authorize_soundness` model proves).
//!
//! Usage:
//!   aterm-nest [--depth N] [--keep] [--gui PATH] -- <command> [args...]
//!   aterm-nest --depth 3 -- claude -p "say hi"
//!
//! It prints the deepest terminal's visible output for the command, then tears the
//! stack down (`--keep` leaves it running and prints the per-level sockets).

use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const DONE: &str = "__ATERM_NEST_DONE__";

struct Args {
    depth: usize,
    keep: bool,
    gui: PathBuf,
    cmd: Vec<String>,
}

fn usage() -> ! {
    eprintln!(
        "usage: aterm-nest [--depth N] [--keep] [--gui PATH] -- <command> [args...]\n\
         \n\
         Builds an N-deep stack of nested headless aterms and runs <command> in the\n\
         deepest, driving each level over its own control socket.\n\
         \n\
         --depth N   number of nested aterm levels (default 1, max 8)\n\
         --keep      leave the stack running; print each level's socket\n\
         --gui PATH  path to the aterm-gui binary (default: sibling of this binary,\n\
                     then $ATERM_NEST_GUI, then `aterm-gui` on PATH)"
    );
    std::process::exit(2);
}

fn parse_args() -> Args {
    let mut depth = 1usize;
    let mut keep = false;
    let mut gui: Option<PathBuf> = None;
    let mut cmd: Vec<String> = Vec::new();
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--depth" => depth = it.next().and_then(|v| v.parse().ok()).unwrap_or_else(|| usage()),
            "--keep" => keep = true,
            "--gui" => gui = Some(PathBuf::from(it.next().unwrap_or_else(|| usage()))),
            "--help" | "-h" => usage(),
            "--" => {
                cmd.extend(it.by_ref());
                break;
            }
            other => {
                // First non-flag begins the command (the `--` is optional).
                cmd.push(other.to_string());
                cmd.extend(it.by_ref());
                break;
            }
        }
    }
    if cmd.is_empty() || depth == 0 || depth > 8 {
        usage();
    }
    Args { depth, keep, gui: gui.unwrap_or_else(resolve_gui), cmd }
}

/// Locate the `aterm-gui` binary: explicit `--gui` (handled by caller), then a
/// sibling of this binary (the common cargo layout), then `$ATERM_NEST_GUI`, then
/// bare `aterm-gui` (PATH lookup).
fn resolve_gui() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let sib = dir.join("aterm-gui");
            if sib.is_file() {
                return sib;
            }
        }
    }
    if let Ok(p) = std::env::var("ATERM_NEST_GUI") {
        return PathBuf::from(p);
    }
    PathBuf::from("aterm-gui")
}

// --- minimal control-socket client (mirrors aterm-ctl's framing) ---

/// The capability token sitting beside the socket (`aterm-<pid>.token`).
fn read_token(sock: &str) -> Option<String> {
    let tok = sock.strip_suffix(".sock")?.to_string() + ".token";
    std::fs::read_to_string(tok).ok().map(|s| s.trim().to_string())
}

/// Send one verb; return the status line (no follow-up payload read).
fn verb_status(sock: &str, verb: &str) -> io::Result<String> {
    let s = UnixStream::connect(sock)?;
    if let Some(tok) = read_token(sock) {
        (&s).write_all(format!("AUTH {tok}\n").as_bytes())?;
    }
    (&s).write_all(format!("{verb}\n").as_bytes())?;
    (&s).flush()?;
    let mut line = String::new();
    BufReader::new(&s).read_line(&mut line)?;
    Ok(line.trim_end().to_string())
}

/// Send a streaming verb (`text`/`screen`): read the `OK <n>` header then the n
/// follow-up lines, returned joined.
fn verb_stream(sock: &str, verb: &str) -> io::Result<String> {
    let s = UnixStream::connect(sock)?;
    if let Some(tok) = read_token(sock) {
        (&s).write_all(format!("AUTH {tok}\n").as_bytes())?;
    }
    (&s).write_all(format!("{verb}\n").as_bytes())?;
    (&s).flush()?;
    let mut r = BufReader::new(&s);
    let mut status = String::new();
    r.read_line(&mut status)?;
    let n: usize = status
        .trim()
        .strip_prefix("OK ")
        .and_then(|x| x.split_whitespace().next())
        .and_then(|x| x.parse().ok())
        .unwrap_or(0);
    let mut out = String::new();
    for _ in 0..n {
        let mut l = String::new();
        if r.read_line(&mut l)? == 0 {
            break;
        }
        out.push_str(&l);
    }
    Ok(out)
}

/// Type `text` into a level's shell and submit it (cmd_send turns a trailing
/// literal `\n` into CR). `text` must not contain a real newline.
fn type_line(sock: &str, text: &str) -> io::Result<()> {
    // `send <text>\n` where the trailing two chars are a literal backslash-n.
    let _ = verb_status(sock, &format!("send {text}\\n"))?;
    Ok(())
}

/// Parse `aterm-gui: control socket listening at <PATH> (token-gated...)`.
fn socket_from_line(line: &str) -> Option<String> {
    let after = line.split("listening at ").nth(1)?;
    let path = after.split(" (token-gated").next()?.trim();
    if path.is_empty() {
        None
    } else {
        Some(path.to_string())
    }
}

/// Spawn level 0 directly, returning (child, socket-path). Reads the child's stderr
/// until the "listening at" line (then stops, leaving the child running).
fn spawn_root(gui: &PathBuf) -> io::Result<(std::process::Child, String)> {
    let mut child = Command::new(gui)
        .env("ATERM_HEADLESS", "1")
        .env("ATERM_LINES", "40")
        .env("ATERM_COLUMNS", "120")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    let err = child.stderr.take().expect("piped stderr");
    let mut r = BufReader::new(err);
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut line = String::new();
    loop {
        line.clear();
        if r.read_line(&mut line)? == 0 || Instant::now() > deadline {
            let _ = child.kill();
            return Err(io::Error::other("aterm-gui (L0) did not announce its socket"));
        }
        if let Some(sock) = socket_from_line(&line) {
            // Drain the rest of stderr in the background so the child never blocks.
            std::thread::spawn(move || {
                let mut sink = Vec::new();
                let _ = r.into_inner().read_to_end(&mut sink);
            });
            return Ok((child, sock));
        }
    }
}

/// Spawn a deeper level by driving `parent_sock`'s shell to exec aterm-gui with its
/// stderr redirected to `errfile`; poll `errfile` for the "listening at" line.
fn spawn_child(parent_sock: &str, gui: &PathBuf, errfile: &str) -> io::Result<String> {
    let _ = std::fs::remove_file(errfile);
    let g = gui.to_string_lossy();
    type_line(
        parent_sock,
        &format!("ATERM_HEADLESS=1 ATERM_LINES=40 ATERM_COLUMNS=120 {g} 2>{errfile}"),
    )?;
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(200));
        if let Ok(body) = std::fs::read_to_string(errfile) {
            for line in body.lines() {
                if let Some(sock) = socket_from_line(line) {
                    return Ok(sock);
                }
            }
        }
    }
    Err(io::Error::other("nested aterm did not announce its socket in time"))
}

fn main() {
    let args = parse_args();
    if !args.gui.is_file() && args.gui.to_string_lossy().contains('/') {
        eprintln!("aterm-nest: aterm-gui not found at {}", args.gui.display());
        std::process::exit(1);
    }
    match run(&args) {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("aterm-nest: {e}");
            std::process::exit(1);
        }
    }
}

fn run(args: &Args) -> io::Result<i32> {
    let pid = std::process::id();
    let (mut root, root_sock) = spawn_root(&args.gui)?;
    eprintln!("aterm-nest: L0 socket {root_sock}");
    let mut socks = vec![root_sock];
    for k in 1..args.depth {
        let errfile = format!("/tmp/aterm-nest-{pid}-L{k}.err");
        let sock = match spawn_child(&socks[k - 1], &args.gui, &errfile) {
            Ok(s) => s,
            Err(e) => {
                let _ = root.kill();
                return Err(e);
            }
        };
        eprintln!("aterm-nest: L{k} socket {sock}");
        socks.push(sock);
    }
    let deepest = socks.last().unwrap().clone();

    // Run the command in the deepest level, fenced by a sentinel so we know when it
    // finished, then capture that level's visible text.
    let cmd = args.cmd.join(" ");
    eprintln!("aterm-nest: running in L{} (depth {}): {cmd}", args.depth - 1, args.depth);
    type_line(&deepest, &format!("{cmd}; echo {DONE}"))?;

    let deadline = Instant::now() + Duration::from_secs(180);
    let mut last = String::new();
    loop {
        std::thread::sleep(Duration::from_millis(500));
        last = verb_stream(&deepest, "text").unwrap_or(last);
        // The sentinel also appears in the COMMAND ECHO (`...; echo __DONE__`), so
        // wait for it on its OWN line — the echo's output — not just anywhere.
        if last.lines().any(|l| l.trim() == DONE) || Instant::now() > deadline {
            break;
        }
    }

    // Print the command's output: the visible lines AFTER the command echo and
    // BEFORE the sentinel.
    print_output(&last, &cmd);

    if args.keep {
        eprintln!("aterm-nest: --keep set; stack left running:");
        for (i, s) in socks.iter().enumerate() {
            eprintln!("  L{i}: {s}");
        }
    } else {
        let _ = root.kill();
        let _ = root.wait();
        for k in 1..args.depth {
            let _ = std::fs::remove_file(format!("/tmp/aterm-nest-{pid}-L{k}.err"));
        }
    }
    Ok(0)
}

/// The command's output region: the lines strictly between the (last) command echo
/// and the sentinel's own line. Split out (pure) for testing.
fn output_region<'a>(text: &'a str, cmd: &str) -> Vec<&'a str> {
    let lines: Vec<&str> = text.lines().collect();
    let echo = lines.iter().rposition(|l| l.contains(cmd));
    let done = lines.iter().rposition(|l| l.trim() == DONE);
    match (echo, done) {
        (Some(e), Some(d)) if d > e => lines[e + 1..d].to_vec(),
        _ => lines.into_iter().filter(|l| !l.trim().is_empty() && !l.contains(DONE)).collect(),
    }
}

/// Print the command's output region from the captured screen text: lines strictly
/// between the (last) command echo and the sentinel, with the sentinel/echo removed.
fn print_output(text: &str, cmd: &str) {
    let out = io::stdout();
    let mut w = out.lock();
    for l in output_region(text, cmd) {
        let _ = writeln!(w, "{}", l.trim_end());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_from_line_extracts_path() {
        let line = "aterm-gui: control socket listening at /tmp/x/aterm-42.sock (token-gated, same-uid only)";
        assert_eq!(socket_from_line(line).as_deref(), Some("/tmp/x/aterm-42.sock"));
        assert_eq!(socket_from_line("aterm-gui: GPU rendering on Metal"), None);
    }

    #[test]
    fn read_token_path_is_sibling() {
        // (pure path derivation; the file need not exist)
        let tok = "/d/aterm-7.sock".strip_suffix(".sock").map(|b| b.to_string() + ".token");
        assert_eq!(tok.as_deref(), Some("/d/aterm-7.token"));
    }

    #[test]
    fn output_region_is_between_echo_and_sentinel() {
        // The command echo also contains the sentinel (we typed `cmd; echo DONE`);
        // the sentinel's OWN line (== DONE) fences the end.
        let cmd = "echo hi";
        let screen = format!(
            "user% {cmd}; echo {DONE}\nhi\n{DONE}\nuser% \n"
        );
        assert_eq!(output_region(&screen, cmd), vec!["hi"]);
    }

    #[test]
    fn output_region_fallback_when_unfenced() {
        // No proper fence -> non-empty, non-sentinel lines (never lose output).
        let got = output_region("alpha\n\nbeta\n", "missing-cmd");
        assert_eq!(got, vec!["alpha", "beta"]);
    }
}
