// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
// Author: Andrew Yates

//! Keyboard mode derivation from terminal state.
//!
//! Provides a bridge-agnostic conversion from `Terminal` accessors
//! (`TerminalModes`, `KittyKeyboardFlags`, `XtermKeyboardState`) into
//! `aterm_types::keyboard::KeyboardMode`.

use aterm_types::keyboard::KeyboardMode;

use super::{KittyKeyboardFlags, Terminal, TerminalModes, XtermKeyboardState};

/// Derive `KeyboardMode` from terminal modes, Kitty flags, and xterm state.
///
/// Delegates to `aterm_types::keyboard::TermMode::from_keyboard_state` as the
/// single source of truth for the 10-flag keyboard projection (#3732).
#[must_use]
pub(crate) fn keyboard_mode_from_state(
    modes: &TerminalModes,
    kitty: KittyKeyboardFlags,
    xterm: XtermKeyboardState,
) -> KeyboardMode {
    let mut km = aterm_types::keyboard::TermMode::from_keyboard_state(
        modes.application_cursor_keys,
        modes.application_keypad,
        modes.vt52_mode,
        kitty,
        xterm,
    )
    .to_keyboard_mode();
    // DECBKM (mode 67) is a legacy-encoding concern outside the TermMode kitty/xterm
    // projection, so fold it in here.
    if modes.backarrow_sends_bs {
        km.insert(KeyboardMode::BACKARROW_SENDS_BS);
    }
    // xterm keyboard private modes 1035/1036/1039 are likewise legacy-encoding
    // concerns folded in here. Each is modeled so the `empty()`/default mode
    // preserves the historical encoder contract:
    //   - 1039 altSendsEscape: a NEGATIVE flag — reset suppresses the Alt ESC.
    //   - 1036 metaSendsEscape: a POSITIVE flag — set adds the Meta ESC.
    //   - 1035 numLock: a NEGATIVE flag — reset strips the NumLock modifier.
    if !modes.alt_send_escape {
        km.insert(KeyboardMode::ALT_NO_ESC);
    }
    if modes.meta_send_escape {
        km.insert(KeyboardMode::META_SENDS_ESC);
    }
    if !modes.special_modifiers {
        km.insert(KeyboardMode::NO_SPECIAL_MODIFIERS);
    }
    km
}

impl Terminal {
    /// Get the keyboard encoding mode flags for this terminal.
    ///
    /// Returns a bridge-agnostic `KeyboardMode` that can be passed directly
    /// to `aterm_types::keyboard::encode_key*` functions.
    #[must_use]
    pub fn keyboard_mode(&self) -> KeyboardMode {
        keyboard_mode_from_state(
            self.modes(),
            self.kitty_keyboard_flags(),
            *self.xterm_keyboard(),
        )
    }
}
