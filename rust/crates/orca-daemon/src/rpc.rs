//! Control-socket RPC dispatch (the spike subset) + the per-session output pump.
//! Requests arrive as `serde_json::Value`; each returns the NDJSON response line
//! the control socket writes back. `createOrAttach` additionally spawns the PTY
//! and the reader thread that streams its output as `data` events.

use crate::protocol::{data_event, exit_event, rpc_err, rpc_ok};
use crate::registry::{Registry, SessionEntry};
use orca_net::encode_ndjson_line;
use orca_pty::{PtyCommand, PtySession, PtySize};
use serde_json::{json, Value};
use std::io::Read;
use std::sync::Arc;
use std::thread;

pub fn dispatch_request(request: &Value, registry: &Arc<Registry>, client_id: &str) -> String {
    let id = request.get("id").and_then(Value::as_str).unwrap_or("");
    let kind = request.get("type").and_then(Value::as_str).unwrap_or("");
    let payload = request.get("payload").cloned().unwrap_or(Value::Null);
    match kind {
        "createOrAttach" => create_or_attach(id, &payload, registry, client_id),
        "write" => {
            let sid = payload
                .get("sessionId")
                .and_then(Value::as_str)
                .unwrap_or("");
            let data = payload.get("data").and_then(Value::as_str).unwrap_or("");
            match registry.with_session(sid, |e| e.pty.write_all(data.as_bytes())) {
                Some(Ok(())) => rpc_ok(id, Value::Null),
                Some(Err(e)) => rpc_err(id, &e.to_string()),
                None => rpc_err(id, "unknown session"),
            }
        }
        "resize" => {
            let sid = payload
                .get("sessionId")
                .and_then(Value::as_str)
                .unwrap_or("");
            let cols = payload.get("cols").and_then(Value::as_u64).unwrap_or(80) as u16;
            let rows = payload.get("rows").and_then(Value::as_u64).unwrap_or(24) as u16;
            match registry.with_session(sid, |e| {
                e.cols = cols;
                e.rows = rows;
                e.pty.resize(PtySize { rows, cols })
            }) {
                Some(Ok(())) => rpc_ok(id, Value::Null),
                Some(Err(e)) => rpc_err(id, &e.to_string()),
                None => rpc_err(id, "unknown session"),
            }
        }
        "kill" => {
            let sid = payload
                .get("sessionId")
                .and_then(Value::as_str)
                .unwrap_or("");
            if let Some(mut entry) = registry.remove_session(sid) {
                let _ = entry.pty.kill();
            }
            rpc_ok(id, Value::Null)
        }
        "ping" => rpc_ok(id, json!({ "pong": true })),
        // Sessions do not yet enumerate across the seam (sub-step 2).
        "listSessions" => rpc_ok(id, json!({ "sessions": [] })),
        other => rpc_err(id, &format!("unsupported in spike: {other}")),
    }
}

fn create_or_attach(
    id: &str,
    payload: &Value,
    registry: &Arc<Registry>,
    client_id: &str,
) -> String {
    let session_id = payload
        .get("sessionId")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if session_id.is_empty() {
        return rpc_err(id, "missing sessionId");
    }
    let cols = payload.get("cols").and_then(Value::as_u64).unwrap_or(80) as u16;
    let rows = payload.get("rows").and_then(Value::as_u64).unwrap_or(24) as u16;
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
    // Pump raw PTY output → `data` events on the owning client's stream socket; on
    // EOF, drop the session and emit `exit`. The reader is an independent clone of
    // the master, so it keeps reading after `pty` moves into the registry.
    let pump_registry = registry.clone();
    let pump_session = session_id.clone();
    let pump_client = client_id.to_string();
    thread::spawn(move || pump_output(reader, pump_registry, pump_session, pump_client));

    registry.insert_session(
        session_id,
        SessionEntry {
            pty,
            client_id: client_id.to_string(),
            cols,
            rows,
            pid,
        },
    );
    rpc_ok(
        id,
        json!({ "isNew": true, "snapshot": null, "pid": pid, "shellState": "unknown" }),
    )
}

/// Resolve the shell/command to spawn. `command` (when present) runs under the
/// login shell; otherwise the user's `$SHELL` (or /bin/sh) starts interactively.
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

fn pump_output(
    mut reader: Box<dyn Read + Send>,
    registry: Arc<Registry>,
    session_id: String,
    client_id: String,
) {
    let mut buf = [0u8; 65536];
    loop {
        match reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let data = String::from_utf8_lossy(&buf[..n]);
                registry.send_to_client(
                    &client_id,
                    encode_ndjson_line(&data_event(&session_id, data.as_ref())),
                );
            }
        }
    }
    // EOF: the child exited. The spike reports code 0 (the real daemon wait()s for
    // the true code); drop the session and notify the client.
    registry.remove_session(&session_id);
    registry.send_to_client(&client_id, encode_ndjson_line(&exit_event(&session_id, 0)));
}
