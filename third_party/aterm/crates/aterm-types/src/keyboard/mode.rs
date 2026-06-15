// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Keyboard-relevant terminal mode flags.
//!
//! A focused subset of terminal mode flags that affect keyboard encoding.
//! This type is bridge-agnostic and lives in `aterm-types` so both
//! `aterm-core-ffi` and `aterm-alacritty-bridge` can use it without
//! creating a dependency between them.

bitflags! {
    /// Terminal mode flags relevant to keyboard encoding.
    ///
    /// Contains only the flags that affect how key presses are encoded
    /// into escape sequences. This is intentionally a subset of the full
    /// terminal mode flags.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct KeyboardMode: u16 {
        /// Kitty keyboard protocol: disambiguate escape codes (CSI u encoding).
        const DISAMBIGUATE_ESC_CODES = 1 << 0;
        /// Kitty keyboard protocol: report key repeat/release events.
        const REPORT_EVENT_TYPES = 1 << 1;
        /// Application cursor keys mode (DECCKM) — arrows use SS3 instead of CSI.
        const APP_CURSOR = 1 << 2;
        /// Application keypad mode (DECKPAM) — numpad uses SS3 sequences.
        const APP_KEYPAD = 1 << 3;
        /// xterm modifyOtherKeys level 1.
        const XTERM_MODIFY_OTHER_KEYS_LEVEL1 = 1 << 4;
        /// xterm modifyOtherKeys level 2.
        const XTERM_MODIFY_OTHER_KEYS_LEVEL2 = 1 << 5;
        /// xterm formatOtherKeys enabled (CSI u format instead of CSI 27;mod;code~).
        const XTERM_FORMAT_OTHER_KEYS = 1 << 6;
        /// Kitty keyboard protocol: report alternate key values (shifted/base layout).
        const REPORT_ALTERNATE_KEYS = 1 << 7;
        /// Kitty keyboard protocol: report all keys as escape sequences.
        const REPORT_ALL_KEYS_AS_ESC = 1 << 8;
        /// Kitty keyboard protocol: report associated text with key events.
        const REPORT_ASSOCIATED_TEXT = 1 << 9;
        /// VT52 compatibility mode — arrow keys use `ESC A`..`ESC D` (no CSI/SS3).
        const VT52_MODE = 1 << 10;
    }
}

impl KeyboardMode {
    /// Get the xterm modifyOtherKeys level (0, 1, or 2).
    #[must_use]
    pub fn xterm_modify_other_keys_level(self) -> u8 {
        if self.contains(Self::XTERM_MODIFY_OTHER_KEYS_LEVEL2) {
            2
        } else if self.contains(Self::XTERM_MODIFY_OTHER_KEYS_LEVEL1) {
            1
        } else {
            0
        }
    }

    /// Check if xterm formatOtherKeys is enabled.
    #[must_use]
    pub fn xterm_format_other_keys(self) -> bool {
        self.contains(Self::XTERM_FORMAT_OTHER_KEYS)
    }
}
