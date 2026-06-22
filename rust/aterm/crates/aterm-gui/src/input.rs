// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! Phase 0.5 — the engine-neutral [`InputEvent`] vocabulary and the [`Source`]
//! audit tag for the [`App::input`](crate::App::input) convergence seam
//! (design Addendum A.2, `docs/design/HIERARCHICAL_SESSIONS.md`).
//!
//! TODAY human input (the winit `on_key`/`on_mouse_*`/`on_cursor_moved`/
//! `on_resize`/`on_focus` handlers) and controller input (the control verbs
//! `cmd_key`/`cmd_ctrl`/`cmd_mouse`/`cmd_scroll`/`cmd_paste`/`cmd_resize`/...)
//! flow through TWO parallel code paths that can drift: only the human path
//! reads real modifiers, runs the click-count FSM, emits intermediate motion
//! reports, snaps-to-bottom + clears-selection on a keystroke, carries the
//! Kitty base-layout key, reports focus, and resets the cursor blink. The
//! controller path hard-codes `mods=0`, has no click-count, jumps straight to a
//! selection result, drops events when tracking is off, and never snaps/clears/
//! resets-blink.
//!
//! This module is the FRONTEND-ONLY data layer: an `InputEvent` is plain data
//! plus engine *types* (`Key`, `Modifiers`, `MouseButton`, `SelectionSide`,
//! `SelectionType`) — no fs, no socket, no winit. Both sources BUILD an
//! `InputEvent` and feed it to the ONE policy site `App::input(ev, src)`, which
//! is the sole reader of `keyboard_mode()`/`mouse_tracking_enabled()` and the
//! sole caller of the encoders / `scroll_display` / `clear_selection` /
//! `snap_to_bottom` / `reset_blink` / `apply_term_resize`. The seam ends at the
//! existing 0e `SinkWriter` (`sink.write_frame`) — unchanged.
//!
//! `Source` is AUDIT-ONLY: the seam MUST NEVER branch behaviour on it (the
//! indistinguishability invariant). The byte-producing core [`seam_egress`]
//! takes NO `Source` — it is STRUCTURALLY impossible for it to branch — and the
//! gesture-state arms of `App::input` read ONLY data carried on the event (never
//! `self.mods`). The Tier-1 tests prove convergence two ways: the two REAL
//! builders (`build_key_input`/`cmd_*` parse) produce structurally-EQUAL events
//! for the same intent, and those events produce byte-identical sink output.

use std::sync::Mutex;

use aterm_core::selection::SelectionSide;
use aterm_core::terminal::Terminal;
use aterm_session::Op;
use aterm_session::sink::SinkWriter;
use aterm_types::keyboard::{Key, Modifiers};
use aterm_types::mouse::MouseButton;

use crate::term_lock;

/// One logical input event, engine-neutral. Built identically by a winit handler
/// (`Source::Human`) and by a control verb (`Source::Controller`); the seam turns
/// it into PTY bytes / viewport side-effects the SAME way for both.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InputEvent {
    /// A key chord. `base_layout` is the US-QWERTY equivalent of the physical key,
    /// fed to `encode_key_with_layout` so Kitty `REPORT_ALTERNATE_KEYS` carries the
    /// 3rd CSI-u field identically for both sources (kills divergences f/h).
    Key {
        key: Key,
        mods: Modifiers,
        base_layout: Option<char>,
        /// Press / Repeat / Release. The human winit path only ever builds
        /// `Press` (releases are dropped at `on_key`); the `key` verb can request
        /// any via `type=press|repeat|release`, so a controller can drive the
        /// Kitty keyboard protocol's event-type CSI-u sub-field (`:1`/`:2`/`:3`)
        /// that a real key-up/repeat produces. Encoded ONCE by `seam_egress`.
        event_type: aterm_types::keyboard::KeyEventType,
    },
    /// Literal text to type: the `on_key` bare-`ev.text` fallback and the IME
    /// `Ime::Commit` path. Each char is encoded as a `Character` key under the
    /// current keyboard mode.
    Text(String),
    /// A mouse button press/release at a grid cell. `mods` is the real modifier
    /// mask (kills a), `click_count` is the authoritative 1..=3 multi-click depth
    /// (kills b), `side` is the cell-half for selection boundaries (kills i), and
    /// `block` is the selection-TYPE intent for a single-click press (kills the
    /// ambient-state read: a controller can drive block-select, a human's held
    /// Alt is captured at build time and never leaks past the event).
    MouseButton {
        button: MouseButton,
        pressed: bool,
        row: u16,
        col: u16,
        mods: u8,
        click_count: u8,
        side: SelectionSide,
        /// Single-click press starts a `Block` (rectangular) selection rather
        /// than `Simple`. Human: `self.mods.alt_key()` snapshotted at build time.
        /// Controller: the `block=…` token (default `false`). Read ONLY here, as
        /// DATA — the seam never re-reads ambient modifier state for the type.
        block: bool,
    },
    /// Pointer motion. `buttons == 3` is a no-button hover (motion report code 3);
    /// `buttons != 3` is a held-button drag (kills c). `side` is the cell-half.
    MouseMove {
        buttons: u8,
        row: u16,
        col: u16,
        mods: u8,
        side: SelectionSide,
    },
    /// A wheel notch / trackpad flick of `lines` lines (kills e: one report per
    /// line when tracking is on, else the viewport scrolls `lines`). `lines` is
    /// clamped to `>= 1` in the seam so a non-positive count can never produce a
    /// silent human/controller asymmetry.
    Wheel {
        dir_up: bool,
        lines: i32,
        row: u16,
        col: u16,
        mods: u8,
    },
    /// Explicit, tracking-agnostic scrollback navigation (the `scroll` verb).
    /// Never emits wheel reports; it only moves the local viewport. A controller
    /// that wants to drive a tracking app's wheel uses `Wheel`/`mouse` instead.
    ScrollView(ScrollIntent),
    /// Paste text as if typed (bracketed when the app enabled DECSET 2004).
    Paste(String),
    /// A geometry change. Re-clamped against `MAX_GRID_*` in the seam.
    ///
    /// `echo_to_window` is a TRANSPORT flag (NOT a `Source` branch): the control
    /// `resize` verb sets it `true` so the seam also asks the window to match the
    /// new grid pixel size (RES-1 — the verb has no window event of its own); the
    /// winit `Resized` handler sets it `false` because the window ALREADY has the
    /// new size and re-`request_inner_size`-ing it would fight an interactive
    /// edge-drag (the RES-1 regression). It is keyed on WHERE the geometry came
    /// from, identical for a human-issued vs controller-issued `resize` verb.
    Resize {
        rows: u16,
        cols: u16,
        echo_to_window: bool,
    },
    /// Focus gained/lost — DEC 1004 focus reporting (kills j). `true` = focus-in.
    Focus(bool),
}

/// Tracking-agnostic scrollback navigation for [`InputEvent::ScrollView`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollIntent {
    /// One screen toward older content.
    Up,
    /// One screen toward the live bottom.
    Down,
    /// `N` lines into history (negative = toward the live bottom).
    By(i32),
    /// Jump to the oldest scrollback.
    Top,
    /// Jump to the live bottom.
    Bottom,
}

/// WHO produced an [`InputEvent`]. AUDIT-ONLY — the seam MUST NOT branch on this
/// (the Tier-1 indistinguishability invariant). `Op` is carried for the §7.5
/// audit log only; it is `Copy`, so `Source` stays `Copy` and the `Wake::Input`
/// drain loop can pass it by value into every event.
///
/// NOTE: design A.2 wrote `Controller { edge: EdgeId }`, but there is NO `EdgeId`
/// type in `aterm-session` (only `SessionId`, `EdgeToken`, `Op`). We carry the
/// `Op` of the OPERATION being performed (the verb's audit class — `ReadScreen` for
/// view control like `scroll`, `WriteInput` for the input verbs), captured at the
/// verb in `control.rs` (`post_input`/`post_input_reply`). It is deliberately NOT
/// read off the connection's `Scope`: the cached connect-time op there can drift from
/// what the verb actually does once the active session swings, which would corrupt the
/// audit trail. The session-owner connection maps to `Controller` too (an owner is
/// still a controller, never `Human`): `Human` is built ONLY by the in-thread winit
/// handlers.
#[derive(Clone, Copy, Debug)]
pub enum Source {
    /// An in-thread winit handler (real keyboard/mouse/focus on this window).
    Human,
    /// A control-socket verb. `op` is the audit class of the OPERATION (the verb's
    /// own op, not the connection's scope). AUDIT-ONLY: captured for a future §7.5
    /// audit log; the seam binds `src` to `_audit` and NEVER reads it for a
    /// behavioural decision (the indistinguishability invariant), so it has no reader.
    Controller {
        #[allow(dead_code)]
        op: Op,
    },
}

/// The reply a reply-bearing verb gets back from the seam. Fire-and-forget
/// callers ignore it. `Copy` so the drain loop can keep the last outcome cheaply.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputOutcome {
    /// Applied (bytes written and/or viewport moved).
    Ok,
    /// A `Resize` fell outside `1..=MAX_GRID_*` and was not applied.
    RangeRejected,
    /// The encoded bytes were NOT (fully) written to the PTY — a short write (peer
    /// closed mid-frame) or a hard error (audit finding: the input seam must not
    /// report OK for bytes that did not land; it is the reply-fidelity contract that
    /// `OK` means delivered).
    WriteFailed,
}

/// Whether [`seam_egress`] actually delivered the event's encoded bytes to the PTY.
/// An event that legitimately encodes to NO bytes (a legacy-mode key release, an
/// un-encodable modifier — faithful to what a real terminal does) is [`Full`]: there
/// was nothing to deliver and nothing was lost. Only a short/failed write is
/// [`Failed`].
///
/// [`Full`]: Delivery::Full
/// [`Failed`]: Delivery::Failed
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delivery {
    /// Every intended byte reached the PTY (or there were none to write).
    Full,
    /// A short or failed PTY write — the bytes did not (fully) land.
    Failed,
}

/// Classify a `write_frame` result against the intended frame length. A partial
/// write (`Ok(n)` with `n < intended`, i.e. the peer closed mid-frame) is a FAILURE
/// just like a hard error — the frame did not land in full.
fn delivered(res: std::io::Result<usize>, intended: usize) -> Delivery {
    match res {
        Ok(n) if n == intended => Delivery::Full,
        _ => Delivery::Failed,
    }
}

/// What [`seam_egress`] did with a mouse/wheel event, so `App::input` knows
/// whether the tracking-OFF local fallback (selection gesture / viewport scroll)
/// must still run. The byte-producing decision lives ENTIRELY in `seam_egress`;
/// the viewport/gesture/window side-effects stay in `App::input` (they need the
/// renderer/window the headless byte test does not have).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Egress {
    /// The event produced a tracking report (or had no local fallback): the seam
    /// is done with it. Carries whether the encoded bytes actually reached the PTY
    /// so the reply-bearing caller is told the truth (audit: no false OK).
    Reported(Delivery),
    /// Mouse tracking is OFF: `App::input` must run the local fallback (selection
    /// gesture for a button/move, viewport scroll of `wheel_lines` for a wheel).
    TrackingOff { wheel_lines: i32, wheel_up: bool },
}

/// THE source-blind byte-producing core of the seam (design A.2 / A.7). It is the
/// SOLE reader of `keyboard_mode()`/`mouse_tracking_enabled()` and the SOLE caller
/// of `encode_key_with_layout` / the `encode_mouse_*` family / `encode_committed_
/// text` / `format_paste` / the focus-report egress, reading the relevant mode
/// ONCE per event under a single `term_lock`, ending at `sink.write_frame`.
///
/// CRITICAL (the indistinguishability invariant): this function takes NO
/// [`Source`] — it is STRUCTURALLY impossible for it to branch on who produced
/// the event, so a Human and a Controller feeding the SAME `InputEvent` get
/// byte-identical output. The Tier-1 `bytes_human_eq_controller` test proves it,
/// and the `Buggy` mutant (which DOES take a source flag) proves the test has
/// teeth.
///
/// Only the byte-producing arms are handled here; the viewport/gesture/clipboard/
/// blink/snap/resize side-effects (which need the renderer + window + gesture
/// state) stay in `App::input`, which calls this and then runs those.
pub fn seam_egress(term: &Mutex<Terminal>, sink: &SinkWriter, ev: &InputEvent) -> Egress {
    match ev {
        InputEvent::Key {
            key,
            mods,
            base_layout,
            event_type,
        } => {
            let bytes = {
                let t = term_lock(term);
                let mode = t.keyboard_mode();
                aterm_types::keyboard::encode_key_with_layout(
                    key,
                    *mods,
                    mode,
                    *event_type,
                    *base_layout,
                )
            };
            let d = if bytes.is_empty() {
                Delivery::Full // faithful no-op (e.g. legacy release): nothing to deliver
            } else {
                delivered(sink.write_frame(&bytes), bytes.len())
            };
            Egress::Reported(d)
        }
        InputEvent::Text(text) => {
            let mut d = Delivery::Full;
            if !text.is_empty() {
                let out = {
                    let mode = term_lock(term).keyboard_mode();
                    crate::keymap::encode_committed_text(text, mode)
                };
                if !out.is_empty() {
                    d = delivered(sink.write_frame(&out), out.len());
                }
            }
            Egress::Reported(d)
        }
        InputEvent::MouseButton {
            button,
            pressed,
            row,
            col,
            mods,
            ..
        } => {
            let report = {
                let t = term_lock(term);
                if t.mouse_tracking_enabled() {
                    if *pressed {
                        Some(t.encode_mouse_press(button.code(), *col, *row, *mods))
                    } else {
                        Some(t.encode_mouse_release(button.code(), *col, *row, *mods))
                    }
                } else {
                    None
                }
            };
            match report {
                Some(bytes) => {
                    let d = match bytes {
                        Some(b) => delivered(sink.write_frame(&b), b.len()),
                        None => Delivery::Full,
                    };
                    Egress::Reported(d)
                }
                None => Egress::TrackingOff {
                    wheel_lines: 0,
                    wheel_up: false,
                },
            }
        }
        InputEvent::MouseMove {
            buttons,
            row,
            col,
            mods,
            ..
        } => {
            let report = {
                let t = term_lock(term);
                if t.mouse_tracking_enabled() {
                    Some(t.encode_mouse_motion(*buttons, *col, *row, *mods))
                } else {
                    None
                }
            };
            match report {
                Some(bytes) => {
                    let d = match bytes {
                        Some(b) => delivered(sink.write_frame(&b), b.len()),
                        None => Delivery::Full,
                    };
                    Egress::Reported(d)
                }
                None => Egress::TrackingOff {
                    wheel_lines: 0,
                    wheel_up: false,
                },
            }
        }
        InputEvent::Wheel {
            dir_up,
            lines,
            row,
            col,
            mods,
        } => {
            // The invariant lives HERE: clamp `lines` to >= 1 so a non-positive
            // count (a future verb/grammar bug) cannot silently emit zero reports
            // for one source and N for another. on_mouse_wheel already guarantees
            // >= 1; this makes it structural for every caller.
            let lines = (*lines).max(1);
            let report = {
                let t = term_lock(term);
                if t.mouse_tracking_enabled() {
                    Some(t.encode_mouse_wheel(*dir_up, *col, *row, *mods))
                } else {
                    None
                }
            };
            match report {
                Some(bytes) => {
                    // One report PER line (kills divergence e).
                    let mut d = Delivery::Full;
                    if let Some(b) = bytes {
                        for _ in 0..lines {
                            if delivered(sink.write_frame(&b), b.len()) == Delivery::Failed {
                                d = Delivery::Failed; // any short/failed report fails the lot
                            }
                        }
                    }
                    Egress::Reported(d)
                }
                None => Egress::TrackingOff {
                    wheel_lines: lines,
                    wheel_up: *dir_up,
                },
            }
        }
        InputEvent::Paste(text) => {
            let out = term_lock(term).format_paste(text);
            let d = if out.is_empty() {
                Delivery::Full
            } else {
                delivered(sink.write_frame(&out), out.len())
            };
            Egress::Reported(d)
        }
        InputEvent::Focus(focused) => {
            // SOLE focus-report egress: ESC[I / ESC[O under DEC 1004, byte-identical
            // to the engine's `encode_focus_state`.
            let mut d = Delivery::Full;
            if term_lock(term).focus_reporting_enabled() {
                let seq: &[u8] = if *focused { b"\x1b[I" } else { b"\x1b[O" };
                d = delivered(sink.write_frame(seq), seq.len());
            }
            Egress::Reported(d)
        }
        // ScrollView / Resize produce no PTY bytes here; `App::input` handles their
        // (viewport / geometry) side-effects directly.
        InputEvent::ScrollView(_) | InputEvent::Resize { .. } => Egress::Reported(Delivery::Full),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aterm_core::terminal::Terminal;
    use std::io::Read;
    use std::os::fd::FromRawFd;
    use std::sync::Arc;

    /// Drive ONE [`InputEvent`] through [`seam_egress`] against a pipe-backed
    /// `SinkWriter` (the `cmd_paste` paste-to-pipe pattern) and return the exact
    /// bytes that reached the "PTY".
    fn egress_bytes(term: &Mutex<Terminal>, ev: &InputEvent) -> Vec<u8> {
        let mut fds = [0i32; 2];
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        let sink = SinkWriter::new(fds[1]);
        seam_egress(term, &sink, ev);
        unsafe { libc::close(fds[1]) };
        let mut buf = Vec::new();
        let mut reader = unsafe { std::fs::File::from_raw_fd(fds[0]) };
        reader.read_to_end(&mut buf).expect("read pipe");
        buf
    }

    /// `delivered` classifies a `write_frame` result: a full write is `Full`; a
    /// short write (peer closed mid-frame, `Ok(n<intended)`) or a hard error is
    /// `Failed` — the property the false-OK fix rests on.
    #[test]
    fn delivered_classifies_short_and_failed_writes() {
        assert_eq!(delivered(Ok(5), 5), Delivery::Full);
        assert_eq!(delivered(Ok(0), 5), Delivery::Failed); // peer closed mid-frame
        assert_eq!(delivered(Ok(3), 5), Delivery::Failed); // short
        assert_eq!(
            delivered(Err(std::io::Error::from(std::io::ErrorKind::BrokenPipe)), 5),
            Delivery::Failed
        );
    }

    /// REPLY FIDELITY (audit Finding 1): when the PTY write FAILS, the seam reports
    /// `Delivery::Failed` (→ `InputOutcome::WriteFailed` → `ERR write failed`), NEVER
    /// a false OK. An invalid fd makes every write fail deterministically (EBADF, so
    /// no SIGPIPE). This is the input-seam conformance to the reply-fidelity property
    /// class: OK iff the bytes actually landed. A faithful empty encoding (a legacy
    /// key RELEASE — nothing to write) stays `Full`: there was nothing to lose.
    #[test]
    fn failed_pty_write_is_reported_not_falsely_ok() {
        use aterm_types::keyboard::{Key, KeyEventType, Modifiers, NamedKey};
        let term = term_with(&[]);
        let sink = SinkWriter::new(-1); // invalid fd -> every write_frame errors
        let press = InputEvent::Key {
            key: Key::Named(NamedKey::ArrowUp),
            mods: Modifiers::empty(),
            base_layout: None,
            event_type: KeyEventType::Press,
        };
        assert_eq!(
            seam_egress(&term, &sink, &press),
            Egress::Reported(Delivery::Failed)
        );

        let release = InputEvent::Key {
            key: Key::Named(NamedKey::ArrowUp),
            mods: Modifiers::empty(),
            base_layout: None,
            event_type: KeyEventType::Release, // legacy: encodes to nothing
        };
        assert_eq!(
            seam_egress(&term, &sink, &release),
            Egress::Reported(Delivery::Full)
        );
    }

    /// A `Terminal` with the given mode-enabling sequences fed in (DECCKM, Kitty,
    /// the mouse modes, focus reporting, bracketed paste).
    fn term_with(seqs: &[&[u8]]) -> Arc<Mutex<Terminal>> {
        let term = Arc::new(Mutex::new(Terminal::new(24, 80)));
        {
            let mut t = term.lock().unwrap();
            for s in seqs {
                t.process(s);
            }
        }
        term
    }

    /// THE Tier-1 indistinguishability invariant (A.7), part 1 — BYTE EQUALITY.
    ///
    /// For the SAME logical `InputEvent`, the bytes a Human source and a Controller
    /// source put on the wire are BYTE-IDENTICAL — across a matrix of keyboard
    /// modes x mouse-tracking modes x event kinds. `seam_egress` takes no `Source`
    /// (so this is enforced by construction); the assertion drives the SAME core
    /// both sources reach via `App::input`. The `Buggy` negative control below
    /// proves the assertion has teeth, and `builders_converge` (part 2) proves the
    /// two REAL builders feed `seam_egress` structurally-equal events for the same
    /// intent — so the chain Human-builder → seam == Controller-builder → seam is
    /// complete, not tautological.
    #[test]
    fn bytes_human_eq_controller() {
        use aterm_types::keyboard::{Key, Modifiers, NamedKey};
        use aterm_types::mouse::{CTRL_MASK, MouseButton, SHIFT_MASK};

        // Keyboard modes: legacy, DECCKM (app cursor keys), Kitty disambiguate +
        // REPORT_ALTERNATE_KEYS (proves base_layout flows identically — divergence
        // h). Mouse modes: off, Normal(1000), Button(1002), Any(1003) + SGR(1006).
        let kbd_modes: &[&[&[u8]]] = &[
            &[],            // legacy
            &[b"\x1b[?1h"], // DECCKM
            &[b"\x1b[>1u"], // Kitty disambiguate
            &[b"\x1b[>5u"], // Kitty disambiguate + report-alternate
        ];
        let mouse_modes: &[&[&[u8]]] = &[
            &[],                               // tracking off
            &[b"\x1b[?1000h", b"\x1b[?1006h"], // Normal + SGR
            &[b"\x1b[?1002h", b"\x1b[?1006h"], // ButtonEvent + SGR
            &[b"\x1b[?1003h", b"\x1b[?1006h"], // AnyEvent + SGR
        ];

        let events = vec![
            InputEvent::Key {
                key: Key::Character('a'),
                mods: Modifiers::CTRL | Modifiers::SHIFT,
                base_layout: Some('a'),
                event_type: aterm_types::keyboard::KeyEventType::Press,
            },
            InputEvent::Key {
                key: Key::Named(NamedKey::ArrowUp),
                mods: Modifiers::empty(),
                base_layout: None,
                event_type: aterm_types::keyboard::KeyEventType::Press,
            },
            InputEvent::Key {
                key: Key::Named(NamedKey::Enter),
                mods: Modifiers::empty(),
                base_layout: None,
                event_type: aterm_types::keyboard::KeyEventType::Press,
            },
            InputEvent::Text("héllo 日本".to_string()),
            InputEvent::MouseButton {
                button: MouseButton::Left,
                pressed: true,
                row: 5,
                col: 9,
                mods: SHIFT_MASK | CTRL_MASK,
                click_count: 2,
                side: SelectionSide::Right,
                block: true,
            },
            InputEvent::MouseButton {
                button: MouseButton::Right,
                pressed: false,
                row: 5,
                col: 9,
                mods: 0,
                click_count: 1,
                side: SelectionSide::Left,
                block: false,
            },
            InputEvent::MouseMove {
                buttons: 0,
                row: 7,
                col: 3,
                mods: 0,
                side: SelectionSide::Left,
            },
            InputEvent::MouseMove {
                buttons: 3,
                row: 7,
                col: 3,
                mods: 0,
                side: SelectionSide::Left,
            },
            InputEvent::Wheel {
                dir_up: true,
                lines: 3,
                row: 2,
                col: 4,
                mods: 0,
            },
            InputEvent::Wheel {
                dir_up: false,
                lines: 1,
                row: 2,
                col: 4,
                mods: 0,
            },
            InputEvent::Paste("rm -rf safe".to_string()),
            InputEvent::Focus(true),
            InputEvent::Focus(false),
        ];

        for kbd in kbd_modes {
            for mouse in mouse_modes {
                // Two INDEPENDENT terminals in the identical mode — one stands in for
                // the human-driven session, one for the controller-driven session.
                let mut seqs: Vec<&[u8]> = Vec::new();
                seqs.extend_from_slice(kbd);
                seqs.extend_from_slice(mouse);
                seqs.push(b"\x1b[?1004h"); // focus reporting on
                seqs.push(b"\x1b[?2004h"); // bracketed paste on
                let term_human = term_with(&seqs);
                let term_ctrl = term_with(&seqs);

                for ev in &events {
                    let human = egress_bytes(&term_human, ev);
                    let controller = egress_bytes(&term_ctrl, ev);
                    assert_eq!(
                        human, controller,
                        "bytes(Human) != bytes(Controller) for {ev:?} under kbd={kbd:?} mouse={mouse:?}"
                    );
                }
            }
        }
    }

    /// THE Tier-1 indistinguishability invariant (A.7), part 2 — BUILDER EQUALITY.
    ///
    /// The byte test (part 1) feeds one event to two terminals; this proves the
    /// event a Human builds equals the event a Controller builds for the same
    /// intent — closing the "but the builders could diverge" gap. We can't
    /// construct a winit `KeyEvent` (its `platform_specific` field is `pub(crate)`),
    /// so we drive the SAME primitives `keymap::build_key_input` uses
    /// (`aterm_types::keyboard::{map_logical_key, base_layout_key_for}`) as the
    /// human side, and the real control-verb parsers (`control::parse_key`,
    /// `parse_ctrl`, `parse_mouse`) as the controller side. For the named-key /
    /// ctrl-chord intents both sides land on the identical `InputEvent` — and then
    /// `seam_egress` gives identical bytes.
    #[test]
    fn builders_converge() {
        use aterm_types::keyboard::{
            Key, Modifiers, NamedKey, base_layout_key_for, map_logical_key,
        };
        use winit::keyboard::{Key as WinitKey, KeyCode, NamedKey as WinitNamed, PhysicalKey};

        // --- "press the Up arrow" --------------------------------------------
        // Human: build_key_input's pure decision on a winit ArrowUp event.
        let human_up = {
            let key = map_logical_key(&WinitKey::Named(WinitNamed::ArrowUp)).expect("up maps");
            let base = base_layout_key_for(PhysicalKey::Code(KeyCode::ArrowUp));
            InputEvent::Key {
                key,
                mods: Modifiers::empty(),
                base_layout: base,
                event_type: aterm_types::keyboard::KeyEventType::Press,
            }
        };
        // Controller: the real `key up` parser.
        let ctrl_up = crate::control::parse_key("up").expect("key up parses");
        assert_eq!(
            human_up, ctrl_up,
            "human `Up` builder != controller `key up` builder"
        );
        assert_eq!(
            human_up,
            InputEvent::Key {
                key: Key::Named(NamedKey::ArrowUp),
                mods: Modifiers::empty(),
                base_layout: None,
                event_type: aterm_types::keyboard::KeyEventType::Press
            },
        );

        // --- "Ctrl+C" --------------------------------------------------------
        // Human: build_key_input's decision on a winit 'c' event with CTRL.
        let human_ctrl_c = {
            let key = map_logical_key(&WinitKey::Character("c".into())).expect("c maps");
            let base = base_layout_key_for(PhysicalKey::Code(KeyCode::KeyC));
            InputEvent::Key {
                key,
                mods: Modifiers::CTRL,
                base_layout: base,
                event_type: aterm_types::keyboard::KeyEventType::Press,
            }
        };
        let ctrl_ctrl_c = crate::control::parse_ctrl("c").expect("ctrl c parses");
        // Both encode Ctrl+c; the human path carries the physical base_layout, the
        // controller carries None. They must produce IDENTICAL BYTES (base_layout
        // only adds the Kitty 3rd field, which a plain `c` does not change), so we
        // assert byte-equality through the seam, not struct-equality, here.
        for seqs in [&[][..], &[&b"\x1b[>1u"[..]][..]] {
            let term = term_with(seqs);
            assert_eq!(
                egress_bytes(&term, &human_ctrl_c),
                egress_bytes(&term, &ctrl_ctrl_c),
                "Ctrl+C bytes diverge (human base_layout vs controller None) under {seqs:?}",
            );
        }

        // --- "mouse press, shift, double-click, right side, block" -----------
        // Controller: the real `mouse press` parser with the full additive grammar.
        let ctrl_press =
            crate::control::parse_mouse("press left 5 9 mods=shift count=2 side=right block=1")
                .expect("mouse press parses");
        // Human-equivalent: the on_mouse_input builder fields. mods=SHIFT_MASK,
        // count=2 from the streak FSM, side=Right (cell half), block from alt held.
        let human_press = InputEvent::MouseButton {
            button: aterm_types::mouse::MouseButton::Left,
            pressed: true,
            row: 5,
            col: 9,
            mods: aterm_types::mouse::SHIFT_MASK,
            click_count: 2,
            side: SelectionSide::Right,
            block: true,
        };
        assert_eq!(human_press, ctrl_press, "mouse-press builder mismatch");

        // --- "wheel up, 1 notch" ---------------------------------------------
        let ctrl_wheel = crate::control::parse_mouse("wheelup left 2 4").expect("wheelup parses");
        let human_wheel = InputEvent::Wheel {
            dir_up: true,
            lines: 1,
            row: 2,
            col: 4,
            mods: 0,
        };
        assert_eq!(human_wheel, ctrl_wheel, "wheel builder mismatch");
    }

    /// NEGATIVE CONTROL: a `Buggy` egress that BRANCHES on the source (the exact
    /// thing the invariant forbids) MUST produce a counterexample — otherwise the
    /// byte test would pass even if someone reintroduced a source branch. Here the
    /// buggy variant drops the modifier bits for the controller, so a Ctrl+Shift
    /// chord diverges. This proves the test has teeth.
    #[test]
    fn buggy_source_branch_is_detectable() {
        use aterm_types::keyboard::{Key, Modifiers};

        fn buggy_key_bytes(
            term: &Mutex<Terminal>,
            ev: &InputEvent,
            is_controller: bool,
        ) -> Vec<u8> {
            let InputEvent::Key {
                key,
                mods,
                base_layout,
                event_type,
            } = ev
            else {
                return Vec::new();
            };
            // THE BUG: a behavioural branch on the source.
            let mods = if is_controller {
                Modifiers::empty()
            } else {
                *mods
            };
            let t = term_lock(term);
            aterm_types::keyboard::encode_key_with_layout(
                key,
                mods,
                t.keyboard_mode(),
                *event_type,
                *base_layout,
            )
        }

        let term = term_with(&[b"\x1b[>1u"]); // Kitty so modifiers are visible in CSI-u
        let ev = InputEvent::Key {
            key: Key::Character('a'),
            mods: Modifiers::CTRL | Modifiers::SHIFT,
            base_layout: Some('a'),
            event_type: aterm_types::keyboard::KeyEventType::Press,
        };
        let human = buggy_key_bytes(&term, &ev, false);
        let controller = buggy_key_bytes(&term, &ev, true);
        assert_ne!(
            human, controller,
            "the Buggy source-branch must be detectable (differing bytes), or the \
             indistinguishability test has no teeth"
        );
        // And the CORRECT, source-blind path agrees for the same event.
        assert_eq!(
            egress_bytes(&term, &ev),
            human,
            "source-blind == human path"
        );
    }

    /// The wheel-line clamp guards the human/controller asymmetry the critique
    /// flagged: a non-positive `lines` must NOT silently emit zero reports while a
    /// positive one emits N. With tracking ON, `lines: 0` and `lines: -3` both
    /// behave as exactly ONE report (the clamp to >= 1), identical to `lines: 1`.
    #[test]
    fn wheel_lines_clamped_to_one() {
        let term = term_with(&[b"\x1b[?1000h", b"\x1b[?1006h"]); // Normal + SGR tracking
        let one = egress_bytes(
            &term,
            &InputEvent::Wheel {
                dir_up: true,
                lines: 1,
                row: 2,
                col: 4,
                mods: 0,
            },
        );
        for bad in [0, -1, -3] {
            let got = egress_bytes(
                &term,
                &InputEvent::Wheel {
                    dir_up: true,
                    lines: bad,
                    row: 2,
                    col: 4,
                    mods: 0,
                },
            );
            assert_eq!(
                got, one,
                "wheel lines={bad} must clamp to exactly one report"
            );
        }
    }
}
