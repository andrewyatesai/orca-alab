//! Control-socket RPC dispatch + the per-session output pump. Requests arrive as
//! `serde_json::Value`; each returns the NDJSON response line the control socket
//! writes back. `createOrAttach` additionally spawns the PTY and the reader thread
//! that routes its output (live if the client's stream is connected, else buffered
//! for reattach — see `registry`).

use crate::protocol::{rpc_err, rpc_ok};
use crate::registry::{Registry, SessionEntry};
use orca_pty::{PtyCommand, PtySession, PtySize};
use serde_json::{json, Value};
use std::io::Read;
use std::sync::Arc;
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
        "getSize" => match registry.session_size(&sid()) {
            Some((cols, rows)) => rpc_ok(id, json!({ "cols": cols, "rows": rows })),
            None => rpc_err(id, "unknown session"),
        },
        "ping" => rpc_ok(id, json!({ "pong": true })),
        "ptySpawnHealth" => rpc_ok(id, json!({ "ok": true })),
        "systemResolverHealth" => rpc_ok(id, json!({ "health": "unknown" })),
        // Need the headless terminal fed the same bytes (next increment); until then
        // report empty so the client falls back to its own scrollback.
        "getSnapshot" => rpc_ok(id, json!({ "snapshot": Value::Null })),
        "getCwd" => rpc_ok(id, Value::Null),
        "getForegroundProcess" => rpc_ok(id, Value::Null),
        "clearScrollback" => rpc_ok(id, Value::Null),
        "cancelCreateOrAttach" => rpc_ok(id, Value::Null),
        // portable-pty has no per-signal API; the app's Ctrl-C flows through `write`
        // (\x03) as PTY input, not this RPC, so erroring here is safe for the spike.
        "signal" => rpc_err(id, "signal not supported in the spike"),
        "shutdown" => {
            // Reply first, then exit so the ok flushes to the client.
            thread::spawn(|| {
                thread::sleep(Duration::from_millis(50));
                std::process::exit(0);
            });
            rpc_ok(id, Value::Null)
        }
        other => rpc_err(id, &format!("unsupported in spike: {other}")),
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
    // Reattach: an existing session keeps running; the client's stream flush replays
    // buffered output. Report isNew:false with the live pid.
    if registry.session_exists(&session_id) {
        let pid = registry.session_pid(&session_id);
        return rpc_ok(
            id,
            json!({ "isNew": false, "snapshot": Value::Null, "pid": pid, "shellState": "unknown" }),
        );
    }

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
    // Pump raw PTY output → routed (live or buffered) by the registry; on EOF, mark
    // the session exited + emit `exit`. The reader is an independent clone of the
    // master, so it keeps reading after `pty` moves into the registry.
    let pump_registry = registry.clone();
    let pump_session = session_id.clone();
    thread::spawn(move || pump_output(reader, pump_registry, pump_session));

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
            alive: true,
        },
    );
    rpc_ok(
        id,
        json!({ "isNew": true, "snapshot": Value::Null, "pid": pid, "shellState": "unknown" }),
    )
}

/// Resolve the shell/command to spawn. `command` (when present) runs under the login
/// shell; otherwise the user's `$SHELL` (or /bin/sh) starts interactively.
fn build_command(payload: &Value) -> PtyCommand {
    let cwd = payload
        .get("cwd")
        .and_then(Value::as_str)
        .map(str::to_string);
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    match payload.get("command").and_then(Value::as_str) {
        Some(cmd) if !cmd.is_empty() => PtyCommand {
            program: shell,
            args: vec!["-lc".to_string(), cmd.to_string()],
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

fn pump_output(mut reader: Box<dyn Read + Send>, registry: Arc<Registry>, session_id: String) {
    let mut buf = [0u8; 65536];
    loop {
        match reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let data = String::from_utf8_lossy(&buf[..n]);
                registry.route_output(&session_id, data.as_ref());
            }
        }
    }
    // EOF: the child exited. The spike reports code 0 (the real daemon wait()s for the
    // true code); mark the session exited and notify the client.
    registry.mark_exited(&session_id, 0);
}
