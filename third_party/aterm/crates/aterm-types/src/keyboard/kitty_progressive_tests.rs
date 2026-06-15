// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Tests for Kitty progressive-enhancement keyboard flags.
//!
//! Validates behavioral encoding differences when `REPORT_ALTERNATE_KEYS`,
//! `REPORT_ALL_KEYS_AS_ESC`, and `REPORT_ASSOCIATED_TEXT` are enabled,
//! including edge cases (named keys, modifier suppression, event type
//! interactions, and flag combinations).

use super::*;

// =========================================================================
// Kitty progressive-enhancement flags: flag presence
// =========================================================================

#[test]
fn mode_new_kitty_flags_default_off() {
    let mode = KeyboardMode::empty();
    assert!(!mode.contains(KeyboardMode::REPORT_ALTERNATE_KEYS));
    assert!(!mode.contains(KeyboardMode::REPORT_ALL_KEYS_AS_ESC));
    assert!(!mode.contains(KeyboardMode::REPORT_ASSOCIATED_TEXT));
}

#[test]
fn mode_new_kitty_flags_independent() {
    let alt = KeyboardMode::REPORT_ALTERNATE_KEYS;
    assert!(alt.contains(KeyboardMode::REPORT_ALTERNATE_KEYS));
    assert!(!alt.contains(KeyboardMode::REPORT_ALL_KEYS_AS_ESC));
    assert!(!alt.contains(KeyboardMode::REPORT_ASSOCIATED_TEXT));

    let all = KeyboardMode::REPORT_ALL_KEYS_AS_ESC;
    assert!(!all.contains(KeyboardMode::REPORT_ALTERNATE_KEYS));
    assert!(all.contains(KeyboardMode::REPORT_ALL_KEYS_AS_ESC));

    let text = KeyboardMode::REPORT_ASSOCIATED_TEXT;
    assert!(text.contains(KeyboardMode::REPORT_ASSOCIATED_TEXT));
    assert!(!text.contains(KeyboardMode::REPORT_ALL_KEYS_AS_ESC));
}

#[test]
fn mode_all_kitty_flags_combinable() {
    let mode = KeyboardMode::DISAMBIGUATE_ESC_CODES
        | KeyboardMode::REPORT_EVENT_TYPES
        | KeyboardMode::REPORT_ALTERNATE_KEYS
        | KeyboardMode::REPORT_ALL_KEYS_AS_ESC
        | KeyboardMode::REPORT_ASSOCIATED_TEXT;
    assert!(mode.contains(KeyboardMode::DISAMBIGUATE_ESC_CODES));
    assert!(mode.contains(KeyboardMode::REPORT_EVENT_TYPES));
    assert!(mode.contains(KeyboardMode::REPORT_ALTERNATE_KEYS));
    assert!(mode.contains(KeyboardMode::REPORT_ALL_KEYS_AS_ESC));
    assert!(mode.contains(KeyboardMode::REPORT_ASSOCIATED_TEXT));
}

// =========================================================================
// REPORT_ALL_KEYS_AS_ESC: forces CSI-u encoding path
// =========================================================================

#[test]
fn report_all_keys_forces_csi_u_for_plain_character() {
    // With REPORT_ALL_KEYS_AS_ESC alone (no DISAMBIGUATE), a plain 'a' should
    // still use the CSI-u path (not legacy single-byte output).
    let mode = KeyboardMode::REPORT_ALL_KEYS_AS_ESC;
    let result = encode_key(&Key::Character('a'), Modifiers::empty(), mode);
    // CSI-u for 'a' (U+0061 = 97): ESC [ 97 u
    assert_eq!(result, b"\x1b[97u");
}

#[test]
fn report_all_keys_forces_csi_u_for_enter() {
    let mode = KeyboardMode::REPORT_ALL_KEYS_AS_ESC;
    let result = encode_key(&Key::Named(NamedKey::Enter), Modifiers::empty(), mode);
    // CSI-u for Enter uses its Kitty code
    let expected_code = NamedKey::Enter.kitty_code();
    let expected = format!("\x1b[{expected_code}u");
    assert_eq!(result, expected.as_bytes());
}

#[test]
fn report_all_keys_without_disambiguate_uses_csi_u() {
    // REPORT_ALL_KEYS_AS_ESC without DISAMBIGUATE_ESC_CODES should still
    // produce CSI-u output, not legacy.
    let mode = KeyboardMode::REPORT_ALL_KEYS_AS_ESC;
    let result = encode_key(&Key::Named(NamedKey::Tab), Modifiers::empty(), mode);
    let expected_code = NamedKey::Tab.kitty_code();
    let expected = format!("\x1b[{expected_code}u");
    assert_eq!(result, expected.as_bytes());
}

// =========================================================================
// REPORT_ALTERNATE_KEYS: shifted alternate codepoints
// =========================================================================

#[test]
fn report_alternate_keys_adds_shifted_letter_codepoint() {
    let mode = KeyboardMode::DISAMBIGUATE_ESC_CODES | KeyboardMode::REPORT_ALTERNATE_KEYS;
    let result = encode_key(&Key::Character('a'), Modifiers::SHIFT, mode);
    assert_eq!(result, b"\x1b[97:65;2u");
}

#[test]
fn report_alternate_keys_adds_shifted_symbol_codepoint() {
    let mode = KeyboardMode::DISAMBIGUATE_ESC_CODES | KeyboardMode::REPORT_ALTERNATE_KEYS;
    let result = encode_key(&Key::Character('1'), Modifiers::SHIFT, mode);
    assert_eq!(result, b"\x1b[49:33;2u");
}

#[test]
fn report_alternate_keys_named_key_has_no_alternate() {
    // Named keys (arrows, function keys) never have alternate codepoints.
    // ArrowUp retains legacy CSI format under Kitty protocol (#7474).
    let mode = KeyboardMode::DISAMBIGUATE_ESC_CODES | KeyboardMode::REPORT_ALTERNATE_KEYS;
    let result = encode_key(&Key::Named(NamedKey::ArrowUp), Modifiers::SHIFT, mode);
    // Legacy format with Kitty modifier encoding: CSI 1;2 A
    assert_eq!(result, b"\x1b[1;2A");
}

#[test]
fn report_alternate_keys_unshifted_character_has_no_alternate() {
    // Without Shift, there is no alternate codepoint to report.
    let mode = KeyboardMode::DISAMBIGUATE_ESC_CODES | KeyboardMode::REPORT_ALTERNATE_KEYS;
    let result = encode_key(&Key::Character('a'), Modifiers::empty(), mode);
    assert_eq!(result, b"\x1b[97u");
}

#[test]
fn report_alternate_keys_ctrl_shift_still_emits_alternate() {
    // Ctrl+Shift+'a': alternate codepoint 'A' should still appear even with Ctrl.
    // shifted_character only checks Shift — Ctrl doesn't suppress the alternate.
    let mode = KeyboardMode::DISAMBIGUATE_ESC_CODES | KeyboardMode::REPORT_ALTERNATE_KEYS;
    let result = encode_key(
        &Key::Character('a'),
        Modifiers::CTRL | Modifiers::SHIFT,
        mode,
    );
    // primary=97, alternate=65, mod=6 (Shift=1 + Ctrl=4 → kitty_encoded = 5+1=6)
    assert_eq!(result, b"\x1b[97:65;6u");
}

#[test]
fn report_alternate_keys_same_char_when_not_mappable() {
    // Shift+'~' maps to '~' itself (identity), so alternate is suppressed.
    let mode = KeyboardMode::DISAMBIGUATE_ESC_CODES | KeyboardMode::REPORT_ALTERNATE_KEYS;
    let result = encode_key(&Key::Character('~'), Modifiers::SHIFT, mode);
    // '~' = U+007E = 126; shifted '~' is still '~', so no alternate
    assert_eq!(result, b"\x1b[126;2u");
}

// =========================================================================
// REPORT_ASSOCIATED_TEXT: text-as-codepoints third parameter
// =========================================================================

#[test]
fn report_associated_text_requires_report_all_keys_mode() {
    let mode = KeyboardMode::DISAMBIGUATE_ESC_CODES | KeyboardMode::REPORT_ASSOCIATED_TEXT;
    let result = encode_key(&Key::Character('a'), Modifiers::empty(), mode);
    assert_eq!(result, b"\x1b[97u");
}

#[test]
fn report_associated_text_appends_codepoint() {
    let mode = KeyboardMode::REPORT_ALL_KEYS_AS_ESC | KeyboardMode::REPORT_ASSOCIATED_TEXT;
    let result = encode_key(&Key::Character('a'), Modifiers::empty(), mode);
    assert_eq!(result, b"\x1b[97;1;97u");
}

#[test]
fn report_associated_text_uses_shifted_character() {
    let mode = KeyboardMode::REPORT_ALL_KEYS_AS_ESC | KeyboardMode::REPORT_ASSOCIATED_TEXT;
    let result = encode_key(&Key::Character('a'), Modifiers::SHIFT, mode);
    assert_eq!(result, b"\x1b[97;2;65u");
}

#[test]
fn report_associated_text_omits_payload_for_release_events() {
    let mode = KeyboardMode::REPORT_ALL_KEYS_AS_ESC
        | KeyboardMode::REPORT_ASSOCIATED_TEXT
        | KeyboardMode::REPORT_EVENT_TYPES;
    let result = encode_key_with_event(
        &Key::Character('a'),
        Modifiers::empty(),
        mode,
        KeyEventType::Release,
    );
    assert_eq!(result, b"\x1b[97;1:3u");
}

#[test]
fn report_associated_text_preserved_for_repeat_events() {
    // Repeat events (unlike Release) should carry associated text per Kitty spec.
    let mode = KeyboardMode::REPORT_ALL_KEYS_AS_ESC
        | KeyboardMode::REPORT_ASSOCIATED_TEXT
        | KeyboardMode::REPORT_EVENT_TYPES;
    let result = encode_key_with_event(
        &Key::Character('a'),
        Modifiers::empty(),
        mode,
        KeyEventType::Repeat,
    );
    // mod=1 (to carry event+text), event=2(repeat), text=97('a')
    assert_eq!(result, b"\x1b[97;1:2;97u");
}

#[test]
fn report_associated_text_omits_payload_for_alt_modified_text() {
    let mode = KeyboardMode::REPORT_ALL_KEYS_AS_ESC | KeyboardMode::REPORT_ASSOCIATED_TEXT;
    let result = encode_key(&Key::Character('a'), Modifiers::ALT, mode);
    assert_eq!(result, b"\x1b[97;3u");
}

#[test]
fn report_associated_text_omits_payload_for_ctrl_modified() {
    // Ctrl modifier suppresses text payload (matches Kitty/Terminal behavior).
    let mode = KeyboardMode::REPORT_ALL_KEYS_AS_ESC | KeyboardMode::REPORT_ASSOCIATED_TEXT;
    let result = encode_key(&Key::Character('a'), Modifiers::CTRL, mode);
    assert_eq!(result, b"\x1b[97;5u");
}

#[test]
fn report_associated_text_omits_payload_for_super_modified() {
    // Super modifier suppresses text payload.
    let mode = KeyboardMode::REPORT_ALL_KEYS_AS_ESC | KeyboardMode::REPORT_ASSOCIATED_TEXT;
    let result = encode_key(&Key::Character('a'), Modifiers::SUPER, mode);
    assert_eq!(result, b"\x1b[97;9u");
}

#[test]
fn report_associated_text_omits_payload_for_named_key() {
    // Named keys (Enter, Tab, etc.) never carry associated text.
    let mode = KeyboardMode::REPORT_ALL_KEYS_AS_ESC | KeyboardMode::REPORT_ASSOCIATED_TEXT;
    let result = encode_key(&Key::Named(NamedKey::Enter), Modifiers::empty(), mode);
    assert_eq!(result, b"\x1b[13u");
}

#[test]
fn report_associated_text_shifted_symbol_pipeline() {
    // Shift+'1' produces '!' (U+0021 = 33) as associated text.
    let mode = KeyboardMode::REPORT_ALL_KEYS_AS_ESC | KeyboardMode::REPORT_ASSOCIATED_TEXT;
    let result = encode_key(&Key::Character('1'), Modifiers::SHIFT, mode);
    // primary=49('1'), mod=2(shift), text=33('!')
    assert_eq!(result, b"\x1b[49;2;33u");
}

// =========================================================================
// Combined progressive flags
// =========================================================================

#[test]
fn all_three_progressive_flags_combined() {
    // REPORT_ALTERNATE_KEYS + REPORT_ALL_KEYS_AS_ESC + REPORT_ASSOCIATED_TEXT
    // Shift+'a': primary=97, alternate=65('A'), mod=2, text=65('A')
    let mode = KeyboardMode::REPORT_ALL_KEYS_AS_ESC
        | KeyboardMode::REPORT_ALTERNATE_KEYS
        | KeyboardMode::REPORT_ASSOCIATED_TEXT;
    let result = encode_key(&Key::Character('a'), Modifiers::SHIFT, mode);
    assert_eq!(result, b"\x1b[97:65;2;65u");
}

#[test]
fn all_three_progressive_flags_unshifted() {
    // All flags on, no shift: primary=97, no alternate, mod=1 (to carry text), text=97
    let mode = KeyboardMode::REPORT_ALL_KEYS_AS_ESC
        | KeyboardMode::REPORT_ALTERNATE_KEYS
        | KeyboardMode::REPORT_ASSOCIATED_TEXT;
    let result = encode_key(&Key::Character('a'), Modifiers::empty(), mode);
    assert_eq!(result, b"\x1b[97;1;97u");
}

#[test]
fn all_five_kitty_flags_with_release_event() {
    // All 5 Kitty flags enabled, release event: text is omitted per spec.
    let mode = KeyboardMode::DISAMBIGUATE_ESC_CODES
        | KeyboardMode::REPORT_EVENT_TYPES
        | KeyboardMode::REPORT_ALTERNATE_KEYS
        | KeyboardMode::REPORT_ALL_KEYS_AS_ESC
        | KeyboardMode::REPORT_ASSOCIATED_TEXT;
    let result = encode_key_with_event(
        &Key::Character('a'),
        Modifiers::SHIFT,
        mode,
        KeyEventType::Release,
    );
    // primary=97, alternate=65, mod=2, event=3(release), no text
    assert_eq!(result, b"\x1b[97:65;2:3u");
}

// =========================================================================
// REPORT_EVENT_TYPES-only: modifier key release/press (#5996)
//
// When REPORT_EVENT_TYPES is set without DISAMBIGUATE_ESC_CODES, modifier
// key release events must produce CSI-u sequences (so applications can
// detect when a modifier is released), while press events remain silent
// (no legacy escape sequence exists for bare modifier presses).
// =========================================================================

#[test]
fn modifier_shift_left_release_with_report_event_types_only() {
    let result = encode_key_with_event(
        &Key::Named(NamedKey::ShiftLeft),
        Modifiers::SHIFT,
        KeyboardMode::REPORT_EVENT_TYPES,
        KeyEventType::Release,
    );
    // ShiftLeft=57441; release removes SHIFT → mod=1; event=3(release)
    assert_eq!(result, b"\x1b[57441;1:3u");
}

#[test]
fn modifier_control_left_release_with_report_event_types_only() {
    let result = encode_key_with_event(
        &Key::Named(NamedKey::ControlLeft),
        Modifiers::CTRL,
        KeyboardMode::REPORT_EVENT_TYPES,
        KeyEventType::Release,
    );
    assert_eq!(result, b"\x1b[57442;1:3u");
}

#[test]
fn modifier_alt_left_release_with_report_event_types_only() {
    let result = encode_key_with_event(
        &Key::Named(NamedKey::AltLeft),
        Modifiers::ALT,
        KeyboardMode::REPORT_EVENT_TYPES,
        KeyEventType::Release,
    );
    assert_eq!(result, b"\x1b[57443;1:3u");
}

#[test]
fn modifier_shift_left_press_with_report_event_types_only_is_empty() {
    let result = encode_key_with_event(
        &Key::Named(NamedKey::ShiftLeft),
        Modifiers::empty(),
        KeyboardMode::REPORT_EVENT_TYPES,
        KeyEventType::Press,
    );
    // Modifier press has no legacy escape sequence; REPORT_EVENT_TYPES-only
    // does not promote press events to Kitty encoding.
    assert!(result.is_empty());
}

#[test]
fn modifier_control_left_press_with_report_event_types_only_is_empty() {
    let result = encode_key_with_event(
        &Key::Named(NamedKey::ControlLeft),
        Modifiers::empty(),
        KeyboardMode::REPORT_EVENT_TYPES,
        KeyEventType::Press,
    );
    assert!(result.is_empty());
}
