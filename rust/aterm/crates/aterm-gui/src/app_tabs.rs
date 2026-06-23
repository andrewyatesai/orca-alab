// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! Tab + session-view management: the windows/pool/focus_order-mutating cluster
//! moved as ONE unit so the invariant-maintaining set stays cohesive. Open/
//! switch/cycle/move/close tabs, detach/migrate a tab to a new window, the
//! tab-strip title/fingerprint/hit-test helpers, session teardown, the close
//! outcome funnel, and the live structural-invariant oracle. A verbatim
//! inherent-impl split of `App`.

use winit::event_loop::ActiveEventLoop;

use crate::platform::AppRt;
use crate::spawn::spawn_session;
use crate::{
    App, TabAction, TabIndex, WindowId, WindowState, pane, session_store, tab_bar, term_lock,
};

impl App {
    /// Whether the visible tab strip is enabled (`tab_strip_rows > 0`). GLOBAL. The
    /// whole strip path (splice + paint + hit-test) is gated on this; `false` is the
    /// byte-identical no-strip path.
    pub(crate) fn tab_strip_enabled(&self) -> bool {
        self.tab_strip_rows > 0
    }

    /// One title per TAB (top-level) of window `wid`, for the strip labels: each
    /// tab's label is its FOCUSED pane's session title (the same title the window
    /// chrome shows for the active tab). A tab whose session can't be found
    /// (impossible mid-frame) yields `"aterm"`. Indexed in lockstep with the
    /// window's `tabs`/`layouts`.
    pub(crate) fn tab_titles(&self, wid: WindowId) -> Vec<String> {
        let Some(ws) = self.windows.get(&wid) else {
            return Vec::new();
        };
        ws.layouts
            .iter()
            .map(|tree| {
                self.pool
                    .get(tree.focus())
                    .map(|s| term_lock(&s.term).title().to_string())
                    .filter(|t| !t.is_empty())
                    .unwrap_or_else(|| "aterm".to_string())
            })
            .collect()
    }

    /// A cheap fingerprint of the VISIBLE tab strip — tab count, active index, and a
    /// hash of every tab's title — folded into the redraw [`RepaintKey`] so a tab
    /// switch / open / close / title change repaints the strip even when the terminal
    /// grid below is unchanged. Always `0` when the strip is disabled, keeping the
    /// key byte-identical to the pre-strip path. Computed from ALREADY-READ titles —
    /// no extra term locks: the redraw hot path reads the per-tab titles ONCE
    /// (`tab_titles`) and feeds the SAME `Vec` to both this and `splice_tab_strip_with`,
    /// instead of locking every tab's terminal twice per present (once to hash, once
    /// to paint).
    /// Byte-identical to hashing `tab_titles(wid)`: same count + active + title bytes.
    pub(crate) fn tab_strip_fingerprint_from(&self, titles: &[String], active: usize) -> u64 {
        if !self.tab_strip_enabled() {
            return 0;
        }
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        titles.len().hash(&mut h);
        active.hash(&mut h);
        for t in titles {
            t.hash(&mut h);
        }
        // Never collide with the disabled-strip sentinel (0): a real strip always
        // sets at least bit 0, so a zero-hash strip still forces the first repaint.
        h.finish() | 1
    }

    /// "Move Tab to New Window" (Cmd-Shift-N / Window ▸ Move Tab to New Window): pull
    /// the frontmost window's ACTIVE tab OUT into a brand-new in-process window. The
    /// view MOVES — the existing `Session` is never spawned, dropped, or duplicated
    /// (the pool's view-count stays 1), so there is zero PTY churn. This is the
    /// logical half: it does everything EXCEPT attach the OS surface, returning the
    /// new window's id (or `None` if the move was refused), so it is headless-testable.
    ///
    /// Refused (returns `None`) when the source window has only ONE tab — detaching
    /// the sole tab would just relocate the window, a no-op.
    pub(crate) fn detach_active_tab_logical(&mut self) -> Option<WindowId> {
        let wid_a = self.frontmost_window?;
        // Can only detach when the source window has MORE than one tab.
        let (i, tree, rows, cols) = match self.windows.get(&wid_a) {
            Some(ws) if ws.layouts.len() > 1 => (
                ws.tabs.active,
                ws.layouts[ws.tabs.active].clone(),
                ws.rows,
                ws.cols,
            ),
            _ => return None,
        };
        // The moved tab's FOCUSED pane is the new window's active session.
        let t = tree.focus();
        // Remove the whole tab (its pane tree) from A; clamp A's active. NO
        // `pool.detach` — the VIEW(s) MOVE to B, the pool's view-counts stay, the
        // Session(s) live on.
        if let Some(ws) = self.windows.get_mut(&wid_a) {
            ws.layouts.remove(i);
            ws.tabs.close(i);
        }
        // Build window B holding the EXISTING tab (no spawn, no pool insert) — its
        // panes are already pooled, so just clone the focused pane's mirror Arcs.
        let s = self.pool.get(t)?;
        let (term, master, sink) = (s.term.clone(), s.master, s.ctx.sink.clone());
        let wid_b = WindowId(self.next_window_id);
        self.next_window_id += 1;
        let ws_b = WindowState::new(
            term,
            master,
            sink,
            t,
            rows,
            cols,
            TabIndex::new(0, 1),
            vec![tree],
        );
        self.windows.insert(wid_b, ws_b);
        self.frontmost_window = Some(wid_b);
        // Re-mirror BOTH: A's active tab changed (it lost its old active), and B is
        // the new frontmost (also re-points the global control/notify handle to B).
        // NOTE: `t`'s reader thread stamped its `Wake::Output` with the OLD window A,
        // but `Wake::Output` routes via `windows_displaying(t)` — now B, since
        // B.active_id == t — so the moved tab's output repaints B without a re-stamp.
        self.sync_window(wid_a);
        self.sync_active_session(); // frontmost = B
        debug_assert!(self.structural_invariants_ok());
        Some(wid_b)
    }

    /// Full "Move Tab to New Window": the logical move + (when not headless) the
    /// winit OS-window attach for the new window. A refused move (single-tab source)
    /// is a silent no-op.
    pub(crate) fn detach_active_tab(&mut self, el: &ActiveEventLoop) {
        // Capture the SOURCE window BEFORE the move (the logical step re-points
        // frontmost to the new window), so a rollback can return the tab to it.
        let wid_a = self.frontmost_window;
        let Some(wid_b) = self.detach_active_tab_logical() else {
            return;
        };
        if !self.headless && !self.attach_os_window(el, wid_b) {
            self.detach_rollback_logical(wid_a, wid_b);
        }
    }

    /// Undo a `detach_active_tab_logical` when the new window's OS surface failed
    /// (el-free). Detach is a PURE view-move (no `pool.attach`/`detach`), so the
    /// moved session is window B's SOLE view; `close_window_logical(B)` would detach
    /// it (views 1→0) and DESTROY the live shell. Instead REVERSE the move: return
    /// the tab's pane tree to source window A (no pool churn → the session survives),
    /// then drop the empty, never-shown B. (Contrast the share/create rollbacks,
    /// where `close_window_logical` is correct: the shared view survives at 2→1, and
    /// a fresh window's brand-new session has no other home.)
    pub(crate) fn detach_rollback_logical(&mut self, wid_a: Option<WindowId>, wid_b: WindowId) {
        let returned = self
            .windows
            .remove(&wid_b)
            .and_then(|ws_b| ws_b.layouts.into_iter().next());
        if let (Some(tree), Some(ws_a)) = (returned, wid_a.and_then(|a| self.windows.get_mut(&a))) {
            ws_a.layouts.push(tree);
            ws_a.tabs.add(); // re-append the tab and make it active again
        }
        self.winit_to_window.retain(|_, &mut v| v != wid_b);
        self.focus_order.retain(|w| *w != wid_b);
        self.frontmost_window = wid_a;
        self.sync_active_session();
    }

    /// "Move Tab to Next Window" (Cmd-Shift-M / Window ▸ Move Tab to Next Window): move
    /// the frontmost window's ACTIVE tab into the NEXT EXISTING window (BTreeMap id
    /// order, wrapping to the first), and follow it there (the destination becomes
    /// frontmost). Unlike `detach_active_tab` — which MOVES the view into a BRAND-NEW
    /// window — this targets an EXISTING window, so it never attaches a winit OS
    /// surface and needs no `ActiveEventLoop`: it is fully headless-safe and the
    /// keyboard/menu paths call it directly (no `Wake` round-trip).
    ///
    /// It is a PURE view-move: the `Session` is never spawned, dropped, or duplicated,
    /// so the pool's view-count stays unchanged (zero PTY churn). If the source window
    /// held ONLY that one tab it becomes empty and is CLOSED — a "merge the source's
    /// last tab into the next window". A no-op with fewer than two windows (nowhere to
    /// move the tab).
    pub(crate) fn migrate_active_tab_to_next_window(&mut self) {
        let Some(wid_a) = self.frontmost_window else {
            return;
        };
        // Need at least two windows: with one there is nowhere to move the tab.
        if self.windows.len() < 2 {
            return;
        }
        // The NEXT window after A in id order, wrapping to the first. With ≥2 windows
        // this resolves to some window other than A.
        let dest = self
            .windows
            .range((std::ops::Bound::Excluded(wid_a), std::ops::Bound::Unbounded))
            .next()
            .map(|(k, _)| *k)
            .or_else(|| self.windows.keys().next().copied());
        let Some(wid_b) = dest else { return };
        if wid_b == wid_a {
            return; // defensive: never move a tab onto its own window
        }
        // Pull A's active tab (its whole pane tree) out (clamp A's active). NO
        // `pool.detach` — the VIEW(s) MOVE to B, the pool's view-counts are untouched,
        // the Session(s) live.
        let (i, tree) = match self.windows.get(&wid_a) {
            Some(ws) if !ws.layouts.is_empty() => {
                (ws.tabs.active, ws.layouts[ws.tabs.active].clone())
            }
            _ => return,
        };
        // Whether A will be EMPTY after the move (it held only the tab we're moving).
        let source_now_empty = self
            .windows
            .get(&wid_a)
            .is_some_and(|ws| ws.layouts.len() == 1);
        if let Some(ws) = self.windows.get_mut(&wid_a) {
            ws.layouts.remove(i);
            ws.tabs.close(i);
        }
        // Append the EXISTING tab (pane tree) to B and make it active there (NO pool
        // change — the view moved; `tabs.add()` bumps count to match the push).
        if let Some(ws) = self.windows.get_mut(&wid_b) {
            ws.layouts.push(tree);
            ws.tabs.add();
        }
        // Focus follows the moved tab: the destination becomes frontmost.
        self.frontmost_window = Some(wid_b);
        // Resize the moved panes to B's grid: a migrate to a DIFFERENT-sized window
        // must SIGWINCH the moved panes' engines + PTYs to B's cell geometry, or they
        // keep A's stale grid (no reflow, no SIGWINCH). `resize_panes` no-ops per pane
        // when the dims already match (so it's free when A and B are the same size)
        // and re-lays + SIGWINCHes otherwise — mirroring how `apply_close_outcome`
        // pairs `resize_panes(wid)` with `sync_window(wid)`.
        self.resize_panes(wid_b);
        // Re-mirror B onto its now-active moved tab `t`. NOTE: `t`'s reader thread
        // stamped its `Wake::Output` with the OLD window A, but `Output` routes via
        // `windows_displaying(t)` — now B, since B.active_id == t after this sync — so
        // the moved tab's output repaints B with no re-stamp.
        self.sync_window(wid_b);
        if source_now_empty {
            // A has no tabs left. Close it BEFORE any structural assert (the oracle
            // forbids a 0-tab window). `t` is already gone from A's tab_ids, so
            // `close_window_logical` iterates A's CURRENT (empty) tab_ids and detaches
            // NOTHING — the moved view's count is untouched (no double-detach). Frontmost
            // is already B (≠ A), so the close's re-point leaves B frontmost.
            let _ = self.close_window_logical(wid_a);
        } else {
            // A survives with its remaining tabs: re-mirror its clamped active tab.
            self.sync_window(wid_a);
        }
        // Frontmost = B: re-point the global control/notify handle onto B's active tab.
        self.sync_active_session();
        debug_assert!(
            self.structural_invariants_ok(),
            "window/session structural invariants violated after migrate_active_tab_to_next_window",
        );
    }

    /// "Open Active Session in New Window" (Cmd-Shift-O / Window ▸ Open Session in New
    /// Window): show the frontmost window's ACTIVE session in a SECOND window, so the
    /// same live terminal grid is visible in two windows at once ("watch a log in one,
    /// type in another"). Unlike `detach_active_tab` this ADDS a view rather than
    /// MOVING one: the source window keeps its tab, and a fresh window is built viewing
    /// the SAME pooled session (no spawn). The pool's view-count goes 1→2, so the PTY
    /// stays open until BOTH viewers detach (each `close_window_logical` of a viewing
    /// tab drops one view); the `pool.attach` here is paired with exactly one future
    /// `pool.detach`. This is the logical half (everything EXCEPT the OS-window attach),
    /// returning the new window's id (or `None` if no session is in view), so it is
    /// headless-testable.
    pub(crate) fn open_active_session_in_new_window_logical(&mut self) -> Option<WindowId> {
        let wid_a = self.frontmost_window?;
        // Share the FOCUSED pane's session as a fresh SINGLE-PANE tab in B. A
        // shared (views>1) session is always a full single-pane tab on each side —
        // it is never split (split-spawned panes are always views=1), so B holds a
        // single-leaf pane tree on the focused session.
        let (s, rows, cols) = match self.windows.get(&wid_a) {
            Some(ws) => (ws.layouts[ws.tabs.active].focus(), ws.rows, ws.cols),
            None => return None,
        };
        // Bump the view count: the session is now displayed by TWO windows. The PTY
        // stays open until BOTH detach (views back to 0).
        self.pool.attach(s);
        // Build window B viewing the SAME pooled session (no spawn). Clone the mirror
        // Arcs from the pool.
        let Some(sess) = self.pool.get(s) else {
            self.pool.detach(s); // unwind the attach on the impossible miss
            return None;
        };
        let (term, master, sink) = (sess.term.clone(), sess.master, sess.ctx.sink.clone());
        let wid_b = WindowId(self.next_window_id);
        self.next_window_id += 1;
        let ws_b = WindowState::new(
            term,
            master,
            sink,
            s,
            rows,
            cols,
            TabIndex::new(0, 1),
            vec![pane::PaneTree::new(s)],
        );
        self.windows.insert(wid_b, ws_b);
        self.frontmost_window = Some(wid_b);
        // Re-mirror BOTH viewers: B is the new frontmost (also re-points the global
        // control/notify handle to B). A is unchanged — it still displays `s`. NOTE:
        // `s`'s reader thread stamps its `Wake::Output` with ONE owning window, but the
        // `Output` arm routes via `windows_displaying(s)` — now BOTH A and B, since both
        // have `active_id == s` — so the shared session's output repaints both viewers
        // with no re-stamp (the multi-viewer fan-out is now genuinely exercised).
        self.sync_active_session(); // frontmost = B
        debug_assert!(self.structural_invariants_ok());
        Some(wid_b)
    }

    /// Full "Open Active Session in New Window": the logical attach-a-view + (when not
    /// headless) the winit OS-window attach for the new window. A no-session-in-view
    /// front window is a silent no-op.
    pub(crate) fn open_active_session_in_new_window(&mut self, el: &ActiveEventLoop) {
        let Some(wid) = self.open_active_session_in_new_window_logical() else {
            return;
        };
        if !self.headless && !self.attach_os_window(el, wid) {
            // GPU surface failed: drop the new viewer. `close_window_logical` detaches
            // its SHARED view (views N→N-1), so the session survives in the original
            // window — no black orphan, no lost session.
            self.close_window_logical(wid);
        }
    }

    /// Test-only: append a stub `session` as a NEW tab of EXISTING window `wid` and
    /// switch to it (mirrors `open_tab`'s id-list edit without a real PTY spawn). The
    /// session is pooled (one view) so `tab_ids[active]` resolves; `session.id` MUST
    /// equal `self.next_session_id` (the test builds it that way), which is then
    /// bumped. Used to stage a multi-tab front window for the detach test. Re-mirrors
    /// the window so its active mirror/`active_id` track the appended tab.
    #[cfg(test)]
    pub(crate) fn push_stub_tab(&mut self, wid: WindowId, session: crate::Session) {
        debug_assert_eq!(
            session.id, self.next_session_id,
            "stub tab session id must match the minted session id",
        );
        let sid = session.id;
        self.next_session_id += 1;
        Self::register_session(&self.store, &session, None);
        self.pool.insert(session);
        if let Some(ws) = self.windows.get_mut(&wid) {
            ws.layouts.push(pane::PaneTree::new(sid));
            ws.tabs.add();
        }
        // `tabs.add()` switched the active tab to the new one; if `wid` is frontmost
        // the global handle must follow it too (matches `open_tab_in`), so the test
        // harness mirrors production's "active-tab change re-points the handle".
        self.resync_active_or_window(wid);
    }

    /// Test-only: split window `wid`'s ACTIVE tab into a 2-pane vertical split,
    /// spawning a fresh stub session for the new (now-focused) pane. Mirrors
    /// `split_focused_pane`'s pooling/registration without a real PTY. Returns the
    /// new pane's session id. Used to exercise split-tab teardown headlessly.
    #[cfg(test)]
    pub(crate) fn split_active_stub_tab(&mut self, wid: WindowId) -> u64 {
        let sid = self.next_session_id;
        self.next_session_id += 1;
        let stub = crate::stub_session(sid);
        Self::register_session(&self.store, &stub, None);
        self.pool.insert(stub);
        if let Some(t) = self.active_tree_mut(wid) {
            assert!(
                t.split_focused(pane::SplitDir::Vertical, sid),
                "stub split must succeed"
            );
        }
        self.sync_window(wid);
        sid
    }

    /// Cmd-T: open a new tab — a fresh shell session in the SAME window — and
    /// switch to it. Spawns the session via the factory (its own PTY/engine/policy/
    /// OSC52/reader + a FRESH shell-integration nonce) at the current grid size. A
    /// spawn failure is logged and ignored (the existing tabs survive); it does NOT
    /// take down the window, unlike a fatal session-0 failure at startup.
    pub(crate) fn open_tab(&mut self) {
        // Cmd-T / menu open in the FRONTMOST window.
        if let Some(front) = self.frontmost_window {
            self.open_tab_in(front);
        }
    }

    /// Open a new tab in window `owner` (window-aware: the tab-strip `+` of a
    /// non-frontmost window opens there, not in the frontmost). The new session is
    /// stamped with `owner` so its output/exit/bell route back to THIS window.
    ///
    /// TRUST anchor: the `NewTab` action of the ty-proven `tab_strip` machine
    /// (`tab_strip_model()`) — appends a tab and re-syncs the owner's native strip.
    #[cfg_attr(
        test,
        aterm_spec::refines(
            machine = "tab_strip",
            action = "NewTab",
            project = "aterm_gui::tab_strip_conformance::project"
        )
    )]
    pub(crate) fn open_tab_in(&mut self, owner: WindowId) {
        let id = self.next_session_id;
        let (rows, cols) = self
            .windows
            .get(&owner)
            .map_or((0, 0), |ws| (ws.rows, ws.cols));
        // A real run always has a proxy; guard rather than panic (test-only None).
        let Some(proxy) = self.proxy.clone() else {
            return;
        };
        match spawn_session(id, owner, rows, cols, &self.session_factory, &proxy) {
            Ok(session) => {
                self.next_session_id += 1;
                // P1.1: register in the process-wide registry (additive index) so a
                // cross-session `@<selector>` verb can reach this tab. The parent is
                // the FOCUSED pane's session of the OWNER window when the tab was
                // opened (the family tree; a user-opened tab is a child of the pane
                // it was opened from).
                let parent = self
                    .windows
                    .get(&owner)
                    .map(|ws| ws.layouts[ws.tabs.active].focus())
                    .and_then(|aid| self.pool.get(aid))
                    .map(|s| s.ctx.self_id.clone());
                Self::register_session(&self.store, &session, parent);
                self.pool.insert(session);
                // Append a fresh single-pane tree (one leaf) and bump the owner
                // window's index in lockstep (keeps `layouts.len() == tabs.count`).
                if let Some(ws) = self.windows.get_mut(&owner) {
                    ws.layouts.push(pane::PaneTree::new(id));
                    ws.tabs.add();
                }
                // Mirror the owner; if it's frontmost, also re-point the globals.
                if self.frontmost_window == Some(owner) {
                    self.sync_active_session();
                } else {
                    self.sync_window(owner);
                }
            }
            Err(e) => eprintln!("aterm-gui: could not open a new tab: {e}"),
        }
    }

    /// Cmd-1..Cmd-9: switch to tab index `i` (0-based) if it exists. No-op (and no
    /// repaint) when `i` is already active or out of range.
    pub(crate) fn switch_tab(&mut self, i: usize) {
        if let Some(front) = self.frontmost_window {
            self.switch_tab_in(front, i);
        }
    }

    /// Switch window `wid` to tab `i` (window-aware: a tab-strip CLICK targets the
    /// clicked window, which may not be the frontmost). Re-mirrors that window; when
    /// it is the frontmost, also re-points the global control/notify handles
    /// (`sync_active_session`), matching the keyboard/menu `switch_tab` behavior.
    pub(crate) fn switch_tab_in(&mut self, wid: WindowId, i: usize) {
        let Some(ws) = self.windows.get_mut(&wid) else {
            return;
        };
        if i == ws.tabs.active || i >= ws.layouts.len() {
            return;
        }
        ws.tabs.switch_to(i);
        if self.frontmost_window == Some(wid) {
            self.sync_active_session();
        } else {
            self.sync_window(wid);
        }
    }

    /// Cmd-Shift-] / Cmd-Shift-[: cycle to the next/previous tab, wrapping. No-op
    /// with a single tab.
    ///
    /// TRUST anchor: the `SelectTab` action of the ty-proven `tab_strip` machine
    /// (`tab_strip_model()`) — the DETERMINISTIC wrap the model encodes (vs the
    /// arbitrary-index `switch_tab_in`); re-syncs the strip selection in lockstep.
    #[cfg_attr(
        test,
        aterm_spec::refines(
            machine = "tab_strip",
            action = "SelectTab",
            project = "aterm_gui::tab_strip_conformance::project"
        )
    )]
    pub(crate) fn cycle_tab(&mut self, forward: bool) {
        let Some(ws) = self.front_mut() else { return };
        if ws.layouts.len() <= 1 {
            return;
        }
        ws.tabs.cycle(forward);
        self.sync_active_session();
    }

    /// Apply a control-socket `tab` verb ([`TabAction`]) to the FRONT window and
    /// return the resulting `(active_index, tab_count)` for the verb's reply. Driven
    /// by [`Wake::TabCmd`] on the main loop (the sole `App` mutator). Each action
    /// reuses the EXISTING command path — `New` => [`Self::open_tab`] (same as File ▸
    /// New Tab / the toolbar "+"), `Select(n)` => [`Self::switch_tab`], `Next`/`Prev`
    /// => [`Self::cycle_tab`] — so the verb adds no parallel tab logic. With no front
    /// window (impossible in a real run) it reports `(0, 0)`.
    pub(crate) fn apply_tab_cmd(&mut self, action: TabAction) -> (usize, usize) {
        match action {
            TabAction::New => self.open_tab(),
            TabAction::Select(n) => self.switch_tab(n),
            TabAction::Next => self.cycle_tab(true),
            TabAction::Prev => self.cycle_tab(false),
            TabAction::Close(which) => self.close_tab_via_verb(which),
            TabAction::Move { from, to } => {
                if let Some(front) = self.frontmost_window {
                    self.move_tab(front, from, to);
                }
            }
        }
        // Read the resulting state off the front window's tab index. If the action
        // closed the window's LAST tab, the window is `pending_close` (still present
        // until `escalate_pending_close` runs), so it still reports a count here.
        self.front()
            .map_or((0, 0), |ws| (ws.tabs.active, ws.tabs.count))
    }

    /// Close the front window's tab `which` (or its ACTIVE tab when `None`) for the
    /// `tab close [N]` verb and the native × button's [`Wake::CloseTab`]. Reuses
    /// [`Self::close_tab_at`] (the SAME whole-tab close the renderer strip's `✕` and
    /// the tab-strip click take); if that was the window's LAST tab it flags
    /// `pending_close` so the `Wake` handler's `escalate_pending_close(el)` tears the
    /// window down (the verb / button paths have no `ActiveEventLoop`), exactly like a
    /// tab-strip close.
    pub(crate) fn close_tab_via_verb(&mut self, which: Option<usize>) {
        let Some(front) = self.frontmost_window else {
            return;
        };
        let i = match which {
            Some(i) => i,
            None => self.windows.get(&front).map_or(0, |ws| ws.tabs.active),
        };
        if self.close_tab_at(front, i)
            && let Some(ws) = self.windows.get_mut(&front)
        {
            ws.pending_close = true;
        }
    }

    /// Reorder window `wid`'s tab from index `from` to index `to`, moving its
    /// `layouts` entry and FIXING `tabs.active` so the same SESSION the user was
    /// viewing stays selected after the move (drag-to-reorder must not silently switch
    /// tabs). Out-of-range `from`/`to`, a stale/unknown window, or `from == to` are
    /// no-ops. Re-mirrors the window (the native strip re-tracks via `sync_window`).
    ///
    /// INVARIANT preserved: `tabs.count == layouts.len()` (a pure permutation — no add
    /// / remove), and `active < count` (clamped). The active index is recomputed by
    /// tracking where the OLD active slot lands under the move, so:
    ///   * moving the active tab itself → active follows it to `to`;
    ///   * moving a tab from before→after the active → active shifts down one;
    ///   * moving a tab from after→before the active → active shifts up one;
    ///   * a move on neither side of active → active unchanged.
    pub(crate) fn move_tab(&mut self, wid: WindowId, from: usize, to: usize) {
        let Some(ws) = self.windows.get_mut(&wid) else {
            return;
        };
        let n = ws.layouts.len();
        if from >= n || to >= n || from == to {
            return;
        }
        // Move the pane tree (Vec remove+insert is a clean reorder for the small tab
        // counts here; n is a handful of tabs).
        let tree = ws.layouts.remove(from);
        ws.layouts.insert(to, tree);
        // Re-derive the active index by following where the OLD active slot moved.
        let old_active = ws.tabs.active;
        let new_active = if old_active == from {
            to
        } else if from < old_active && old_active <= to {
            old_active - 1
        } else if to <= old_active && old_active < from {
            old_active + 1
        } else {
            old_active
        };
        ws.tabs.active = new_active.min(n.saturating_sub(1));
        // Mirror the window so the native strip re-tracks the new order/selection.
        if self.frontmost_window == Some(wid) {
            self.sync_active_session();
        } else {
            self.sync_window(wid);
        }
    }

    /// Re-sync window `wid`'s NATIVE toolbar tab strip to the app's current tab
    /// state: rebuild the view-based strip's per-tab views (one per tab, the active
    /// one accented, the whole strip hidden at ≤1 tab) from [`Self::tab_titles`] + the
    /// window's active index, via [`toolbar::set_window_tabs`]. Called from
    /// [`Self::sync_window`] so the strip tracks EVERY tab mutation (open / close /
    /// switch / detach / migrate / reorder). A no-op off macOS and for a window with no
    /// toolbar handle (headless / a window whose toolbar failed to install).
    pub(crate) fn refresh_window_tabs(&mut self, wid: WindowId) {
        let titles = self.tab_titles(wid);
        let active = self.windows.get(&wid).map_or(0, |ws| ws.tabs.active);
        // Shadow what the native strip is being told to render BEFORE the push, so a
        // tab mutation that forgets to call this fn leaves the recorded strip state
        // stale — the only way a headless test can witness the strip↔model desync the
        // `tab_strip` machine proves can't happen. (`titles.len()` == tab count.)
        if let Some(ws) = self.windows.get(&wid) {
            ws.strip_shadow.set((titles.len(), active));
        }
        if let Some(handle) = self._toolbars.get(&wid) {
            self.apprt.set_toolbar_tabs(handle, &titles, active);
        }
    }

    /// Cmd-W: close the FOCUSED pane of the FRONTMOST window's active tab. Returns
    /// `Some(window)` — the window whose last tab just closed — iff that was the LAST
    /// pane of the LAST tab, so the caller escalates to closing THAT window (the
    /// frontmost), not whichever window an input event was stamped for. Returns
    /// `None` otherwise. Closing a pane in a SPLIT tab collapses the split onto its
    /// sibling (the sibling — and its reader thread — survive); closing the only pane
    /// of a non-last tab closes the tab. Honors `--hold` ONLY for the implicit close
    /// on a session's own EOF (see `close_session`); an explicit Cmd-W always closes.
    pub(crate) fn close_active_tab(&mut self) -> Option<WindowId> {
        let window = self.frontmost_window?;
        let tab = self.front().map_or(0, |ws| ws.tabs.active);
        let outcome = self.active_tree_mut(window).map(|t| t.close_focused())?;
        // `true` = the frontmost window's last tab closed → tell the caller WHICH
        // window to escalate-close (always the frontmost we operated on).
        self.apply_close_outcome(window, tab, outcome)
            .then_some(window)
    }

    /// Close the PANE holding session `id` in window `window` (its reader hit EOF).
    /// With `--hold`, the pane is KEPT so the final output stays visible (the user
    /// closes it with Cmd-W). Returns `true` iff the app should now exit (the last
    /// pane of the last tab of the last window closed and `--hold` is off). A
    /// `Wake::Exit` for an already-closed/unknown session is a no-op.
    pub(crate) fn close_session(&mut self, window: WindowId, id: u64) -> bool {
        if self.hold {
            return false; // keep the window/pane open after the command exits
        }
        // Which tab of THIS window holds this session? (Unknown / closed → no-op.)
        let Some(ws) = self.windows.get(&window) else {
            return false;
        };
        let Some(tab) = ws.layouts.iter().position(|t| t.contains(id)) else {
            return false;
        };
        let outcome = self
            .windows
            .get_mut(&window)
            .map(|ws| ws.layouts[tab].close_pane(id));
        match outcome {
            Some(o) => self.apply_close_outcome(window, tab, o),
            None => false,
        }
    }

    /// LOGICAL core of the `Wake::Exit` handler (no winit/`el`): mark `session`
    /// `Exited`, then close it in EVERY window that views it. A CO-VIEWED
    /// (Cmd-Shift-O) session is displayed in more than one window but has a SINGLE
    /// reader thread, so its shell exit emits exactly ONE `Wake::Exit`; closing only
    /// the first owner would leave every OTHER viewer pinned to a dead, still-pooled
    /// pane. Owners are collected FIRST (closing mutates `self.windows`); each
    /// `close_session` detaches exactly one pool view, so the refcount drains to 0
    /// across the set and the registry deregisters once. Returns the windows whose
    /// LAST tab thereby closed — the caller escalates each to a window close (the
    /// last window closing exits the app, the `ExitIffEmpty` invariant). This is the
    /// el-free twin the multi-window tests drive; `Wake::Exit` wraps it with
    /// `close_window`/`el.exit()`. An already-closed/unknown session finds no owner.
    pub(crate) fn exit_session_logical(&mut self, session: u64) -> Vec<WindowId> {
        self.store
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .set_state(session, session_store::SessionState::Exited);
        let owners: Vec<WindowId> = self
            .windows
            .iter()
            .filter(|(_, ws)| ws.layouts.iter().any(|t| t.contains(session)))
            .map(|(w, _)| *w)
            .collect();
        let mut to_close = Vec::new();
        for o in owners {
            if self.close_session(o, session) {
                to_close.push(o);
            }
        }
        to_close
    }

    /// A click in window `wid`'s tab strip at column `col`: resolve it against that
    /// window's cached segments ([`WindowState::tab_segments`]) and SWITCH / CLOSE /
    /// open a tab. A click on bare strip background is ignored. The CLOSE of the last
    /// tab signals the window to close via `ws.pending_close` (the mouse handler has
    /// no `ActiveEventLoop`), mirroring Cmd-W. Repaints after any state change.
    pub(crate) fn handle_tab_strip_click(&mut self, wid: WindowId, col: u16) {
        let Some(segs) = self.windows.get(&wid).map(|ws| ws.tab_segments.clone()) else {
            return;
        };
        let Some(hit) = tab_bar::hit_test(&segs, col) else {
            return; // bare strip background
        };
        match hit {
            // Target the CLICKED window, not the frontmost — Close already does, so
            // Select/NewTab must too (a click on a non-front window's strip must act
            // on THAT window even if focus hasn't transferred yet).
            tab_bar::TabHit::Select(i) => self.switch_tab_in(wid, i),
            tab_bar::TabHit::NewTab => self.open_tab_in(wid),
            tab_bar::TabHit::Close(i) => {
                if self.close_tab_at(wid, i)
                    && let Some(ws) = self.windows.get_mut(&wid)
                {
                    ws.pending_close = true;
                }
            }
        }
        if let Some(ws) = self.windows.get(&wid)
            && let Some(w) = &ws.os_window
        {
            w.request_redraw();
        }
    }

    /// Close the ENTIRE tab at index `i` of window `wid` (every pane in it), as a
    /// unit — the tab strip's close `x` closes a whole tab, unlike Cmd-W which closes
    /// one pane. DRAINS each of the tab's panes' sessions and `pool.detach`es each
    /// (the last view closes that PTY master), drops its pane tree, and keeps
    /// `tabs`/`layouts` aligned. Returns `true` iff that was the LAST tab (the caller
    /// signals the window to close). Out-of-range `i` is a no-op (returns `false`).
    ///
    /// TRUST anchor: the `Close` action of the ty-proven `tab_strip` machine
    /// (`tab_strip_model()`) — shrinks the tab set and MUST re-sync the clicked
    /// window's native strip (the non-front-window re-sync this fn now performs).
    #[cfg_attr(
        test,
        aterm_spec::refines(
            machine = "tab_strip",
            action = "Close",
            project = "aterm_gui::tab_strip_conformance::project"
        )
    )]
    pub(crate) fn close_tab_at(&mut self, wid: WindowId, i: usize) -> bool {
        let Some(ws) = self.windows.get_mut(&wid) else {
            return false;
        };
        if i >= ws.layouts.len() {
            return false;
        }
        if ws.tabs.close(i) {
            return true; // last tab → signal the window to close
        }
        // Drain EVERY pane's session of the removed tab and detach each (NOT a Vec
        // remove): DETACH the pool view FIRST (the last view drops the Session,
        // closing its PTY master), and deregister from the process-wide registry
        // ONLY when that detach actually dropped the session. A shared (Cmd-Shift-O)
        // session still viewed in another window keeps its single store entry while a
        // view remains; a genuinely-closed id then fail-closes a later @<selector>.
        let closing = ws.layouts[i].sessions();
        ws.layouts.remove(i);
        for sid in closing {
            if self.pool.detach(sid) {
                self.store
                    .write()
                    .unwrap_or_else(|p| p.into_inner())
                    .deregister_local(sid);
            }
        }
        // Re-sync the CLICKED window — its active index shifted when the tab was
        // removed. Mirror `open_tab_in`'s owner-sync: the global handles follow the
        // FRONT window, but a NON-front window must still re-sync its OWN mirror +
        // native tab strip (`sync_window` → `refresh_window_tabs`), or it keeps a
        // PHANTOM segment past the closed tab. (Proven by `tab_strip` + its Tier-1
        // conformance: closing a tab in a non-front window must not desync its strip.)
        if self.frontmost_window == Some(wid) {
            self.sync_active_session();
        } else {
            self.sync_window(wid);
        }
        false
    }

    /// Live structural oracle for the window/session model (debug builds only;
    /// `debug_assert`-ed after each tab mutation, mirroring how the engine fuzz
    /// harness wires grid invariants as an always-on oracle). It must hold at
    /// every STABLE point:
    ///   - there is always ≥1 logical window and `frontmost_window` names one;
    ///   - every window has ≥1 tab, with `tabs.count == layouts.len()` and
    ///     `tabs.active` in range;
    ///   - the window's active mirror id equals its active tab's FOCUSED pane
    ///     (`layouts[tabs.active].focus()`); and
    ///   - every pane's session is owned by the pool (resolvable).
    ///
    /// This is the CODE-LEVEL shadow of the ty-proven `window_routing_model`'s
    /// `ExitIffEmpty`/`FrontmostLive`/`FrontmostAllocated` (crates/aterm-spec).
    //
    // NOT `#[cfg(debug_assertions)]`: the `debug_assert!` call sites type-check
    // their condition in release too (the macro only gates EXECUTION, not
    // compilation), so a debug-only definition fails the release build with
    // E0599. Define it unconditionally; `allow(dead_code)` silences the
    // release-only "never called" warning (debug builds do call it).
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub(crate) fn structural_invariants_ok(&self) -> bool {
        let Some(fid) = self.frontmost_window else {
            return false;
        };
        if !self.windows.contains_key(&fid) {
            return false;
        }
        self.windows.values().all(|ws| {
            !ws.layouts.is_empty()
                && ws.tabs.count == ws.layouts.len()
                && ws.tabs.active < ws.layouts.len()
                && ws.active_id == ws.layouts[ws.tabs.active].focus()
                && ws
                    .layouts
                    .iter()
                    .flat_map(|t| t.sessions())
                    .all(|id| self.pool.get(id).is_some())
        })
    }

    /// Apply a [`pane::CloseOutcome`] from tab `tab` of window `wid`, keeping the
    /// pool, `layouts`, and `tabs` consistent, and re-mirror the focused pane.
    /// Returns `true` iff that was the last pane of the last tab of the last window
    /// (caller signals the window to close). Detaching the removed view drops the
    /// `Session` (closing its PTY master) iff it was the last view; every OTHER pane
    /// is untouched.
    pub(crate) fn apply_close_outcome(
        &mut self,
        wid: WindowId,
        tab: usize,
        outcome: pane::CloseOutcome,
    ) -> bool {
        match outcome {
            pane::CloseOutcome::Collapsed { .. } => {
                // The tab survives (a sibling remained). Detach just the closed
                // pane's view; the sibling's reader thread stays alive.
                self.teardown_session(outcome.closed());
                // The active tab's geometry changed (a sibling grew); the closed
                // tab may not be the active one (background EOF), but re-laying the
                // active tab is cheap and correct. Resize panes to the new layout.
                self.resize_panes(wid);
                // The active pane MOVED (the focused pane collapsed onto its sibling);
                // re-point the global handle, not just the per-window mirror, so a
                // control verb can't keep driving the just-closed pane's session.
                self.resync_active_or_window(wid);
                false
            }
            pane::CloseOutcome::LastPane { .. } => {
                // That pane was the tab's only one → the tab closes. `tabs.close`
                // returns true iff it was the LAST tab (caller signals the window to
                // close; the last window closing exits the app).
                let last_tab = self
                    .windows
                    .get_mut(&wid)
                    .map(|ws| ws.tabs.close(tab))
                    .unwrap_or(false);
                if last_tab {
                    return true;
                }
                // Detach EVERY pane's view of the removed tab (a LastPane close has
                // exactly one, but draining `sessions()` is robust and explicit),
                // then drop the tab's tree.
                let drained: Vec<u64> = self
                    .windows
                    .get(&wid)
                    .map(|ws| ws.layouts[tab].sessions())
                    .unwrap_or_default();
                if let Some(ws) = self.windows.get_mut(&wid) {
                    ws.layouts.remove(tab);
                }
                for sid in drained {
                    self.teardown_session(sid);
                }
                // The active TAB changed (the closed tab's neighbor became active);
                // re-point the global handle so verbs follow the close-induced switch.
                self.resync_active_or_window(wid);
                false
            }
        }
    }

    /// Tear down exactly the session `id`: DETACH its pool view (which drops its
    /// `Session` — closing its PTY master, ending its reader thread — iff it was the
    /// LAST view) FIRST, then deregister from the process-wide registry (P1.1) ONLY
    /// when that detach actually dropped the session. A REFCOUNTED (Cmd-Shift-O
    /// shared) session still live in another window must NOT be deregistered while a
    /// view remains: `pool.detach` returns `true` iff the view count hit 0. A later
    /// `@<selector>` to a genuinely-closed id fail-closes (unknown -> Deny).
    pub(crate) fn teardown_session(&mut self, id: u64) {
        let dropped = self.pool.detach(id);
        if dropped {
            self.store
                .write()
                .unwrap_or_else(|p| p.into_inner())
                .deregister_local(id);
        }
    }
}
