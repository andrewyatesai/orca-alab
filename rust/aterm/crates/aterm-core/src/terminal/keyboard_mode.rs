// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

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
