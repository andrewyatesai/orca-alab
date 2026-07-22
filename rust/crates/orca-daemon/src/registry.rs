//! Shared daemon state behind ONE mutex: live sessions (`sessionId → PTY + engine`)
//! and per-client event senders (`clientId → stream socket`). Sessions outlive the
//! control connection that created them (detach/reattach — the reason the daemon is a
//! separate process). Only LIVE sessions live in the map: a child exit reaps its
//! entry (`reap_and_mark_exited`), matching the Node daemon's `reapSession`, so
//! `listSessions` never shows zombies and a reattach to an exited id spawns fresh.
//!
//! Output while a client is detached is NOT buffered for raw replay — the reattach
//! response is a full engine snapshot (authoritative), so the stream simply drops
//! output for an absent client and the snapshot restores it. The `SessionEngine`
//! (headless terminal + the incremental-checkpoint record log) sits behind its OWN
//! per-session lock so the reader pump can feed it without the registry lock, and so
//! `takePendingOutput` can drain records + serialize a snapshot atomically.

use crate::pending_output::PendingOutput;
use crate::protocol::exit_event;
use crate::shell_ready_barrier::ShellReadyBarrier;
use crate::stream_coalescing::StreamItem;
use orca_pty::PtySession;
use orca_terminal::HeadlessTerminal;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

/// The per-session engine state, behind one lock: the headless aterm terminal (the
/// daemon answers getSnapshot/getCwd from it, no napi hop) plus the incremental
/// checkpoint record log. Bundling them under one lock lets the pump feed BOTH
/// atomically (so a checkpoint's drained records and its serialized snapshot can
/// never disagree — the load-bearing atomicity in takePendingOutput).
pub struct SessionEngine {
    pub terminal: HeadlessTerminal,
    pub pending: PendingOutput,
}

pub struct SessionEntry {
    pub pty: PtySession,
    pub client_id: String,
    pub cols: u16,
    pub rows: u16,
    pub pid: Option<u32>,
    pub created_at_ms: u128,
    pub engine: Arc<Mutex<SessionEngine>>,
    /// The shell-ready startup barrier (session.ts pre-ready stdin queue).
    /// `None` when the client didn't request readiness detection. Lock order:
    /// registry → barrier (never the reverse) — the pump locks it alone.
    pub barrier: Option<Arc<Mutex<ShellReadyBarrier>>>,
}

#[derive(Default)]
struct Inner {
    sessions: HashMap<String, SessionEntry>,
    /// One SEMANTIC-item sender per client stream socket. Wire format is a
    /// property of the socket's writer thread (stream_coalescing), not the
    /// channel — the registry routes the same items to every transport.
    streams: HashMap<String, Sender<StreamItem>>,
    /// v1019 read-only fan-out: `sessionId → subscriber clientIds` that mirror a
    /// session's data/exit events without owning it. Kept beside (not inside)
    /// `SessionEntry` so a disconnecting client is purged across all sessions in
    /// one pass (`remove_subscriber_from_all`).
    subscribers: HashMap<String, Vec<String>>,
}

#[derive(Default)]
pub struct Registry {
    inner: Mutex<Inner>,
    /// The daemon's socket path, so `shutdown` can unlink it (parity with the Node
    /// server.close→unlinkSync). `None` in the parity harness / standalone tests.
    socket_path: Mutex<Option<String>>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record the socket path for `unlink_socket` at shutdown. Called by `serve`.
    pub fn set_socket_path(&self, path: &str) {
        *self.socket_path.lock().unwrap() = Some(path.to_string());
    }

    /// Remove the socket file on graceful shutdown so a stale path can't linger.
    pub fn unlink_socket(&self) {
        if let Some(path) = self.socket_path.lock().unwrap().as_deref() {
            let _ = std::fs::remove_file(path);
        }
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
    pub fn reattach_if_alive(
        &self,
        session_id: &str,
        new_client_id: &str,
    ) -> Option<(Arc<Mutex<SessionEngine>>, &'static str)> {
        let mut guard = self.inner.lock().unwrap();
        let inner = &mut *guard;
        let entry = inner.sessions.get_mut(session_id)?;
        entry.client_id = new_client_id.to_string();
        // A subscriber that reattaches is PROMOTED to owner: drop its read-only
        // registration so it isn't double-delivered and authority keys off
        // ownership alone.
        if let Some(subs) = inner.subscribers.get_mut(session_id) {
            subs.retain(|c| c != new_client_id);
            if subs.is_empty() {
                inner.subscribers.remove(session_id);
            }
        }
        Some((Arc::clone(&entry.engine), shell_state_of(entry)))
    }

    /// Register `client_id` as a read-only subscriber of a live session (v1019).
    /// Deliberately does NOT touch `entry.client_id`: mirroring must not steal the
    /// owner's stream (only createOrAttach rebinds). Returns the engine handle +
    /// shell state so the caller can build the hydration snapshot; `None` if the
    /// id is unknown. Idempotent per (session, client).
    pub fn add_subscriber(
        &self,
        session_id: &str,
        client_id: &str,
    ) -> Option<(Arc<Mutex<SessionEngine>>, &'static str)> {
        let mut guard = self.inner.lock().unwrap();
        let inner = &mut *guard;
        let entry = inner.sessions.get(session_id)?;
        let subs = inner.subscribers.entry(session_id.to_string()).or_default();
        if !subs.iter().any(|c| c == client_id) {
            subs.push(client_id.to_string());
        }
        Some((Arc::clone(&entry.engine), shell_state_of(entry)))
    }

    /// Drop one subscriber registration. A no-op for an unknown session or a
    /// client that never subscribed (unsubscribe is idempotent, like detach).
    pub fn remove_subscriber(&self, session_id: &str, client_id: &str) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(subs) = inner.subscribers.get_mut(session_id) {
            subs.retain(|c| c != client_id);
            if subs.is_empty() {
                inner.subscribers.remove(session_id);
            }
        }
    }

    /// Purge a disconnecting client from every session's subscriber set — stream
    /// teardown calls this so a vanished follower never lingers as a fan-out
    /// target. The owner (and every other subscriber) is untouched.
    pub fn remove_subscriber_from_all(&self, client_id: &str) {
        let mut inner = self.inner.lock().unwrap();
        inner.subscribers.retain(|_, subs| {
            subs.retain(|c| c != client_id);
            !subs.is_empty()
        });
    }

    /// True when `client_id` may only READ this session: it is a registered
    /// subscriber and not the current owner (an owner that also subscribed keeps
    /// write authority — ownership wins).
    pub fn is_read_only_subscriber(&self, session_id: &str, client_id: &str) -> bool {
        let inner = self.inner.lock().unwrap();
        let subscribed = inner
            .subscribers
            .get(session_id)
            .is_some_and(|subs| subs.iter().any(|c| c == client_id));
        subscribed
            && inner
                .sessions
                .get(session_id)
                .map_or(true, |e| e.client_id != client_id)
    }

    /// Dispose a session outright (drop its PTY + engine). Used to clear a dead entry
    /// before re-creating the same id.
    pub fn remove_session(&self, id: &str) {
        self.inner.lock().unwrap().sessions.remove(id);
    }

    /// Register a client's stream-socket sender. Items are semantic (not
    /// pre-encoded), so one registration serves both NDJSON and binary
    /// sockets — the connection's writer thread encodes per its hello.
    pub fn register_stream(&self, client_id: String, tx: Sender<StreamItem>) {
        self.inner.lock().unwrap().streams.insert(client_id, tx);
    }

    pub fn unregister_stream(&self, client_id: &str) {
        self.inner.lock().unwrap().streams.remove(client_id);
    }

    /// Deliver one PTY output chunk to the owning client's stream (if attached) and
    /// fan it out to the session's subscribers (v1019); drop it for absent targets.
    /// A detached session isn't replayed raw — its reattach snapshot (built from
    /// the engine, which the pump keeps current) is authoritative.
    pub fn route_output(&self, session_id: &str, data: &str) {
        // Resolve every target Sender under the lock (cheap Sender clones), then
        // send OUTSIDE it. Recipients are bound HERE, per read — the invariant
        // that keeps a reattach snapshot and the live tail duplication-free
        // (the rejected pump-side batching deferred this binding). Encoding
        // moved into each socket's writer thread (stream_coalescing), which
        // coalesces adjacent same-session items, so this stays cheap per read.
        let txs = {
            let inner = self.inner.lock().unwrap();
            let Some(entry) = inner.sessions.get(session_id) else {
                return;
            };
            let mut targets: Vec<&str> = vec![entry.client_id.as_str()];
            if let Some(subs) = inner.subscribers.get(session_id) {
                for c in subs {
                    // Dedupe: an owner that also subscribed must not get doubled bytes.
                    if !targets.contains(&c.as_str()) {
                        targets.push(c.as_str());
                    }
                }
            }
            targets
                .iter()
                .filter_map(|c| inner.streams.get(*c).cloned())
                .collect::<Vec<_>>()
        };
        for tx in txs {
            let _ = tx.send(StreamItem::Data {
                session_id: session_id.to_string(),
                text: data.to_string(),
            });
        }
    }

    /// Run `f` against a live session (write/resize/kill). None if the id is unknown.
    pub fn with_session<R>(&self, id: &str, f: impl FnOnce(&mut SessionEntry) -> R) -> Option<R> {
        self.inner.lock().unwrap().sessions.get_mut(id).map(f)
    }

    /// Deliver a synthetic exit event to a specific client's stream. Used when a
    /// write/resize targets an unknown session: write/resize are fire-and-forget, so
    /// this exit is the only signal the renderer gets to clear a stale pane binding
    /// (parity with the Node daemon's sendExitEvent on SessionNotFoundError).
    pub fn route_exit_to_client(&self, client_id: &str, session_id: &str, code: i64) {
        let tx = self.inner.lock().unwrap().streams.get(client_id).cloned();
        if let Some(tx) = tx {
            send_exit(&tx, session_id, code);
        }
    }

    /// SIGKILL every live session's child — daemon `shutdown {killSessions:true}`,
    /// parity with the Node host.dispose(). Children die with the daemon via PTY
    /// master close anyway, but one that ignores that SIGHUP needs the explicit kill.
    pub fn kill_all_sessions(&self) {
        let mut inner = self.inner.lock().unwrap();
        for entry in inner.sessions.values_mut() {
            let _ = entry.pty.kill();
        }
    }

    /// Force-kill (SIGKILL) `session_id` iff it still maps to a live session with
    /// `expected_pid`. The graceful-kill escalation: a child that ignored SIGHUP
    /// within the kill timeout is force-killed, but the pid guard avoids killing a
    /// different session recreated on the same id in the meantime.
    pub fn force_kill_if_still_pid(&self, session_id: &str, expected_pid: Option<u32>) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(entry) = inner.sessions.get_mut(session_id) {
            if entry.pid == expected_pid {
                let _ = entry.pty.kill();
            }
        }
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

    /// Clone out the session's engine handle so a caller can query/feed it (snapshot,
    /// cwd, output/resize/clear records) without holding the registry lock.
    pub fn engine_of(&self, id: &str) -> Option<Arc<Mutex<SessionEngine>>> {
        self.inner
            .lock()
            .unwrap()
            .sessions
            .get(id)
            .map(|e| Arc::clone(&e.engine))
    }

    /// Drain the session's incremental-checkpoint batch and, when requested, serialize
    /// a full snapshot in the SAME engine-lock turn — the atomicity types.ts flags as
    /// load-bearing (a snapshot taken separately could include bytes a later drain
    /// would replay, duplicating them on cold restore). `None` if the id is unknown.
    pub fn take_pending_output(
        &self,
        session_id: &str,
        include_snapshot: bool,
        teardown_snapshot: bool,
    ) -> Option<(Vec<Value>, u64, bool, Value)> {
        let (engine, barrier) = {
            let inner = self.inner.lock().unwrap();
            let entry = inner.sessions.get(session_id)?;
            (Arc::clone(&entry.engine), entry.barrier.clone())
        };
        // Final (teardown) takes release the barrier's held partial-marker bytes
        // — session.ts prepareForFinalSnapshot. They are fed to the engine (its
        // parser just buffers the incomplete OSC, so the snapshot won't render
        // them) and returned as a post-checkpoint log-tail record below.
        let released_held = if include_snapshot && teardown_snapshot {
            barrier
                .map(|b| b.lock().unwrap().release_held_bytes())
                .unwrap_or_default()
        } else {
            String::new()
        };
        let mut engine = engine.lock().unwrap();
        if !released_held.is_empty() {
            engine.terminal.process(released_held.as_bytes());
        }
        // Always drain (resets the accumulator + advances seq), but a full-snapshot
        // checkpoint SUPERSEDES the incremental log: return the snapshot and DROP the
        // records, matching session.ts (which returns [] when includeSnapshot — held
        // bytes are the one exception, as a post-checkpoint tail). A plain
        // incremental take returns the records with no snapshot.
        let (records, seq, overflowed) = engine.pending.take();
        if include_snapshot {
            let snapshot = crate::rpc::build_snapshot(&mut engine.terminal);
            let records = if released_held.is_empty() {
                Vec::new()
            } else {
                vec![json!({ "kind": "output", "data": released_held })]
            };
            Some((records, seq, overflowed, snapshot))
        } else {
            Some((records, seq, overflowed, Value::Null))
        }
    }

    /// EOF on a session's PTY: REMOVE the session from the map (matching the Node
    /// daemon's reapSession — no zombies, no leaked engine), reap the child for its
    /// real exit code, and deliver an `exit` event to the owning client's stream
    /// and to every subscriber's (their mirrors must close too).
    /// The entry is removed UNDER the lock but `wait()`ed OUTSIDE it, so a child that
    /// hit master EOF while still alive (e.g. it closed its own slave fd) can't wedge
    /// the whole daemon on a blocking wait.
    pub fn reap_and_mark_exited(&self, session_id: &str) {
        let (mut entry, txs) = {
            let mut guard = self.inner.lock().unwrap();
            let inner = &mut *guard;
            let Some(entry) = inner.sessions.remove(session_id) else {
                return;
            };
            // Subscribers get the exit too (their mirror must close with the
            // session), and the session's fan-out set dies with its entry.
            let mut targets: Vec<String> = vec![entry.client_id.clone()];
            if let Some(subs) = inner.subscribers.remove(session_id) {
                for c in subs {
                    if !targets.contains(&c) {
                        targets.push(c);
                    }
                }
            }
            let txs: Vec<Sender<StreamItem>> = targets
                .iter()
                .filter_map(|c| inner.streams.get(c).cloned())
                .collect();
            (entry, txs)
        };
        let code = entry.pty.wait().map(|c| c as i64).unwrap_or(0);
        for tx in txs {
            send_exit(&tx, session_id, code);
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
                // shellState is the session's live barrier state (pending/ready/
                // timed_out), or "unsupported" when no barrier was requested.
                let cwd = e
                    .engine
                    .lock()
                    .ok()
                    .and_then(|eng| eng.terminal.cwd().map(str::to_string));
                json!({
                    "sessionId": sid,
                    "state": "running",
                    "shellState": shell_state_of(e),
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

/// Enqueue one exit event as a semantic item; the socket's writer thread
/// flushes any coalescing-pending data first, then encodes it per transport.
fn send_exit(tx: &Sender<StreamItem>, session_id: &str, code: i64) {
    let _ = tx.send(StreamItem::Event {
        json: exit_event(session_id, code),
    });
}

/// The session's wire `ShellReadyState`. Locked inside the registry lock —
/// consistent with the registry → barrier lock order used everywhere.
fn shell_state_of(entry: &SessionEntry) -> &'static str {
    entry
        .barrier
        .as_ref()
        .map(|b| b.lock().unwrap().state().as_wire())
        .unwrap_or("unsupported")
}
