// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! Input-injection verbs + their parsers — the control protocol's "drive the
//! shell" surface: key/ctrl/send/feed/signal/mouse/paste/focus/resize/scroll/tab.
//! Moved verbatim from `control.rs` (behavior-preserving). The seam plumbing
//! (`post_input`/`post_input_reply`) and the cross-session `apply_scroll_intent`
//! cluster stay in `control.rs`; this module reaches them via `super::`.

use std::sync::{Arc, Mutex};

use aterm_core::grid::{MAX_GRID_COLS, MAX_GRID_ROWS};
use aterm_core::terminal::Terminal;
use aterm_session::Op;
use aterm_session::sink::SinkWriter;
use winit::event_loop::EventLoopProxy;

use super::{post_input, post_input_reply};
use crate::input::{InputEvent, InputOutcome, ScrollIntent};
use crate::{TabAction, Wake, term_lock};

/// `scroll <up|down|top|bottom|N>` -> move the scrollback viewport and report
/// the new position as `OK <display_offset> <scrollback_lines>\n`. `up`/`down`
/// move one screen into/out of history; `top`/`bottom` jump; a signed integer
/// `N` moves N lines into history (negative = toward the live bottom). With no
/// argument it just reports the current position. After moving it nudges a
/// windowed session to repaint (no-op when headless).
pub(crate) fn cmd_scroll(
    term: &Arc<Mutex<Terminal>>,
    proxy: &EventLoopProxy<Wake>,
    rest: &str,
) -> String {
    // Parse to a tracking-agnostic ScrollIntent; the SEAM is the sole
    // `scroll_display`/`scroll_to_*` caller. `""` (just report position) maps to a
    // zero-line `By(0)` so the round-trip still reports the current offset.
    let intent = match rest.trim() {
        "" => ScrollIntent::By(0),
        "top" => ScrollIntent::Top,
        "bottom" => ScrollIntent::Bottom,
        "up" => ScrollIntent::Up,
        "down" => ScrollIntent::Down,
        n => match n.parse::<i32>() {
            Ok(d) => ScrollIntent::By(d),
            Err(_) => return "ERR usage: scroll <up|down|top|bottom|N>\n".to_string(),
        },
    };
    // Reply-bearing: the reply is sent AFTER the seam applied the scroll on the
    // main thread, so the position read below is NOT racy with the apply.
    // `scroll` is read-side view control (display_offset only) — audit class ReadScreen.
    match post_input_reply(proxy, Op::ReadScreen, vec![InputEvent::ScrollView(intent)]) {
        Ok(_) => {}
        Err(e) => return e,
    }
    let t = term_lock(term);
    let offset = t.grid().display_offset();
    let max = t.grid().scrollback_lines();
    format!("OK {offset} {max}\n")
}

/// `send <text>` -> write `<text>` to the PTY. A trailing literal `\n` (a
/// backslash followed by `n`) becomes carriage-return 0x0d so commands run.
pub(crate) fn cmd_send(sink: &SinkWriter, rest: &str) -> String {
    let bytes: Vec<u8> = if let Some(head) = rest.strip_suffix("\\n") {
        let mut b = head.as_bytes().to_vec();
        b.push(0x0d);
        b
    } else {
        rest.as_bytes().to_vec()
    };
    write_pty(sink, &bytes);
    "OK\n".to_string()
}

/// Parse the optional trailing `mods=<list>` token (e.g. `mods=ctrl+shift`),
/// returning the modifier mask and the rest of the line with the token removed.
/// Additive: a verb line WITHOUT `mods=` parses to `Modifiers::empty()` and the
/// untouched line, so every existing caller stays byte-compatible.
pub(crate) fn take_mods(rest: &str) -> (aterm_types::keyboard::Modifiers, String) {
    use aterm_types::keyboard::Modifiers;
    let mut m = Modifiers::empty();
    let mut kept: Vec<&str> = Vec::new();
    for tok in rest.split_whitespace() {
        if let Some(list) = tok.strip_prefix("mods=") {
            for name in list.split(['+', ',']) {
                match name {
                    "shift" => m |= Modifiers::SHIFT,
                    "ctrl" | "control" => m |= Modifiers::CTRL,
                    "alt" | "option" => m |= Modifiers::ALT,
                    // `meta` is its OWN modifier (Kitty CSI-u bit 8), distinct from
                    // ALT — a controller can now drive a real Meta chord. Legacy /
                    // xterm encoders ignore META/HYPER so their bytes are unchanged;
                    // only the Kitty keyboard protocol gains the extra bit.
                    "meta" => m |= Modifiers::META,
                    "hyper" => m |= Modifiers::HYPER,
                    "super" | "cmd" | "command" => m |= Modifiers::SUPER,
                    _ => {}
                }
            }
        } else {
            kept.push(tok);
        }
    }
    (m, kept.join(" "))
}

/// Parse the optional trailing `type=<press|repeat|release>` token, returning the
/// event type (default `Press`) and the body with the token removed. ADDITIVE: a
/// line without `type=` yields `Press` and the untouched body. An unrecognized
/// value yields `None` so [`parse_key`] rejects the whole line rather than
/// silently defaulting. `down`/`up` are accepted aliases for `press`/`release`.
fn take_event_type(rest: &str) -> Option<(aterm_types::keyboard::KeyEventType, String)> {
    use aterm_types::keyboard::KeyEventType;
    let mut et = KeyEventType::Press;
    let mut kept: Vec<&str> = Vec::new();
    for tok in rest.split_whitespace() {
        if let Some(v) = tok.strip_prefix("type=") {
            et = match v {
                "press" | "down" => KeyEventType::Press,
                "repeat" => KeyEventType::Repeat,
                "release" | "up" => KeyEventType::Release,
                _ => return None,
            };
        } else {
            kept.push(tok);
        }
    }
    Some((et, kept.join(" ")))
}

/// Parse the optional trailing `base=<char>` token — the US-QWERTY base-layout
/// key fed to Kitty `REPORT_ALTERNATE_KEYS` (the 3rd CSI-u sub-field), so a
/// controller on a non-US layout can reproduce the byte a human on that layout
/// emits. ADDITIVE: no `base=` yields `None` (the existing behaviour). A `base=`
/// whose value is not exactly one char yields the parser `None`.
fn take_base_layout(rest: &str) -> Option<(Option<char>, String)> {
    let mut base: Option<char> = None;
    let mut kept: Vec<&str> = Vec::new();
    for tok in rest.split_whitespace() {
        if let Some(v) = tok.strip_prefix("base=") {
            let mut chars = v.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => base = Some(c),
                _ => return None,
            }
        } else {
            kept.push(tok);
        }
    }
    Some((base, kept.join(" ")))
}

/// Map a `key` verb wire token to a [`NamedKey`](aterm_types::keyboard::NamedKey),
/// or `None` if it is not a named key (the caller then tries a single character).
/// Covers the FULL `NamedKey` vocabulary the engine models — navigation, editing,
/// locks/system, F1–F35, numpad, modifier-side keys, and media/audio — so every
/// physical key a human can press is reachable by a controller. The original 25
/// tokens keep their exact spelling for byte-compatibility.
fn named_key_from_token(body: &str) -> Option<aterm_types::keyboard::NamedKey> {
    use aterm_types::keyboard::NamedKey as Nk;
    Some(match body {
        // --- original 25 (byte-identical spellings) ---
        "enter" => Nk::Enter,
        "tab" => Nk::Tab,
        "esc" | "escape" => Nk::Escape,
        "backspace" => Nk::Backspace,
        "delete" | "del" => Nk::Delete,
        "insert" | "ins" => Nk::Insert,
        "up" => Nk::ArrowUp,
        "down" => Nk::ArrowDown,
        "right" => Nk::ArrowRight,
        "left" => Nk::ArrowLeft,
        "home" => Nk::Home,
        "end" => Nk::End,
        "pageup" | "pgup" => Nk::PageUp,
        "pagedown" | "pgdn" => Nk::PageDown,
        "f1" => Nk::F1,
        "f2" => Nk::F2,
        "f3" => Nk::F3,
        "f4" => Nk::F4,
        "f5" => Nk::F5,
        "f6" => Nk::F6,
        "f7" => Nk::F7,
        "f8" => Nk::F8,
        "f9" => Nk::F9,
        "f10" => Nk::F10,
        "f11" => Nk::F11,
        "f12" => Nk::F12,
        // --- editing / system ---
        "space" => Nk::Space,
        "capslock" => Nk::CapsLock,
        "numlock" => Nk::NumLock,
        "scrolllock" => Nk::ScrollLock,
        "printscreen" | "prtsc" => Nk::PrintScreen,
        "pause" | "break" => Nk::Pause,
        "menu" | "contextmenu" => Nk::ContextMenu,
        // --- F13..F35 ---
        "f13" => Nk::F13,
        "f14" => Nk::F14,
        "f15" => Nk::F15,
        "f16" => Nk::F16,
        "f17" => Nk::F17,
        "f18" => Nk::F18,
        "f19" => Nk::F19,
        "f20" => Nk::F20,
        "f21" => Nk::F21,
        "f22" => Nk::F22,
        "f23" => Nk::F23,
        "f24" => Nk::F24,
        "f25" => Nk::F25,
        "f26" => Nk::F26,
        "f27" => Nk::F27,
        "f28" => Nk::F28,
        "f29" => Nk::F29,
        "f30" => Nk::F30,
        "f31" => Nk::F31,
        "f32" => Nk::F32,
        "f33" => Nk::F33,
        "f34" => Nk::F34,
        "f35" => Nk::F35,
        // --- numpad (kp* spellings) ---
        "kp0" => Nk::Numpad0,
        "kp1" => Nk::Numpad1,
        "kp2" => Nk::Numpad2,
        "kp3" => Nk::Numpad3,
        "kp4" => Nk::Numpad4,
        "kp5" => Nk::Numpad5,
        "kp6" => Nk::Numpad6,
        "kp7" => Nk::Numpad7,
        "kp8" => Nk::Numpad8,
        "kp9" => Nk::Numpad9,
        "kpdot" | "kpdecimal" => Nk::NumpadDecimal,
        "kpdiv" | "kpdivide" => Nk::NumpadDivide,
        "kpmul" | "kpmultiply" => Nk::NumpadMultiply,
        "kpsub" | "kpminus" => Nk::NumpadSubtract,
        "kpadd" | "kpplus" => Nk::NumpadAdd,
        "kpenter" => Nk::NumpadEnter,
        "kpequal" => Nk::NumpadEqual,
        "kpsep" | "kpseparator" => Nk::NumpadSeparator,
        "kpbegin" => Nk::NumpadBegin,
        "kpleft" => Nk::NumpadArrowLeft,
        "kpright" => Nk::NumpadArrowRight,
        "kpup" => Nk::NumpadArrowUp,
        "kpdown" => Nk::NumpadArrowDown,
        "kppageup" | "kppgup" => Nk::NumpadPageUp,
        "kppagedown" | "kppgdn" => Nk::NumpadPageDown,
        "kphome" => Nk::NumpadHome,
        "kpend" => Nk::NumpadEnd,
        "kpinsert" | "kpins" => Nk::NumpadInsert,
        "kpdelete" | "kpdel" => Nk::NumpadDelete,
        // --- modifier-side keys (reported as key events under Kitty) ---
        "shiftleft" => Nk::ShiftLeft,
        "shiftright" => Nk::ShiftRight,
        "ctrlleft" | "controlleft" => Nk::ControlLeft,
        "ctrlright" | "controlright" => Nk::ControlRight,
        "altleft" => Nk::AltLeft,
        "altright" => Nk::AltRight,
        "superleft" => Nk::SuperLeft,
        "superright" => Nk::SuperRight,
        "hyperleft" => Nk::HyperLeft,
        "hyperright" => Nk::HyperRight,
        "metaleft" => Nk::MetaLeft,
        "metaright" => Nk::MetaRight,
        // --- media / audio ---
        "mediaplay" => Nk::MediaPlay,
        "mediapause" => Nk::MediaPause,
        "mediaplaypause" => Nk::MediaPlayPause,
        "mediastop" => Nk::MediaStop,
        "mediareverse" => Nk::MediaReverse,
        "mediafastforward" | "mediaff" => Nk::MediaFastForward,
        "mediarewind" => Nk::MediaRewind,
        "medianext" | "mediatracknext" => Nk::MediaTrackNext,
        "mediaprev" | "mediatrackprevious" => Nk::MediaTrackPrevious,
        "mediarecord" => Nk::MediaRecord,
        "volumeup" => Nk::AudioVolumeUp,
        "volumedown" => Nk::AudioVolumeDown,
        "mute" => Nk::AudioVolumeMute,
        _ => return None,
    })
}

/// PURE parser for `key <name> [mods=<list>] [type=<t>] [base=<c>]` -> an
/// [`InputEvent::Key`]. Factored out of [`cmd_key`] so the additive grammar is
/// unit-testable WITHOUT an `EventLoopProxy` (the verb can't run headless — it
/// posts a `Wake::Input`). The SAME (Key, mods, base_layout, event_type) tuple a
/// human's named-key press builds, so the seam (the sole encoder caller) yields
/// byte-identical output incl. Kitty REPORT_ALTERNATE_KEYS. All trailing tokens
/// are ADDITIVE — a bare `key up` still parses to empty mods / Press / no base.
/// Returns `None` for an unknown key name or a malformed `type=`/`base=` value.
pub(crate) fn parse_key(rest: &str) -> Option<InputEvent> {
    use aterm_types::keyboard::Key;
    let (mut mods, body) = take_mods(rest);
    let (event_type, body) = take_event_type(&body)?;
    let (base_explicit, body) = take_base_layout(&body)?;
    // Inline modifier prefixes: `ctrl+u`, `alt+x`, `ctrl+shift+a`, ... The
    // prefixes are ADDITIVE with any trailing `mods=` token, so `ctrl+u` and
    // `u mods=ctrl` agree. After stripping them, a single residual character
    // (e.g. `u`) becomes a `Key::Character` event — the SAME (Key, mods) seam
    // `parse_ctrl` builds, so the encoder derives the control byte itself
    // (`ctrl+u` -> 0x15) rather than us hand-rolling it.
    let (prefix_mods, body) = take_inline_mods(body.trim());
    mods |= prefix_mods;
    let body = body.trim();
    let Some(named) = named_key_from_token(body) else {
        // Not a named key: a single residual character (after stripping inline
        // modifier prefixes) becomes a `Key::Character`. `ctrl+u` lands here as
        // `u` + CTRL, byte-identical to `parse_ctrl("u")` -> the encoder emits
        // 0x15. Lower-cased so `ctrl+U` == `ctrl+u`, matching `parse_ctrl`.
        let mut chars = body.chars();
        return match (chars.next(), chars.next()) {
            (Some(c), None) => Some(InputEvent::Key {
                key: Key::Character(c.to_ascii_lowercase()),
                mods,
                base_layout: base_explicit,
                event_type,
            }),
            _ => None,
        };
    };
    Some(InputEvent::Key {
        key: Key::Named(named),
        mods,
        base_layout: base_explicit,
        event_type,
    })
}

/// Strip leading inline modifier prefixes (`ctrl+`, `alt+`, `shift+`, `super+`
/// and their aliases) from a `key` body, returning the accumulated modifier
/// mask and the remaining body. Mirrors the `mods=` alias table in
/// [`take_mods`] so `ctrl+u` and `u mods=ctrl` are equivalent. Only consumes a
/// prefix when a `+` follows a recognized modifier name, so a bare named key
/// like `up` (no `+`) is returned untouched.
fn take_inline_mods(body: &str) -> (aterm_types::keyboard::Modifiers, &str) {
    use aterm_types::keyboard::Modifiers;
    let mut m = Modifiers::empty();
    let mut rest = body;
    while let Some(plus) = rest.find('+') {
        let bit = match &rest[..plus] {
            "shift" => Modifiers::SHIFT,
            "ctrl" | "control" => Modifiers::CTRL,
            "alt" | "option" => Modifiers::ALT,
            // `meta`/`hyper` are their own bits (see `take_mods`).
            "meta" => Modifiers::META,
            "hyper" => Modifiers::HYPER,
            "super" | "cmd" | "command" => Modifiers::SUPER,
            _ => break,
        };
        m |= bit;
        rest = &rest[plus + 1..];
    }
    (m, rest)
}

/// `key <name> [mods=<list>]` -> build an [`InputEvent::Key`] and post it to the
/// seam (the SOLE encoder caller, under the CURRENT keyboard mode). See
/// [`parse_key`] for the grammar.
pub(crate) fn cmd_key(proxy: &EventLoopProxy<Wake>, rest: &str) -> String {
    match parse_key(rest) {
        // Reply-bearing: OK means the seam APPLIED the event (bytes written),
        // not merely that it was enqueued. With no frontmost window the seam
        // drops the reply sender, so the caller gets ERR rather than a false OK.
        Some(ev) => input_reply_to_str(post_input_reply(proxy, Op::WriteInput, vec![ev])),
        None => "ERR\n".to_string(),
    }
}

/// Map a reply-bearing input outcome to a verb reply line. `Ok` (applied) and
/// `RangeRejected` (out-of-range geometry — not relevant to key/mouse/paste, but
/// handled for completeness) become OK / ERR; an `Err` (event loop closed / no
/// window) is already a full `ERR …\n` string.
fn input_reply_to_str(r: Result<InputOutcome, String>) -> String {
    match r {
        Ok(InputOutcome::Ok) => "OK\n".to_string(),
        Ok(InputOutcome::RangeRejected) => "ERR out of range\n".to_string(),
        Ok(InputOutcome::WriteFailed) => "ERR write failed\n".to_string(),
        Err(e) => e,
    }
}

/// PURE parser for `ctrl <letter>` -> a Control-modified character key. Factored
/// out of [`cmd_ctrl`] for headless unit-testing. The seam encodes it (under the
/// CURRENT keyboard mode) as a proper CSI-u sequence (Kitty/xterm) or the legacy
/// control byte, byte-identical to a human Ctrl chord. Returns `None` unless
/// `rest` is exactly one (non-whitespace) char.
pub(crate) fn parse_ctrl(rest: &str) -> Option<InputEvent> {
    use aterm_types::keyboard::{Key, Modifiers};
    let mut chars = rest.trim().chars();
    let (Some(c), None) = (chars.next(), chars.next()) else {
        return None;
    };
    Some(InputEvent::Key {
        key: Key::Character(c.to_ascii_lowercase()),
        mods: Modifiers::CTRL,
        base_layout: None,
        event_type: aterm_types::keyboard::KeyEventType::Press,
    })
}

/// `ctrl <letter>` -> a Control-modified character key posted to the seam. See
/// [`parse_ctrl`].
pub(crate) fn cmd_ctrl(proxy: &EventLoopProxy<Wake>, rest: &str) -> String {
    match parse_ctrl(rest) {
        Some(ev) => post_input(proxy, Op::WriteInput, vec![ev]),
        None => "ERR usage: ctrl <single-letter>\n".to_string(),
    }
}

/// `feed <hex>` -> write raw bytes (decoded from a hex string, whitespace
/// allowed) straight to the PTY. The escape hatch for control/binary bytes the
/// line-delimited `send` verb can't carry: `feed 03` = Ctrl-C, `feed 1b5b41` =
/// ESC[A, `feed 0a` = a real newline. Replies `OK <n> bytes\n` or `ERR bad hex`.
pub(crate) fn cmd_feed(sink: &SinkWriter, rest: &str) -> String {
    let hex: String = rest.chars().filter(|c| !c.is_whitespace()).collect();
    if !hex.len().is_multiple_of(2) {
        return "ERR bad hex (odd length)\n".to_string();
    }
    let mut bytes = Vec::with_capacity(hex.len() / 2);
    let h = hex.as_bytes();
    let mut i = 0;
    while i < h.len() {
        let hi = (h[i] as char).to_digit(16);
        let lo = (h[i + 1] as char).to_digit(16);
        let (Some(hi), Some(lo)) = (hi, lo) else {
            return "ERR bad hex\n".to_string();
        };
        bytes.push((hi * 16 + lo) as u8);
        i += 2;
    }
    let n = bytes.len();
    write_pty(sink, &bytes);
    format!("OK {n} bytes\n")
}

/// `signal <name>` -> deliver a job-control signal to the PTY's CURRENT
/// foreground process group (via `tcgetpgrp` on the master + `killpg`).
/// `name` is one of `int`/`c`, `quit`, `tstp`/`z`, `hup`, `term`, `kill`.
/// This makes Ctrl-C/Ctrl-\\/Ctrl-Z effects deliverable and testable regardless
/// of the line discipline / launch context (which may not generate them).
pub(crate) fn cmd_signal(master: i32, rest: &str) -> String {
    let sig = match rest.trim() {
        "int" | "c" | "sigint" => libc::SIGINT,
        "quit" | "sigquit" => libc::SIGQUIT,
        "tstp" | "z" | "sigtstp" => libc::SIGTSTP,
        "hup" | "sighup" => libc::SIGHUP,
        "term" | "sigterm" => libc::SIGTERM,
        "kill" | "sigkill" => libc::SIGKILL,
        other => return format!("ERR unknown signal: {other}\n"),
    };
    let pgrp = unsafe { libc::tcgetpgrp(master) };
    if pgrp <= 0 {
        return "ERR no foreground process group\n".to_string();
    }
    let rc = unsafe { libc::killpg(pgrp, sig) };
    if rc == 0 {
        format!("OK signalled pgrp {pgrp}\n")
    } else {
        "ERR killpg failed\n".to_string()
    }
}

const MOUSE_USAGE: &str = "ERR usage: mouse <press|release|move|wheelup|wheeldown> <left|middle|right> <row> <col> [mods=..] [count=N] [side=left|right] [block=0|1]\n";

/// PURE parser for the `mouse` verb -> an engine-neutral mouse [`InputEvent`].
/// Factored out of [`cmd_mouse`] so the additive `mods=`/`count=`/`side=`/`block=`
/// grammar is unit-testable without an `EventLoopProxy`. Returns `Err(usage/err
/// string)` for a malformed line, `Ok(event)` otherwise.
///
/// Grammar: `mouse <action> <button> <row> <col> [mods=..] [count=N]
/// [side=left|right] [block=0|1]`. `action` is `press|release|move|wheelup|
/// wheeldown`; `button` is `left|middle|right` (ignored for the wheel actions);
/// `row`/`col` are 0-based. The additive tokens carry the data that closes the
/// human/controller divergences: `mods=` the real modifier mask (kills a),
/// `count=` the click depth 1..=3 (kills b), `side=` the cell-half (kills i),
/// `block=1` the rectangular-selection intent for a single-click press (the same
/// intent a human encodes from a held Alt, carried as DATA so the seam never
/// reads ambient modifier state).
pub(crate) fn parse_mouse(rest: &str) -> Result<InputEvent, String> {
    use aterm_core::selection::SelectionSide;
    use aterm_types::mouse::{ALT_MASK, CTRL_MASK, MouseButton, SHIFT_MASK};
    let mut action = "";
    let mut mods: u8 = 0;
    let mut click_count: u8 = 1;
    let mut side = SelectionSide::Left;
    let mut block = false;
    // POSITIONAL tokens (in order), interpreted per-action below: this keeps
    // press/release/wheel as `<button> <row> <col>` (byte-compatible with the
    // pre-Phase-0.5 grammar) AND lets `move` be EITHER `<row> <col>` (no-button
    // hover, code 3) OR `<button> <row> <col>` (held-button drag).
    let mut positional: Vec<&str> = Vec::new();
    for tok in rest.split_whitespace() {
        if let Some(list) = tok.strip_prefix("mods=") {
            for name in list.split(['+', ',']) {
                match name {
                    "shift" => mods |= SHIFT_MASK,
                    "alt" | "option" | "meta" => mods |= ALT_MASK,
                    "ctrl" | "control" => mods |= CTRL_MASK,
                    _ => {}
                }
            }
        } else if let Some(c) = tok.strip_prefix("count=") {
            click_count = c.parse::<u8>().unwrap_or(1).clamp(1, 3);
        } else if let Some(s) = tok.strip_prefix("side=") {
            side = if s == "right" {
                SelectionSide::Right
            } else {
                SelectionSide::Left
            };
        } else if let Some(b) = tok.strip_prefix("block=") {
            block = matches!(b, "1" | "true" | "yes" | "block");
        } else if action.is_empty() {
            action = tok;
        } else {
            positional.push(tok);
        }
    }
    let parse_btn = |s: &str| -> Result<MouseButton, String> {
        match s {
            "left" => Ok(MouseButton::Left),
            "middle" => Ok(MouseButton::Middle),
            "right" => Ok(MouseButton::Right),
            _ => Err("ERR bad button\n".to_string()),
        }
    };
    let parse_rc = |r: &str, c: &str| -> Result<(u16, u16), String> {
        match (r.parse::<u16>(), c.parse::<u16>()) {
            (Ok(r), Ok(c)) => Ok((r, c)),
            _ => Err("ERR bad args\n".to_string()),
        }
    };
    let ev = match action {
        // `move` with two positionals = no-button hover (code 3); with three =
        // `<button> <row> <col>` held-button drag (kills divergence c at the verb).
        "move" => match positional.as_slice() {
            [r, c] => {
                let (row, col) = parse_rc(r, c)?;
                InputEvent::MouseMove {
                    buttons: 3,
                    row,
                    col,
                    mods,
                    side,
                }
            }
            [b, r, c] => {
                let button = parse_btn(b)?;
                let (row, col) = parse_rc(r, c)?;
                InputEvent::MouseMove {
                    buttons: button.code(),
                    row,
                    col,
                    mods,
                    side,
                }
            }
            _ => return Err(MOUSE_USAGE.to_string()),
        },
        "press" | "release" | "wheelup" | "wheeldown" => {
            let [b, r, c] = positional.as_slice() else {
                return Err(MOUSE_USAGE.to_string());
            };
            // `button` is ignored for the wheel actions but still required as a
            // positional (byte-compatible with the old `<button> <row> <col>` form).
            let button = parse_btn(b)?;
            let (row, col) = parse_rc(r, c)?;
            match action {
                "press" => InputEvent::MouseButton {
                    button,
                    pressed: true,
                    row,
                    col,
                    mods,
                    click_count,
                    side,
                    block,
                },
                "release" => InputEvent::MouseButton {
                    button,
                    pressed: false,
                    row,
                    col,
                    mods,
                    click_count,
                    side,
                    block,
                },
                "wheelup" => InputEvent::Wheel {
                    dir_up: true,
                    lines: 1,
                    row,
                    col,
                    mods,
                },
                _ => InputEvent::Wheel {
                    dir_up: false,
                    lines: 1,
                    row,
                    col,
                    mods,
                },
            }
        }
        _ => return Err("ERR bad action\n".to_string()),
    };
    Ok(ev)
}

/// `mouse <action> <button> <row> <col> [mods=..] [count=N] [side=left|right]
/// [block=0|1]` -> BUILD an engine-neutral mouse [`InputEvent`] (via [`parse_mouse`])
/// and post it to the seam, which reads the CURRENT mouse mode ONCE and either
/// emits the `encode_mouse_*` report (tracking ON) or runs the local selection
/// gesture (tracking OFF).
///
/// Phase 0.5 CONTRACT CHANGE (divergences a/b/d/i): the old `OK (mouse off)`
/// short-circuit is GONE — a tracking-OFF press/release now runs the SAME
/// selection machinery as the human (not a no-op), and `mods`/`count`/`side`/
/// `block` are carried as data instead of hard-coded. The verb returns `OK\n`
/// (fire-and-forget) once the batch is posted.
///
/// DRAG CONVERGENCE (divergence c) — SCOPE: one `mouse move` verb line posts ONE
/// `MouseMove`, so a controller that wants intermediate motion reports under a
/// tracking app issues a `press` then N `move`s then a `release` as separate verb
/// lines (the seam reports each, identical to the human's per-pixel `MouseMove`s).
/// A single-line `press→N×move→release` BATCH grammar is deliberately deferred —
/// the seam already supports a batched `Wake::Input` (A.2.3), so it is additive
/// and out of scope for this convergence commit.
pub(crate) fn cmd_mouse(proxy: &EventLoopProxy<Wake>, rest: &str) -> String {
    match parse_mouse(rest) {
        // Reply-bearing: OK means the seam ran (report emitted or local fallback
        // applied), not merely enqueued. No window ⇒ ERR, not a false OK.
        Ok(ev) => input_reply_to_str(post_input_reply(proxy, Op::WriteInput, vec![ev])),
        Err(e) => e,
    }
}

/// `paste <text>` -> write `<text>` to the PTY exactly as if the user pasted
/// it: [`Terminal::format_paste`] strips control bytes that could escape the
/// guards (ESC, C1 controls), converts line breaks to CR, and wraps the body
/// in the bracketed-paste guards `ESC[200~` ... `ESC[201~` when the app has
/// enabled bracketed paste (DECSET 2004). The text is the rest of the line
/// taken literally; a literal trailing `\n` (backslash + n) becomes a line
/// break (sent as CR, like a real paste) so a paste can end in one. For raw
/// unsanitized bytes use `feed`/`send` instead.
pub(crate) fn cmd_paste(proxy: &EventLoopProxy<Wake>, rest: &str) -> String {
    // The seam runs `format_paste` (bracketing + sanitize) under the lock and the
    // snap-to-bottom, converging with the human Cmd-V path. Reply-bearing so OK
    // means the paste reached the PTY (no window ⇒ ERR, not a false OK).
    input_reply_to_str(post_input_reply(
        proxy,
        Op::WriteInput,
        vec![InputEvent::Paste(paste_text(rest))],
    ))
}

/// The `paste` verb's text transform: a literal trailing `\n` (backslash + n)
/// becomes a real line break (sent as CR by `format_paste`). Pure, so the
/// bracketing/sanitize bytes stay unit-testable without an event loop.
pub(crate) fn paste_text(rest: &str) -> String {
    match rest.strip_suffix("\\n") {
        Some(head) => format!("{head}\n"),
        None => rest.to_string(),
    }
}

/// `focus <in|out>` -> drive DEC 1004 focus reporting (kills divergence j: a
/// controller-only session can now satisfy a focus-tracking app's oracle). The
/// seam emits ESC[I / ESC[O when the app enabled focus reporting, byte-identical
/// to the human `on_focus` egress. `in`/`1`/`true` = focus-in.
pub(crate) fn cmd_focus(proxy: &EventLoopProxy<Wake>, rest: &str) -> String {
    match parse_focus(rest) {
        Some(focused) => post_input(proxy, Op::WriteInput, vec![InputEvent::Focus(focused)]),
        None => "ERR usage: focus <in|out>\n".to_string(),
    }
}

/// PURE parser for the `focus` verb's `in/out` argument, factored out of
/// [`cmd_focus`] so the self (active-tab) and cross-session paths build the SAME
/// [`InputEvent::Focus`] from the SAME grammar. `in`/`1`/`true`/`focus` => focus-in.
pub(crate) fn parse_focus(rest: &str) -> Option<bool> {
    match rest.trim() {
        "in" | "1" | "true" | "focus" => Some(true),
        "out" | "0" | "false" | "blur" => Some(false),
        _ => None,
    }
}

/// Parse a `tab <arg>` request into the [`TabAction`] it drives (the PURE part, so
/// it is unit-testable without an event loop). Grammar: `new` opens a tab, a
/// 0-based integer `<N>` selects tab N, `next`/`prev` cycle. `None` (an unknown /
/// missing arg) maps the caller to the usage error.
pub(crate) fn parse_tab(rest: &str) -> Option<TabAction> {
    let rest = rest.trim();
    // Multi-word forms first: `close [N]` and `move <from> <to>`.
    let mut it = rest.split_whitespace();
    match it.next() {
        Some("new") if it.next().is_none() => return Some(TabAction::New),
        Some("next") if it.next().is_none() => return Some(TabAction::Next),
        Some("prev") if it.next().is_none() => return Some(TabAction::Prev),
        // `close` (active tab) or `close <N>` (a specific tab).
        Some("close") => {
            return match it.next() {
                None => Some(TabAction::Close(None)),
                Some(n) => {
                    let i = n.parse::<usize>().ok()?;
                    // Reject trailing junk after the index.
                    it.next().is_none().then_some(TabAction::Close(Some(i)))
                }
            };
        }
        // `move <from> <to>` — reorder.
        Some("move") => {
            let (from, to) = (it.next()?, it.next()?);
            if it.next().is_some() {
                return None; // trailing junk
            }
            let from = from.parse::<usize>().ok()?;
            let to = to.parse::<usize>().ok()?;
            return Some(TabAction::Move { from, to });
        }
        _ => {}
    }
    // Otherwise a bare 0-based index selects a tab.
    rest.parse::<usize>().ok().map(TabAction::Select)
}

/// `tab new | <N> | next | prev` -> DRIVE the FRONT window's native tabs and reply
/// `OK <active_index> <tab_count>`.
///
/// MAIN-THREAD HOP (mirrors [`cmd_chrome`]): mutating `App` (its tabs) may ONLY
/// happen on the event loop, but this runs on a background control thread. So we
/// parse the action, post [`Wake::TabCmd`] with a one-shot reply channel, and BLOCK
/// on the reply; the main thread resolves `self.frontmost_window`, applies the
/// action via the SAME command paths the keyboard/menu use (`open_tab` / `switch_tab`
/// / `cycle_tab`), and sends back the resulting `(active, count)`. `new` reuses the
/// new-tab path; the native toolbar segments then re-track via `App::sync_window`.
pub(crate) fn cmd_tab(proxy: &EventLoopProxy<Wake>, rest: &str) -> String {
    let Some(action) = parse_tab(rest) else {
        return "ERR usage: tab <new|N|next|prev|close [N]|move <from> <to>>\n".to_string();
    };
    let (tx, rx) = std::sync::mpsc::channel();
    if proxy
        .send_event(Wake::TabCmd { action, reply: tx })
        .is_err()
    {
        return "ERR event loop gone\n".to_string();
    }
    match rx.recv() {
        Ok((active, count)) => format!("OK {active} {count}\n"),
        Err(_) => "ERR tab command failed\n".to_string(),
    }
}

/// Parse + range-check a `resize <r> <c>` request (the PURE part, so it is unit
/// testable without an event loop). Returns the validated `(rows, cols)` or the
/// exact error string the verb replies with.
///
/// Requests outside `1..=MAX_GRID_ROWS`/`MAX_GRID_COLS` are rejected with
/// `ERR out of range` rather than silently clamped, so a caller learns its
/// requested size was not applied.
pub(crate) fn parse_resize(rest: &str) -> Result<(u16, u16), String> {
    let mut it = rest.split_whitespace();
    let (Some(rs), Some(cs)) = (it.next(), it.next()) else {
        return Err("ERR usage: resize <r> <c>\n".to_string());
    };
    let (Ok(r), Ok(c)) = (rs.parse::<u16>(), cs.parse::<u16>()) else {
        return Err("ERR bad args\n".to_string());
    };
    if !(1..=MAX_GRID_ROWS).contains(&r) || !(1..=MAX_GRID_COLS).contains(&c) {
        return Err("ERR out of range\n".to_string());
    }
    Ok((r, c))
}

/// `resize <r> <c>` -> resize the engine grid, the PTY, AND the GUI (RES-1).
///
/// The main thread is the SOLE geometry owner (`App.rows/cols`, the framebuffer,
/// the window). Resizing the term + PTY here directly — as the verb used to —
/// left `App` stale and sent no repaint, so a follow-up `image`/`dims` (which
/// read `App`/the framebuffer) disagreed with the engine. So the verb now ONLY
/// validates and forwards an `InputEvent::Resize` (in a `Wake::Input`) to the main
/// thread, which applies the term + PTY + window resize and requests a redraw in
/// one owner. A dropped proxy (event loop gone) means the GUI is shutting down:
/// report it.
///
/// RES-1: the verb sets `echo_to_window: true` so the seam ALSO asks the window to
/// match the new grid pixel size (the verb has no window event of its own). The
/// interactive winit `Resized` path uses `echo_to_window: false` (the window is
/// already that size). `echo_to_window` is a transport flag, NOT a `Source` branch.
pub(crate) fn cmd_resize(proxy: &EventLoopProxy<Wake>, rest: &str) -> String {
    // Range-check up front (keeps the precise `ERR out of range` / usage strings),
    // then post a reply-bearing Resize through the seam. The seam re-clamps and
    // reports `RangeRejected` if somehow out of range — but a valid request here
    // returns `Ok`, so the contract is unchanged for existing callers.
    let (r, c) = match parse_resize(rest) {
        Ok(rc) => rc,
        Err(e) => return e,
    };
    match post_input_reply(
        proxy,
        Op::WriteInput,
        vec![InputEvent::Resize {
            rows: r,
            cols: c,
            echo_to_window: true,
        }],
    ) {
        Ok(InputOutcome::RangeRejected) => "ERR out of range\n".to_string(),
        Ok(_) => "OK\n".to_string(),
        Err(e) => e,
    }
}

/// Funnel all control-verb bytes through the active session's single SinkWriter
/// (whole-frame atomicity vs the GUI keyboard path + reader-thread replies). Drops
/// a closed-peer error like the legacy writer did. Used ONLY by the audited raw
/// hatch (`send`/`feed`); the human-vocabulary verbs go through the seam instead.
pub(crate) fn write_pty(sink: &SinkWriter, data: &[u8]) {
    let _ = sink.write_frame(data);
}
