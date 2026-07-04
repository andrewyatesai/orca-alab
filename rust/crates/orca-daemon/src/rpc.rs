//! Control-socket RPC dispatch + the per-session output pump. Requests arrive as
//! `serde_json::Value`; each returns the NDJSON response line the control socket
//! writes back. `createOrAttach` additionally spawns the PTY and the reader thread
//! that routes its output (live if the client's stream is connected, else buffered
//! for reattach — see `registry`).

use crate::protocol::{rpc_err, rpc_ok};
use crate::registry::{Registry, SessionEntry};
use orca_pty::{PtyCommand, PtySession, PtySize};
use orca_terminal::{HeadlessTerminal, MouseTracking, DEFAULT_SCROLLBACK};
use serde_json::{json, Value};
use std::io::Read;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn field_str<'a>(payload: &'a Value, key: &str) -> &'a str {
    payload.get(key).and_then(Value::as_str).unwrap_or("")
}

fn field_u16(payload: &Value, key: &str, default: u16) -> u16 {
    payload
        .get(key)
        .and_then(Value::as_u64)
        .unwrap_or(default as u64) as u16
}

pub fn dispatch_request(request: &Value, registry: &Arc<Registry>, client_id: &str) -> String {
    let id = request.get("id").and_then(Value::as_str).unwrap_or("");
    let kind = request.get("type").and_then(Value::as_str).unwrap_or("");
    let payload = request.get("payload").cloned().unwrap_or(Value::Null);
    let sid = || field_str(&payload, "sessionId").to_string();
    match kind {
        "createOrAttach" => create_or_attach(id, &payload, registry, client_id),
        "write" => {
            let data = field_str(&payload, "data").to_string();
            match registry.with_session(&sid(), |e| e.pty.write_all(data.as_bytes())) {
                Some(Ok(())) => rpc_ok(id, Value::Null),
                Some(Err(e)) => rpc_err(id, &e.to_string()),
                None => rpc_err(id, "unknown session"),
            }
        }
        "resize" => {
            let cols = field_u16(&payload, "cols", 80);
            let rows = field_u16(&payload, "rows", 24);
            match registry.with_session(&sid(), |e| {
                e.cols = cols;
                e.rows = rows;
                if let Ok(mut t) = e.terminal.lock() {
                    t.resize(rows as usize, cols as usize);
                }
                e.pty.resize(PtySize { rows, cols })
            }) {
                Some(Ok(())) => rpc_ok(id, Value::Null),
                Some(Err(e)) => rpc_err(id, &e.to_string()),
                None => rpc_err(id, "unknown session"),
            }
        }
        // Kill the child; the pump's EOF then marks the session exited + emits `exit`.
        "kill" => {
            registry.with_session(&sid(), |e| {
                let _ = e.pty.kill();
            });
            rpc_ok(id, Value::Null)
        }
        // The session already survives control-socket close, so detach is a no-op ack.
        "detach" => rpc_ok(id, Value::Null),
        "takePendingOutput" => rpc_ok(id, json!({ "data": registry.take_pending(&sid()) })),
        "listSessions" => rpc_ok(id, registry.list_sessions()),
        // Wire shape mirrors the Node daemon: `{ size: { cols, rows } }` (not the
        // dims at the payload top level) — see daemon-server.ts `getAppliedSize`.
        "getSize" => match registry.session_size(&sid()) {
            Some((cols, rows)) => rpc_ok(id, json!({ "size": { "cols": cols, "rows": rows } })),
            None => rpc_err(id, "unknown session"),
        },
        "ping" => rpc_ok(id, json!({ "pong": true })),
        // Real probe: open a PTY and spawn a trivial child. If the PTY subsystem is
        // healthy it spawns + exits; any failure surfaces as an error. Mirrors the
        // Node daemon running checkPtySpawnHealth before answering healthy:true.
        "ptySpawnHealth" => match probe_pty_spawn() {
            Ok(()) => rpc_ok(id, json!({ "healthy": true })),
            Err(e) => rpc_err(id, &format!("pty spawn health failed: {e}")),
        },
        // Real resolver health from the daemon's own process (scutil on macOS);
        // "unknown" elsewhere. Lets the launcher's preserve/replace decision see a
        // Rust daemon that lost its scoped system resolver — same as the Node daemon.
        "systemResolverHealth" => {
            rpc_ok(id, json!({ "health": crate::resolver_health::system_resolver_health() }))
        }
        // Real engine state from the session's headless aterm terminal — no napi hop.
        "getSnapshot" => match registry.terminal_of(&sid()) {
            Some(terminal) => {
                let snapshot = build_snapshot(&mut terminal.lock().unwrap());
                rpc_ok(id, json!({ "snapshot": snapshot }))
            }
            None => rpc_ok(id, json!({ "snapshot": Value::Null })),
        },
        // Wire shape mirrors the Node daemon: `{ cwd: <string|null> }`.
        "getCwd" => {
            let cwd = registry
                .terminal_of(&sid())
                .and_then(|t| t.lock().unwrap().cwd().map(str::to_string));
            rpc_ok(id, json!({ "cwd": cwd }))
        }
        // Wire shape mirrors the Node daemon: `{ foregroundProcess: <…|null> }`.
        // Process-group query isn't wired yet, so the value is null (a safe stub).
        "getForegroundProcess" => rpc_ok(id, json!({ "foregroundProcess": Value::Null })),
        "clearScrollback" => {
            if let Some(t) = registry.terminal_of(&sid()) {
                t.lock().unwrap().clear_scrollback();
            }
            rpc_ok(id, Value::Null)
        }
        "cancelCreateOrAttach" => rpc_ok(id, Value::Null),
        // Deliver a named signal to the child (node-pty's `kill(signal)`). Errors
        // from a dead child are dropped like the Node daemon; an unknown session
        // errors (host.signal throws on a missing session there too).
        "signal" => {
            let sig = field_str(&payload, "signal").to_string();
            match registry.with_session(&sid(), |e| e.pty.signal(&sig)) {
                Some(_) => rpc_ok(id, Value::Null),
                None => rpc_err(id, "unknown session"),
            }
        }
        "shutdown" => {
            // Reply first, then exit so the ok flushes to the client.
            thread::spawn(|| {
                thread::sleep(Duration::from_millis(50));
                std::process::exit(0);
            });
            rpc_ok(id, Value::Null)
        }
        other => rpc_err(id, &format!("unsupported request type: {other}")),
    }
}

/// Health probe for `ptySpawnHealth`: open a PTY and spawn a trivial child that
/// exits at once, then reap it. Bypasses the login shell (no `-lc`) so the check
/// stays fast and free of user-profile side effects. Any error means the PTY
/// subsystem can't currently spawn.
fn probe_pty_spawn() -> std::io::Result<()> {
    let mut probe = PtySession::spawn(&probe_command(), PtySize { rows: 1, cols: 1 })?;
    // `exit 0` returns immediately; wait() reaps it so the probe leaves no child.
    let _ = probe.wait();
    Ok(())
}

#[cfg(unix)]
fn probe_command() -> PtyCommand {
    PtyCommand {
        program: "/bin/sh".to_string(),
        args: vec!["-c".to_string(), "exit 0".to_string()],
        cwd: None,
        env: Vec::new(),
    }
}

#[cfg(windows)]
fn probe_command() -> PtyCommand {
    PtyCommand {
        program: default_shell(),
        args: vec!["/C".to_string(), "exit".to_string(), "0".to_string()],
        cwd: None,
        env: Vec::new(),
    }
}

fn create_or_attach(
    id: &str,
    payload: &Value,
    registry: &Arc<Registry>,
    client_id: &str,
) -> String {
    let session_id = field_str(payload, "sessionId").to_string();
    if session_id.is_empty() {
        return rpc_err(id, "missing sessionId");
    }
    // Reattach a live session: rebind it to this (possibly new) client, flush its
    // backlog, and return a REAL snapshot so the reattacher repaints — matching
    // terminal-host.ts's live branch (getSnapshot + detachAllClients + attachClient).
    // A blank snapshot here would leave a warm-reattached pane frozen after relaunch.
    if let Some(terminal) = registry.reattach_if_alive(&session_id, client_id) {
        let snapshot = build_snapshot(&mut terminal.lock().unwrap());
        let pid = registry.session_pid(&session_id);
        return rpc_ok(
            id,
            json!({ "isNew": false, "snapshot": snapshot, "pid": pid, "shellState": SHELL_STATE }),
        );
    }
    // Not a live session: drop any lingering dead entry for this id, then spawn fresh.
    registry.remove_session(&session_id);

    let cols = field_u16(payload, "cols", 80);
    let rows = field_u16(payload, "rows", 24);
    let command = build_command(payload);
    let pty = match PtySession::spawn(&command, PtySize { rows, cols }) {
        Ok(p) => p,
        Err(e) => return rpc_err(id, &format!("spawn failed: {e}")),
    };
    let pid = pty.process_id();
    let reader = match pty.try_clone_reader() {
        Ok(r) => r,
        Err(e) => return rpc_err(id, &format!("reader clone failed: {e}")),
    };
    // A headless aterm engine per session: the pump tees raw PTY output into it so
    // getSnapshot/getCwd are answered from real engine state (no napi hop).
    let terminal = Arc::new(Mutex::new(HeadlessTerminal::with_scrollback(
        rows as usize,
        cols as usize,
        DEFAULT_SCROLLBACK,
    )));
    // Pump raw PTY output → routed (live or buffered) by the registry AND teed into
    // the headless engine; on EOF, reap the child (remove the session + emit `exit`).
    // The reader is an independent clone of the master, so it keeps reading after
    // `pty` moves into the registry entry.
    let pump_registry = registry.clone();
    let pump_session = session_id.clone();
    let pump_terminal = Arc::clone(&terminal);
    thread::spawn(move || pump_output(reader, pump_registry, pump_session, pump_terminal));

    let created_at_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    registry.insert_session(
        session_id,
        SessionEntry {
            pty,
            client_id: client_id.to_string(),
            cols,
            rows,
            pid,
            created_at_ms,
            pending: Vec::new(),
            terminal,
        },
    );
    rpc_ok(
        id,
        json!({ "isNew": true, "snapshot": Value::Null, "pid": pid, "shellState": SHELL_STATE }),
    )
}

/// The daemon doesn't run OSC-133 shell-readiness detection, so it reports the one
/// honest, VALID `ShellReadyState` (types.ts) for that: `unsupported`. (The prior
/// `"unknown"` was not a member of the union and would confuse the client's gate.)
const SHELL_STATE: &str = "unsupported";

/// Resolve the shell/command to spawn. `command` (when present) runs under the login
/// shell; otherwise the user's `$SHELL` (or /bin/sh) starts interactively.
fn build_command(payload: &Value) -> PtyCommand {
    let cwd = payload
        .get("cwd")
        .and_then(Value::as_str)
        .map(str::to_string);
    let shell = default_shell();
    match payload.get("command").and_then(Value::as_str) {
        Some(cmd) if !cmd.is_empty() => PtyCommand {
            program: shell,
            args: shell_run_args(cmd),
            cwd,
            env: Vec::new(),
        },
        _ => PtyCommand {
            program: shell,
            args: Vec::new(),
            cwd,
            env: Vec::new(),
        },
    }
}

#[cfg(unix)]
fn default_shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())
}

#[cfg(unix)]
fn shell_run_args(cmd: &str) -> Vec<String> {
    vec!["-lc".to_string(), cmd.to_string()]
}

/// Windows twin: ConPTY sessions run under `%ComSpec%` (cmd.exe), the platform's
/// interactive default; `/C` is the `-lc` analogue (cmd has no login semantics).
#[cfg(windows)]
fn default_shell() -> String {
    std::env::var("ComSpec").unwrap_or_else(|_| "cmd.exe".to_string())
}

#[cfg(windows)]
fn shell_run_args(cmd: &str) -> Vec<String> {
    vec!["/C".to_string(), cmd.to_string()]
}

fn pump_output(
    mut reader: Box<dyn Read + Send>,
    registry: Arc<Registry>,
    session_id: String,
    terminal: Arc<Mutex<HeadlessTerminal>>,
) {
    let mut buf = [0u8; 65536];
    loop {
        match reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                // Tee: raw bytes into the headless engine (correct VT parsing), and a
                // lossy-UTF-8 copy out to the client stream.
                if let Ok(mut t) = terminal.lock() {
                    t.process(&buf[..n]);
                }
                let data = String::from_utf8_lossy(&buf[..n]);
                registry.route_output(&session_id, data.as_ref());
            }
        }
    }
    // EOF means the child closed the PTY — i.e. it exited. Reap it for the REAL
    // exit code (wait() returns at once now) and notify the client.
    registry.reap_and_mark_exited(&session_id);
}

/// Build a `TerminalSnapshot` (types.ts) from the session's headless aterm engine.
/// ansi/cwd/modes are REAL engine state; `rehydrateSequences` replays the screen and
/// input modes on reattach, and `oscLinks` carries the scrollback/screen hyperlink
/// ranges so links survive a reconnect.
fn build_snapshot(term: &mut HeadlessTerminal) -> Value {
    let snapshot_ansi = term.serialize_ansi(None);
    let scrollback_ansi = term.serialize_scrollback_ansi(None);
    let osc_links: Vec<Value> = term
        .osc_link_ranges(None)
        .into_iter()
        .map(|l| json!({ "row": l.row, "startCol": l.start_col, "endCol": l.end_col, "uri": l.uri }))
        .collect();
    let cwd = term.cwd().map(str::to_string);
    let bracketed = term.bracketed_paste();
    let app_cursor = term.application_cursor();
    let alt_screen = term.is_alternate_screen();
    let title = term.title();
    let (rows, cols) = term.size();
    let scrollback_lines = term.scrollback_len();
    let (mouse_on, mouse_mode) = match term.mouse_tracking() {
        MouseTracking::None => (false, "none"),
        MouseTracking::X10 => (true, "x10"),
        MouseTracking::Normal => (true, "vt200"),
        MouseTracking::Button => (true, "drag"),
        MouseTracking::Any => (true, "any"),
    };
    // SGR mouse encoding (DECSET 1006) + its pixel variant (1016). The Node
    // daemon carries both in TerminalModes; aterm exposes them directly.
    let sgr_mouse = term.sgr_mouse();
    let sgr_pixels = term.sgr_pixels();
    let rehydrate =
        rehydrate_sequences(mouse_on, mouse_mode, bracketed, app_cursor, alt_screen, sgr_mouse, sgr_pixels);
    json!({
        "snapshotAnsi": snapshot_ansi,
        "scrollbackAnsi": scrollback_ansi,
        "rehydrateSequences": rehydrate,
        "oscLinks": osc_links,
        "cwd": cwd,
        "modes": {
            "bracketedPaste": bracketed,
            "mouseTracking": mouse_on,
            "mouseTrackingMode": mouse_mode,
            "sgrMouseMode": sgr_mouse,
            "sgrMousePixelsMode": sgr_pixels,
            "applicationCursor": app_cursor,
            "alternateScreen": alt_screen,
        },
        "cols": cols,
        "rows": rows,
        "scrollbackLines": scrollback_lines,
        "lastTitle": title,
    })
}

/// Control sequences that re-apply screen/input modes on reattach — a faithful
/// port of headless-emulator.ts `buildRehydrateSequences`. Order matters (alt
/// screen, bracketed paste, app cursor, mouse tracking, then SGR encoding), and
/// the SGR encoding is preserved even when mouse reporting is off.
fn rehydrate_sequences(
    mouse_on: bool,
    mouse_mode: &str,
    bracketed: bool,
    app_cursor: bool,
    alt_screen: bool,
    sgr_mouse: bool,
    sgr_pixels: bool,
) -> String {
    let mut s = String::new();
    if alt_screen {
        s.push_str("\x1b[?1049h");
    }
    if bracketed {
        s.push_str("\x1b[?2004h");
    }
    if app_cursor {
        s.push_str("\x1b[?1h");
    }
    match if mouse_on { mouse_mode } else { "none" } {
        "x10" => s.push_str("\x1b[?9h"),
        "vt200" => s.push_str("\x1b[?1000h"),
        "drag" => s.push_str("\x1b[?1002h"),
        "any" => s.push_str("\x1b[?1003h"),
        _ => {}
    }
    if sgr_pixels {
        s.push_str("\x1b[?1016h");
    } else if sgr_mouse {
        s.push_str("\x1b[?1006h");
    }
    s
}
