//! The daemon's shared state: live sessions (`sessionId → PTY`) and the per-client
//! event channel (`clientId → sender into that client's stream socket`). Sessions
//! outlive the control connection that created them (the whole reason the daemon
//! is a separate process — detach/reattach), so they live here, not on a socket.
//!
//! Spike scope: sessions do not yet persist across a daemon restart (that is
//! sub-step 3, via orca-store), and a `data` event produced before the matching
//! stream socket registers is dropped rather than buffered (sub-step 2 adds
//! `takePendingOutput`).

use orca_pty::PtySession;
use std::collections::HashMap;
use std::sync::mpsc::Sender;
use std::sync::Mutex;

/// One live terminal session: the PTY for write/resize/kill, plus the owning
/// client so its output/exit events route to the right stream socket.
pub struct SessionEntry {
    pub pty: PtySession,
    pub cols: u16,
    pub rows: u16,
    // Populated for the `SessionInfo` the full `listSessions`/reattach will report
    // (sub-step 2); the spike routes events via the pump's own copies, so these
    // aren't read yet.
    #[allow(dead_code)]
    pub client_id: String,
    #[allow(dead_code)]
    pub pid: Option<u32>,
}

#[derive(Default)]
pub struct Registry {
    sessions: Mutex<HashMap<String, SessionEntry>>,
    /// clientId → sender of ready-to-write NDJSON lines for that client's stream socket.
    streams: Mutex<HashMap<String, Sender<String>>>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_stream(&self, client_id: String, tx: Sender<String>) {
        self.streams.lock().unwrap().insert(client_id, tx);
    }

    pub fn unregister_stream(&self, client_id: &str) {
        self.streams.lock().unwrap().remove(client_id);
    }

    /// Route one already-encoded NDJSON line to a client's stream socket. A no-op
    /// (dropped) if that client has no stream socket yet — see the spike-scope note.
    pub fn send_to_client(&self, client_id: &str, line: String) {
        if let Some(tx) = self.streams.lock().unwrap().get(client_id) {
            let _ = tx.send(line);
        }
    }

    pub fn insert_session(&self, id: String, entry: SessionEntry) {
        self.sessions.lock().unwrap().insert(id, entry);
    }

    pub fn remove_session(&self, id: &str) -> Option<SessionEntry> {
        self.sessions.lock().unwrap().remove(id)
    }

    /// Run `f` against a live session under the lock (write/resize). `None` if the
    /// session id is unknown (already exited / never created).
    pub fn with_session<R>(&self, id: &str, f: impl FnOnce(&mut SessionEntry) -> R) -> Option<R> {
        self.sessions.lock().unwrap().get_mut(id).map(f)
    }
}
