// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Regression tests for keyboard encoding internals (write_u32), plus the
//! KEYBOARD-SHIFT REFINEMENT obligation (the always-on twin of the Trust
//! `a7_keyboard_shift` SMT bundle / `clean/keyboard_shift.lean`).

use super::{Key, KeyboardMode, Modifiers, encode_key, shifted_character, write_u32};

// =========================================================================
// KEYBOARD-SHIFT SPEC — the single source of truth, authored INDEPENDENTLY of
// the implementation (from the ANSI US-QWERTY layout), so this is a genuine
// refinement check, not a tautology against the code's own table.
//
// This is the spec the "Shift doesn't work" regression violated: the legacy
// encoder applied Shift with `to_ascii_uppercase`, which is the identity on
// every non-letter, so Shift+2 emitted '2' instead of '@'. The two load-bearing
// properties below — REFINEMENT (the encoder equals this spec) and EFFECTIVENESS
// (Shift changes every shiftable key) — each independently forbid that bug.
// =========================================================================

/// SPEC: the US-QWERTY glyph produced by holding Shift on the key whose
/// unshifted character is `c`. `None` for keys with no distinct shifted form.
/// Authored from the physical ANSI layout, NOT from `shifted_character`.
fn spec_shifted(c: char) -> Option<char> {
    Some(match c {
        'a'..='z' => c.to_ascii_uppercase(),
        '1' => '!',
        '2' => '@',
        '3' => '#',
        '4' => '$',
        '5' => '%',
        '6' => '^',
        '7' => '&',
        '8' => '*',
        '9' => '(',
        '0' => ')',
        '`' => '~',
        '-' => '_',
        '=' => '+',
        '[' => '{',
        ']' => '}',
        '\\' => '|',
        ';' => ':',
        '\'' => '"',
        ',' => '<',
        '.' => '>',
        '/' => '?',
        _ => return None,
    })
}

/// THEOREM (refinement, `a7_keyboard_shift/shift_refines_spec.smt2` twin): for
/// every key with a distinct shifted glyph, the engine's single shift map
/// `shifted_character` — the one BOTH the legacy and Kitty paths now use —
/// equals the independent spec. A second shift table that disagreed (the
/// original `to_ascii_uppercase` legacy branch) cannot pass this.
#[test]
fn shifted_character_refines_independent_spec() {
    for b in 0u8..=127 {
        let c = b as char;
        if let Some(want) = spec_shifted(c) {
            assert_eq!(
                shifted_character(c, Modifiers::SHIFT),
                Some(want),
                "shift map diverges from the ANSI spec at {c:?}"
            );
        }
    }
}

/// THEOREM (effectiveness, `shift_is_effective.smt2` twin): Shift must CHANGE
/// every shiftable key end-to-end in legacy mode. This is the property the bug
/// most directly broke and the one that needs no knowledge of the exact glyph:
/// `encode(Shift+c) != encode(c)` for every key that has a shifted form.
#[test]
fn legacy_shift_changes_every_shiftable_key() {
    for b in 0u8..=127 {
        let c = b as char;
        if spec_shifted(c).is_some() {
            let shifted = encode_key(&Key::Character(c), Modifiers::SHIFT, KeyboardMode::empty());
            let plain = encode_key(
                &Key::Character(c),
                Modifiers::empty(),
                KeyboardMode::empty(),
            );
            assert_ne!(
                shifted, plain,
                "Shift on a shiftable key {c:?} must not emit the unshifted byte"
            );
        }
    }
}

/// PROVE-AND-CATCH (`catches_uppercase_bug_sat.smt2` twin): the obligation has
/// teeth — the ORIGINAL buggy `to_ascii_uppercase`-only shift map FAILS the spec
/// on at least one shiftable key. If this ever finds zero counterexamples the
/// refinement test above has gone vacuous.
#[test]
fn uppercase_only_shift_is_caught_by_the_spec() {
    let buggy = |c: char| c.to_ascii_uppercase(); // the pre-fix legacy branch
    let mut caught = Vec::new();
    for b in 0u8..=127 {
        let c = b as char;
        if let Some(spec) = spec_shifted(c)
            && buggy(c) != spec
        {
            caught.push(c);
        }
    }
    assert!(
        caught.contains(&'2') && caught.len() >= 20,
        "the spec must reject the to_ascii_uppercase bug on the digit/symbol rows; caught={caught:?}"
    );
}

/// Regression test: write_u32 with values >= 2,000,000,000 must not
/// infinite-loop or produce incorrect output.
///
/// Bug #2775: The original implementation used `u32` for the divisor.
/// When val >= 2B, `divisor * 10` overflowed u32, causing an infinite
/// loop in the digit-extraction while loop. Fix: use u64 for divisor.
#[test]
fn write_u32_two_billion_boundary() {
    let mut buf = Vec::new();
    write_u32(&mut buf, 2_000_000_000);
    assert_eq!(buf, b"2000000000");
}

#[test]
fn write_u32_max() {
    let mut buf = Vec::new();
    write_u32(&mut buf, u32::MAX); // 4294967295
    assert_eq!(buf, b"4294967295");
}

#[test]
fn write_u32_zero() {
    let mut buf = Vec::new();
    write_u32(&mut buf, 0);
    assert_eq!(buf, b"0");
}

#[test]
fn write_u32_small_values() {
    let mut buf = Vec::new();
    write_u32(&mut buf, 1);
    assert_eq!(buf, b"1");

    buf.clear();
    write_u32(&mut buf, 97); // 'a' codepoint
    assert_eq!(buf, b"97");

    buf.clear();
    write_u32(&mut buf, 999_999_999);
    assert_eq!(buf, b"999999999");
}
