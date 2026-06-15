// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Legacy/xterm-compatible keyboard encoding helpers.

use super::{KeyboardMode, Modifiers, NamedKey};

pub(super) fn encode_character_legacy(c: char, modifiers: Modifiers) -> Vec<u8> {
    if modifiers.contains(Modifiers::CTRL)
        && let Some(ctrl_char) = ctrl_character(c)
    {
        if modifiers.contains(Modifiers::ALT) {
            return vec![0x1b, ctrl_char];
        }
        return vec![ctrl_char];
    }

    if modifiers.contains(Modifiers::ALT) {
        let mut buf = vec![0x1b];
        let lower = if modifiers.contains(Modifiers::SHIFT) {
            c.to_ascii_uppercase()
        } else {
            c.to_ascii_lowercase()
        };
        let mut char_buf = [0u8; 4];
        let encoded = lower.encode_utf8(&mut char_buf);
        buf.extend_from_slice(encoded.as_bytes());
        return buf;
    }

    let output = if modifiers.contains(Modifiers::SHIFT) {
        c.to_ascii_uppercase()
    } else {
        c
    };

    let mut buf = [0u8; 4];
    let encoded = output.encode_utf8(&mut buf);
    encoded.as_bytes().to_vec()
}

pub(super) fn encode_named_legacy(
    key: NamedKey,
    modifiers: Modifiers,
    mode: KeyboardMode,
) -> Vec<u8> {
    let app_cursor = mode.contains(KeyboardMode::APP_CURSOR);
    let app_keypad = mode.contains(KeyboardMode::APP_KEYPAD);
    let has_modifiers = !modifiers.is_empty();

    if let Some(encoded) = encode_control_named_legacy(key, modifiers) {
        return encoded;
    }

    let vt52 = mode.contains(KeyboardMode::VT52_MODE);
    if let Some(encoded) =
        encode_navigation_named_legacy(key, app_cursor, vt52, modifiers, has_modifiers)
    {
        return encoded;
    }

    if let Some(encoded) = encode_function_named_legacy(key, modifiers, has_modifiers) {
        return encoded;
    }

    if let Some(encoded) = encode_numpad_named_legacy(key, app_keypad, vt52, modifiers) {
        return encoded;
    }

    Vec::new()
}

fn encode_control_named_legacy(key: NamedKey, modifiers: Modifiers) -> Option<Vec<u8>> {
    Some(match key {
        NamedKey::Enter => {
            if modifiers.contains(Modifiers::ALT) {
                vec![0x1b, 0x0d]
            } else {
                vec![0x0d]
            }
        }
        // NumpadEnter is handled by encode_numpad_named_legacy (DECKPAM → SS3 M, #7558).
        NamedKey::Tab => {
            if modifiers.contains(Modifiers::SHIFT) {
                vec![0x1b, b'[', b'Z']
            } else if modifiers.contains(Modifiers::ALT) {
                vec![0x1b, 0x09]
            } else {
                vec![0x09]
            }
        }
        NamedKey::Escape => {
            if modifiers.contains(Modifiers::ALT) {
                vec![0x1b, 0x1b]
            } else {
                vec![0x1b]
            }
        }
        NamedKey::Backspace => {
            if modifiers.contains(Modifiers::CTRL) {
                vec![0x08]
            } else if modifiers.contains(Modifiers::ALT) {
                vec![0x1b, 0x7f]
            } else {
                vec![0x7f]
            }
        }
        NamedKey::Space => {
            if modifiers.contains(Modifiers::CTRL) {
                vec![0x00]
            } else if modifiers.contains(Modifiers::ALT) {
                vec![0x1b, 0x20]
            } else {
                vec![0x20]
            }
        }
        _ => return None,
    })
}

fn encode_navigation_named_legacy(
    key: NamedKey,
    app_cursor: bool,
    vt52: bool,
    modifiers: Modifiers,
    has_modifiers: bool,
) -> Option<Vec<u8>> {
    Some(match key {
        NamedKey::ArrowUp | NamedKey::NumpadArrowUp => {
            encode_arrow(b'A', app_cursor, vt52, modifiers, has_modifiers)
        }
        NamedKey::ArrowDown | NamedKey::NumpadArrowDown => {
            encode_arrow(b'B', app_cursor, vt52, modifiers, has_modifiers)
        }
        NamedKey::ArrowRight | NamedKey::NumpadArrowRight => {
            encode_arrow(b'C', app_cursor, vt52, modifiers, has_modifiers)
        }
        NamedKey::ArrowLeft | NamedKey::NumpadArrowLeft => {
            encode_arrow(b'D', app_cursor, vt52, modifiers, has_modifiers)
        }
        NamedKey::Home | NamedKey::NumpadHome => {
            encode_home_end(b'H', app_cursor, modifiers, has_modifiers)
        }
        NamedKey::End | NamedKey::NumpadEnd => {
            encode_home_end(b'F', app_cursor, modifiers, has_modifiers)
        }
        NamedKey::PageUp | NamedKey::NumpadPageUp => encode_tilde_key(5, modifiers, has_modifiers),
        NamedKey::PageDown | NamedKey::NumpadPageDown => {
            encode_tilde_key(6, modifiers, has_modifiers)
        }
        NamedKey::Insert | NamedKey::NumpadInsert => encode_tilde_key(2, modifiers, has_modifiers),
        NamedKey::Delete | NamedKey::NumpadDelete => encode_tilde_key(3, modifiers, has_modifiers),
        _ => return None,
    })
}

fn encode_function_named_legacy(
    key: NamedKey,
    modifiers: Modifiers,
    has_modifiers: bool,
) -> Option<Vec<u8>> {
    Some(match key {
        NamedKey::F1 => encode_f1_f4(b'P', modifiers, has_modifiers),
        NamedKey::F2 => encode_f1_f4(b'Q', modifiers, has_modifiers),
        NamedKey::F3 => encode_f1_f4(b'R', modifiers, has_modifiers),
        NamedKey::F4 => encode_f1_f4(b'S', modifiers, has_modifiers),
        NamedKey::F5 => encode_tilde_key(15, modifiers, has_modifiers),
        NamedKey::F6 => encode_tilde_key(17, modifiers, has_modifiers),
        NamedKey::F7 => encode_tilde_key(18, modifiers, has_modifiers),
        NamedKey::F8 => encode_tilde_key(19, modifiers, has_modifiers),
        NamedKey::F9 => encode_tilde_key(20, modifiers, has_modifiers),
        NamedKey::F10 => encode_tilde_key(21, modifiers, has_modifiers),
        NamedKey::F11 => encode_tilde_key(23, modifiers, has_modifiers),
        NamedKey::F12 => encode_tilde_key(24, modifiers, has_modifiers),
        NamedKey::F13 => encode_tilde_key(25, modifiers, has_modifiers),
        NamedKey::F14 => encode_tilde_key(26, modifiers, has_modifiers),
        NamedKey::F15 => encode_tilde_key(28, modifiers, has_modifiers),
        NamedKey::F16 => encode_tilde_key(29, modifiers, has_modifiers),
        NamedKey::F17 => encode_tilde_key(31, modifiers, has_modifiers),
        NamedKey::F18 => encode_tilde_key(32, modifiers, has_modifiers),
        NamedKey::F19 => encode_tilde_key(33, modifiers, has_modifiers),
        NamedKey::F20 => encode_tilde_key(34, modifiers, has_modifiers),
        NamedKey::F21 => encode_tilde_key(35, modifiers, has_modifiers),
        NamedKey::F22 => encode_tilde_key(36, modifiers, has_modifiers),
        NamedKey::F23 => encode_tilde_key(37, modifiers, has_modifiers),
        NamedKey::F24 => encode_tilde_key(38, modifiers, has_modifiers),
        NamedKey::F25
        | NamedKey::F26
        | NamedKey::F27
        | NamedKey::F28
        | NamedKey::F29
        | NamedKey::F30
        | NamedKey::F31
        | NamedKey::F32
        | NamedKey::F33
        | NamedKey::F34
        | NamedKey::F35 => Vec::new(),
        _ => return None,
    })
}

fn encode_numpad_named_legacy(
    key: NamedKey,
    app_keypad: bool,
    vt52: bool,
    modifiers: Modifiers,
) -> Option<Vec<u8>> {
    Some(match key {
        NamedKey::Numpad0 => encode_numpad(b'p', '0', app_keypad, vt52, modifiers),
        NamedKey::Numpad1 => encode_numpad(b'q', '1', app_keypad, vt52, modifiers),
        NamedKey::Numpad2 => encode_numpad(b'r', '2', app_keypad, vt52, modifiers),
        NamedKey::Numpad3 => encode_numpad(b's', '3', app_keypad, vt52, modifiers),
        NamedKey::Numpad4 => encode_numpad(b't', '4', app_keypad, vt52, modifiers),
        NamedKey::Numpad5 => encode_numpad(b'u', '5', app_keypad, vt52, modifiers),
        NamedKey::Numpad6 => encode_numpad(b'v', '6', app_keypad, vt52, modifiers),
        NamedKey::Numpad7 => encode_numpad(b'w', '7', app_keypad, vt52, modifiers),
        NamedKey::Numpad8 => encode_numpad(b'x', '8', app_keypad, vt52, modifiers),
        NamedKey::Numpad9 => encode_numpad(b'y', '9', app_keypad, vt52, modifiers),
        NamedKey::NumpadDecimal => encode_numpad(b'n', '.', app_keypad, vt52, modifiers),
        NamedKey::NumpadDivide => encode_numpad(b'o', '/', app_keypad, vt52, modifiers),
        NamedKey::NumpadMultiply => encode_numpad(b'j', '*', app_keypad, vt52, modifiers),
        NamedKey::NumpadSubtract => encode_numpad(b'm', '-', app_keypad, vt52, modifiers),
        NamedKey::NumpadAdd => encode_numpad(b'k', '+', app_keypad, vt52, modifiers),
        // NumpadEnter: SS3 M in DECKPAM, CR otherwise. Per VT420 spec,
        // this distinguishes numpad Enter from main Enter (#7558).
        NamedKey::NumpadEnter => encode_numpad(b'M', '\r', app_keypad, vt52, modifiers),
        NamedKey::NumpadEqual => encode_character_legacy('=', modifiers),
        // NumpadSeparator: comma on some international keyboards (SS3 l in DECKPAM).
        NamedKey::NumpadSeparator => encode_numpad(b'l', ',', app_keypad, vt52, modifiers),
        // NumpadBegin (KP_BEGIN / center 5 key): SS3 E in DECKPAM, '5' otherwise.
        // xterm encodes this as ESC O E in app mode, ESC [E in normal mode.
        NamedKey::NumpadBegin => {
            let effective_app = app_keypad && !modifiers.contains(Modifiers::SHIFT);
            if modifiers.contains(Modifiers::ALT) {
                vec![0x1b, b'5']
            } else if vt52 && effective_app {
                // VT52 application keypad: ESC ? 5
                vec![0x1b, b'?', b'5']
            } else if effective_app {
                vec![0x1b, b'O', b'E']
            } else {
                vec![b'5']
            }
        }
        // Numpad navigation (NumpadArrow*, NumpadHome, etc.) is handled by
        // encode_navigation_named_legacy which runs earlier in the call chain.
        _ => return None,
    })
}

fn ctrl_character(c: char) -> Option<u8> {
    let c_upper = c.to_ascii_uppercase();
    if c_upper.is_ascii_uppercase() {
        Some(c_upper as u8 - b'A' + 1)
    } else {
        match c {
            ' ' | '@' | '2' => Some(0x00),
            '[' | '3' => Some(0x1b),
            '\\' | '4' => Some(0x1c),
            ']' | '5' => Some(0x1d),
            '^' | '6' => Some(0x1e),
            '_' | '/' | '7' => Some(0x1f),
            '?' | '8' => Some(0x7f),
            _ => None,
        }
    }
}

fn encode_arrow(
    suffix: u8,
    app_cursor: bool,
    vt52: bool,
    modifiers: Modifiers,
    has_modifiers: bool,
) -> Vec<u8> {
    // VT52 mode: cursor keys are ESC A/B/C/D (no CSI/SS3), ignoring DECCKM.
    if vt52 {
        return vec![0x1b, suffix];
    }
    if has_modifiers {
        let mut buf = vec![0x1b, b'[', b'1', b';'];
        super::write_u8(&mut buf, modifiers.xterm_encoded());
        buf.push(suffix);
        buf
    } else if app_cursor {
        vec![0x1b, b'O', suffix]
    } else {
        vec![0x1b, b'[', suffix]
    }
}

fn encode_home_end(
    suffix: u8,
    app_cursor: bool,
    modifiers: Modifiers,
    has_modifiers: bool,
) -> Vec<u8> {
    if has_modifiers {
        let mut buf = vec![0x1b, b'[', b'1', b';'];
        super::write_u8(&mut buf, modifiers.xterm_encoded());
        buf.push(suffix);
        buf
    } else if app_cursor {
        // DECCKM: Home → SS3 H, End → SS3 F (matches xterm/kitty/alacritty)
        vec![0x1b, b'O', suffix]
    } else {
        vec![0x1b, b'[', suffix]
    }
}

fn encode_tilde_key(num: u8, modifiers: Modifiers, has_modifiers: bool) -> Vec<u8> {
    let mut buf = vec![0x1b, b'['];
    super::write_u8(&mut buf, num);

    if has_modifiers {
        buf.push(b';');
        super::write_u8(&mut buf, modifiers.xterm_encoded());
    }

    buf.push(b'~');
    buf
}

fn encode_f1_f4(suffix: u8, modifiers: Modifiers, has_modifiers: bool) -> Vec<u8> {
    if has_modifiers {
        let mut buf = vec![0x1b, b'[', b'1', b';'];
        super::write_u8(&mut buf, modifiers.xterm_encoded());
        buf.push(suffix);
        buf
    } else {
        vec![0x1b, b'O', suffix]
    }
}

fn encode_numpad(
    ss3_suffix: u8,
    char_val: char,
    app_keypad: bool,
    vt52: bool,
    modifiers: Modifiers,
) -> Vec<u8> {
    // Per xterm: Shift cancels application keypad mode, forcing numeric output.
    // Ctrl has no effect on numpad digits in legacy encoding. (#7480)
    let effective_app = app_keypad && !modifiers.contains(Modifiers::SHIFT);
    if modifiers.contains(Modifiers::ALT) {
        let mut buf = vec![0x1b];
        buf.push(char_val as u8);
        buf
    } else if vt52 && effective_app {
        // VT52 application keypad: ESC ? followed by the digit/symbol character.
        vec![0x1b, b'?', char_val as u8]
    } else if effective_app {
        vec![0x1b, b'O', ss3_suffix]
    } else {
        vec![char_val as u8]
    }
}
