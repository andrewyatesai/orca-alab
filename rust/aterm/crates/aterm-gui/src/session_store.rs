// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! Process-wide SESSION REGISTRY (design P1.1) — the additive index that makes
//! every live session resolvable by its stable [`SessionId`] (and by the
//! process-local `u64` id the GUI already routes `Wake`s with), WITHOUT moving
//! the GUI's `Vec<Session>` pane model.
//!
//! ## Why it lives here (not in `aterm-session`)
//!
//! A [`SessionHandle`] holds an `Arc<Mutex<Terminal>>` (an `aterm-core` type) and
//! an `Arc<SessionCtx>` (a `aterm-gui` type). `aterm-session` deliberately depends
//! on NEITHER (it is the headless policy/transport core), so the registry that
//! binds the live engine handle to the fabric identity has to live in the binary
//! that owns both. The IDENTITY (`SessionId`/`LaunchNonce`) and the AUTHORITY
//! (`EdgeTable`/`decide_edge`) it gates on are still the `aterm-session` types — we
//! only add the in-process index over them.
//!
//! ## Discipline (the one hard rule)
//!
//! The registry is read on the control thread to resolve a cross-session target;
//! the resolver CLONES the `(term, sink, sid, nonce, ...)` tuple OUT of the store
//! and DROPS the store guard BEFORE locking the target `Terminal` — exactly the
//! clone-then-release discipline `resolve_active` uses. The store lock is NEVER
//! held across a `Terminal` lock, so two agents driving each other (A→B, B→A)
//! cannot deadlock on the registry.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use aterm_core::terminal::Terminal;
use aterm_session::{LaunchNonce, SessionId};

use crate::SessionCtx;

/// A session's lifecycle as the registry observes it. A session stays READABLE
/// after its command exits (`Exited`) until the pane is torn down and it is
/// deregistered. `Spawning` is the brief pre-`Alive` window; in the current
/// single-step `spawn_session` path a handle is registered already `Alive`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SessionState {
    /// Reader thread not yet producing (reserved for the future async-spawn path).
    // Part of the documented lifecycle; the current single-step spawn registers
    // already-`Alive`, so this variant is constructed only by tests today.
    #[allow(dead_code)]
    Spawning,
    /// Live: a reader thread is feeding the engine.
    Alive,
    /// The command exited; the engine is still readable until the pane closes.
    Exited,
}

impl SessionState {
    /// The stable wire token for the `sessions` verb.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            SessionState::Spawning => "spawning",
            SessionState::Alive => "alive",
            SessionState::Exited => "exited",
        }
    }
}

/// One registered session: its stable fabric identity, the process-local id the
/// GUI routes with, the live engine + sink handles, and its lifecycle/title.
///
/// `term`/`sink`/`ctx` are the SAME `Arc`s the owning `Session` holds, so a
/// cross-session read is literally `handle.term.lock()` — zero new data path,
/// fully live, zero-copy.
#[derive(Clone)]
pub struct SessionHandle {
    /// Stable, pid-free fabric identity (the canonical registry key).
    pub sid: SessionId,
    /// This launch's nonce — an edge binds to it so a restart under a reused id
    /// fails closed (confused-deputy safe). The cross-session gate reads the live
    /// `ctx.nonce` (same value); this mirror is recorded for cross-process restart
    /// safety per the design and for audit.
    #[allow(dead_code)]
    pub nonce: LaunchNonce,
    /// The process-local id the GUI's `Wake`/`Vec<Session>` routing uses.
    pub local_id: u64,
    /// The spawning session's `sid`, if any (the family tree; `None` for tab-0 /
    /// user-opened tabs).
    pub parent: Option<SessionId>,
    /// Lifecycle as the registry observes it.
    pub state: SessionState,
    /// The live window title (best-effort; updated on relabel).
    pub title: String,
    /// The live engine handle — shared with the owning `Session` (zero-copy read).
    pub term: Arc<Mutex<Terminal>>,
    /// This session's PTY master fd (for `signal`'s `tcgetpgrp`/`killpg`).
    pub master: i32,
    /// The per-session fabric context (sink + edge table + identity).
    pub ctx: Arc<SessionCtx>,
}

/// The process-wide registry. Keyed canonically by [`SessionId`]; a second index
/// bridges the GUI's `u64` ids to those sids. Both key spaces are mutated under
/// the one outer `RwLock`, so a register/deregister is atomic across them.
#[derive(Default)]
pub struct SessionStore {
    by_id: HashMap<SessionId, SessionHandle>,
    by_local: HashMap<u64, SessionId>,
}

/// Shared handle to the registry, cloned into the control thread alongside the
/// existing `ActiveHandle`.
pub type Store = Arc<RwLock<SessionStore>>;

/// A new, empty, shared registry.
#[must_use]
pub fn new_store() -> Store {
    Arc::new(RwLock::new(SessionStore::default()))
}

impl SessionStore {
    /// Register (or replace) a handle, wiring BOTH key spaces atomically. Replacing
    /// an existing `sid` (e.g. a relabel) keeps the `by_local` bridge consistent.
    pub fn register(&mut self, handle: SessionHandle) {
        self.by_local.insert(handle.local_id, handle.sid.clone());
        self.by_id.insert(handle.sid.clone(), handle);
    }

    /// Deregister the session with process-local id `local_id`, removing it from
    /// BOTH key spaces atomically. A no-op (returns `false`) if it is unknown — a
    /// late deregister mirrors the existing `is_active_session` miss.
    pub fn deregister_local(&mut self, local_id: u64) -> bool {
        match self.by_local.remove(&local_id) {
            Some(sid) => {
                self.by_id.remove(&sid);
                true
            }
            None => false,
        }
    }

    /// Mark the session's lifecycle state (e.g. `Exited` on `Wake::Exit`). A no-op
    /// if the id is unknown.
    pub fn set_state(&mut self, local_id: u64, state: SessionState) {
        if let Some(sid) = self.by_local.get(&local_id) {
            if let Some(h) = self.by_id.get_mut(sid) {
                h.state = state;
            }
        }
    }

    /// Update the live title for a session (best-effort, on relabel). Takes `&str`
    /// (the caller no longer allocates a `String` per redraw) and only mutates on an
    /// ACTUAL change, so a no-op relabel reuses the existing buffer. No-op if unknown.
    pub fn set_title(&mut self, local_id: u64, title: &str) {
        if let Some(sid) = self.by_local.get(&local_id) {
            if let Some(h) = self.by_id.get_mut(sid) {
                if h.title != title {
                    h.title.clear();
                    h.title.push_str(title);
                }
            }
        }
    }

    /// Look up a handle by its stable [`SessionId`]. Total + fail-closed: an
    /// unknown id returns `None`.
    #[must_use]
    pub fn by_sid(&self, sid: &SessionId) -> Option<&SessionHandle> {
        self.by_id.get(sid)
    }

    /// Look up a handle by its process-local `u64` id. Total + fail-closed.
    #[must_use]
    pub fn by_local(&self, local_id: u64) -> Option<&SessionHandle> {
        self.by_local.get(&local_id).and_then(|sid| self.by_id.get(sid))
    }

    /// Number of registered sessions.
    #[must_use]
    #[allow(dead_code)] // used by tests + the forward-compat subscribe cap (P1.3)
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    /// Whether the registry is empty.
    #[must_use]
    #[allow(dead_code)] // used by tests
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    /// A snapshot of every registered handle, for the `sessions` verb. Cloned so
    /// the caller can drop the store guard before formatting (and never holds it
    /// across a `Terminal` lock). Sorted by `local_id` for a stable listing.
    #[must_use]
    pub fn snapshot(&self) -> Vec<SessionHandle> {
        let mut v: Vec<SessionHandle> = self.by_id.values().cloned().collect();
        v.sort_by_key(|h| h.local_id);
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aterm_session::sink::SinkWriter;
    use aterm_session::EdgeTable;

    fn handle(local_id: u64, parent: Option<SessionId>) -> SessionHandle {
        let sid = SessionId::generate();
        let nonce = LaunchNonce::generate();
        let ctx = Arc::new(SessionCtx {
            sink: Arc::new(SinkWriter::new(-1)),
            edges: Mutex::new(EdgeTable::new()),
            self_id: sid.clone(),
            nonce,
            cast: Arc::new(Mutex::new(crate::cast::CastRecorder::new(80, 24))),
            temporal: Arc::new(Mutex::new(crate::temporal::TemporalRecorder::new())),
            byte_fanout: Arc::new(crate::cast::ByteFanout::new()),
        });
        SessionHandle {
            sid,
            nonce,
            local_id,
            parent,
            state: SessionState::Alive,
            title: format!("tab-{local_id}"),
            term: Arc::new(Mutex::new(Terminal::new(24, 80))),
            master: -1,
            ctx,
        }
    }

    #[test]
    fn register_indexes_both_key_spaces_and_deregister_clears_both() {
        let mut store = SessionStore::default();
        let h = handle(7, None);
        let sid = h.sid.clone();
        store.register(h);

        assert_eq!(store.len(), 1);
        assert!(store.by_sid(&sid).is_some(), "resolvable by sid");
        assert!(store.by_local(7).is_some(), "resolvable by local id");
        assert_eq!(store.by_local(7).unwrap().sid, sid, "both keys agree");

        // Deregister clears BOTH key spaces atomically.
        assert!(store.deregister_local(7));
        assert!(store.by_sid(&sid).is_none(), "sid index cleared");
        assert!(store.by_local(7).is_none(), "local index cleared");
        assert!(store.is_empty());
        // A second deregister is a no-op (late/duplicate close).
        assert!(!store.deregister_local(7));
    }

    #[test]
    fn unknown_lookup_is_fail_closed_none() {
        let store = SessionStore::default();
        assert!(store.by_local(999).is_none());
        assert!(store.by_sid(&SessionId::new("s-nope")).is_none());
    }

    #[test]
    fn snapshot_is_sorted_by_local_id_and_carries_parent() {
        let mut store = SessionStore::default();
        let root = handle(0, None);
        let root_sid = root.sid.clone();
        store.register(root);
        store.register(handle(2, Some(root_sid.clone())));
        store.register(handle(1, Some(root_sid.clone())));

        let snap = store.snapshot();
        assert_eq!(snap.iter().map(|h| h.local_id).collect::<Vec<_>>(), vec![0, 1, 2]);
        assert_eq!(snap[0].parent, None, "root has no parent");
        assert_eq!(snap[1].parent.as_ref(), Some(&root_sid), "child links to root");
    }

    #[test]
    fn set_state_and_title_mutate_in_place() {
        let mut store = SessionStore::default();
        store.register(handle(3, None));
        store.set_state(3, SessionState::Exited);
        store.set_title(3, "renamed");
        let h = store.by_local(3).unwrap();
        assert_eq!(h.state, SessionState::Exited);
        assert_eq!(h.title, "renamed");
        // Unknown ids are no-ops, not panics.
        store.set_state(99, SessionState::Exited);
        store.set_title(99, "x");
    }
}
