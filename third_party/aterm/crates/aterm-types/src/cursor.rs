// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Cursor style types for terminal emulation.

/// Cursor style (DECSCUSR + extensions).
///
/// Terminal-domain representation: 6 DECSCUSR variants (3 shapes x blink/steady)
/// plus `Hidden` (cursor invisible, DECTCEM off) and `HollowBlock` (outline-only
/// block, common in unfocused windows and vi normal mode).
///
/// Intentionally distinct from `gpu::pipeline::CursorStyle` which collapses
/// to 3 shape-only variants for GPU rendering, and `gpu::ffi::AtermCursorStyle`
/// which provides a C-ABI-safe equivalent. See `gpu::renderer::terminal_cursor_style_to_gpu`
/// for the conversion bridge.
#[non_exhaustive]
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CursorStyle {
    /// Blinking block (default) - Ps = 1.
    #[default]
    BlinkingBlock = 1,
    /// Steady block - Ps = 2.
    SteadyBlock = 2,
    /// Blinking underline - Ps = 3.
    BlinkingUnderline = 3,
    /// Steady underline - Ps = 4.
    SteadyUnderline = 4,
    /// Blinking bar - Ps = 5.
    BlinkingBar = 5,
    /// Steady bar - Ps = 6.
    SteadyBar = 6,
    /// Hidden cursor (invisible). Not a DECSCUSR parameter; represents DECTCEM off
    /// or an explicit "no cursor" state from the terminal frontend.
    Hidden = 7,
    /// Hollow block cursor (outline only). Not a DECSCUSR parameter; used by
    /// terminal frontends for unfocused windows and vi normal mode.
    HollowBlock = 8,
}

impl CursorStyle {
    /// Map a DECSCUSR parameter to a cursor style.
    pub fn from_param(param: u16) -> Option<Self> {
        match param {
            0 | 1 => Some(Self::BlinkingBlock),
            2 => Some(Self::SteadyBlock),
            3 => Some(Self::BlinkingUnderline),
            4 => Some(Self::SteadyUnderline),
            5 => Some(Self::BlinkingBar),
            6 => Some(Self::SteadyBar),
            _ => None,
        }
    }
}
