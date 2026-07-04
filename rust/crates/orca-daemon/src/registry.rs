//! Shared daemon state behind ONE mutex: live sessions (`sessionId → PTY + buffered
//! output`) and per-client event senders (`clientId → stream socket`). Sessions
//! outlive the control connection that created them (detach/reattach — the reason
//! the daemon is a separate process); output produced while the owning client has no
//! stream socket is BUFFERED on the session and replayed on reattach (stream flush /
//! `takePendingOutput`). One mutex over both maps keeps routing race-free without a
//! lock-order discipline. Only LIVE sessions live in the map: a child exit reaps its
//! entry (`reap_and_mark_exited`), matching the Node daemon's `reapSession` — so
//! `listSessions` never shows zombies and a reattach to an exited id spawns fresh.

use crate::protocol::{data_event, exit_event};
use orca_net::encode_ndjson_line;
use orca_pty::PtySession;
use orca_terminal::HeadlessTerminal;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

pub struct SessionEntry {
    pub pty: PtySession,
    pub client_id: String,
    pub cols: u16,
    pub rows: u16,
    pub pid: Option<u32>,
    pub created_at_ms: u128,
    /// Output produced while the owning client had no stream socket; drained on reattach.
    pub pending: Vec<u8>,
    /// The headless aterm engine this session's raw output is teed into, so the
    /// daemon answers getSnapshot/getCwd from real engine state (no napi hop). Its
    /// own lock lets the reader pump feed it without holding the registry lock.
    pub terminal: Arc<Mutex<HeadlessTerminal>>,
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

    /// Reattach an existing LIVE session to a (possibly different) client — the
    /// warm-reattach path after an app relaunch mints a new clientId. Rebinds the
    /// session's owning client so live output/exit route to the reattacher, and
    /// returns the engine handle so the caller can build the reattach snapshot.
    /// `None` if the id is unknown (→ the caller spawns fresh). Mirrors
    /// terminal-host.ts createOrAttach's detachAllClients()+attachClient()+getSnapshot().
    ///
    /// The detached backlog is DISCARDED, not replayed: every byte was already teed
    /// into the engine, so the snapshot the caller builds from it is authoritative —
    /// replaying the raw bytes on top would double-apply the detached output. Only
    /// output produced AFTER this reattach (buffered until the stream reconnects) is
    /// replayed, by register_stream_and_flush.
    pub fn reattach_if_alive(
        &self,
        session_id: &str,
        new_client_id: &str,
    ) -> Option<Arc<Mutex<HeadlessTerminal>>> {
        let mut inner = self.inner.lock().unwrap();
        let entry = inner.sessions.get_mut(session_id)?;
        entry.client_id = new_client_id.to_string();
        entry.pending.clear();
        Some(Arc::clone(&entry.terminal))
    }

    /// Dispose a session outright (drop its PTY + engine). Used to clear a dead
    /// entry before re-creating the same id.
    pub fn remove_session(&self, id: &str) {
        self.inner.lock().unwrap().sessions.remove(id);
    }

    /// A stream socket (re)connected for `client_id`: install its sender AND replay
    /// every owned session's buffered backlog in ONE lock turn, sending the backlog
    /// before any later `route_output` can run — so the replay can't be overtaken by
    /// live output. Replaces the old register-then-flush two-step (a delivery race).
    pub fn register_stream_and_flush(&self, client_id: String, tx: Sender<String>) {
        let mut inner = self.inner.lock().unwrap();
        let drained: Vec<(String, Vec<u8>)> = inner
            .sessions
            .iter_mut()
            .filter(|(_, e)| e.client_id == client_id && !e.pending.is_empty())
            .map(|(sid, e)| (sid.clone(), std::mem::take(&mut e.pending)))
            .collect();
        for (sid, bytes) in &drained {
            let text = String::from_utf8_lossy(bytes);
            let _ = tx.send(encode_ndjson_line(&data_event(sid, text.as_ref())));
        }
        inner.streams.insert(client_id, tx);
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

    /// Clone out the session's headless terminal handle so a caller can query it
    /// (snapshot/cwd) without holding the registry lock.
    pub fn terminal_of(&self, id: &str) -> Option<Arc<Mutex<HeadlessTerminal>>> {
        self.inner
            .lock()
            .unwrap()
            .sessions
            .get(id)
            .map(|e| Arc::clone(&e.terminal))
    }

    /// EOF on a session's PTY: REMOVE the session from the map (matching the Node
    /// daemon's reapSession — no zombies, no leaked engine), reap the child for its
    /// real exit code, and deliver an `exit` event to the owning client's stream.
    /// The entry is removed UNDER the lock but `wait()`ed OUTSIDE it, so a child that
    /// hit master EOF while still alive (e.g. it closed its own slave fd) can't wedge
    /// the whole daemon on a blocking wait.
    pub fn reap_and_mark_exited(&self, session_id: &str) {
        let (mut entry, tx) = {
            let mut inner = self.inner.lock().unwrap();
            let Some(entry) = inner.sessions.remove(session_id) else {
                return;
            };
            let tx = inner.streams.get(&entry.client_id).cloned();
            (entry, tx)
        };
        let code = entry.pty.wait().map(|c| c as i64).unwrap_or(0);
        if let Some(tx) = tx {
            let _ = tx.send(encode_ndjson_line(&exit_event(session_id, code)));
        }
        // `entry` (PTY + headless engine) is dropped here, off the lock.
    }

    /// `SessionInfo[]` for `listSessions`.
    pub fn list_sessions(&self) -> Value {
        let inner = self.inner.lock().unwrap();
        let sessions: Vec<Value> = inner
            .sessions
            .iter()
            .map(|(sid, e)| {
                // Every entry in the map is live (exited sessions are reaped), so
                // state is always running/isAlive:true — the Node daemon likewise
                // only lists live sessions. Real engine cwd (OSC 7), not null;
                // shellState is the honest valid ShellReadyState ("unsupported" — no
                // OSC-133 readiness detection here).
                let cwd = e.terminal.lock().ok().and_then(|t| t.cwd().map(str::to_string));
                json!({
                    "sessionId": sid,
                    "state": "running",
                    "shellState": "unsupported",
                    "isAlive": true,
                    "pid": e.pid,
                    "cwd": cwd,
                    "cols": e.cols,
                    "rows": e.rows,
                    "createdAt": e.created_at_ms as u64,
                })
            })
            .collect();
        json!({ "sessions": sessions })
    }
}
