// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Keyboard input encoding for terminal emulators.
//!
//! Encodes key presses into terminal escape sequences, supporting both legacy
//! VT100/xterm encoding and the Kitty keyboard protocol.

#[path = "encode_legacy.rs"]
mod encode_legacy;

use super::{Key, KeyEventType, KeyboardMode, Modifiers, NamedKey};
use encode_legacy::{encode_character_legacy, encode_named_legacy};

/// Encode a key press into terminal escape sequence bytes.
///
/// Automatically selects between legacy encoding and Kitty keyboard protocol
/// based on the terminal mode flags.
#[must_use]
pub fn encode_key(key: &Key, modifiers: Modifiers, mode: KeyboardMode) -> Vec<u8> {
    encode_key_with_event(key, modifiers, mode, KeyEventType::Press)
}

/// Encode a key event with event type information.
///
/// Extends `encode_key` to support key repeat and release events
/// when using the Kitty keyboard protocol.
///
/// Progressive Kitty enhancements:
/// - `REPORT_ALL_KEYS_AS_ESC` forces CSI-u encoding for keys that would
///   otherwise use legacy escapes.
/// - `REPORT_ALTERNATE_KEYS` emits `unicode:alternate` in the first CSI
///   parameter when a shifted alternate codepoint is known.
/// - `REPORT_ASSOCIATED_TEXT` appends text-as-codepoints as the third CSI
///   parameter when paired with `REPORT_ALL_KEYS_AS_ESC`.
#[must_use]
pub fn encode_key_with_event(
    key: &Key,
    modifiers: Modifiers,
    mode: KeyboardMode,
    event_type: KeyEventType,
) -> Vec<u8> {
    encode_key_with_layout(key, modifiers, mode, event_type, None)
}

/// Encode a key event with optional `base_layout_key` for Kitty protocol.
///
/// The `base_layout_key` is the character that the physical key would produce
/// on a US QWERTY layout, regardless of the user's active keyboard layout.
/// When `REPORT_ALTERNATE_KEYS` mode is active, this is emitted as the third
/// colon-delimited value in the first CSI parameter: `key[:shifted[:base_layout]]`.
///
/// Pass `None` when the platform cannot determine the base layout key or when
/// the base layout key is the same as the primary key.
#[must_use]
pub fn encode_key_with_layout(
    key: &Key,
    modifiers: Modifiers,
    mode: KeyboardMode,
    event_type: KeyEventType,
    base_layout_key: Option<char>,
) -> Vec<u8> {
    if should_encode_kitty_event(key, modifiers, mode, event_type) {
        return encode_kitty(key, modifiers, mode, event_type, base_layout_key);
    }

    // For release events without Kitty protocol, return nothing
    if event_type == KeyEventType::Release {
        return Vec::new();
    }

    if let Some(bytes) = encode_xterm_other_keys(key, modifiers, mode) {
        return bytes;
    }

    encode_legacy(key, modifiers, mode)
}

fn should_encode_kitty_event(
    key: &Key,
    modifiers: Modifiers,
    mode: KeyboardMode,
    event_type: KeyEventType,
) -> bool {
    if mode.intersects(KeyboardMode::DISAMBIGUATE_ESC_CODES | KeyboardMode::REPORT_ALL_KEYS_AS_ESC)
    {
        return true;
    }

    if event_type == KeyEventType::Press || !mode.contains(KeyboardMode::REPORT_EVENT_TYPES) {
        return false;
    }

    requires_escape_sequence_without_report_all(key, modifiers)
}

fn requires_escape_sequence_without_report_all(key: &Key, modifiers: Modifiers) -> bool {
    match key {
        Key::Character(_) => requires_escape_sequence_for_character_without_report_all(modifiers),
        Key::Named(named) => named_requires_escape_sequence_without_report_all(*named, modifiers),
    }
}

fn named_requires_escape_sequence_without_report_all(
    named: NamedKey,
    modifiers: Modifiers,
) -> bool {
    !named_produces_text_without_report_all(named, modifiers)
}

fn named_produces_text_without_report_all(named: NamedKey, modifiers: Modifiers) -> bool {
    match named {
        // Shifted Tab uses back-tab escape sequences in legacy mode.
        NamedKey::Tab => !modifiers.contains(Modifiers::SHIFT),
        NamedKey::Enter | NamedKey::Backspace | NamedKey::Space | NamedKey::NumpadEnter => true,
        _ => false,
    }
}

fn requires_escape_sequence_for_character_without_report_all(modifiers: Modifiers) -> bool {
    // Kitty's default mode keeps these modifier combinations in the legacy
    // text/control paths; other combinations already need CSI-u bytes.
    modifiers != Modifiers::empty()
        && modifiers != Modifiers::SHIFT
        && modifiers != Modifiers::ALT
        && modifiers != Modifiers::CTRL
        && modifiers != (Modifiers::SHIFT | Modifiers::ALT)
        && modifiers != (Modifiers::CTRL | Modifiers::ALT)
}

/// Encode using the Kitty keyboard protocol.
///
/// Per the Kitty spec, functional keys that have existing legacy CSI
/// representations (arrows, Home/End, Insert/Delete, Page Up/Down,
/// F1-F24) retain their legacy format unless `REPORT_ALL_KEYS_AS_ESC`
/// is active. Keys without legacy representations (Escape, Enter, Tab,
/// Backspace, Space, modifier keys, media keys, numpad keys, etc.) use
/// the CSI u format: `CSI unicode [; modifiers [: event-type]] u`.
fn encode_kitty(
    key: &Key,
    modifiers: Modifiers,
    mode: KeyboardMode,
    event_type: KeyEventType,
    base_layout_key: Option<char>,
) -> Vec<u8> {
    let kitty_modifiers = kitty_modifiers_for_event(key, modifiers, event_type);

    // When REPORT_ALL_KEYS_AS_ESC is NOT set, functional keys with legacy CSI
    // representations retain their legacy format per Kitty spec (#7474).
    if !mode.contains(KeyboardMode::REPORT_ALL_KEYS_AS_ESC)
        && let Some(legacy) = encode_kitty_legacy_functional(key, kitty_modifiers, mode, event_type)
    {
        return legacy;
    }

    let (primary_code, alternate_code, base_layout_code) =
        kitty_key_codes(key, kitty_modifiers, mode, base_layout_key);
    let associated_text = associated_text_codepoints(key, kitty_modifiers, mode, event_type);

    let mod_value = kitty_modifiers.kitty_encoded();
    let report_events = mode.contains(KeyboardMode::REPORT_EVENT_TYPES);
    let include_event_type = report_events && event_type != KeyEventType::Press;
    let include_modifiers = mod_value > 1 || include_event_type || associated_text.is_some();

    let mut buf = Vec::with_capacity(16);
    buf.extend_from_slice(b"\x1b[");

    write_u32(&mut buf, primary_code);
    if alternate_code.is_some() || base_layout_code.is_some() {
        buf.push(b':');
        if let Some(alt) = alternate_code {
            write_u32(&mut buf, alt);
        }
        if let Some(base) = base_layout_code {
            buf.push(b':');
            write_u32(&mut buf, base);
        }
    }

    if include_modifiers {
        buf.push(b';');
        write_u8(&mut buf, mod_value);

        if include_event_type {
            buf.push(b':');
            write_u8(&mut buf, event_type.kitty_value());
        }
    }

    if let Some(associated_text) = associated_text {
        buf.push(b';');
        let mut codepoints = associated_text.into_iter();
        if let Some(first) = codepoints.next() {
            write_u32(&mut buf, first);
            for codepoint in codepoints {
                buf.push(b':');
                write_u32(&mut buf, codepoint);
            }
        }
    }

    buf.push(b'u');
    buf
}

/// For named keys with legacy CSI representations, encode using the legacy
/// format with Kitty modifier encoding (1+mods). Returns `None` for keys
/// that have no legacy representation and should use CSI u.
///
/// Legacy formats preserved under Kitty protocol:
/// - Arrows: CSI [1;{mod}] A/B/C/D
/// - Home/End: CSI [1;{mod}] H/F
/// - Insert/Delete/PageUp/PageDown: CSI {num} [;{mod}] ~
/// - F1-F4: CSI [1;{mod}] P/Q/R/S (or SS3 P/Q/R/S without mods)
/// - F5-F24: CSI {num} [;{mod}] ~
fn encode_kitty_legacy_functional(
    key: &Key,
    modifiers: Modifiers,
    mode: KeyboardMode,
    event_type: KeyEventType,
) -> Option<Vec<u8>> {
    let named = match key {
        Key::Named(n) => *n,
        Key::Character(_) => return None,
    };

    let report_events = mode.contains(KeyboardMode::REPORT_EVENT_TYPES);
    let include_event_type = report_events && event_type != KeyEventType::Press;
    let mod_value = modifiers.kitty_encoded();
    let has_modifiers_or_event = mod_value > 1 || include_event_type;

    // Helper: build the modifier suffix ";{mod}[:event]" portion.
    let append_mod_event = |buf: &mut Vec<u8>| {
        if has_modifiers_or_event {
            buf.push(b';');
            write_u8(buf, mod_value);
            if include_event_type {
                buf.push(b':');
                write_u8(buf, event_type.kitty_value());
            }
        }
    };

    // Letter-final keys: CSI [1;{mod}[:event]] {letter}
    // Without modifiers/event: CSI {letter}
    let letter_final = |letter: u8| -> Vec<u8> {
        let mut buf = Vec::with_capacity(12);
        buf.extend_from_slice(b"\x1b[");
        if has_modifiers_or_event {
            buf.push(b'1');
            append_mod_event(&mut buf);
        }
        buf.push(letter);
        buf
    };

    // Tilde-final keys: CSI {num} [;{mod}[:event]] ~
    let tilde_final = |num: u8| -> Vec<u8> {
        let mut buf = Vec::with_capacity(12);
        buf.extend_from_slice(b"\x1b[");
        write_u8(&mut buf, num);
        append_mod_event(&mut buf);
        buf.push(b'~');
        buf
    };

    Some(match named {
        // Arrows
        NamedKey::ArrowUp => letter_final(b'A'),
        NamedKey::ArrowDown => letter_final(b'B'),
        NamedKey::ArrowRight => letter_final(b'C'),
        NamedKey::ArrowLeft => letter_final(b'D'),
        // Home/End
        NamedKey::Home => letter_final(b'H'),
        NamedKey::End => letter_final(b'F'),
        // Insert/Delete/PageUp/PageDown
        NamedKey::Insert => tilde_final(2),
        NamedKey::Delete => tilde_final(3),
        NamedKey::PageUp => tilde_final(5),
        NamedKey::PageDown => tilde_final(6),
        // F1-F4 (letter finals)
        NamedKey::F1 => letter_final(b'P'),
        NamedKey::F2 => letter_final(b'Q'),
        NamedKey::F3 => letter_final(b'R'),
        NamedKey::F4 => letter_final(b'S'),
        // F5-F24 (tilde finals)
        NamedKey::F5 => tilde_final(15),
        NamedKey::F6 => tilde_final(17),
        NamedKey::F7 => tilde_final(18),
        NamedKey::F8 => tilde_final(19),
        NamedKey::F9 => tilde_final(20),
        NamedKey::F10 => tilde_final(21),
        NamedKey::F11 => tilde_final(23),
        NamedKey::F12 => tilde_final(24),
        NamedKey::F13 => tilde_final(25),
        NamedKey::F14 => tilde_final(26),
        NamedKey::F15 => tilde_final(28),
        NamedKey::F16 => tilde_final(29),
        NamedKey::F17 => tilde_final(31),
        NamedKey::F18 => tilde_final(32),
        NamedKey::F19 => tilde_final(33),
        NamedKey::F20 => tilde_final(34),
        NamedKey::F21 => tilde_final(35),
        NamedKey::F22 => tilde_final(36),
        NamedKey::F23 => tilde_final(37),
        NamedKey::F24 => tilde_final(38),
        _ => return None,
    })
}

fn kitty_modifiers_for_event(
    key: &Key,
    modifiers: Modifiers,
    event_type: KeyEventType,
) -> Modifiers {
    let modifier_flag = match key {
        Key::Named(NamedKey::ShiftLeft) | Key::Named(NamedKey::ShiftRight) => Modifiers::SHIFT,
        Key::Named(NamedKey::ControlLeft) | Key::Named(NamedKey::ControlRight) => Modifiers::CTRL,
        Key::Named(NamedKey::AltLeft) | Key::Named(NamedKey::AltRight) => Modifiers::ALT,
        Key::Named(NamedKey::SuperLeft) | Key::Named(NamedKey::SuperRight) => Modifiers::SUPER,
        _ => return modifiers,
    };

    let mut adjusted = modifiers;
    match event_type {
        KeyEventType::Release => adjusted.remove(modifier_flag),
        KeyEventType::Press | KeyEventType::Repeat => adjusted.insert(modifier_flag),
    }
    adjusted
}

fn kitty_key_codes(
    key: &Key,
    modifiers: Modifiers,
    mode: KeyboardMode,
    base_layout_key: Option<char>,
) -> (u32, Option<u32>, Option<u32>) {
    match key {
        Key::Named(named) => (named.kitty_code(), None, None),
        Key::Character(c) => {
            let primary = *c as u32;
            if !mode.contains(KeyboardMode::REPORT_ALTERNATE_KEYS) {
                return (primary, None, None);
            }
            let alternate = shifted_character(*c, modifiers)
                .map(u32::from)
                .filter(|alt| *alt != primary);
            // base_layout_key: the US QWERTY equivalent of this physical key.
            // Only emit when it differs from the primary key (#7678).
            let base_layout = base_layout_key
                .map(u32::from)
                .filter(|base| *base != primary);
            (primary, alternate, base_layout)
        }
    }
}

fn associated_text_codepoints(
    key: &Key,
    modifiers: Modifiers,
    mode: KeyboardMode,
    event_type: KeyEventType,
) -> Option<Vec<u32>> {
    if !mode.contains(KeyboardMode::REPORT_ASSOCIATED_TEXT)
        || !mode.contains(KeyboardMode::REPORT_ALL_KEYS_AS_ESC)
        || event_type == KeyEventType::Release
    {
        return None;
    }

    // Match Kitty/Terminal behavior: modified control/meta paths do not carry text payloads.
    if modifiers.intersects(Modifiers::ALT | Modifiers::CTRL | Modifiers::SUPER) {
        return None;
    }

    match key {
        Key::Character(c) => {
            let text = shifted_character(*c, modifiers).unwrap_or(*c);
            if text.is_control() {
                None
            } else {
                Some(vec![text as u32])
            }
        }
        Key::Named(_) => None,
    }
}

fn shifted_character(c: char, modifiers: Modifiers) -> Option<char> {
    if !modifiers.contains(Modifiers::SHIFT) {
        return None;
    }

    match c {
        'a'..='z' => Some(c.to_ascii_uppercase()),
        '1' => Some('!'),
        '2' => Some('@'),
        '3' => Some('#'),
        '4' => Some('$'),
        '5' => Some('%'),
        '6' => Some('^'),
        '7' => Some('&'),
        '8' => Some('*'),
        '9' => Some('('),
        '0' => Some(')'),
        '`' => Some('~'),
        '-' => Some('_'),
        '=' => Some('+'),
        '[' => Some('{'),
        ']' => Some('}'),
        '\\' => Some('|'),
        ';' => Some(':'),
        '\'' => Some('"'),
        ',' => Some('<'),
        '.' => Some('>'),
        '/' => Some('?'),
        _ => Some(c),
    }
}

/// Encode using legacy terminal sequences.
fn encode_legacy(key: &Key, modifiers: Modifiers, mode: KeyboardMode) -> Vec<u8> {
    match key {
        Key::Character(c) => encode_character_legacy(*c, modifiers),
        Key::Named(named) => encode_named_legacy(*named, modifiers, mode),
    }
}

fn encode_xterm_other_keys(key: &Key, modifiers: Modifiers, mode: KeyboardMode) -> Option<Vec<u8>> {
    let level = mode.xterm_modify_other_keys_level();
    if level == 0 {
        return None;
    }

    let effective_mods =
        modifiers & (Modifiers::SHIFT | Modifiers::ALT | Modifiers::CTRL | Modifiers::SUPER);
    let apply = match level {
        1 => effective_mods.contains(Modifiers::ALT),
        2 => !effective_mods.is_empty(),
        _ => false,
    };
    if !apply {
        return None;
    }

    let code: u32 = match *key {
        Key::Character(c) => c as u32,
        Key::Named(NamedKey::Tab) => 9,
        Key::Named(NamedKey::Enter) | Key::Named(NamedKey::NumpadEnter) => 13,
        Key::Named(NamedKey::Escape) => 27,
        Key::Named(NamedKey::Backspace) => 127,
        Key::Named(NamedKey::Space) => 32,
        Key::Named(NamedKey::NumpadEqual) => u32::from(b'='),
        _ => return None,
    };

    let mod_value = effective_mods.xterm_encoded();
    if mode.xterm_format_other_keys() {
        // formatOtherKeys=1: CSI code ; modifier u
        let mut buf = Vec::with_capacity(16);
        buf.extend_from_slice(b"\x1b[");
        write_u32(&mut buf, code);
        buf.push(b';');
        write_u8(&mut buf, mod_value);
        buf.push(b'u');
        Some(buf)
    } else {
        // Default format: CSI 27 ; modifier ; code ~
        let mut buf = Vec::with_capacity(20);
        buf.extend_from_slice(b"\x1b[27;");
        write_u8(&mut buf, mod_value);
        buf.push(b';');
        write_u32(&mut buf, code);
        buf.push(b'~');
        Some(buf)
    }
}

/// Write a u8 as decimal digits to a buffer.
fn write_u8(buf: &mut Vec<u8>, val: u8) {
    if val >= 100 {
        buf.push(b'0' + val / 100);
    }
    if val >= 10 {
        buf.push(b'0' + (val / 10) % 10);
    }
    buf.push(b'0' + val % 10);
}
/// Write a u32 as decimal digits to a buffer.
fn write_u32(buf: &mut Vec<u8>, val: u32) {
    if val == 0 {
        buf.push(b'0');
        return;
    }
    let mut divisor = 1u64; // u64 prevents overflow when val >= 2B (#2775)
    while divisor * 10 <= u64::from(val) {
        divisor *= 10;
    }
    while divisor > 0 {
        buf.push(b'0' + (u64::from(val) / divisor % 10) as u8);
        divisor /= 10;
    }
}
#[cfg(test)]
#[path = "encode_tests.rs"]
mod encode_tests;
