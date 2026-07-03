//! Shared daemon state behind ONE mutex: live sessions (`sessionId → PTY + buffered
//! output`) and per-client event senders (`clientId → stream socket`). Sessions
//! outlive the control connection that created them (detach/reattach — the reason
//! the daemon is a separate process); output produced while the owning client has no
//! stream socket is BUFFERED on the session and replayed on reattach (stream flush /
//! `takePendingOutput`). One mutex over both maps keeps routing race-free without a
//! lock-order discipline.

use crate::protocol::{data_event, exit_event};
use orca_net::encode_ndjson_line;
use orca_pty::PtySession;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::mpsc::Sender;
use std::sync::Mutex;

pub struct SessionEntry {
    pub pty: PtySession,
    pub client_id: String,
    pub cols: u16,
    pub rows: u16,
    pub pid: Option<u32>,
    pub created_at_ms: u128,
    /// Output produced while the owning client had no stream socket; drained on reattach.
    pub pending: Vec<u8>,
    pub alive: bool,
}

#[derive(Default)]
struct Inner {
    sessions: HashMap<String, SessionEntry>,
    streams: HashMap<String, Sender<String>>,
}

#[derive(Default)]
pub struct Registry {
    inner: Mutex<Inner>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert_session(&self, id: String, entry: SessionEntry) {
        self.inner.lock().unwrap().sessions.insert(id, entry);
    }

    pub fn session_exists(&self, id: &str) -> bool {
        self.inner.lock().unwrap().sessions.contains_key(id)
    }

    pub fn register_stream(&self, client_id: String, tx: Sender<String>) {
        self.inner.lock().unwrap().streams.insert(client_id, tx);
    }

    pub fn unregister_stream(&self, client_id: &str) {
        self.inner.lock().unwrap().streams.remove(client_id);
    }

    /// Route one PTY output chunk: deliver live if the owning client has a stream
    /// socket, else buffer it on the session for replay on reattach.
    pub fn route_output(&self, session_id: &str, data: &str) {
        let mut inner = self.inner.lock().unwrap();
        let Some(client_id) = inner.sessions.get(session_id).map(|e| e.client_id.clone()) else {
            return;
        };
        if let Some(tx) = inner.streams.get(&client_id) {
            let _ = tx.send(encode_ndjson_line(&data_event(session_id, data)));
        } else if let Some(entry) = inner.sessions.get_mut(session_id) {
            entry.pending.extend_from_slice(data.as_bytes());
        }
    }

    /// A stream socket (re)connected for `client_id`: flush every owned session's
    /// buffered output to it as `data` events, clearing the buffers.
    pub fn flush_pending_for_client(&self, client_id: &str) {
        let mut inner = self.inner.lock().unwrap();
        let drained: Vec<(String, Vec<u8>)> = inner
            .sessions
            .iter_mut()
            .filter(|(_, e)| e.client_id == client_id && !e.pending.is_empty())
            .map(|(sid, e)| (sid.clone(), std::mem::take(&mut e.pending)))
            .collect();
        if let Some(tx) = inner.streams.get(client_id) {
            for (sid, bytes) in drained {
                let text = String::from_utf8_lossy(&bytes);
                let _ = tx.send(encode_ndjson_line(&data_event(&sid, text.as_ref())));
            }
        }
    }

    /// Drain + return a session's buffered output (`takePendingOutput`).
    pub fn take_pending(&self, session_id: &str) -> String {
        let mut inner = self.inner.lock().unwrap();
        match inner.sessions.get_mut(session_id) {
            Some(entry) => {
                String::from_utf8_lossy(&std::mem::take(&mut entry.pending)).into_owned()
            }
            None => String::new(),
        }
    }

    /// Run `f` against a live session (write/resize/kill). None if the id is unknown.
    pub fn with_session<R>(&self, id: &str, f: impl FnOnce(&mut SessionEntry) -> R) -> Option<R> {
        self.inner.lock().unwrap().sessions.get_mut(id).map(f)
    }

    /// The session's (cols, rows) for `getSize`.
    pub fn session_size(&self, id: &str) -> Option<(u16, u16)> {
        self.inner
            .lock()
            .unwrap()
            .sessions
            .get(id)
            .map(|e| (e.cols, e.rows))
    }

    /// The session's pid for the `createOrAttach` reattach response.
    pub fn session_pid(&self, id: &str) -> Option<u32> {
        self.inner
            .lock()
            .unwrap()
            .sessions
            .get(id)
            .and_then(|e| e.pid)
    }

    /// Mark a session's child dead and deliver an `exit` event to its client (if a
    /// stream is connected; an exit while detached is reflected by `listSessions`
    /// `isAlive:false` on reattach — spike scope, no exit buffering).
    pub fn mark_exited(&self, session_id: &str, code: i64) {
        let mut inner = self.inner.lock().unwrap();
        let Some(entry) = inner.sessions.get_mut(session_id) else {
            return;
        };
        entry.alive = false;
        let client_id = entry.client_id.clone();
        let line = encode_ndjson_line(&exit_event(session_id, code));
        if let Some(tx) = inner.streams.get(&client_id) {
            let _ = tx.send(line);
        }
    }

    /// `SessionInfo[]` for `listSessions`.
    pub fn list_sessions(&self) -> Value {
        let inner = self.inner.lock().unwrap();
        let sessions: Vec<Value> = inner
            .sessions
            .iter()
            .map(|(sid, e)| {
                json!({
                    "sessionId": sid,
                    "state": if e.alive { "running" } else { "exited" },
                    "shellState": "unknown",
                    "isAlive": e.alive,
                    "pid": e.pid,
                    "cwd": Value::Null,
                    "cols": e.cols,
                    "rows": e.rows,
                    "createdAt": e.created_at_ms as u64,
                })
            })
            .collect();
        json!({ "sessions": sessions })
    }
}
