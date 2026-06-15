// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Shared types for the aterm terminal engine.
//!
//! Provides primitive types (`Rgb`, `CursorStyle`, `BiDiMode`, `ColorPalette`,
//! `TerminalSize`) and cross-crate type definitions for domain, input,
//! keyboard, mouse, selection, GPU views, text shaping, terminal hosting,
//! perception, and verification.
//!
//! FFI boundary types (error enums, safety helpers, pointer lifecycle tracking)
//! live in `aterm-ffi-types` (#3353).

#![deny(unsafe_op_in_unsafe_fn)]
#![deny(missing_docs)]
#![deny(clippy::all)]

mod mutex_ext;
pub use mutex_ext::MutexExt;

pub mod sync;

pub mod verification;

pub mod ordered_map;

// ============================================================================
// Domain types — owned by this crate
// ============================================================================
mod cursor;

// Grid indexing types (Line, Column, Point, etc.) — extracted from aterm-alacritty-bridge (#3828).
pub mod index;
// Re-export commonly used index types at crate root for ergonomic imports.
pub use index::{Boundary, Column, Dimensions, Direction, Line, Point, Scroll, Side};

#[macro_use]
mod bitflags;

pub mod dirs;

mod time;
pub use time::duration_to_nanos;

pub mod color_math;
mod color_palette;
pub mod control_socket;
pub mod fs_restricted;
mod x11_colors;
// Re-export cursor style at crate root (was inline, extracted for file size).
pub use cursor::CursorStyle;
// Re-export color palette at crate root (extracted for file size, Part of #5332).
pub use color_palette::ColorPalette;

pub mod block;
mod buffer_access;
pub mod buffer_command;
mod buffer_view;
mod clipboard;
pub mod domain;
mod env_sanitize;
pub mod input;
pub mod keyboard;
mod kitty_keyboard;
pub mod latency_stats;
pub mod mouse;
pub mod perception;
pub mod pipeline_timestamps;
pub use pipeline_timestamps::PipelineTimestamps;
pub mod callback_events;
mod callback_registry;
mod callback_types;
pub mod charset;
pub mod paragraph_direction;
pub mod screen_capture;
pub mod selection;
mod shell_blocks;
mod shell_types;
mod terminal_core;
pub mod terminal_host;
mod terminal_modes;
pub mod text_shaping;
pub mod vt_level;
mod window;
mod xterm_keyboard;
// Re-export callback event types at crate root (Part of #5663 Phase 1).
pub use callback_events::{SshConductorCallbackEvent, TmuxCallbackEvent};
// Re-export ParagraphDirection at crate root (Part of #5663).
pub use paragraph_direction::ParagraphDirection;
// Re-export VtLevel at crate root (Part of #5663 Phase 1).
pub use vt_level::VtLevel;
// Re-export charset types at crate root (Part of #5663 Phase 1).
pub use charset::{
    CharacterSet, CharacterSet96, CharacterSetState, GlMapping, GrMapping, SingleShift,
};
// Re-export window types at crate root (Part of #5663 Phase 1).
pub use window::{WindowOperation, WindowResponse};
// Re-export shell block types at crate root (Part of #5663 Phase 3).
pub use shell_blocks::{BlockState, OutputBlock, RowSpan};
// Re-export shell types at crate root (Part of #5663 Phase 3).
pub use shell_types::{Annotation, CommandMark, ShellEvent, TerminalMark, current_time_ms};
// Re-export callback registry types at crate root (Part of #5663 Phase 2).
pub use callback_registry::{
    CALLBACK_REGISTRY, CallbackCategory, CallbackInfo, callback_by_name, callback_count,
    callback_info,
};
// Re-export core terminal types at crate root (Part of #5663).
pub use terminal_core::{TerminalCapabilities, TerminalSnapshot};
// Re-export terminal mode flags at crate root (Part of #5663).
pub use terminal_modes::TerminalModes;
// Re-export kitty keyboard types at crate root (Part of #5663).
pub use kitty_keyboard::{
    KittyKeyboardFlags, KittyKeyboardState, KittyKeyboardStateSnapshot, ScreenBuffer,
};
// Re-export xterm keyboard type at crate root (Part of #5663).
pub use xterm_keyboard::XtermKeyboardState;
// Re-export clipboard types at crate root (Part of #5663).
pub use clipboard::{ClipboardOperation, ClipboardSelection, CopyToClipboardOperation};
pub mod osc;
mod search_content;
// Re-export SearchContent trait at crate root (#5759: decouple aterm-scrollback from aterm-search).
pub use search_content::SearchContent;
// Re-export OSC protocol types at crate root (Part of #5663).
pub use osc::{
    Iterm2CellSize, Iterm2SetColor, Iterm2ShellIntegrationVersion, MULTIPART_FILE_MAX_SIZE,
    MultipartFileOperation, MultipartFileState, Notification, NotificationUrgency, RemoteHost,
    SemanticBlock, SemanticBlockEvent, SemanticButton, SemanticButtonEvent, SemanticButtonType,
    TaskbarProgress, TextSizingAlignment, TextSizingOperation,
};
// Re-export callback type aliases at crate root (Part of #5663, Phase 2).
pub use callback_types::{
    AdvancedNotificationCallback, BufferActivationCallback, ClipboardCallback, ColorChangeCallback,
    ColorChangeOp, CopyToClipboardCallback, DcsCallback, HighlightCursorLineCallback,
    KittyImageCallback, KittyImageData, KvpCallback, MultipartFileCallback, NotificationCallback,
    RemoteHostCallback, ReportCellSizeCallback, ReportVariableCallback, SemanticBlockCallback,
    SemanticButtonCallback, SetBadgeFormatCallback, SetColorsCallback, SetProfileCallback,
    ShellCallback, ShellIntegrationVersionCallback, TextSizingCallback, TitleCallback,
    TitleEventCallback, TitleType, WindowCallback,
};
// Re-export unified buffer types at crate root for ergonomic imports.
pub use buffer_access::BufferAccess;
pub use buffer_command::BufferCommand;
pub use buffer_view::{BufferMatch, BufferView};

// ============================================================================
// Terminal Size
// ============================================================================

/// Terminal dimensions (rows and columns).
///
/// Extracted to `aterm-types` so both `aterm-core` and integration crates
/// can share the same size type without circular dependencies (#2440, #2341).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TerminalSize {
    rows: u16,
    cols: u16,
}

impl TerminalSize {
    /// Create a new terminal size.
    #[must_use]
    pub const fn new(rows: u16, cols: u16) -> Self {
        Self { rows, cols }
    }

    /// Number of rows.
    #[must_use]
    pub const fn rows(&self) -> u16 {
        self.rows
    }

    /// Number of columns.
    #[must_use]
    pub const fn cols(&self) -> u16 {
        self.cols
    }
}

// ============================================================================
// SGR Dim (Faint) Factor
// ============================================================================

/// Dim (SGR faint) brightness multiplier applied to each RGB channel.
///
/// Centralized here so all render paths (CPU color resolve, FFI cell export,
/// GPU shader logic) share one authoritative value. The WGSL shader cannot
/// import Rust constants — keep its inline `0.5` in sync manually.
///
/// aterm uses 0.5; Alacritty upstream uses 0.66.
pub const DIM_FACTOR: f32 = 0.5;

// ============================================================================
// RGB Color Type
// ============================================================================
/// RGB color value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Rgb {
    /// Red component (0-255).
    pub r: u8,
    /// Green component (0-255).
    pub g: u8,
    /// Blue component (0-255).
    pub b: u8,
}

impl Rgb {
    /// Create a new RGB color.
    #[must_use]
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Calculate contrast ratio between two colors.
    ///
    /// Returns the WCAG relative luminance contrast ratio.
    #[must_use]
    pub fn contrast(self, other: Self) -> f64 {
        let lum1 = self.luminance();
        let lum2 = other.luminance();
        let (lighter, darker) = if lum1 > lum2 {
            (lum1, lum2)
        } else {
            (lum2, lum1)
        };
        (lighter + 0.05) / (darker + 0.05)
    }

    /// Calculate relative luminance per WCAG 2.0.
    fn luminance(self) -> f64 {
        fn linearize(c: u8) -> f64 {
            let c = f64::from(c) / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * linearize(self.r) + 0.7152 * linearize(self.g) + 0.0722 * linearize(self.b)
    }
}

impl std::ops::Mul<f32> for Rgb {
    type Output = Self;

    fn mul(self, rhs: f32) -> Self::Output {
        Self {
            r: (f32::from(self.r) * rhs).clamp(0.0, 255.0) as u8,
            g: (f32::from(self.g) * rhs).clamp(0.0, 255.0) as u8,
            b: (f32::from(self.b) * rhs).clamp(0.0, 255.0) as u8,
        }
    }
}

impl std::ops::Add for Rgb {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self {
            r: self.r.saturating_add(rhs.r),
            g: self.g.saturating_add(rhs.g),
            b: self.b.saturating_add(rhs.b),
        }
    }
}

// ============================================================================
// BiDi Mode
// ============================================================================

/// BiDi (Bidirectional Text) display mode.
///
/// These modes align with the Terminal Working Group BiDi specification:
/// <https://terminal-wg.pages.freedesktop.org/bidi/>
#[non_exhaustive]
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BiDiMode {
    /// BiDi processing disabled. Text displays in logical order.
    Disabled = 0,
    /// Implicit BiDi mode (default). Each line is automatically analyzed.
    #[default]
    Implicit = 1,
    /// Explicit BiDi mode. Application controls direction via escape sequences.
    Explicit = 2,
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // Rgb
    // =========================================================================

    #[test]
    fn rgb_new() {
        let c = Rgb::new(10, 20, 30);
        assert_eq!(c.r, 10);
        assert_eq!(c.g, 20);
        assert_eq!(c.b, 30);
    }

    #[test]
    fn rgb_eq() {
        assert_eq!(Rgb::new(0, 0, 0), Rgb::new(0, 0, 0));
        assert_ne!(Rgb::new(0, 0, 0), Rgb::new(0, 0, 1));
    }

    // =========================================================================
    // CursorStyle
    // =========================================================================

    #[test]
    fn cursor_style_default_is_blinking_block() {
        assert_eq!(CursorStyle::default(), CursorStyle::BlinkingBlock);
    }

    #[test]
    fn cursor_style_from_param_valid() {
        // Param 0 maps to BlinkingBlock (same as 1 per DECSCUSR)
        assert_eq!(CursorStyle::from_param(0), Some(CursorStyle::BlinkingBlock));
        assert_eq!(CursorStyle::from_param(1), Some(CursorStyle::BlinkingBlock));
        assert_eq!(CursorStyle::from_param(2), Some(CursorStyle::SteadyBlock));
        assert_eq!(
            CursorStyle::from_param(3),
            Some(CursorStyle::BlinkingUnderline)
        );
        assert_eq!(
            CursorStyle::from_param(4),
            Some(CursorStyle::SteadyUnderline)
        );
        assert_eq!(CursorStyle::from_param(5), Some(CursorStyle::BlinkingBar));
        assert_eq!(CursorStyle::from_param(6), Some(CursorStyle::SteadyBar));
    }

    #[test]
    fn cursor_style_from_param_invalid() {
        assert_eq!(CursorStyle::from_param(7), None);
        assert_eq!(CursorStyle::from_param(255), None);
        assert_eq!(CursorStyle::from_param(u16::MAX), None);
    }

    // =========================================================================
    // BiDiMode
    // =========================================================================

    #[test]
    fn bidi_mode_default_is_implicit() {
        assert_eq!(BiDiMode::default(), BiDiMode::Implicit);
    }

    /// Verify BiDiMode discriminants match checkpoint wire format (#7278).
    #[test]
    fn bidi_mode_discriminants_match_wire_format() {
        assert_eq!(BiDiMode::Disabled as u8, 0);
        assert_eq!(BiDiMode::Implicit as u8, 1);
        assert_eq!(BiDiMode::Explicit as u8, 2);
    }
}
