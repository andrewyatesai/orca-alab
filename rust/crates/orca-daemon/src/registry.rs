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

use crate::bounded_stream_channel::StreamSender;
use crate::pending_output::{CheckpointSeqExhausted, PendingOutput};
use crate::protocol::exit_event;
use crate::shell_ready_barrier::ShellReadyBarrier;
use crate::stream_coalescing::StreamItem;
use orca_pty::PtySession;
use orca_terminal::HeadlessTerminal;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// One `take_pending_output` drain: `(records, seq, overflowed, snapshot)`.
pub type PendingOutputTake = (Vec<Value>, u64, bool, Value);

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
    /// Set (under the registry lock) when a `kill` is dispatched. A terminating
    /// session is fenced from reattach (`reattach_if_alive` → `Terminating`) so a
    /// createOrAttach within the graceful-kill window can't rebind the dying shell
    /// and then have the 5s SIGKILL tear down the freshly-reopened pane — parity
    /// with terminal-host.ts's `isTerminating`.
    pub terminating: bool,
}

/// The result of a `reattach_if_alive` attempt (createOrAttach's live branch).
pub enum ReattachOutcome {
    /// Rebound to a live session; carries the engine handle + wire shell state.
    Reattached(Arc<Mutex<SessionEngine>>, &'static str),
    /// The session exists but is mid-teardown — the caller must error ("Session is
    /// terminating"), NOT spawn fresh (that would orphan the dying child and let its
    /// reap remove the fresh entry).
    Terminating,
    /// No such session — the caller spawns fresh.
    Unknown,
}

#[derive(Default)]
struct Inner {
    sessions: HashMap<String, SessionEntry>,
    /// One SEMANTIC-item sender per client stream socket. Wire format is a
    /// property of the socket's writer thread (stream_coalescing), not the
    /// channel — the registry routes the same items to every transport.
    streams: HashMap<String, StreamSender>,
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
    /// v1021 federated search: the transient-replay cache for cold/parked
    /// content, keyed by (sessionId, checkpoint generation). Its OWN lock —
    /// a 5MB replay must never stall session routing under `inner`.
    search_replay_cache: Arc<Mutex<crate::session_search::SearchReplayCache>>,
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
    pub fn reattach_if_alive(&self, session_id: &str, new_client_id: &str) -> ReattachOutcome {
        let mut guard = self.inner.lock().unwrap();
        let inner = &mut *guard;
        let Some(entry) = inner.sessions.get_mut(session_id) else {
            return ReattachOutcome::Unknown;
        };
        // Fence a session mid-teardown: rebinding the dying shell here would let the
        // 5s graceful-kill SIGKILL escalation tear down the freshly-reopened pane.
        if entry.terminating {
            return ReattachOutcome::Terminating;
        }
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
        ReattachOutcome::Reattached(Arc::clone(&entry.engine), shell_state_of(entry))
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
    pub fn register_stream(&self, client_id: String, tx: StreamSender) {
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

    /// Complete a dispatched `kill` WITHOUT waiting for PTY master EOF — the
    /// "intent" teardown seam (authority model D4.4), twin of the "exit" seam
    /// `reap_and_mark_exited`. They answer different questions: EOF means *the last
    /// holder of the slave closed it*, which a backgrounded descendant defers
    /// indefinitely. Measured with a HUP-immune holder in its own process group —
    /// Linux keeps the master open forever (`disassociate_ctty` skips the hangup for
    /// PTYs), macOS/XNU revokes the ctty on session-leader exit and EOFs at once —
    /// so on Linux a closed pane stayed in the map, listed as running, still
    /// writable, and (because `terminating` is set) fenced from reattach for good.
    ///
    /// Force-kills, removes, reaps for the real exit code, and delivers `exit` to
    /// the owner + every subscriber. Every blocking step runs OFF the registry
    /// lock: portable_pty's `kill` SIGHUPs and polls up to 250ms before SIGKILL,
    /// which under the lock froze all other sessions' routing (measured 215ms).
    /// Idempotent with the exit seam — whichever removes the entry first wins — so
    /// both may fire for one pane. No Windows variant needed: `kill` there is
    /// TerminateProcess through the same removal/emit path.
    pub fn finalize_kill(&self, session_id: &str, expected_pid: Option<u32>) {
        let (mut entry, txs) = {
            let mut guard = self.inner.lock().unwrap();
            let inner = &mut *guard;
            // `terminating` is the load-bearing guard: a session freshly recreated
            // on this id has it clear, so a late finalize can't tear down a live
            // pane. The pid comparison additionally fences a recycled pid — but
            // only as equality, never `is_some()`: a platform that reports no pid
            // must still be finalizable, or such an entry is unreapable.
            let finalizable = inner
                .sessions
                .get(session_id)
                .is_some_and(|e| e.terminating && e.pid == expected_pid);
            if !finalizable {
                return;
            }
            let Some(entry) = inner.sessions.remove(session_id) else {
                return;
            };
            let txs = take_exit_targets(inner, session_id, &entry.client_id);
            (entry, txs)
        };
        let _ = entry.pty.kill();
        let code = exit_code_from_wait(entry.pty.wait());
        self.emit_exit_unless_superseded(session_id, &txs, code);
        // `entry` (PTY + headless engine) is dropped here, off the lock.
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

    /// Engine handles for `searchSessions`: every live session, or only the
    /// controller's allowlisted ids. Sorted by sessionId so the wire order is
    /// deterministic (HashMap iteration is not). Engines are searched OFF the
    /// registry lock — one slow regex must not wedge routing.
    pub fn engines_for_search(
        &self,
        allowlist: Option<&[String]>,
    ) -> Vec<(String, Arc<Mutex<SessionEngine>>)> {
        let inner = self.inner.lock().unwrap();
        let mut engines: Vec<(String, Arc<Mutex<SessionEngine>>)> = inner
            .sessions
            .iter()
            .filter(|(sid, _)| allowlist.map_or(true, |ids| ids.iter().any(|a| a == *sid)))
            .map(|(sid, e)| (sid.clone(), Arc::clone(&e.engine)))
            .collect();
        engines.sort_by(|a, b| a.0.cmp(&b.0));
        engines
    }

    /// The federated-search transient-replay cache (see `session_search`).
    pub fn search_replay_cache(
        &self,
    ) -> Arc<Mutex<crate::session_search::SearchReplayCache>> {
        Arc::clone(&self.search_replay_cache)
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
    ) -> Option<Result<PendingOutputTake, CheckpointSeqExhausted>> {
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
        let (records, seq, overflowed) = match engine.pending.take() {
            Ok(batch) => batch,
            Err(exhausted) => return Some(Err(exhausted)),
        };
        if include_snapshot {
            let snapshot = crate::rpc::build_snapshot(&mut engine.terminal);
            let records = if released_held.is_empty() {
                Vec::new()
            } else {
                vec![json!({ "kind": "output", "data": released_held })]
            };
            Some(Ok((records, seq, overflowed, snapshot)))
        } else {
            Some(Ok((records, seq, overflowed, Value::Null)))
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
            let txs = take_exit_targets(inner, session_id, &entry.client_id);
            (entry, txs)
        };
        let code = exit_code_from_wait(entry.pty.wait());
        self.emit_exit_unless_superseded(session_id, &txs, code);
        // `entry` (PTY + headless engine) is dropped here, off the lock.
    }

    /// Emit the reaped session's `exit` — UNLESS this id was recreated while we were
    /// off the lock in `wait()`. Between removing the exited entry and finishing
    /// `wait()` (unbounded when the child closed its slave fd while alive), a
    /// createOrAttach for the SAME id can spawn a fresh session bound to the same
    /// client stream; delivering the stale exit would tear that live pane down. The
    /// contains-key check + the enqueue are atomic w.r.t. `insert_session` (both hold
    /// `inner`), so no recreate can slip between them, and `send`/enqueue is
    /// non-blocking so holding the lock across it is cheap.
    fn emit_exit_unless_superseded(&self, session_id: &str, txs: &[StreamSender], code: i64) {
        let inner = self.inner.lock().unwrap();
        if inner.sessions.contains_key(session_id) {
            return;
        }
        for tx in txs {
            send_exit(tx, session_id, code);
        }
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

/// The `exit` recipients of a session just removed from the map: its owner plus
/// every subscriber (their mirrors must close with it). Takes the fan-out set
/// with the entry, so both teardown seams retire it exactly once.
fn take_exit_targets(inner: &mut Inner, session_id: &str, owner: &str) -> Vec<StreamSender> {
    let mut targets: Vec<String> = vec![owner.to_string()];
    if let Some(subs) = inner.subscribers.remove(session_id) {
        for c in subs {
            if !targets.contains(&c) {
                targets.push(c);
            }
        }
    }
    targets
        .iter()
        .filter_map(|c| inner.streams.get(c).cloned())
        .collect()
}

/// Map a PTY `wait()` result to the exit code sent on the `exit` event. A
/// SUCCESSFUL wait carries the child's real code (signals already surface as
/// non-zero via portable_pty). A wait FAILURE (already-reaped child, ConPTY
/// error) must NOT masquerade as `0` — code `0` is the clean-exit value the
/// renderer and session-restore treat as a successful shell exit, so a reap
/// failure would present a crashed/killed child as a clean termination
/// (false-green). Emit `-1`, the daemon's synthetic-abnormal convention
/// (`unknown_session_exit`), so a failed reap is never confused with a clean exit.
fn exit_code_from_wait(wait: std::io::Result<u32>) -> i64 {
    match wait {
        Ok(code) => code as i64,
        Err(_) => -1,
    }
}

/// Enqueue one exit event as a semantic item; the socket's writer thread
/// flushes any coalescing-pending data first, then encodes it per transport.
fn send_exit(tx: &StreamSender, session_id: &str, code: i64) {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A successful wait carries the child's real code through unchanged.
    #[test]
    fn exit_code_from_a_successful_wait_is_the_real_code() {
        assert_eq!(exit_code_from_wait(Ok(0)), 0);
        assert_eq!(exit_code_from_wait(Ok(1)), 1);
        assert_eq!(exit_code_from_wait(Ok(137)), 137);
    }

    /// The fix: a wait FAILURE must NOT report `0` (clean exit) — that false-green
    /// would present a crashed/unreapable child as a successful shell exit. It maps
    /// to the daemon's synthetic-abnormal `-1` instead.
    #[test]
    fn exit_code_from_a_failed_wait_is_minus_one_not_zero() {
        let err = Err(std::io::Error::other("wait failed: child already reaped"));
        assert_eq!(exit_code_from_wait(err), -1);
        let err = Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "ConPTY wait error",
        ));
        assert_eq!(exit_code_from_wait(err), -1);
    }

    /// A live `SessionEntry` backed by a real PTY child running `script`, for the
    /// registry-level fence/supersede tests. Killed via `kill_all_sessions` at test end.
    #[cfg(unix)]
    fn make_entry(client: &str, script: &str) -> SessionEntry {
        let pty = PtySession::spawn(
            &orca_pty::PtyCommand {
                program: "/bin/sh".to_string(),
                args: vec!["-c".to_string(), script.to_string()],
                ..Default::default()
            },
            orca_pty::PtySize { rows: 24, cols: 80 },
        )
        .expect("spawn");
        let pid = pty.process_id();
        SessionEntry {
            pty,
            client_id: client.to_string(),
            cols: 80,
            rows: 24,
            pid,
            created_at_ms: 0,
            engine: Arc::new(Mutex::new(SessionEngine {
                terminal: HeadlessTerminal::with_scrollback(24, 80, 1000),
                pending: PendingOutput::default(),
            })),
            barrier: None,
            terminating: false,
        }
    }

    /// Finding #2 fence: a session mid-teardown must NOT be reattached — the caller
    /// errors ("Session is terminating") instead of rebinding the dying shell.
    #[cfg(unix)]
    #[test]
    fn reattach_to_a_terminating_session_is_fenced() {
        let reg = Registry::new();
        let mut entry = make_entry("c1", "sleep 30");
        entry.terminating = true;
        reg.insert_session("s".to_string(), entry);
        assert!(matches!(
            reg.reattach_if_alive("s", "c2"),
            ReattachOutcome::Terminating
        ));
        reg.kill_all_sessions();
    }

    /// A live (non-terminating) session reattaches and rebinds to the new client.
    #[cfg(unix)]
    #[test]
    fn reattach_to_a_live_session_rebinds() {
        let reg = Registry::new();
        reg.insert_session("s".to_string(), make_entry("c1", "sleep 30"));
        assert!(matches!(
            reg.reattach_if_alive("s", "c2"),
            ReattachOutcome::Reattached(..)
        ));
        // Ownership rebound to the reattacher.
        reg.with_session("s", |e| assert_eq!(e.client_id, "c2"));
        reg.kill_all_sessions();
    }

    /// An unknown id is `Unknown` (→ caller spawns fresh), distinct from terminating.
    #[test]
    fn reattach_to_an_unknown_session_is_unknown() {
        let reg = Registry::new();
        assert!(matches!(
            reg.reattach_if_alive("nope", "c"),
            ReattachOutcome::Unknown
        ));
    }

    /// Finding #2: the escalation is a NO-OP on a non-terminating session even
    /// with a matching pid — so a fresh session recreated on this id is protected.
    /// The entry survives and the child runs to natural completion (exit 0).
    #[cfg(unix)]
    #[test]
    fn finalize_kill_is_a_noop_without_terminating() {
        let reg = Registry::new();
        let (tx, rx) = crate::bounded_stream_channel::stream_channel();
        reg.register_stream("c".to_string(), tx);
        let entry = make_entry("c", "sleep 0.4");
        let pid = entry.pid;
        reg.insert_session("s".to_string(), entry);
        reg.finalize_kill("s", pid); // not terminating → no-op
        let code = reg
            .with_session("s", |e| e.pty.wait().unwrap_or(0))
            .expect("a live pane must NOT be torn down by a late finalize");
        assert_eq!(code, 0, "an un-force-killed child exits cleanly, not by signal");
        assert!(rx.try_recv().is_err(), "no exit event for a live pane");
        reg.remove_session("s");
    }

    /// Same fence against a RECYCLED pid: terminating, but the entry's pid is not
    /// the one the kill was dispatched against.
    #[cfg(unix)]
    #[test]
    fn finalize_kill_is_a_noop_on_a_pid_mismatch() {
        let reg = Registry::new();
        let mut entry = make_entry("c", "sleep 30");
        entry.terminating = true;
        let pid = entry.pid.expect("spawned child reports a pid");
        reg.insert_session("s".to_string(), entry);
        reg.finalize_kill("s", Some(pid.wrapping_add(1)));
        assert!(reg.session_pid("s").is_some(), "the entry survives a pid mismatch");
        reg.kill_all_sessions();
        reg.remove_session("s");
    }

    /// The intent seam: a terminating session is force-killed AND removed AND
    /// reported as exited — with no EOF anywhere (this entry has no pump, which
    /// is exactly the state a slave-holding descendant leaves on Linux).
    #[cfg(unix)]
    #[test]
    fn finalize_kill_removes_a_terminating_session_without_any_eof() {
        let reg = Registry::new();
        let (tx, rx) = crate::bounded_stream_channel::stream_channel();
        reg.register_stream("c".to_string(), tx);
        let mut entry = make_entry("c", "trap '' HUP; while :; do sleep 1; done");
        entry.terminating = true;
        let pid = entry.pid;
        reg.insert_session("s".to_string(), entry);
        reg.finalize_kill("s", pid);
        assert!(
            reg.session_pid("s").is_none(),
            "a finalized pane must be OUT of the map, not left listed as running"
        );
        match rx.try_recv() {
            Ok(StreamItem::Event { json }) => {
                assert!(json.contains("\"exit\""), "the client is told it exited: {json}")
            }
            _ => panic!("finalize must deliver the exit the EOF reap never will"),
        }
    }

    /// Both seams may fire for one pane: after the intent seam finalized it, the
    /// pump's late EOF reap is a no-op — no panic, and no second exit event.
    #[cfg(unix)]
    #[test]
    fn the_exit_seam_is_a_noop_after_the_intent_seam_finalized() {
        let reg = Registry::new();
        let (tx, rx) = crate::bounded_stream_channel::stream_channel();
        reg.register_stream("c".to_string(), tx);
        let mut entry = make_entry("c", "sleep 30");
        entry.terminating = true;
        let pid = entry.pid;
        reg.insert_session("s".to_string(), entry);
        reg.finalize_kill("s", pid);
        assert!(rx.try_recv().is_ok(), "the finalize emitted one exit");
        reg.reap_and_mark_exited("s");
        assert!(rx.try_recv().is_err(), "the late EOF reap must not double-emit");
    }

    /// Finding #4: an exit for a reaped session must be SUPPRESSED when the id was
    /// recreated (a live entry now holds it) — else the fresh pane is torn down.
    #[cfg(unix)]
    #[test]
    fn stale_exit_is_suppressed_when_the_id_was_recreated() {
        let reg = Registry::new();
        let (tx, rx) = crate::bounded_stream_channel::stream_channel();
        // A fresh live session now owns "s" (simulating recreation during wait()).
        reg.insert_session("s".to_string(), make_entry("c", "sleep 30"));
        reg.emit_exit_unless_superseded("s", std::slice::from_ref(&tx), 0);
        assert!(
            rx.try_recv().is_err(),
            "no exit may be emitted for a recreated id"
        );
        reg.kill_all_sessions();
    }

    /// Finding #4: when the id is truly gone, the exit IS emitted (the normal path).
    #[test]
    fn exit_is_emitted_when_the_id_is_truly_gone() {
        let reg = Registry::new();
        let (tx, rx) = crate::bounded_stream_channel::stream_channel();
        reg.emit_exit_unless_superseded("s", std::slice::from_ref(&tx), 3);
        match rx.try_recv() {
            Ok(StreamItem::Event { json }) => {
                assert!(json.contains("\"exit\""), "an exit event: {json}");
                assert!(json.contains("\"code\":3"), "carries the real code: {json}");
            }
            _ => panic!("expected an exit event for a truly-gone session"),
        }
    }
}
