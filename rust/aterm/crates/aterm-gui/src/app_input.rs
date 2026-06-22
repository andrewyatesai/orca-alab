// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! Keyboard + IME + action dispatch: the `App::input` convergence seam, `on_key`
//! (keybinding lookup + hardcoded chords + search-mode), IME preedit/commit,
//! action + menu dispatch, the seam-left mouse press, and `mouse_modifiers`.
//! Plus the `egress_to_outcome` reply mapper and the `base_logical_key` cfg pair.
//! A verbatim inherent-impl split of `App`.

use std::sync::{Arc, Mutex};

use aterm_core::selection::SelectionType;
use aterm_core::terminal::Terminal;
use aterm_session::sink::SinkWriter;
use winit::event::{ElementState, KeyEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, ModifiersState, NamedKey};

use crate::input::{self, InputEvent, InputOutcome, ScrollIntent, Source};
use crate::{App, FONT_ZOOM_STEP, Wake, WindowId, keybinding, keymap, menu, pane, term_lock};

/// Map the seam's [`input::Egress`] to the reply-bearing [`InputOutcome`]: a failed
/// PTY write becomes `WriteFailed` (→ `ERR write failed`) so a reply-bearing verb is
/// never told OK for bytes that did not land (the input-path reply-fidelity contract).
pub(crate) fn egress_to_outcome(e: input::Egress) -> InputOutcome {
    match e {
        input::Egress::Reported(input::Delivery::Failed) => InputOutcome::WriteFailed,
        _ => InputOutcome::Ok,
    }
}

/// The modifier-INDEPENDENT logical key of a winit event (the unshifted base
/// key), used for the keybinding chord lookup so a binding written as the base
/// key (`cmd+shift+]`, not `cmd+}`) matches regardless of how Shift composes the
/// glyph on the active layout. On macOS this is `key_without_modifiers()` (a
/// platform extension); elsewhere winit's plain `logical_key` is the closest
/// equivalent (aterm-gui ships on macOS — this keeps the crate compiling for the
/// host test build). It returns an OWNED key so the borrow on `ev` ends before
/// `on_key`'s later `&ev.logical_key` matches.
#[cfg(target_os = "macos")]
pub(crate) fn base_logical_key(ev: &KeyEvent) -> Key {
    use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
    ev.key_without_modifiers()
}

/// Non-macOS fallback for [`base_logical_key`]: `key_without_modifiers` is a
/// platform extension, so off macOS the plain logical key is used.
#[cfg(not(target_os = "macos"))]
pub(crate) fn base_logical_key(ev: &KeyEvent) -> Key {
    ev.logical_key.clone()
}

impl App {
    /// Phase 0.5 — the App::input CONVERGENCE SEAM (design Addendum A.2).
    ///
    /// The SOLE policy site for input egress. The byte-producing core lives in the
    /// source-blind [`input::seam_egress`] (the ONLY reader of `keyboard_mode()` /
    /// `mouse_tracking_enabled()` and the ONLY caller of `encode_key_with_layout` /
    /// the `encode_mouse_*` family / `encode_committed_text` / `format_paste` / the
    /// focus-report egress, reading the relevant mode ONCE per event under a single
    /// `term_lock` — closing the mid-event mode-flip window the two-lock
    /// `on_mouse_input` had, ending at `self.sink.write_frame`, the 0e floor). This
    /// method wraps it with the viewport/gesture/clipboard/geometry side-effects
    /// that need the renderer + window + gesture state: it is the ONLY caller of
    /// `seam_egress` / `scroll_display` / `clear_selection` / `snap_to_bottom` /
    /// `reset_blink` / `apply_term_resize`.
    ///
    /// `src` is recorded for audit and NEVER branched on: `seam_egress` takes no
    /// `Source`, so the bytes a Human and a Controller produce for the SAME
    /// `InputEvent` are byte-identical (the indistinguishability invariant, proven
    /// by `input::tests::bytes_human_eq_controller`).
    pub(crate) fn input(&mut self, wid: WindowId, ev: InputEvent, src: Source) -> InputOutcome {
        // AUDIT-ONLY: bind `src` so the one allowed use (a future §7.5 audit log)
        // is obvious and so a stray behavioural `match src` would stand out in
        // review. It must NEVER gate bytes. The byte-producing core
        // (`input::seam_egress`) takes NO `Source` at all — it is structurally
        // impossible for it to branch (the Tier-1 invariant; the `Buggy` mutant
        // proves the test has teeth).
        let _audit = src;
        // The active session's term/sink for this window. Cheap `Arc` clones held
        // for the duration of this call so `seam_egress` / `term_lock` can run
        // alongside the `&mut self` side-effect method calls below (the moved
        // fields now live behind `windows.get_mut`, which borrows all of `self`).
        let (term, sink) = match self.windows.get(&wid) {
            Some(ws) => (ws.term.clone(), ws.sink.clone()),
            None => return InputOutcome::Ok,
        };
        match ev {
            // --- Keyboard egress (kills f/h; uniform k/g side-effects) ---------
            ev @ (InputEvent::Key { .. } | InputEvent::Text(_)) => {
                // reset_blink -> snap_to_bottom -> clear_selection run for BOTH
                // sources (divergences d/g/k): controller key verbs now snap +
                // deselect + keep the cursor solid exactly like human typing. The
                // ENCODE (sole keyboard-mode read + encoder call) is `seam_egress`.
                self.reset_blink(wid);
                self.snap_to_bottom(wid);
                self.clear_selection(wid);
                egress_to_outcome(input::seam_egress(&term, &sink, &ev))
            }
            // --- Mouse button: tracking-ON report else local gesture (a/b/d/i) -
            ev @ InputEvent::MouseButton { .. } => self.input_mouse_button(wid, &ev, &term, &sink),
            // --- Mouse motion: tracking-ON report else drag the selection (c) ---
            ev @ InputEvent::MouseMove { .. } => {
                let (row, col, side) = if let InputEvent::MouseMove { row, col, side, .. } = ev {
                    (row, col, side)
                } else {
                    unreachable!()
                };
                if let Some(ws) = self.windows.get_mut(&wid) {
                    ws.last_mouse_cell = (row, col);
                    ws.last_mouse_side = side;
                }
                // A held-button drag with tracking OFF grows the local selection
                // (regardless of mode — finishing a drag the app started tracking
                // mid-gesture still settles locally, matching the old handler).
                if self.windows.get(&wid).is_some_and(|ws| ws.selecting) {
                    self.drag_selection(wid, row, col);
                    return InputOutcome::Ok;
                }
                egress_to_outcome(input::seam_egress(&term, &sink, &ev))
            }
            // --- Wheel: N reports/line when tracking ON else scroll viewport (e) -
            ev @ InputEvent::Wheel { .. } => self.input_wheel(wid, &ev, &term, &sink),
            // --- Explicit, tracking-agnostic scrollback nav (A.6) --------------
            InputEvent::ScrollView(intent) => self.input_scroll_view(wid, intent, &term),
            ev @ InputEvent::Paste(_) => self.input_paste(wid, ev, &term, &sink),
            // --- Geometry (range-reject reportable) ----------------------------
            InputEvent::Resize {
                rows,
                cols,
                echo_to_window,
            } => self.input_resize(wid, rows, cols, echo_to_window),
            // --- Focus reporting (kills j) -------------------------------------
            ev @ InputEvent::Focus(_) => {
                // SOLE focus-report egress (in `seam_egress`): identical bytes to
                // the engine's `encode_focus_state` (ESC[I / ESC[O), gated on DEC
                // 1004. The GUI-visual blink/cursor-override side-effect stays in
                // `on_focus`.
                input::seam_egress(&term, &sink, &ev);
                InputOutcome::Ok
            }
        }
    }

    /// `InputEvent::MouseButton` arm of [`App::input`]: a tracking-ON press reports
    /// inside `seam_egress`; tracking-OFF runs the LOCAL left-button selection
    /// gesture for BOTH sources. `click_count` (1/2/3), `side`, and `block` are
    /// carried data — the Human handler ran the MULTI_CLICK_MS streak FSM; a
    /// Controller passes an authoritative count without touching `last_press`
    /// (A.2.2). `block` is the selection-TYPE intent carried ON the event so the
    /// seam never reads `self.mods` (a held human Alt can't leak into a controller
    /// press, and a controller can drive block-select). Only the left button
    /// selects. `ev` must be `InputEvent::MouseButton { .. }`.
    fn input_mouse_button(
        &mut self,
        wid: WindowId,
        ev: &InputEvent,
        term: &Arc<Mutex<Terminal>>,
        sink: &Arc<SinkWriter>,
    ) -> InputOutcome {
        // Carry the gesture-relevant fields out before `seam_egress` (which borrows
        // `ev`) for the tracking-OFF local fallback.
        let (button, pressed, row, col, click_count, side, block) =
            if let &InputEvent::MouseButton {
                button,
                pressed,
                row,
                col,
                click_count,
                side,
                block,
                ..
            } = ev
            {
                (button, pressed, row, col, click_count, side, block)
            } else {
                unreachable!()
            };
        let egress = input::seam_egress(term, sink, ev);
        if let input::Egress::TrackingOff { .. } = egress
            && button == aterm_types::mouse::MouseButton::Left
        {
            if let Some(ws) = self.windows.get_mut(&wid) {
                ws.last_mouse_cell = (row, col);
                ws.last_mouse_side = side;
            }
            if pressed {
                self.seam_left_press(wid, row, col, click_count, block);
            } else if self.windows.get(&wid).is_some_and(|ws| ws.selecting) {
                self.finish_selection(wid);
            }
        }
        egress_to_outcome(egress)
    }

    /// `InputEvent::Wheel` arm of [`App::input`]: tracking ON emitted the per-line
    /// reports inside `seam_egress`; tracking OFF scrolls the LOCAL viewport by the
    /// wheel's lines (>0, guaranteed by the handler/verb) and repaints. Positive
    /// `display_offset` = older content, so wheel up -> history. `ev` must be
    /// `InputEvent::Wheel { .. }`.
    fn input_wheel(
        &mut self,
        wid: WindowId,
        ev: &InputEvent,
        term: &Arc<Mutex<Terminal>>,
        sink: &Arc<SinkWriter>,
    ) -> InputOutcome {
        let egress = input::seam_egress(term, sink, ev);
        if let input::Egress::TrackingOff {
            wheel_lines,
            wheel_up,
        } = egress
        {
            term_lock(term).scroll_display(if wheel_up { wheel_lines } else { -wheel_lines });
            if let Some(w) = self.windows.get(&wid).and_then(|ws| ws.os_window.as_ref()) {
                w.request_redraw();
            }
        }
        egress_to_outcome(egress)
    }

    /// `InputEvent::ScrollView` arm of [`App::input`] (A.6): pure history nav — even
    /// when the app is mouse-tracking it touches only the LOCAL viewport (never emits
    /// wheel bytes), so a read-only edge can't drive a tracking app through it. The
    /// SEAM is the sole `scroll_display`/`scroll_to_*` caller.
    fn input_scroll_view(
        &mut self,
        wid: WindowId,
        intent: ScrollIntent,
        term: &Arc<Mutex<Terminal>>,
    ) -> InputOutcome {
        {
            let mut term = term_lock(term);
            let page = i32::from(term.rows()).max(1);
            match intent {
                ScrollIntent::Up => term.scroll_display(page),
                ScrollIntent::Down => term.scroll_display(-page),
                ScrollIntent::By(n) => term.scroll_display(n),
                ScrollIntent::Top => term.scroll_to_top(),
                ScrollIntent::Bottom => term.scroll_to_bottom(),
            }
        }
        if let Some(w) = self.windows.get(&wid).and_then(|ws| ws.os_window.as_ref()) {
            w.request_redraw();
        }
        InputOutcome::Ok
    }

    /// `InputEvent::Paste` arm of [`App::input`]: a paste, like typing, jumps the
    /// viewport back to live; the `format_paste` bytes come from `seam_egress`.
    ///
    /// Offload the (blocking) PTY write OFF the winit UI thread. A large paste (up to
    /// MAX_PASTE_BYTES = 16 MiB) into a foreground program that is not currently
    /// reading stdin would otherwise park `write_frame` — and therefore the event
    /// loop that serves rendering AND input for EVERY window/tab — inside a blocking
    /// `write(2)` until the consumer drains (the tty input buffer is only ~8 KiB).
    /// The bytes are still produced by the SAME `seam_egress`, so Human and Controller
    /// paste stay byte-identical (the indistinguishability invariant is untouched —
    /// only WHERE the write runs moves, and only for the Human/GUI path). The detached
    /// thread holds `Arc` clones of the term + sink, so the PTY master fd stays open
    /// for the whole write (the OwnedFd-closes-on-last-clone-drop contract) and
    /// whole-frame atomicity is preserved (the sink lock still wraps the entire paste
    /// frame). On session teardown the slave closes and the parked write returns an
    /// error, so the thread always ends — no leak. `ev` must be `InputEvent::Paste(_)`.
    fn input_paste(
        &mut self,
        wid: WindowId,
        ev: InputEvent,
        term: &Arc<Mutex<Terminal>>,
        sink: &Arc<SinkWriter>,
    ) -> InputOutcome {
        self.snap_to_bottom(wid);
        let term = term.clone();
        let sink = sink.clone();
        std::thread::spawn(move || {
            input::seam_egress(&term, &sink, &ev);
        });
        InputOutcome::Ok
    }

    /// `InputEvent::Resize` arm of [`App::input`] (range-reject reportable):
    /// `echo_to_window` picks the apply path WITHOUT branching on `Source` (it is
    /// keyed on WHERE the geometry came from). The control `resize` verb (no window
    /// event) echoes the new size to the window (`apply_grid_resize` ->
    /// `request_inner_size`); the winit `Resized` handler (window already this size)
    /// applies just the term+PTY+framebuffer (`apply_term_resize`) so it never fights
    /// an interactive edge-drag — the RES-1 regression fix. A `Resized` for a SHARED
    /// (Cmd-Shift-O) session is driven to the element-wise min across co-viewers
    /// inside `apply_term_resize` so it can't corrupt the other viewer's display.
    fn input_resize(
        &mut self,
        wid: WindowId,
        rows: u16,
        cols: u16,
        echo_to_window: bool,
    ) -> InputOutcome {
        if !(1..=aterm_core::grid::MAX_GRID_ROWS).contains(&rows)
            || !(1..=aterm_core::grid::MAX_GRID_COLS).contains(&cols)
        {
            return InputOutcome::RangeRejected;
        }
        if echo_to_window {
            self.apply_grid_resize(rows, cols);
        } else {
            self.apply_term_resize(wid, rows, cols);
        }
        InputOutcome::Ok
    }

    /// Seam-internal left-press gesture dispatch shared by both sources (the
    /// tracking-OFF branch of `InputEvent::MouseButton`). `click_count` is
    /// authoritative (Human: from the streak FSM; Controller: carried 1..=3); it
    /// does NOT touch `App.last_press` here — the streak state belongs to the
    /// Human handler, which owns it (A.2.2).
    ///
    /// SOURCE-BLIND: the single-click selection TYPE (Block vs Simple) comes from
    /// the `block` flag carried ON the event, NOT from `self.mods` — so a held
    /// human Alt can't leak into a controller-driven press, and a controller can
    /// drive a block selection by sending `block=1`. The Human builder snapshots
    /// `self.mods.alt_key()` into `block` at event-build time in `on_mouse_input`.
    pub(crate) fn seam_left_press(
        &mut self,
        wid: WindowId,
        row: u16,
        col: u16,
        click_count: u8,
        block: bool,
    ) {
        let Some(term) = self.windows.get(&wid).map(|ws| ws.term.clone()) else {
            return;
        };
        let sel_row = i32::from(row) - term_lock(&term).grid().display_offset() as i32;
        match click_count {
            2 => self.select_word_click(wid, sel_row, col),
            3 => self.select_line_click(wid, sel_row, col),
            _ => self.begin_selection(
                wid,
                if block {
                    SelectionType::Block
                } else {
                    SelectionType::Simple
                },
            ),
        }
    }

    pub(crate) fn on_key(&mut self, wid: WindowId, ev: KeyEvent) {
        if ev.state != ElementState::Pressed {
            return;
        }
        // The current modifier state for this window (a `Copy` snapshot, so the
        // borrow does not outlive the read). No such window ⇒ nothing to do
        // (mirrors the old "no window" no-op).
        let Some(mods) = self.windows.get(&wid).map(|ws| ws.mods) else {
            return;
        };
        // Typing makes the cursor solid and restarts the blink period.
        self.reset_blink(wid);
        // User-rebindable shortcuts (config `[keybindings]`) take precedence. The
        // lookup is O(1) and SKIPPED entirely when no bindings are configured
        // (the empty-map default), so the hardcoded path below is byte-identical
        // with no config. A configured chord dispatches its action and returns; a
        // MISS falls through to the hardcoded matches, so an unbound key (or a
        // key the user did NOT remap) behaves exactly as before. Keybindings are
        // GLOBAL; dispatch is threaded with the routed `wid`.
        if !self.keybindings.is_empty() {
            // Match on the modifier-independent BASE key (e.g. `]` under Shift,
            // not `}`) so a binding the user wrote matches across layouts — the
            // same base key `build_key_input` encodes with.
            let base = base_logical_key(&ev);
            if let Some(action) = self.keybindings.lookup(&base, mods) {
                self.dispatch_action(wid, action);
                return;
            }
        }
        if self.on_key_super_shift_chord(mods, &ev) {
            return;
        }
        if self.on_key_super_chord(mods, &ev) {
            return;
        }
        // Cmd-F enters find mode; while active, keystrokes drive the find (query
        // edit + match navigation) instead of reaching the PTY.
        if mods.super_key()
            && let Key::Character(s) = &ev.logical_key
            && s.eq_ignore_ascii_case("f")
        {
            self.search_enter();
            return;
        }
        if self.on_key_search_mode(wid, mods, &ev) {
            return;
        }
        // Cmd-C -> copy the selection to the system clipboard (before the
        // snap-to-bottom: copying must neither clear the selection nor move
        // the viewport). With no selection it falls through to normal handling.
        if mods.super_key()
            && let Key::Character(s) = &ev.logical_key
            && s.eq_ignore_ascii_case("c")
            && self.copy_selection()
        {
            return;
        }
        // Any key press past this point jumps the viewport back to the live view
        // if scrolled into history — PRESERVED at the original position (after
        // Cmd-C, before Cmd-V/zoom/IME-suppress) so the human parity is exact:
        // zoom keys and an IME-composing key that returns early still snap, just
        // like HEAD. The seam ALSO snaps in its Key/Text/Paste arms (idempotent
        // when already at the bottom) so the CONTROLLER path snaps too — that arm
        // is the convergence point, this early call is the human-parity point.
        self.snap_to_bottom(wid);
        // Cmd-V -> paste the system clipboard (bracketed when the app enabled
        // it). Pasting does not clear the selection.
        if mods.super_key()
            && let Key::Character(s) = &ev.logical_key
            && s.eq_ignore_ascii_case("v")
        {
            self.paste_clipboard();
            return;
        }
        if self.on_key_font_zoom(mods, &ev) {
            return;
        }
        // IME-1: while a composition (CJK / dead key) is in flight, SUPPRESS the
        // direct key send — the keystrokes belong to the composer; the resulting
        // text arrives via `Ime::Commit` (encoded through the same engine path).
        // Without this the composing keys would ALSO emit raw bytes (double
        // input). ASCII typing with no active composition is unaffected (preedit
        // is empty), so normal keys still send below. The Ctrl+letter `& 0x1f`
        // branch is intentionally GONE: K-1 routing (below) encodes Ctrl, Alt,
        // named keys, and Kitty CSI-u via the engine's `keymap` encoder.
        if self
            .windows
            .get(&wid)
            .is_some_and(|ws| keymap::suppress_direct_send(&ws.preedit))
        {
            return;
        }
        // option_as_meta = false (config opt-out): the macOS Option key types its
        // OS-COMPOSED character (Option+a → "å") instead of the ESC-prefixed Meta
        // sequence the engine encoder produces by default. Only when Option/Alt is
        // the SOLE relevant modifier (no Ctrl/Super, which keep their engine
        // encoding) and winit resolved a composed `text` — so a bare Alt+arrow or
        // an Alt chord with no text still falls through to the encoder below. With
        // the default (`option_as_meta = true`), and on the no-config path, this
        // block is skipped entirely, so the encode path is byte-identical.
        if !self.option_as_meta
            && mods.alt_key()
            && !mods.control_key()
            && !mods.super_key()
            && let Some(text) = &ev.text
            && !text.is_empty()
        {
            self.input(wid, InputEvent::Text(text.to_string()), Source::Human);
            return;
        }
        // Phase 0.5: BUILD an engine-neutral InputEvent and call the seam in-thread
        // (no hop, no latency cost). The seam is the sole reader of keyboard_mode()
        // and the sole caller of the encoder + reset_blink/snap_to_bottom/
        // clear_selection — so a human key and the `key`/`ctrl` verbs that build the
        // SAME (Key, mods, base_layout) triple produce byte-identical PTY output
        // (kills divergences f/h; uniform g/k side-effects). The keymap is demoted
        // to a BUILDER (`build_key_input`) that fills `base_layout` from the
        // physical key for Kitty REPORT_ALTERNATE_KEYS.
        let km_mods = keymap::modifiers_from_winit(mods);
        if let Some((key, km_mods, base_layout)) = keymap::build_key_input(&ev, km_mods) {
            // `on_key` returns early for any non-`Pressed` winit state (see top of
            // this fn), so the human path is always a `Press` — byte-identical to
            // the pre-event_type behaviour the seam hard-coded.
            self.input(
                wid,
                InputEvent::Key {
                    key,
                    mods: km_mods,
                    base_layout,
                    event_type: aterm_types::keyboard::KeyEventType::Press,
                },
                Source::Human,
            );
            return;
        }
        // IME/dead-key fallback: the keymap mapped no engine key (an unencodable
        // key, or a layout-composed character that `key_without_modifiers`
        // stripped). Honor winit's resolved `text` so a plain layout character
        // still types when no IME composition is active — but NEVER for
        // Ctrl/Alt/Super, whose ESC/control encoding the engine already owns above.
        let bare = !mods.control_key() && !mods.alt_key() && !mods.super_key();
        if let Some(text) = &ev.text
            && bare
            && !text.is_empty()
        {
            self.input(wid, InputEvent::Text(text.to_string()), Source::Human);
        }
    }

    /// Hardcoded Cmd-Shift chords of [`on_key`], handled FIRST among the Cmd combos
    /// because they need Shift (which the `!shift_key()` block excludes). Returns
    /// `true` when a chord fired (the caller must then return); `false` (incl. an
    /// unrecognized Cmd-Shift character) falls through to the rest of `on_key`. On a
    /// US layout Shift maps `]`/`[`/`d`/`o`/`m`/`n` to `}`/`{`/`D`/`O`/`M`/`N`, so
    /// both forms are accepted.
    fn on_key_super_shift_chord(&mut self, mods: ModifiersState, ev: &KeyEvent) -> bool {
        if mods.super_key()
            && mods.shift_key()
            && let Key::Character(s) = &ev.logical_key
        {
            match s.as_str() {
                // Cmd-Shift-] / Cmd-Shift-[ cycle to the next / previous in-window
                // TAB (wrapping).
                "]" | "}" => {
                    self.cycle_tab(true);
                    return true;
                }
                "[" | "{" => {
                    self.cycle_tab(false);
                    return true;
                }
                // Cmd-Shift-N "Move Tab to New Window": pull the frontmost
                // window's active tab out into a fresh in-process window.
                // `on_key` has no `ActiveEventLoop`, so post a Wake; the
                // `user_event` arm (which has `el`) runs the move + OS attach.
                "n" | "N" => {
                    if let Some(proxy) = self.proxy.as_ref() {
                        let _ = proxy.send_event(Wake::DetachActiveTab);
                    }
                    return true;
                }
                // Cmd-Shift-D: split the FOCUSED pane HORIZONTALLY (panes stacked
                // top/bottom). This is the default chord for `Action::SplitHorizontal`
                // (keybinding parity). The multi-window "view active session in a
                // second window" affordance was RELOCATED to Cmd-Shift-O (below) to
                // resolve the Cmd-Shift-D double-binding.
                "d" | "D" => {
                    self.split_focused_pane(pane::SplitDir::Horizontal);
                    return true;
                }
                // Cmd-Shift-O "Open Active Session in New Window": show the
                // frontmost window's active session in a SECOND window (same live
                // grid in two windows). RELOCATED here from Cmd-Shift-D (which is
                // now SplitHorizontal). `on_key` has no `ActiveEventLoop`, so post a
                // Wake; the `user_event` arm (which has `el`) runs the attach +
                // OS-window create.
                "o" | "O" => {
                    if let Some(proxy) = self.proxy.as_ref() {
                        let _ = proxy.send_event(Wake::ViewActiveSessionInNewWindow);
                    }
                    return true;
                }
                // Cmd-Shift-M "Move Tab to Next Window": move the frontmost window's
                // active tab into the NEXT existing window (wrapping). The destination
                // already exists, so there is no OS-window attach and no `el` is
                // needed — call the move directly (no Wake round-trip). A <2-window
                // app is a no-op.
                "m" | "M" => {
                    self.migrate_active_tab_to_next_window();
                    return true;
                }
                _ => {}
            }
        }
        false
    }

    /// Hardcoded Cmd (no Shift) chords of [`on_key`]: Cmd-N opens a new IN-PROCESS
    /// WINDOW (the standard macOS "new window", sharing this process's
    /// renderer/device); Cmd-T opens a new in-window TAB (a fresh shell session
    /// sharing this window); Cmd-D splits vertically; Cmd-W closes the focused pane
    /// (escalating to the window close on the LAST pane of the LAST tab); Cmd-1..Cmd-9
    /// jump straight to that tab (1-based). Returns `true` when a chord fired.
    fn on_key_super_chord(&mut self, mods: ModifiersState, ev: &KeyEvent) -> bool {
        if mods.super_key()
            && !mods.shift_key()
            && let Key::Character(s) = &ev.logical_key
        {
            let lc = s.to_ascii_lowercase();
            match lc.as_str() {
                "n" => {
                    // Cmd-N opens a real IN-PROCESS window. `on_key` has no
                    // `ActiveEventLoop`, so post a `Wake::CreateWindow`; the
                    // `user_event` arm (which has `el`) runs the creation.
                    if let Some(proxy) = self.proxy.as_ref() {
                        let _ = proxy.send_event(Wake::CreateWindow);
                    }
                    return true;
                }
                "t" => {
                    self.open_tab();
                    return true;
                }
                // Cmd-D: split the FOCUSED pane VERTICALLY (panes side by side).
                "d" => {
                    self.split_focused_pane(pane::SplitDir::Vertical);
                    return true;
                }
                "w" => {
                    // Close the FOCUSED PANE of this (frontmost) window's active
                    // tab. A split tab's Cmd-W collapses one pane onto its sibling;
                    // the only pane of a non-last tab closes the tab in-place. The
                    // LAST pane of the LAST tab's close sets `pending_close` so
                    // `window_event` (which has the `ActiveEventLoop`) escalates to
                    // closing the WINDOW after `on_key` returns — `on_key` itself
                    // has no `el` to do so. The app exits only when that was the
                    // last window.
                    // Escalate on the window `close_active_tab` actually closed
                    // (the FRONTMOST), not the event-stamped `wid` — they can
                    // differ when the keypress was routed to a non-front window.
                    if let Some(closed) = self.close_active_tab()
                        && let Some(ws) = self.windows.get_mut(&closed)
                    {
                        ws.pending_close = true;
                    }
                    return true;
                }
                // Cmd-1..Cmd-9 → switch to that tab (1-based → 0-based index).
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" => {
                    if let Some(d) = lc.chars().next().and_then(|c| c.to_digit(10)) {
                        self.switch_tab(d as usize - 1);
                    }
                    return true;
                }
                _ => {}
            }
        }
        false
    }

    /// Find-mode keystroke dispatch of [`on_key`]: while a window's `search` is
    /// active, keystrokes drive the find (query edit + match navigation) instead of
    /// reaching the PTY. Returns `true` whenever search is active (so the caller
    /// returns — matching the inline block's unconditional `return`); `false` when no
    /// search is in flight.
    fn on_key_search_mode(&mut self, wid: WindowId, mods: ModifiersState, ev: &KeyEvent) -> bool {
        if self.windows.get(&wid).is_none_or(|ws| ws.search.is_none()) {
            return false;
        }
        match &ev.logical_key {
            Key::Named(NamedKey::Escape) => self.search_exit(),
            Key::Named(NamedKey::Enter) => self.search_step(!mods.shift_key()),
            Key::Named(NamedKey::Backspace) => {
                if let Some(s) = self.windows.get_mut(&wid).and_then(|ws| ws.search.as_mut()) {
                    s.query.pop();
                }
                self.search_recompute();
            }
            _ => {
                // Plain typing edits the query; modifier combos are swallowed.
                if !mods.super_key()
                    && !mods.control_key()
                    && let Some(text) = &ev.text
                    && !text.is_empty()
                {
                    if let Some(s) = self.windows.get_mut(&wid).and_then(|ws| ws.search.as_mut()) {
                        s.query.push_str(text);
                    }
                    self.search_recompute();
                }
            }
        }
        true
    }

    /// Cmd-= / Cmd-+ / Cmd-- / Cmd-0 live font zoom (grow / shrink / reset) of
    /// [`on_key`]. Returns `true` when a zoom chord fired.
    fn on_key_font_zoom(&mut self, mods: ModifiersState, ev: &KeyEvent) -> bool {
        if mods.super_key()
            && let Key::Character(s) = &ev.logical_key
        {
            match s.as_str() {
                "=" | "+" => {
                    self.set_font_px(self.font_px + FONT_ZOOM_STEP);
                    return true;
                }
                "-" => {
                    self.set_font_px(self.font_px - FONT_ZOOM_STEP);
                    return true;
                }
                "0" => {
                    self.set_font_px(self.default_font_px);
                    return true;
                }
                _ => {}
            }
        }
        false
    }

    /// Run a user-bound [`keybinding::Action`] — the configurable trigger for an
    /// existing hardcoded `on_key` command. Each arm calls the SAME method the
    /// built-in key calls, so a binding does exactly what the default did (no new
    /// behavior, just a configurable chord). Keybindings are GLOBAL but dispatch is
    /// routed with the originating window `wid`; Cmd-W's close result sets that
    /// window's per-window `pending_close` exactly as the hardcoded path does.
    pub(crate) fn dispatch_action(&mut self, wid: WindowId, action: keybinding::Action) {
        use keybinding::Action;
        match action {
            Action::NewTab => self.open_tab(),
            Action::CloseTab => {
                // Set `pending_close` on the window whose last tab closed (the
                // FRONTMOST that `close_active_tab` operated on), not the event `wid`.
                let _ = wid;
                if let Some(closed) = self.close_active_tab()
                    && let Some(ws) = self.windows.get_mut(&closed)
                {
                    ws.pending_close = true;
                }
            }
            Action::NewWindow => {
                // In-process, consistent with the hardcoded Cmd-N and the menu
                // (the multi-window flip: a new window lives in THIS process, not a
                // fresh subprocess). dispatch_action has no `ActiveEventLoop`, so
                // post Wake::CreateWindow; user_event runs create_window_internal.
                if let Some(proxy) = self.proxy.as_ref() {
                    let _ = proxy.send_event(Wake::CreateWindow);
                }
            }
            Action::NextTab => self.cycle_tab(true),
            Action::PrevTab => self.cycle_tab(false),
            // 1-based as the user wrote it → 0-based index (Cmd-1..Cmd-9 parity).
            Action::SwitchTab(n) => self.switch_tab(usize::from(n).saturating_sub(1)),
            Action::SplitVertical => self.split_focused_pane(pane::SplitDir::Vertical),
            Action::SplitHorizontal => self.split_focused_pane(pane::SplitDir::Horizontal),
            // Copy is a no-op with no selection (matches the hardcoded fall-through).
            Action::Copy => {
                self.copy_selection();
            }
            Action::Paste => {
                // A paste, like the hardcoded Cmd-V, jumps the viewport to live.
                self.snap_to_bottom(wid);
                self.paste_clipboard();
            }
            Action::Find => self.search_enter(),
            Action::FontIncrease => self.set_font_px(self.font_px + FONT_ZOOM_STEP),
            Action::FontDecrease => self.set_font_px(self.font_px - FONT_ZOOM_STEP),
            Action::FontReset => self.set_font_px(self.default_font_px),
        }
    }

    /// IME-1: a composition update (`Ime::Preedit`) — track the marked text so a
    /// preedit indicator can render and direct key sends stay suppressed while
    /// composing. An empty preedit ends the composition. Requests a repaint so
    /// the (minimal) on-screen indicator follows the composition.
    pub(crate) fn on_ime_preedit(&mut self, wid: WindowId, text: String) {
        let Some(ws) = self.windows.get_mut(&wid) else {
            return;
        };
        let changed = ws.preedit != text;
        ws.preedit = text;
        if changed && let Some(w) = &ws.os_window {
            w.request_redraw();
        }
    }

    /// IME-1: composition committed (`Ime::Commit`) — the finished CJK/dead-key
    /// text. End the composition and send the committed text to the PTY via the
    /// engine path (each grapheme encoded as a `Character` key, NOT `& 0x1f`), so
    /// it goes out exactly as typed text. Clears the selection like any typing.
    pub(crate) fn on_ime_commit(&mut self, wid: WindowId, text: String) {
        if let Some(ws) = self.windows.get_mut(&wid) {
            ws.preedit.clear();
        }
        if text.is_empty() {
            return;
        }
        // Phase 0.5: committed text goes through the seam's Text path (the sole
        // keyboard-mode reader + `encode_committed_text` caller), converging with
        // the controller's text egress.
        self.input(wid, InputEvent::Text(text), Source::Human);
        if let Some(w) = self.windows.get(&wid).and_then(|ws| ws.os_window.as_ref()) {
            w.request_redraw();
        }
    }

    /// Current keyboard modifiers as a mouse-report modifier mask (shift/alt/ctrl
    /// bits the engine ORs into the button byte).
    pub(crate) fn mouse_modifiers(&self, wid: WindowId) -> u8 {
        use aterm_types::mouse::{ALT_MASK, CTRL_MASK, SHIFT_MASK};
        let Some(mods) = self.windows.get(&wid).map(|ws| ws.mods) else {
            return 0;
        };
        let mut m = 0u8;
        if mods.shift_key() {
            m |= SHIFT_MASK;
        }
        if mods.alt_key() {
            m |= ALT_MASK;
        }
        if mods.control_key() {
            m |= CTRL_MASK;
        }
        m
    }

    /// Dispatch a macOS menu-bar click into the EXISTING `App` command method the
    /// matching keybinding already uses — the menu adds an entry point, never a
    /// parallel implementation. Anything the user could do from the menu, they can
    /// still do from the keyboard (handled in `on_key`), byte-for-byte the same.
    /// `el` is needed only for the items that must exit the loop (Quit, and Close
    /// Tab when it closes the last tab). Off macOS this is reachable code (the
    /// `Wake::MenuAction` arm calls it) but never actually fired (no platform menu
    /// ever constructs the variant), so it stays warning-clean on every target.
    pub(crate) fn dispatch_menu_action(&mut self, el: &ActiveEventLoop, action: menu::MenuAction) {
        use menu::MenuAction;
        match action {
            // App menu --------------------------------------------------------
            // About shows the standard macOS About panel (name + version from
            // Info.plist + the bundled Credits.html). Preferences / Help remain
            // no-op stubs (the item exists and dispatches; the pane is a follow-up).
            MenuAction::About => menu::show_about_panel(),
            MenuAction::Preferences | MenuAction::Help => {}
            MenuAction::Hide => self.hide_app(),
            MenuAction::Quit => el.exit(),
            // File ------------------------------------------------------------
            // Window ▸ New Window opens a real in-process window. `dispatch_menu_action`
            // already has `el`, so create it directly (no Wake round-trip needed).
            MenuAction::NewWindow => {
                self.create_window_internal(el);
            }
            MenuAction::NewTab => self.open_tab(),
            // Window ▸ Move Tab to New Window: pull the active tab out into a fresh
            // in-process window. `dispatch_menu_action` already has `el`, so the
            // logical move + OS-window attach run directly (no Wake round-trip).
            MenuAction::MoveTabToNewWindow => self.detach_active_tab(el),
            // Window ▸ Move Tab to Next Window: move the active tab into the NEXT
            // EXISTING window (wrapping). The destination already exists, so there is
            // no OS-window attach and no `el` is needed.
            MenuAction::MoveTabToNextWindow => self.migrate_active_tab_to_next_window(),
            // Window ▸ Open Session in New Window: show the active session in a SECOND
            // window (same live grid in two windows). `dispatch_menu_action` already
            // has `el`, so the logical attach + OS-window create run directly.
            MenuAction::ViewSessionInNewWindow => self.open_active_session_in_new_window(el),
            MenuAction::CloseTab => {
                // Same rule as Cmd-W: close the frontmost window's active tab; when
                // that was its LAST tab, escalate to closing THAT window (which exits
                // the app IFF it was the last window).
                if let Some(closed) = self.close_active_tab() {
                    self.close_window(el, closed);
                }
            }
            // Edit ------------------------------------------------------------
            // Copy with no selection is a harmless no-op (the bool is ignored here,
            // exactly like the Cmd-C fall-through in on_key).
            MenuAction::Copy => {
                let _ = self.copy_selection();
            }
            MenuAction::Paste => self.paste_clipboard(),
            MenuAction::SelectAll => self.select_all(),
            MenuAction::Find => self.search_enter(),
            // View ------------------------------------------------------------
            MenuAction::ToggleFullScreen => self.toggle_fullscreen(),
            // Window ----------------------------------------------------------
            MenuAction::Minimize => {
                if let Some(w) = self.front().and_then(|ws| ws.os_window.as_ref()) {
                    w.set_minimized(true);
                }
            }
            MenuAction::Zoom => {
                // Zoom toggles maximised, like the green-button / Window ▸ Zoom.
                if let Some(w) = self.front().and_then(|ws| ws.os_window.as_ref()) {
                    w.set_maximized(!w.is_maximized());
                }
            }
        }
    }
}
