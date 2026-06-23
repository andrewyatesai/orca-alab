// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! Window lifecycle: logical + OS window create/attach/close/focus orchestration
//! (`create_window_logical`/`create_window_internal`/`attach_os_window`,
//! `close_window`/`close_window_logical`/`escalate_pending_close`, focus
//! bookkeeping), the `front`/`front_mut` accessors, and `apply_title`. The native
//! window chrome (colour-space/appearance, background, toolbar, menu) is reached
//! through the platform [`crate::platform::AppRt`] seam. A verbatim inherent-impl
//! split of `App`.

use std::sync::Arc;

use winit::event_loop::ActiveEventLoop;
use winit::window::Window;

use crate::app_config::resolve_force_scale;
use crate::platform::AppRt;
use crate::spawn::spawn_session;
use crate::{
    App, Backend, CloseOutcome, FONT_PX, FONT_PX_MAX, FONT_PX_MIN, PresentTarget, Session,
    TabIndex, WindowId, WindowState, build_backend, pad_for_scale, pane,
};

impl App {
    /// The frontmost logical window's state (immutable). Transitional — every
    /// caller is single-window today; later steps route by an explicit WindowId.
    pub(crate) fn front(&self) -> Option<&WindowState> {
        self.frontmost_window.and_then(|id| self.windows.get(&id))
    }

    pub(crate) fn front_mut(&mut self) -> Option<&mut WindowState> {
        self.frontmost_window
            .and_then(move |id| self.windows.get_mut(&id))
    }

    /// LOGICAL window creation (NO winit): mint a fresh [`WindowId`], spawn a new
    /// single-tab session at `rows`×`cols`, register it, and install a fresh
    /// [`WindowState`] as the new frontmost window. Returns the new id, or `None`
    /// if the spawn failed (in which case NO window is minted — we never leave a
    /// broken, session-less window behind). This is the fully-testable seam the
    /// multi-window conformance test drives; `create_window_internal` wraps it with
    /// the winit surface attach.
    pub(crate) fn create_window_logical(&mut self, rows: u16, cols: u16) -> Option<WindowId> {
        // Mint the window id FIRST so the spawned session's `Wake`s are stamped with
        // the window that will own them (Output/Exit/Bell route back to THIS window).
        let wid = WindowId(self.next_window_id);
        self.next_window_id += 1;
        let sid = self.next_session_id;
        // A real run always has a proxy; only `headless_for_test` lacks one (and it
        // never calls this — it installs stub sessions directly). Guard, don't panic.
        let proxy = self.proxy.clone()?;
        let session = match spawn_session(sid, wid, rows, cols, &self.session_factory, &proxy) {
            Ok(s) => s,
            Err(e) => {
                // Spawn failed: do NOT mint a broken (session-less) window. The id is
                // burned (never reused), which is fine — ids are monotonic, not dense.
                eprintln!("aterm-gui: could not open a new window: {e}");
                return None;
            }
        };
        self.next_session_id += 1;
        self.install_window_state(wid, session, rows, cols);
        Some(wid)
    }

    /// Install an already-spawned `session` as the sole tab of a fresh window `wid`
    /// and make it frontmost. Factored out of `create_window_logical` so the spawn
    /// (real PTY) and the pure windows/pool/frontmost bookkeeping are separable:
    /// the unit test drives THIS with a stub `Session`, exercising the real
    /// frontmost/windows/pool state transitions with no PTY.
    pub(crate) fn install_window_state(
        &mut self,
        wid: WindowId,
        session: Session,
        rows: u16,
        cols: u16,
    ) {
        let sid = session.id;
        // Clone the mirror Arcs BEFORE moving the session into the pool (the pool
        // then OWNS it; these are the window's active-tab mirror, source-of-truth in
        // the pool).
        let (term, master, sink) = (
            session.term.clone(),
            session.master,
            session.ctx.sink.clone(),
        );
        // P1.1: register in the process-wide registry. A new window's first tab has
        // no parent (it is a fresh root, like session 0).
        Self::register_session(&self.store, &session, None);
        self.pool.insert(session);
        let ws = WindowState::new(
            term,
            master,
            sink,
            sid,
            rows,
            cols,
            TabIndex::new(0, 1),
            vec![pane::PaneTree::new(sid)],
        );
        self.windows.insert(wid, ws);
        // The new window becomes frontmost (the standard "open and focus" behavior).
        self.frontmost_window = Some(wid);
        debug_assert!(
            self.structural_invariants_ok(),
            "window/session structural invariants violated after create_window_logical",
        );
    }

    /// Test-only window creation that drives the SAME wid/session-id minting +
    /// `install_window_state` bookkeeping as [`Self::create_window_logical`], but
    /// takes a pre-built (stub) `Session` instead of spawning a real PTY — so the
    /// multi-window state-transition test exercises the real frontmost/windows/pool
    /// transitions with no event loop and no shell. `session.id` MUST equal the
    /// caller's `self.next_session_id` so the pool/window ids stay consistent (the
    /// test builds it that way). Returns the freshly-minted, strictly-increasing id.
    ///
    /// SPEC (TRUST_VACUITY_GATE §2.3 / finding 3): this real seam IS the
    /// `WindowRouting.CreateWindow` action — minting the next monotonic id, bumping
    /// `win_count`, and re-pointing `frontmost`. The `#[refines]` makes `window_routing`
    /// an ACTIVELY-BOUND machine in the gate (so it is coverage-gated, no longer a
    /// report-only model), and the gate now also RUNS its Tier-1 conformance
    /// (`run_window_routing_conformance`) — the "already green" claim is no longer a
    /// conflation of two disconnected tests. PROJECTION
    /// (`aterm_gui::App::project_window_routing`): `App` → `<<win_count, frontmost,
    /// next_id, exited>>` (the load-bearing +1 remap is in `window_routing_conformance::project`).
    #[cfg(test)]
    #[cfg_attr(
        test,
        aterm_spec::refines(
            machine = "window_routing",
            action = "CreateWindow",
            project = "aterm_gui::App::project_window_routing"
        )
    )]
    pub(crate) fn insert_logical_window(
        &mut self,
        session: Session,
        rows: u16,
        cols: u16,
    ) -> WindowId {
        debug_assert_eq!(
            session.id, self.next_session_id,
            "stub session id must match the minted session id",
        );
        let wid = WindowId(self.next_window_id);
        self.next_window_id += 1;
        self.next_session_id += 1;
        self.install_window_state(wid, session, rows, cols);
        wid
    }

    /// Full window creation: the logical seam + (when not headless) the winit OS
    /// window attach. The new window inherits the front window's grid size (or an
    /// 80×24 default if somehow no window exists). Under headless the window stays
    /// logical-only (no OS surface); a headless 2nd window is refused EARLIER, at the
    /// `Wake::CreateWindow` arm, so this stays logical-only there only defensively.
    pub(crate) fn create_window_internal(&mut self, el: &ActiveEventLoop) -> Option<WindowId> {
        let (rows, cols) = self.front().map_or((80, 24), |ws| (ws.rows, ws.cols));
        let wid = self.create_window_logical(rows, cols)?;
        if !self.headless && !self.attach_os_window(el, wid) {
            // GPU surface failed: roll back the just-created window + its fresh
            // session rather than leave a present-less black window.
            self.close_window_logical(wid);
            return None;
        }
        // The new window is now frontmost: re-point the GLOBAL control/notify handle
        // at its session. `install_window_state` set `frontmost_window` but does NOT
        // sync the global handle, and the OS `Focused(true)` that would normally do so
        // is a no-op here (its `frontmost != Some(wid)` guard is already satisfied) —
        // so without this the control socket keeps targeting the PREVIOUS window's
        // session for the new window's whole life. Mirrors every other new-frontmost
        // path (Cmd-Shift-O, detach-to-new-window, open_tab_in). On the attach-failure
        // path above, `close_window_logical` already re-synced the surviving front.
        self.sync_active_session();
        Some(wid)
    }

    /// Create the OS window + present surface for logical window `wid` and attach
    /// them to its [`WindowState`]. Factored out of `resumed` so it serves BOTH the
    /// first window (at `resumed`) and every Cmd-N 2nd..Nth window (at
    /// `create_window_internal`). Sizes the OS window from the window's stored grid,
    /// installs the macOS menu (FIRST window only), and builds the GPU or CPU present
    /// target. NEVER called in headless (no OS window is ever created there). A
    /// missing `wid` (stale) is a silent no-op on the present-target writes.
    /// Returns `true` iff the OS window was created AND a present target installed.
    /// `false` means a GPU swapchain failure (no CPU fallback exists in GPU mode):
    /// the just-created OS window is dropped rather than installed present-less (which
    /// would show a permanently black window), and the caller rolls back the logical
    /// window (or, for the first window, exits).
    #[must_use]
    pub(crate) fn attach_os_window(&mut self, el: &ActiveEventLoop, wid: WindowId) -> bool {
        let (rows, cols) = self
            .windows
            .get(&wid)
            .map_or((0, 0), |ws| (ws.rows, ws.cols));
        // The window holds the terminal grid PLUS the tab-strip rows at the top PLUS
        // the `2·pad` interior border. `window_frame_px` folds in both; with both
        // zero this is the original `rows * ch` (byte-identical).
        let mut size = self.window_frame_px(rows, cols);
        let attrs = Window::default_attributes()
            .with_title("aterm")
            .with_inner_size(size);
        let window = Arc::new(el.create_window(attrs).expect("create window"));
        // Native macOS menu bar (menu.rs): build + install NSApp.mainMenu now the
        // FIRST window exists, so aterm presents as a real Mac app. There is ONE
        // shared NSApp.mainMenu, so window 2..N must NOT reinstall it (that would
        // drop the first install's retained action target and rebuild the bar). The
        // `_menu.is_none()` guard makes the install fire exactly once. Skipped under
        // `--headless` (this fn is never reached there); a no-op off macOS. The
        // returned action target is RETAINED in `self` (AppKit holds a menu item's
        // target only weakly) for the run loop's life.
        if self._menu.is_none()
            && let Some(proxy) = self.proxy.as_ref()
        {
            self._menu = self.apprt.install_menu(proxy);
        }
        // IME-1: opt into IME so the window receives `WindowEvent::Ime`
        // (Preedit/Commit) for CJK/dead-key/Option composition. Never enabled
        // before, so composition input was impossible.
        window.set_ime_allowed(true);
        // HiDPI / Retina auto-scale. aterm rasterizes glyphs at `font_px` PHYSICAL
        // pixels and works in physical units throughout, so on a 2× Retina display
        // the built-in 13 px default renders at ~6.5 LOGICAL points — crisp but tiny.
        // The display scale factor is only knowable once the window exists, so apply
        // it HERE: when the size is the DEFAULT (no `$ATERM_FONT_PX`, no
        // `config.font_px`), scale it to `round(FONT_PX × scale)`. An EXPLICIT size is
        // honored verbatim — never double-scaled. NOTE: the GPU branch rebuilds the
        // font IN PLACE (`set_font_theme`) so the SHARED device + every OTHER
        // window's swapchain survive; only the CPU path does a full backend swap.
        // An explicit render-scale override ($ATERM_FORCE_SCALE / --scale) wins over
        // the window's real scale_factor(), driving BOTH the auto-scaled font and the
        // interior padding so a forced scale renders identically to that real DPI.
        let scale = resolve_force_scale().unwrap_or_else(|| window.scale_factor());
        if !self.font_px_explicit && scale > 1.0 {
            let scaled = (FONT_PX * scale as f32)
                .round()
                .clamp(FONT_PX_MIN, FONT_PX_MAX);
            // Cmd-0 should reset to this scaled default, not the tiny FONT_PX base.
            let rebuilt = match &mut self.backend {
                Backend::Gpu(g) => match g.set_font_theme(scaled, self.theme) {
                    Ok(()) => true,
                    Err(e) => {
                        eprintln!("aterm-gui: HiDPI GPU font rebuild failed: {e}");
                        false
                    }
                },
                Backend::Cpu(_) => {
                    match build_backend(
                        scaled,
                        self.use_gpu,
                        self.theme,
                        self.font_family.as_deref(),
                    ) {
                        Some(backend) => {
                            self.backend = backend;
                            true
                        }
                        None => {
                            eprintln!("aterm-gui: HiDPI font rebuild failed; keeping {FONT_PX}px");
                            false
                        }
                    }
                }
            };
            if rebuilt {
                self.font_px = scaled;
                self.default_font_px = scaled;
                self.introspect_gpu = aterm_gpu::WindowGpu::new();
                if let Some(ws) = self.windows.get_mut(&wid) {
                    ws.last_present = None;
                }
            }
        }
        // Apply the interior padding at the window's REAL scale and recompute `size`
        // so the window — and the GPU swapchain configured from it below — fits the
        // grid PLUS this border (and the new cell metrics if the font was rebuilt)
        // PLUS the tab strip.
        self.backend.set_pad(pad_for_scale(scale));
        size = self.window_frame_px(rows, cols);
        let _ = window.request_inner_size(size);
        // Native macOS window toolbar (toolbar.rs): a unified-style NSToolbar with a
        // "+" New Tab button, so the window presents as a real Mac app. Installed HERE
        // — BEFORE the GPU/CPU present split — so BOTH backends get it (the GPU arm
        // `return`s below and never reaches the CPU tail). The "+" reuses File ▸ New
        // Tab (posts the same `Wake::MenuAction { NewTab }`). The retained backing
        // objects are kept in `self._toolbars` keyed by window (AppKit holds the
        // toolbar's delegate + the item's target only WEAKLY). A no-op off macOS;
        // never reached under `--headless`. Cloning the proxy avoids borrowing `self`
        // immutably (proxy) and mutably (`_toolbars`) at once.
        if let Some(proxy) = self.proxy.clone()
            && let Some(handle) = self.apprt.install_toolbar(&window, &proxy, wid)
        {
            self._toolbars.insert(wid, handle);
        }
        // Paint the window background the terminal's theme background colour so the
        // transparent titlebar — and the bare single-tab compact bar — reads as a
        // seamless extension of the terminal body instead of a distinct lighter chrome
        // strip (Ghostty's "transparent" titlebar look). Runs for BOTH backends: the
        // GPU arm `return`s below, so this must precede the split. No-op off macOS
        // (the Linux apprt's `window_set_background_color` does nothing).
        self.apprt
            .window_set_background_color(&window, self.theme.bg);
        if self.backend.is_gpu() {
            // GPU mode: a wgpu swapchain on the SAME instance/adapter as the
            // offscreen renderer. The offscreen frame is blitted into it and
            // presented on the GPU — no softbuffer surface is created.
            let (w_px, h_px) = (size.width, size.height);
            match self
                .backend
                .gpu_mut()
                .unwrap()
                .create_window_surface(window.clone(), w_px, h_px)
            {
                Ok(surf) => {
                    self.winit_to_window.insert(window.id(), wid);
                    if let Some(ws) = self.windows.get_mut(&wid) {
                        ws.os_window = Some(window);
                        ws.present = Some(PresentTarget::Gpu {
                            gpu_surface: surf,
                            window_gpu: aterm_gpu::WindowGpu::new(),
                        });
                    }
                    return true;
                }
                Err(e) => {
                    // A swapchain failure is FATAL for the GPU present path (the CPU
                    // softbuffer surface is not built in GPU mode). Do NOT install a
                    // present-less window — that would show a permanently black
                    // window. Drop the just-created OS window and report failure so
                    // the caller rolls back the logical window (or exits if it was
                    // the first/only one).
                    eprintln!("aterm-gui: GPU surface creation failed: {e}");
                    drop(window);
                    return false;
                }
            }
        }
        let context = softbuffer::Context::new(window.clone()).expect("softbuffer context");
        let surface =
            softbuffer::Surface::new(&context, window.clone()).expect("softbuffer surface");
        // Drop CoreAnimation's per-frame colour-space conversion (see the apprt
        // method's docs): softbuffer tags its content device-RGB; match the window
        // so the compositor doesn't CMS-convert every frame on the main thread. Also
        // sets the titlebar light/dark appearance. No-op off macOS.
        self.apprt.window_set_appearance(&window, self.window_theme);
        self.winit_to_window.insert(window.id(), wid);
        if let Some(ws) = self.windows.get_mut(&wid) {
            ws.os_window = Some(window);
            ws.present = Some(PresentTarget::Cpu {
                surface,
                _context: context,
            });
        }
        true
    }

    /// LOGICAL window teardown (NO winit/`el`): close window `wid` — drop every one
    /// of its tabs' PANES' views (the last view closes the PTY master via
    /// `Session::drop`), remove the window (dropping its present target →
    /// surface/`Arc<Window>`; the SHARED GPU device on the `Backend` is NEVER
    /// dropped — the S6 invariant), clear its winit mapping, and re-point
    /// `frontmost_window` to a surviving window if it named the closed one. Returns
    /// whether the APP should now exit ([`CloseOutcome::Exit`] iff no windows
    /// remain). A stale/unknown `wid` is a silent `Stay`.
    ///
    /// SPEC (TRUST_VACUITY_GATE §2.3 / finding 3): this real production seam IS the
    /// `WindowRouting.CloseWindow` action — decrement `win_count`, exit-iff-empty, and
    /// the nondeterministic frontmost re-point. The `#[refines]` (paired with the
    /// `CreateWindow` anchor) makes `window_routing` actively-bound + coverage-gated;
    /// its Tier-1 conformance is run by the gate (`run_window_routing_conformance`).
    /// PROJECTION `aterm_gui::App::project_window_routing` (the `window_routing_conformance::project`).
    #[cfg_attr(
        test,
        aterm_spec::refines(
            machine = "window_routing",
            action = "CloseWindow",
            project = "aterm_gui::App::project_window_routing"
        )
    )]
    pub(crate) fn close_window_logical(&mut self, wid: WindowId) -> CloseOutcome {
        let Some(ws) = self.windows.get(&wid) else {
            return CloseOutcome::Stay; // stale/unknown id → no-op
        };
        // Snapshot EVERY pane's session id across every tab before mutating (a split
        // tab has >1 session). `layouts` is borrowed off `ws`, so collect to drop the
        // borrow on `self` before the detach loop.
        let ids: Vec<u64> = ws.layouts.iter().flat_map(|tree| tree.sessions()).collect();
        // Drop every pane's view. DETACH the pool view FIRST (which drops the Session
        // iff it was the last view, closing its PTY master), and deregister from the
        // process-wide registry ONLY when that detach actually dropped the session —
        // a shared (Cmd-Shift-O) session still viewed in ANOTHER window keeps its
        // single store entry while a view remains. A genuinely-closed id then
        // fail-closes a later @<selector>. EACH pane is detached (not once per tab)
        // so a split-tab window releases every pane's PTY.
        for id in ids {
            if self.pool.detach(id) {
                self.store
                    .write()
                    .unwrap_or_else(|p| p.into_inner())
                    .deregister_local(id);
            }
        }
        // Drop the WindowState (its PresentTarget → GpuSurface/softbuffer Surface +
        // Arc<Window>; the shared GPU DEVICE on the Backend is untouched).
        self.windows.remove(&wid);
        // Release this window's retained native toolbar backing objects (no-op off
        // macOS / when none was installed) so they don't outlive the window.
        self._toolbars.remove(&wid);
        // Clear the winit→logical mapping for this window (its OS id is gone).
        self.winit_to_window.retain(|_, &mut v| v != wid);
        // Drop the closed window from the focus-order stack so it can never be picked
        // as a survivor below.
        self.focus_order.retain(|w| *w != wid);
        // Re-point frontmost if it named the just-closed window: the most-recently
        // focused SURVIVOR (matching the window the OS raises), with a deterministic
        // lowest-live-id fallback. See `next_frontmost_after_close`.
        if self.frontmost_window == Some(wid) {
            self.frontmost_window = self.next_frontmost_after_close();
        }
        if !self.windows.is_empty() {
            debug_assert!(
                self.structural_invariants_ok(),
                "window/session structural invariants violated after close_window_logical",
            );
            // A survivor became (or stayed) frontmost: re-mirror the control socket /
            // notify target onto its active tab, exactly like a tab/focus switch.
            self.sync_active_session();
            debug_assert!(
                self.structural_invariants_ok(),
                "window/session structural invariants violated after re-mirror",
            );
        }
        if self.windows.is_empty() {
            CloseOutcome::Exit
        } else {
            CloseOutcome::Stay
        }
    }

    /// Record that `wid` gained OS focus — move it to the most-recent end of the
    /// focus-order (MRU) stack consulted when the frontmost window closes. Removing
    /// any prior occurrence before pushing keeps the stack deduped and bounded by the
    /// live-window count. Called only from `WindowEvent::Focused(true)`, so headless
    /// (no OS focus events) leaves `focus_order` empty and the re-point falls back to
    /// the lowest live id — byte-identical to the pre-MRU behavior.
    pub(crate) fn note_window_focused(&mut self, wid: WindowId) {
        self.focus_order.retain(|w| *w != wid);
        self.focus_order.push(wid);
    }

    /// The window to make frontmost when the current front window closes: the
    /// most-recently-FOCUSED window that still exists (matching the window macOS
    /// raises — usually NOT the lowest id), falling back to the lowest live
    /// `WindowId` when no focus history applies (headless, or a window never
    /// focused). The fallback keeps the choice DETERMINISTIC where there is no OS
    /// focus to honor — the behavior the headless multi-window tests pin. Returns
    /// `None` only when no window remains.
    pub(crate) fn next_frontmost_after_close(&self) -> Option<WindowId> {
        self.focus_order
            .iter()
            .rev()
            .find(|w| self.windows.contains_key(w))
            .copied()
            .or_else(|| self.windows.keys().next().copied())
    }

    /// Close window `wid` and exit the app IFF it was the LAST window (the
    /// `ExitIffEmpty` invariant). The single routing point for every close path
    /// (CloseRequested, last-tab Cmd-W/CloseTab, a last-tab `Wake::Exit`).
    pub(crate) fn close_window(&mut self, el: &ActiveEventLoop, wid: WindowId) {
        if matches!(self.close_window_logical(wid), CloseOutcome::Exit) {
            el.exit();
        }
    }

    /// Escalate any window whose LAST-tab close set `pending_close`: close it (the
    /// close paths have no `ActiveEventLoop`, so they flag instead). The flag is set
    /// on the FRONTMOST window by keyboard/menu Cmd-W and on the CLICKED window by a
    /// tab-strip close — either may differ from the event-stamped window — so SCAN
    /// for it rather than assume the event window. At most one is set per action;
    /// clearing it first guards against a re-trigger if the close somehow no-ops.
    pub(crate) fn escalate_pending_close(&mut self, el: &ActiveEventLoop) {
        let to_close: Vec<WindowId> = self
            .windows
            .iter()
            .filter(|(_, ws)| ws.pending_close)
            .map(|(w, _)| *w)
            .collect();
        for w in to_close {
            if let Some(ws) = self.windows.get_mut(&w) {
                ws.pending_close = false;
            }
            self.close_window(el, w);
        }
    }

    /// Reflect the program-set title (OSC 0/2) in the window chrome, falling back
    /// to "aterm" when nothing has set one. Calls `set_title` only on an actual
    /// change (a cheap String compare), so it is safe to call every frame — even
    /// on the redraw early-out path, where a title-only change still updates the
    /// titlebar without a pixel repaint.
    ///
    /// IME-1: while a composition is in flight, the marked preedit text is shown
    /// as `title [‹preedit›]` — the minimal inline indicator that an
    /// IME/dead-key composition is active and what it currently holds. Because
    /// this runs on the early-out path too, the indicator follows the
    /// composition without forcing a full pixel repaint.
    ///
    /// TABS: with more than one in-window tab, a ` — [active/total]` indicator is
    /// appended (e.g. `aterm — [2/3]`) so the (visual-tab-bar-less) tab state is
    /// visible in the window chrome. A single tab shows no indicator, so a
    /// one-session window's title is byte-identical to before. (The count is the
    /// number of TABS, not panes — a split tab is still one tab in the indicator.)
    pub(crate) fn apply_title(&mut self, id: WindowId, window: &Window, title: &str) {
        // Keep the registry's title for the FOCUSED pane's session fresh
        // (best-effort), so a cross-session `sessions` read reflects the live window
        // title. Gate on the per-window `(session, title)` cache: take the process-
        // wide store WRITE lock (contended with the control thread) ONLY when the
        // active session or its title actually changed since the last publish — a
        // steady screen no longer grabs the exclusive lock every redraw. Resolve the
        // active session via the TARGET window's active tab focus → pool.
        if let Some(aid) = self
            .windows
            .get(&id)
            .map(|ws| ws.layouts[ws.tabs.active].focus())
        {
            let stale = self
                .windows
                .get(&id)
                .is_some_and(|ws| ws.store_title.0 != aid || ws.store_title.1 != title);
            if stale {
                if let Some(s) = self.pool.get(aid) {
                    self.store
                        .write()
                        .unwrap_or_else(|p| p.into_inner())
                        .set_title(s.id, title);
                }
                if let Some(ws) = self.windows.get_mut(&id) {
                    ws.store_title.0 = aid;
                    ws.store_title.1.clear();
                    ws.store_title.1.push_str(title);
                }
            }
        }
        let base = if title.is_empty() { "aterm" } else { title };
        let preedit = self.windows.get(&id).map_or("", |ws| ws.preedit.as_str());
        let desired = if preedit.is_empty() {
            base.to_string()
        } else {
            format!("{base} [‹{preedit}›]")
        };
        // No "[active/total]" tab counter in the title: the visible tab strip already
        // shows the tabs, so a title-bar counter is redundant clutter (and macOS apps
        // like Ghostty/Terminal don't do it). The title is just the program/cwd title.
        let title_changed = {
            let Some(ws) = self.windows.get_mut(&id) else {
                return;
            };
            if desired != ws.current_title {
                window.set_title(&desired);
                ws.current_title.clear();
                ws.current_title.push_str(&desired);
                true
            } else {
                false
            }
        };
        // LIVE TAB TITLES: the native tab strip labels each tab with its session's title
        // (the cwd / running command the shell integration sets via OSC 0/2). That title
        // changes constantly (every `cd`, every command) but `refresh_window_tabs` only
        // ran on STRUCTURAL tab changes (`sync_window`), so the strip labels froze at
        // tab-creation time. Refresh the strip whenever the active tab's title changes
        // (it re-reads EVERY tab), so the tabs track the live cwd like Ghostty/iTerm.
        // Cheap + gated: only on an ACTUAL title change, and a no-op off macOS / with no
        // native strip.
        if title_changed {
            self.refresh_window_tabs(id);
        }
    }
}
