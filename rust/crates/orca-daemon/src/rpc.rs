//! Control-socket RPC dispatch + the per-session output pump. Requests arrive as
//! `serde_json::Value`; each returns the NDJSON response line the control socket
//! writes back. `createOrAttach` additionally spawns the PTY and the reader thread
//! that feeds the session engine (terminal + checkpoint records) and streams output
//! live to the client (dropped when detached — the reattach snapshot restores it).

use crate::pending_output::PendingOutput;
use crate::protocol::{rpc_err, rpc_ok};
use crate::registry::{Registry, SessionEngine, SessionEntry};
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

/// A void ack. The Node daemon returns `{}` (not null) for side-effecting RPCs, so
/// match its payload shape for wire byte-parity.
fn void_ack(id: &str) -> String {
    rpc_ok(id, json!({}))
}

pub fn dispatch_request(request: &Value, registry: &Arc<Registry>, client_id: &str) -> String {
    let id = request.get("id").and_then(Value::as_str).unwrap_or("");
    let kind = request.get("type").and_then(Value::as_str).unwrap_or("");
    // Borrow the payload — do NOT clone it: a `write` payload carries the full data
    // chunk (up to NDJSON_MAX_LINE_BYTES), and cloning it here would copy megabytes
    // per keystroke burst before field_str even reads it.
    let null_payload = Value::Null;
    let payload = request.get("payload").unwrap_or(&null_payload);
    let sid = || field_str(payload, "sessionId").to_string();
    match kind {
        "createOrAttach" => create_or_attach(id, payload, registry, client_id),
        "write" => {
            let data = field_str(payload, "data").to_string();
            match registry.with_session(&sid(), |e| e.pty.write_all(data.as_bytes())) {
                Some(Ok(())) => void_ack(id),
                Some(Err(e)) => rpc_err(id, &e.to_string()),
                None => rpc_err(id, "unknown session"),
            }
        }
        "resize" => {
            let cols = field_u16(payload, "cols", 80);
            let rows = field_u16(payload, "rows", 24);
            match registry.with_session(&sid(), |e| {
                e.cols = cols;
                e.rows = rows;
                if let Ok(mut engine) = e.engine.lock() {
                    engine.terminal.resize(rows as usize, cols as usize);
                    engine.pending.record_resize(cols, rows);
                }
                e.pty.resize(PtySize { rows, cols })
            }) {
                Some(Ok(())) => void_ack(id),
                Some(Err(e)) => rpc_err(id, &e.to_string()),
                None => rpc_err(id, "unknown session"),
            }
        }
        // Kill the child; the pump's EOF then reaps the session + emits `exit`. An
        // unknown session errors, like the Node daemon's getAliveSession.
        "kill" => match registry.with_session(&sid(), |e| {
            let _ = e.pty.kill();
        }) {
            Some(()) => void_ack(id),
            None => rpc_err(id, "unknown session"),
        },
        // The session already survives control-socket close, so detach is a no-op ack.
        "detach" => void_ack(id),
        // The incremental checkpoint batch: typed records + monotonic seq + overflow
        // flag, and (when requested) a snapshot serialized in the same atomic turn.
        // Mirrors the Node daemon's TakePendingOutputResult (types.ts) — the client
        // appends each batch to the on-disk history log for crash cold-restore.
        "takePendingOutput" => {
            let include_snapshot = payload
                .get("includeSnapshot")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            match registry.take_pending_output(&sid(), include_snapshot) {
                Some((records, seq, overflowed, snapshot)) => rpc_ok(
                    id,
                    json!({ "records": records, "seq": seq, "overflowed": overflowed, "snapshot": snapshot }),
                ),
                None => rpc_err(id, "unknown session"),
            }
        }
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
        "getSnapshot" => match registry.engine_of(&sid()) {
            Some(engine) => {
                let snapshot = build_snapshot(&mut engine.lock().unwrap().terminal);
                rpc_ok(id, json!({ "snapshot": snapshot }))
            }
            None => rpc_ok(id, json!({ "snapshot": Value::Null })),
        },
        // Wire shape mirrors the Node daemon: `{ cwd: <string|null> }`.
        "getCwd" => {
            let cwd = registry
                .engine_of(&sid())
                .and_then(|engine| engine.lock().unwrap().terminal.cwd().map(str::to_string));
            rpc_ok(id, json!({ "cwd": cwd }))
        }
        // Wire shape mirrors the Node daemon: `{ foregroundProcess: <…|null> }`.
        // Process-group query isn't wired yet, so the value is null (a safe stub).
        "getForegroundProcess" => rpc_ok(id, json!({ "foregroundProcess": Value::Null })),
        // An unknown session errors, like host.clearScrollback → getAliveSession.
        "clearScrollback" => match registry.engine_of(&sid()) {
            Some(engine) => {
                let mut engine = engine.lock().unwrap();
                engine.terminal.clear_scrollback();
                engine.pending.record_clear();
                void_ack(id)
            }
            None => rpc_err(id, "unknown session"),
        },
        "cancelCreateOrAttach" => void_ack(id),
        // Deliver a named signal to the child (node-pty's `kill(signal)`). Errors
        // from a dead child are dropped like the Node daemon; an unknown session
        // errors (host.signal throws on a missing session there too).
        "signal" => {
            let sig = field_str(payload, "signal").to_string();
            match registry.with_session(&sid(), |e| e.pty.signal(&sig)) {
                Some(_) => void_ack(id),
                None => rpc_err(id, "unknown session"),
            }
        }
        "shutdown" => {
            // Reply first, then exit so the ok flushes to the client.
            thread::spawn(|| {
                thread::sleep(Duration::from_millis(50));
                std::process::exit(0);
            });
            void_ack(id)
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
        ..PtyCommand::default()
    }
}

#[cfg(windows)]
fn probe_command() -> PtyCommand {
    PtyCommand {
        program: default_shell(),
        args: vec!["/C".to_string(), "exit".to_string(), "0".to_string()],
        ..PtyCommand::default()
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
    // Reattach a live session: rebind it to this (possibly new) client and return a
    // REAL snapshot so the reattacher repaints — matching terminal-host.ts's live
    // branch (getSnapshot + detachAllClients + attachClient). A blank snapshot here
    // would leave a warm-reattached pane frozen after relaunch.
    if let Some(engine) = registry.reattach_if_alive(&session_id, client_id) {
        let snapshot = build_snapshot(&mut engine.lock().unwrap().terminal);
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
    // Per-session engine (headless aterm terminal + checkpoint record log) behind one
    // lock, so the pump feeds both atomically and getSnapshot/getCwd/takePendingOutput
    // read consistent state — no napi hop.
    let engine = Arc::new(Mutex::new(SessionEngine {
        terminal: HeadlessTerminal::with_scrollback(rows as usize, cols as usize, DEFAULT_SCROLLBACK),
        pending: PendingOutput::default(),
    }));
    // Pump raw PTY output → fed into the engine (terminal + records) AND streamed live
    // by the registry; on EOF, reap the child (remove the session + emit `exit`). The
    // reader is an independent clone of the master, so it keeps reading after `pty`
    // moves into the registry entry.
    let pump_registry = registry.clone();
    let pump_session = session_id.clone();
    let pump_engine = Arc::clone(&engine);
    thread::spawn(move || pump_output(reader, pump_registry, pump_session, pump_engine));

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
            engine,
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
    let (program, args) = match payload.get("command").and_then(Value::as_str) {
        Some(cmd) if !cmd.is_empty() => (shell, shell_run_args(cmd)),
        _ => (shell, Vec::new()),
    };
    PtyCommand {
        program,
        args,
        cwd,
        // Per-session env overrides (agent hooks, per-profile vars) and deletions —
        // the createOrAttach `env` / `envToDelete` the adapter forwards. Dropping
        // these ran daemon-spawned shells with only the daemon's inherited env.
        env: payload_env(payload),
        env_remove: payload_env_to_delete(payload),
    }
}

/// The `env` object (`{ KEY: "value" }`) as override pairs; empty if absent.
fn payload_env(payload: &Value) -> Vec<(String, String)> {
    payload
        .get("env")
        .and_then(Value::as_object)
        .map(|map| {
            map.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

/// The `envToDelete` array of inherited var names to remove; empty if absent.
fn payload_env_to_delete(payload: &Value) -> Vec<String> {
    payload
        .get("envToDelete")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default()
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
    engine: Arc<Mutex<SessionEngine>>,
) {
    let mut buf = [0u8; 65536];
    loop {
        match reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let data = String::from_utf8_lossy(&buf[..n]);
                // Feed the engine ATOMICALLY: raw bytes into the VT parser AND the
                // same chunk into the checkpoint record log, under one lock so a
                // concurrent takePendingOutput can't see the terminal updated but the
                // record missing (which would duplicate bytes on cold restore).
                if let Ok(mut engine) = engine.lock() {
                    engine.terminal.process(&buf[..n]);
                    engine.pending.record_output(data.as_ref());
                }
                // Stream a lossy-UTF-8 copy live to the attached client (dropped if
                // detached — the reattach snapshot restores it).
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
/// ranges so links survive a reconnect. `pub(crate)` so the registry can serialize a
/// snapshot in the same engine-lock turn as a checkpoint drain (takePendingOutput).
pub(crate) fn build_snapshot(term: &mut HeadlessTerminal) -> Value {
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
    let mut snapshot = json!({
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
    });
    // `lastTitle` is an OPTIONAL field: the Node daemon OMITS the key when no title
    // has been set, rather than emitting null. Match that so the wire shape agrees.
    if let Some(title) = title {
        snapshot["lastTitle"] = json!(title);
    }
    snapshot
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
