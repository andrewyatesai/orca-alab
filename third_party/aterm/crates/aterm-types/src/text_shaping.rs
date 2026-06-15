// Copyright 2026 The aterm Authors
// Author: The aterm Authors
// SPDX-License-Identifier: Apache-2.0

//! Text shaping configuration types for terminal rendering.
//!
//! Provides configuration types for:
//! - Ligature rendering mode
//! - OpenType font features
//! - Ambiguous-width character handling
//!
//! These settings flow from UI → FFI → rendering pipeline, affecting both
//! text shaping and grapheme width calculation.
//!
//! Extracted from `aterm-core::text_shaping_config` to break cross-crate
//! dependency chains (Part of #2584).

/// Ambiguous-width character handling.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
#[repr(u8)]
pub enum AmbiguousWidth {
    /// Single-width (default).
    #[default]
    Single = 0,
    /// Double-width (CJK mode).
    Double = 1,
}

/// Ligature rendering mode.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
#[repr(u8)]
pub enum LigatureMode {
    /// Always render ligatures.
    #[default]
    Enabled = 0,
    /// Disable ligatures at cursor position.
    CursorDisabled = 1,
    /// Never render ligatures.
    Disabled = 2,
}

/// OpenType font feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
#[repr(C)]
pub struct FontFeature {
    /// 4-byte OpenType tag (e.g., b"calt", b"ss01").
    pub tag: [u8; 4],
    /// Feature value: 0 = disabled, 1 = enabled, >1 for stylistic alternates.
    pub value: u32,
}

impl FontFeature {
    /// Create a new font feature.
    #[must_use]
    pub const fn new(tag: [u8; 4], value: u32) -> Self {
        Self { tag, value }
    }
}

/// Per-font feature set.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FontFeatureSet {
    /// Font identifier.
    pub font_id: u32,
    /// Feature overrides.
    pub features: Vec<FontFeature>,
}

/// Text shaping configuration.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextShapingConfig {
    /// Ligature rendering mode.
    pub ligature_mode: LigatureMode,
    /// Ambiguous-width character handling.
    pub ambiguous_width: AmbiguousWidth,
    /// Per-font OpenType features.
    pub font_features: Vec<FontFeatureSet>,
}

impl TextShapingConfig {
    /// Get display width for ambiguous characters (1 or 2).
    #[inline]
    #[must_use]
    pub const fn ambiguous_char_width(&self) -> usize {
        match self.ambiguous_width {
            AmbiguousWidth::Single => 1,
            AmbiguousWidth::Double => 2,
        }
    }

    /// Check if ligatures should be disabled for a glyph run given cursor position.
    ///
    /// Parameters:
    /// - `cursor`: Optional (row, col) tuple. None if cursor not visible.
    /// - `shaping_row`: The row being shaped (0-indexed from viewport top).
    /// - `glyph_start_col`: Start column of the ligature glyph run.
    /// - `glyph_end_col`: End column (exclusive) of the ligature glyph run.
    ///
    /// Returns true if:
    /// - `ligature_mode == Disabled`, OR
    /// - `ligature_mode == CursorDisabled` AND cursor is ON this row AND within glyph range
    #[inline]
    #[must_use]
    pub fn should_disable_ligatures(
        &self,
        cursor: Option<(usize, usize)>,
        shaping_row: usize,
        glyph_start_col: usize,
        glyph_end_col: usize,
    ) -> bool {
        match self.ligature_mode {
            LigatureMode::Enabled => false,
            LigatureMode::Disabled => true,
            LigatureMode::CursorDisabled => {
                if let Some((cursor_row, cursor_col)) = cursor {
                    cursor_row == shaping_row
                        && cursor_col >= glyph_start_col
                        && cursor_col < glyph_end_col
                } else {
                    false
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ambiguous_width_default_is_single() {
        assert_eq!(AmbiguousWidth::default(), AmbiguousWidth::Single);
    }

    #[test]
    fn ligature_mode_default_is_enabled() {
        assert_eq!(LigatureMode::default(), LigatureMode::Enabled);
    }

    #[test]
    fn text_shaping_config_default() {
        let cfg = TextShapingConfig::default();
        assert_eq!(cfg.ligature_mode, LigatureMode::Enabled);
        assert_eq!(cfg.ambiguous_width, AmbiguousWidth::Single);
        assert!(cfg.font_features.is_empty());
    }

    #[test]
    fn ambiguous_char_width_single() {
        let cfg = TextShapingConfig::default();
        assert_eq!(cfg.ambiguous_char_width(), 1);
    }

    #[test]
    fn ambiguous_char_width_double() {
        let cfg = TextShapingConfig {
            ambiguous_width: AmbiguousWidth::Double,
            ..Default::default()
        };
        assert_eq!(cfg.ambiguous_char_width(), 2);
    }

    #[test]
    fn should_disable_ligatures_enabled_mode() {
        let cfg = TextShapingConfig::default();
        assert!(!cfg.should_disable_ligatures(Some((0, 5)), 0, 3, 8));
    }

    #[test]
    fn should_disable_ligatures_disabled_mode() {
        let cfg = TextShapingConfig {
            ligature_mode: LigatureMode::Disabled,
            ..Default::default()
        };
        assert!(cfg.should_disable_ligatures(None, 0, 0, 10));
    }

    #[test]
    fn should_disable_ligatures_cursor_disabled_mode() {
        let cfg = TextShapingConfig {
            ligature_mode: LigatureMode::CursorDisabled,
            ..Default::default()
        };
        // Cursor on same row, within glyph range
        assert!(cfg.should_disable_ligatures(Some((0, 5)), 0, 3, 8));
        // Cursor on different row
        assert!(!cfg.should_disable_ligatures(Some((1, 5)), 0, 3, 8));
        // Cursor before glyph range
        assert!(!cfg.should_disable_ligatures(Some((0, 2)), 0, 3, 8));
        // Cursor after glyph range
        assert!(!cfg.should_disable_ligatures(Some((0, 8)), 0, 3, 8));
        // No cursor
        assert!(!cfg.should_disable_ligatures(None, 0, 3, 8));
    }

    #[test]
    fn font_feature_new() {
        let f = FontFeature::new(*b"calt", 1);
        assert_eq!(f.tag, *b"calt");
        assert_eq!(f.value, 1);
    }
}
