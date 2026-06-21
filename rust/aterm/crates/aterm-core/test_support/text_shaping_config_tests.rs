// Copyright 2026 The aterm Authors
// Author: The aterm Authors
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn test_ambiguous_width_default() {
    let config = TextShapingConfig::default();
    assert_eq!(config.ambiguous_char_width(), 1);
}

#[test]
fn test_ambiguous_width_double() {
    let config = TextShapingConfig {
        ambiguous_width: AmbiguousWidth::Double,
        ..Default::default()
    };
    assert_eq!(config.ambiguous_char_width(), 2);
}

#[test]
fn test_ligature_mode_enabled() {
    let config = TextShapingConfig {
        ligature_mode: LigatureMode::Enabled,
        ..Default::default()
    };
    assert!(!config.should_disable_ligatures(Some((0, 1)), 0, 0, 2));
    assert!(!config.should_disable_ligatures(None, 0, 0, 2));
}

#[test]
fn test_ligature_mode_disabled() {
    let config = TextShapingConfig {
        ligature_mode: LigatureMode::Disabled,
        ..Default::default()
    };
    assert!(config.should_disable_ligatures(Some((0, 1)), 0, 0, 2));
    assert!(config.should_disable_ligatures(None, 0, 0, 2));
}

#[test]
fn test_ligature_mode_cursor_disabled() {
    let config = TextShapingConfig {
        ligature_mode: LigatureMode::CursorDisabled,
        ..Default::default()
    };

    // Cursor at row 0 col 1, glyph spans cols 0-2 → overlaps
    assert!(config.should_disable_ligatures(Some((0, 1)), 0, 0, 2));

    // Cursor at row 0 col 5, glyph spans cols 0-2 → no overlap
    assert!(!config.should_disable_ligatures(Some((0, 5)), 0, 0, 2));

    // Cursor at row 1, glyph on row 0 → different row
    assert!(!config.should_disable_ligatures(Some((1, 1)), 0, 0, 2));

    // No cursor visible → ligatures enabled
    assert!(!config.should_disable_ligatures(None, 0, 0, 2));

    // Cursor at boundary (col 0, start of glyph)
    assert!(config.should_disable_ligatures(Some((0, 0)), 0, 0, 2));

    // Cursor at boundary (col 2, end exclusive)
    assert!(!config.should_disable_ligatures(Some((0, 2)), 0, 0, 2));
}

#[test]
fn test_parse_font_features_basic() {
    let features = parse_font_features("+ss01 -calt");
    assert_eq!(features.len(), 2);
    assert_eq!(&features[0].tag, b"ss01");
    assert_eq!(features[0].value, 1);
    assert_eq!(&features[1].tag, b"calt");
    assert_eq!(features[1].value, 0);
}

#[test]
fn test_parse_font_features_short_tag() {
    let features = parse_font_features("+cv1");
    assert_eq!(features.len(), 1);
    assert_eq!(&features[0].tag, b"cv1 ");
    assert_eq!(features[0].value, 1);
}

#[test]
fn test_parse_font_features_ignores_invalid() {
    // No prefix
    let features = parse_font_features("ss01 calt");
    assert!(features.is_empty());

    // Too long (>4 chars)
    let features = parse_font_features("+toolong");
    assert!(features.is_empty());

    // Empty tag (just prefix)
    let features = parse_font_features("+ -");
    assert!(features.is_empty());
}

#[test]
fn test_parse_font_features_empty() {
    let features = parse_font_features("");
    assert!(features.is_empty());

    let features = parse_font_features("   ");
    assert!(features.is_empty());
}

#[test]
fn test_font_feature_new() {
    let feature = FontFeature::new(*b"liga", 1);
    assert_eq!(&feature.tag, b"liga");
    assert_eq!(feature.value, 1);
}

#[test]
fn test_ligature_mode_discriminant() {
    assert_eq!(LigatureMode::Enabled as u8, 0);
    assert_eq!(LigatureMode::CursorDisabled as u8, 1);
    assert_eq!(LigatureMode::Disabled as u8, 2);
}

#[test]
fn test_ambiguous_width_discriminant() {
    assert_eq!(AmbiguousWidth::Single as u8, 0);
    assert_eq!(AmbiguousWidth::Double as u8, 1);
}
