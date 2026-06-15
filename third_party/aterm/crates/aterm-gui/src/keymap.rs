// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! Pure keyboard-event → PTY-bytes decision (K-1).
//!
//! The GUI used to hand-roll character encoding in `on_key`: `(c.to_uppercase()
//! as u8) & 0x1f` for Ctrl and a raw `ev.text` write otherwise. That bypassed
//! the engine's protocol, so Alt/Option lost its ESC prefix, Ctrl on non-alpha
//! keys and the Kitty char encoding (CSI-u, alternate/base-layout keys) all
//! diverged from `aterm_types::keyboard::encode_key`. This module routes EVERY
//! key through the engine encoder and exposes the decision as a PURE function so
//! it is unit-testable without a window or event loop.
//!
//! The winit→engine key map (Character / Named / numpad / base-layout) lives in
//! `aterm_types::keyboard` (the `winit-keymap` feature, K-2) so the future
//! native shell reuses the same table.

use aterm_types::keyboard::{
    self, KeyEventType, KeyboardMode, Modifiers,
};
use winit::event::KeyEvent;
use winit::keyboard::{Key as WinitKey, ModifiersState, PhysicalKey};

/// Translate winit's [`ModifiersState`] into the engine's [`Modifiers`].
///
/// Super/Cmd is carried through (the old inline path dropped it); the engine's
/// encoder uses it for the Kitty/xterm modifier value and for `modifyOtherKeys`.
#[must_use]
pub fn modifiers_from_winit(mods: ModifiersState) -> Modifiers {
    let mut out = Modifiers::empty();
    if mods.shift_key() {
        out |= Modifiers::SHIFT;
    }
    if mods.control_key() {
        out |= Modifiers::CTRL;
    }
    if mods.alt_key() {
        out |= Modifiers::ALT;
    }
    if mods.super_key() {
        out |= Modifiers::SUPER;
    }
    out
}

/// The PURE key-encoding decision (K-1): given a winit key event, the live
/// modifiers, and the terminal's current [`KeyboardMode`], return the bytes to
/// write to the PTY — or `None` when the event maps to no terminal sequence
/// (an unencodable key, or a bare modifier press).
///
/// All encoding is delegated to `aterm_types::keyboard::encode_key_with_layout`,
/// so Ctrl/Alt/Shift, the legacy vs Kitty vs `modifyOtherKeys` selection, and
/// the alternate/base-layout key reporting are exactly the engine's protocol —
/// no `& 0x1f`, no raw-text passthrough. The `base_layout_key` (US-QWERTY
/// equivalent of the physical key) is supplied for the Kitty
/// `REPORT_ALTERNATE_KEYS` enhancement.
///
/// `logical_key` is the key to encode: pass `key_without_modifiers()` so a
/// composed character (Option+a → "å") is NOT what gets encoded — Alt must
/// produce the ESC-prefixed base key, not the composed glyph.
#[must_use]
pub fn encode_key_event(
    logical_key: &WinitKey,
    physical_key: PhysicalKey,
    mods: Modifiers,
    mode: KeyboardMode,
) -> Option<Vec<u8>> {
    let key = keyboard::map_logical_key(logical_key)?;
    // base_layout_key only matters for Character keys under REPORT_ALTERNATE_KEYS;
    // the engine ignores it otherwise, so deriving it unconditionally is harmless.
    let base_layout = keyboard::base_layout_key_for(physical_key);
    let bytes = keyboard::encode_key_with_layout(&key, mods, mode, KeyEventType::Press, base_layout);
    if bytes.is_empty() {
        None
    } else {
        Some(bytes)
    }
}

/// IME-1: whether a direct key send must be SUPPRESSED because an IME
/// composition (CJK / dead key) is currently active.
///
/// `preedit` is the marked text being composed; a non-empty preedit means the
/// keystrokes belong to the composer and the resulting text will arrive via
/// `Ime::Commit` — sending them directly too would double-input. When the
/// preedit is empty (no composition), ASCII typing proceeds normally.
#[must_use]
pub fn suppress_direct_send(preedit: &str) -> bool {
    !preedit.is_empty()
}

/// IME-1: encode committed composition text (`Ime::Commit`) for the PTY.
///
/// Each character is encoded as a `Character` key through the engine (NOT a raw
/// `& 0x1f` byte), so committed CJK/dead-key text goes out exactly as typed
/// text. Returns the concatenated bytes (empty for empty input).
#[must_use]
pub fn encode_committed_text(text: &str, mode: KeyboardMode) -> Vec<u8> {
    let mut out = Vec::new();
    for c in text.chars() {
        out.extend_from_slice(&keyboard::encode_key(
            &keyboard::Key::Character(c),
            Modifiers::empty(),
            mode,
        ));
    }
    out
}

/// Convenience wrapper that pulls the logical + physical key out of a winit
/// [`KeyEvent`]. Used by the GUI's `on_key`; the inner [`encode_key_event`]
/// stays pure and testable. `key_without_modifiers` is the layout-resolved base
/// key (on macOS this strips the Option-composition so Alt+a is `a`, not `å`).
#[cfg(target_os = "macos")]
#[must_use]
pub fn encode_winit_key_event(
    ev: &KeyEvent,
    mods: Modifiers,
    mode: KeyboardMode,
) -> Option<Vec<u8>> {
    use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
    encode_key_event(&ev.key_without_modifiers(), ev.physical_key, mods, mode)
}

/// Non-macOS fallback: `key_without_modifiers` is a platform extension trait, so
/// off macOS we encode the plain logical key. (aterm-gui targets macOS; this
/// keeps the crate compiling on other hosts for CI/tests.)
#[cfg(not(target_os = "macos"))]
#[must_use]
pub fn encode_winit_key_event(
    ev: &KeyEvent,
    mods: Modifiers,
    mode: KeyboardMode,
) -> Option<Vec<u8>> {
    encode_key_event(&ev.logical_key, ev.physical_key, mods, mode)
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::keyboard::{KeyCode, NamedKey as WinitNamed, SmolStr};

    fn ch(c: &str) -> WinitKey {
        WinitKey::Character(SmolStr::new(c))
    }

    /// Alt+a must emit the ESC-prefixed base key (ESC a), NOT the macOS
    /// Option-composed "å". We pass the layout base key ('a'); the engine
    /// prefixes ESC for ALT. This is THE K-1 regression: the old GUI wrote
    /// `ev.text`, which on macOS is the composed glyph.
    #[test]
    fn alt_a_is_esc_prefixed_not_composed() {
        let bytes = encode_key_event(
            &ch("a"),
            PhysicalKey::Code(KeyCode::KeyA),
            Modifiers::ALT,
            KeyboardMode::empty(),
        )
        .expect("alt+a encodes");
        assert_eq!(bytes, vec![0x1b, b'a'], "Alt+a must be ESC a, not composed å");
    }

    /// Ctrl+Space => NUL (0x00). The old `& 0x1f` branch only fired for ASCII
    /// alphabetics, so Ctrl+Space fell through to a raw " " write — wrong.
    #[test]
    fn ctrl_space_is_nul() {
        let bytes = encode_key_event(
            &WinitKey::Named(WinitNamed::Space),
            PhysicalKey::Code(KeyCode::Space),
            Modifiers::CTRL,
            KeyboardMode::empty(),
        )
        .expect("ctrl+space encodes");
        assert_eq!(bytes, vec![0x00], "Ctrl+Space must be NUL");
    }

    /// Ctrl+\\ => FS (0x1c). The old `& 0x1f` branch ignored non-alpha keys, so
    /// Ctrl+\\ produced a raw backslash instead of the control byte.
    #[test]
    fn ctrl_backslash_is_fs() {
        let bytes = encode_key_event(
            &ch("\\"),
            PhysicalKey::Code(KeyCode::Backslash),
            Modifiers::CTRL,
            KeyboardMode::empty(),
        )
        .expect("ctrl+backslash encodes");
        assert_eq!(bytes, vec![0x1c], "Ctrl+\\ must be FS (0x1c)");
    }

    /// Under DISAMBIGUATE_ESC_CODES (Kitty), a plain printable key is reported
    /// as CSI-u: `ESC [ <code> u`. The old raw-text write never produced this.
    #[test]
    fn printable_under_disambiguate_is_csi_u() {
        let bytes = encode_key_event(
            &ch("a"),
            PhysicalKey::Code(KeyCode::KeyA),
            Modifiers::empty(),
            KeyboardMode::DISAMBIGUATE_ESC_CODES,
        )
        .expect("a encodes");
        assert_eq!(bytes, b"\x1b[97u", "printable under Kitty disambiguate must be CSI-u");
    }

    /// Existing Ctrl-C / Ctrl-D byte output is UNCHANGED by the rewrite: the
    /// classic control bytes 0x03 / 0x04 still reach the PTY.
    #[test]
    fn ctrl_c_and_ctrl_d_unchanged() {
        let c = encode_key_event(
            &ch("c"),
            PhysicalKey::Code(KeyCode::KeyC),
            Modifiers::CTRL,
            KeyboardMode::empty(),
        )
        .expect("ctrl+c encodes");
        assert_eq!(c, vec![0x03], "Ctrl-C must stay 0x03");
        let d = encode_key_event(
            &ch("d"),
            PhysicalKey::Code(KeyCode::KeyD),
            Modifiers::CTRL,
            KeyboardMode::empty(),
        )
        .expect("ctrl+d encodes");
        assert_eq!(d, vec![0x04], "Ctrl-D must stay 0x04");
    }

    /// A plain printable key with no modifiers and no Kitty mode writes the
    /// literal byte — ordinary ASCII typing still works after the rewrite.
    #[test]
    fn plain_ascii_writes_literal() {
        let bytes = encode_key_event(
            &ch("a"),
            PhysicalKey::Code(KeyCode::KeyA),
            Modifiers::empty(),
            KeyboardMode::empty(),
        )
        .expect("a encodes");
        assert_eq!(bytes, vec![b'a']);
    }

    /// A bare modifier press (Shift alone) encodes to nothing in legacy mode.
    #[test]
    fn bare_modifier_press_is_none() {
        assert_eq!(
            encode_key_event(
                &WinitKey::Named(WinitNamed::Shift),
                PhysicalKey::Code(KeyCode::ShiftLeft),
                Modifiers::SHIFT,
                KeyboardMode::empty(),
            ),
            None,
            "a bare Shift press must produce no bytes in legacy mode"
        );
    }

    /// winit ModifiersState → engine Modifiers carries Super/Cmd through
    /// (the old inline path dropped it).
    #[test]
    fn super_modifier_carried_through() {
        let m = modifiers_from_winit(ModifiersState::SUPER);
        assert!(m.contains(Modifiers::SUPER));
    }

    /// IME-1: while a composition is active (non-empty preedit), direct key
    /// sends are SUPPRESSED so the composing keystrokes don't double-input; with
    /// no composition (empty preedit) they proceed.
    #[test]
    fn composition_suppresses_direct_send() {
        assert!(
            suppress_direct_send("か"),
            "an active preedit must suppress direct key sends"
        );
        assert!(
            !suppress_direct_send(""),
            "no composition (empty preedit) must NOT suppress direct sends"
        );
    }

    /// IME-1: a committed composition is encoded as ordinary text through the
    /// engine (each char a `Character` key), NOT a raw `& 0x1f` byte. ASCII
    /// commits round-trip to their bytes; multi-byte CJK to their UTF-8.
    #[test]
    fn commit_sends_committed_text() {
        // ASCII commit (e.g. an accented dead-key result reduced to ASCII).
        assert_eq!(
            encode_committed_text("hi", KeyboardMode::empty()),
            b"hi".to_vec()
        );
        // CJK commit: UTF-8 of "日本" goes out as typed text.
        assert_eq!(
            encode_committed_text("日本", KeyboardMode::empty()),
            "日本".as_bytes().to_vec()
        );
        // Empty commit is empty.
        assert!(encode_committed_text("", KeyboardMode::empty()).is_empty());
    }
}
