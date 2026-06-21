// Copyright 2026 The aterm Authors
// Author: The aterm Authors
// SPDX-License-Identifier: Apache-2.0

//! Text shaping configuration for terminal rendering.
//!
//! Core types are defined in `aterm-types::text_shaping` and re-exported here
//! for backward compatibility.

// Re-export core types from aterm-types (Part of #2584).
pub(crate) use aterm_types::text_shaping::TextShapingConfig;
// `FontFeature` is consumed only by the cfg(test) `parse_font_features` helper
// (and its tests); gate the re-export to those consumers so the default build
// stays warning-clean.
#[cfg(test)]
pub(crate) use aterm_types::text_shaping::FontFeature;
// AmbiguousWidth and LigatureMode are consumed only by test code within aterm-core;
// aterm-gpu imports them directly from aterm-types.
#[cfg(test)]
pub use aterm_types::text_shaping::{AmbiguousWidth, LigatureMode};

/// Parse font features from string like "+ss01 -calt".
///
/// Format:
/// - `+tag` enables feature (value = 1)
/// - `-tag` disables feature (value = 0)
/// - Tags must be 1-4 ASCII characters, padded with spaces if shorter
///
/// Note: WezTerm-style `tag=value` format is not yet supported (see Phase 4).
#[cfg(test)]
pub(crate) fn parse_font_features(s: &str) -> Vec<FontFeature> {
    let mut features = Vec::new();
    for token in s.split_whitespace() {
        let (tag_str, value) = if let Some(rest) = token.strip_prefix('+') {
            (rest, 1u32)
        } else if let Some(rest) = token.strip_prefix('-') {
            (rest, 0u32)
        } else {
            continue;
        };
        // Require 1-4 ASCII characters (empty tag is invalid)
        if !tag_str.is_empty() && tag_str.len() <= 4 && tag_str.is_ascii() {
            let mut tag = [b' '; 4];
            tag[..tag_str.len()].copy_from_slice(tag_str.as_bytes());
            features.push(FontFeature::new(tag, value));
        }
    }
    features
}

// FFI-safe text shaping types (AtermLigatureMode, AtermAmbiguousWidth,
// AtermFontFeature, AtermFontFeatureSet, AtermTextShapingConfig) now live
// in aterm-gpu/src/ffi/hybrid/text_shaping_ffi.rs. cbindgen picks them up
// via parse.include. See #2777.

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
#[path = "../test_support/text_shaping_config_tests.rs"]
mod tests;
