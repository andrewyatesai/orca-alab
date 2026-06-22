// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0

//! Named color schemes ("theme palettes") — the SINGLE source of truth for a
//! terminal's colors, the way iTerm2 / Windows Terminal / Alacritty / Kitty do it.
//!
//! A [`ColorScheme`] bundles the foreground/background/cursor/selection chrome AND
//! the full 16-entry ANSI palette. It projects to BOTH sinks the GUI drives:
//!   * the renderer chrome — via [`ColorScheme::to_theme_parts`] (packed `u32`s the
//!     GUI assembles into an `aterm_render::Theme`); and
//!   * the engine indexed palette — via [`ColorScheme::to_color_palette`].
//!
//! It deliberately exposes ONLY [`Rgb`] / packed-`u32` primitives (never an
//! `aterm_render::Theme`), so `aterm-types` stays at the bottom of the crate graph
//! (`aterm-render -> aterm-core -> aterm-types`); the `Theme`/`TerminalConfig`
//! projections live in `aterm-gui`, which already owns both.

use crate::{ColorPalette, Rgb};

/// Whether a scheme is meant for a dark or light background. Drives the optional
/// `dark:…,light:…` auto-split (a future GUI concern); carried here so a scheme
/// is self-describing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Appearance {
    /// A scheme intended for a dark background (the default).
    #[default]
    Dark,
    /// A scheme intended for a light background.
    Light,
}

/// The renderer chrome of a scheme as packed `0x00RRGGBB` values — the exact shape
/// `aterm_render::Theme` wants, but as plain `u32`s so this crate need not depend on
/// the renderer (no crate cycle). The GUI builds the `Theme` from these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThemeParts {
    /// Default foreground / window clear foreground.
    pub fg: u32,
    /// Default background / window clear color (also the interior-padding fill).
    pub bg: u32,
    /// Cursor color.
    pub cursor: u32,
    /// Selection highlight background.
    pub selection: u32,
}

/// A complete named color scheme: chrome + the 16 ANSI slots.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ColorScheme {
    /// Display name (e.g. `"Dracula"`); `""` for an anonymous inline scheme.
    pub name: String,
    /// Dark/light hint.
    pub appearance: Appearance,
    /// Default foreground.
    pub foreground: Rgb,
    /// Default background (and window clear / interior padding fill).
    pub background: Rgb,
    /// Cursor color; `None` follows the foreground (matches the engine's fallback).
    pub cursor: Option<Rgb>,
    /// Selection highlight bg; `None` uses the renderer's built-in selection color.
    pub selection: Option<Rgb>,
    /// The 16 ANSI slots: `[0..8)` normal (black, red, green, yellow, blue, magenta,
    /// cyan, white), `[8..16)` their bright variants. THE authoritative ANSI table.
    pub ansi: [Rgb; 16],
}

/// The renderer's built-in selection color (`aterm_render::Theme::default().selection`,
/// `#264F78`), used when a scheme leaves `selection` as `None`. Verified by the
/// `default_theme_parts_are_historical` test in this module.
const SELECTION_DEFAULT: Rgb = Rgb::new(0x26, 0x4F, 0x78);

impl Default for ColorScheme {
    /// Byte-identical to aterm's historical look: fg `#D0D0D0`, bg `#111318`, cursor
    /// `#50FA7B`, selection `#264F78`, and `ansi[i] == ColorPalette::default_color(i)`
    /// (so an empty config resolves to exactly today's colors).
    fn default() -> Self {
        let mut ansi = [Rgb::new(0, 0, 0); 16];
        for (i, slot) in ansi.iter_mut().enumerate() {
            *slot = ColorPalette::default_color(i as u8);
        }
        Self {
            name: "Default".to_string(),
            appearance: Appearance::Dark,
            foreground: Rgb::new(0xD0, 0xD0, 0xD0),
            background: Rgb::new(0x11, 0x13, 0x18),
            cursor: Some(Rgb::new(0x50, 0xFA, 0x7B)),
            selection: Some(SELECTION_DEFAULT),
            ansi,
        }
    }
}

impl ColorScheme {
    /// Build the engine indexed palette from this scheme: each of the 16 ANSI slots
    /// is applied via [`ColorPalette::set`], leaving indices 16-255 as the computed
    /// xterm cube/ramp. (`set` elides a slot that equals `default_color(i)`, but that
    /// is render-identical: `get(i)` then falls back to the same `default_color(i)`.)
    #[must_use]
    pub fn to_color_palette(&self) -> ColorPalette {
        let mut p = ColorPalette::new();
        for (i, c) in self.ansi.iter().enumerate() {
            p.set(i as u8, *c);
        }
        p
    }

    /// Project the chrome to packed `0x00RRGGBB` [`ThemeParts`]. `cursor` falls back
    /// to the foreground and `selection` to the renderer default when unset.
    #[must_use]
    pub fn to_theme_parts(&self) -> ThemeParts {
        let pack = |c: Rgb| (u32::from(c.r) << 16) | (u32::from(c.g) << 8) | u32::from(c.b);
        ThemeParts {
            fg: pack(self.foreground),
            bg: pack(self.background),
            cursor: pack(self.cursor.unwrap_or(self.foreground)),
            selection: pack(self.selection.unwrap_or(SELECTION_DEFAULT)),
        }
    }
}

/// A compile-time built-in scheme: all-`Rgb` (const-constructible) so the registry
/// lives in `.rodata` with no allocation. Materialized into a [`ColorScheme`] (which
/// owns a `String` name) on lookup.
struct Builtin {
    name: &'static str,
    appearance: Appearance,
    fg: Rgb,
    bg: Rgb,
    cursor: Rgb,
    selection: Rgb,
    ansi: [Rgb; 16],
}

impl Builtin {
    fn to_scheme(&self) -> ColorScheme {
        ColorScheme {
            name: self.name.to_string(),
            appearance: self.appearance,
            foreground: self.fg,
            background: self.bg,
            cursor: Some(self.cursor),
            selection: Some(self.selection),
            ansi: self.ansi,
        }
    }
}

// Shorthand for terse, readable palette tables below.
const fn c(r: u8, g: u8, b: u8) -> Rgb {
    Rgb::new(r, g, b)
}

/// The bundled, well-known schemes (canonical values verified against
/// github.com/mbadolato/iTerm2-Color-Schemes). `"Default"` is NOT here — it is
/// served from [`ColorScheme::default`] so the historical look has one source.
static BUILTINS: &[Builtin] = &[
    // Palette values verified against github.com/mbadolato/iTerm2-Color-Schemes and
    // each theme's own spec. NOTE: Nord and Catppuccin's canonical files list a
    // near-WHITE selection (their selection-*foreground*); aterm paints selection as
    // a background only, so those are replaced with each theme's proper dark
    // selection surface (Nord nord3 #4c566a, Catppuccin Surface2 #585b70).
    Builtin {
        name: "Dracula",
        appearance: Appearance::Dark,
        fg: c(0xf8, 0xf8, 0xf2),
        bg: c(0x28, 0x2a, 0x36),
        cursor: c(0xf8, 0xf8, 0xf2),
        selection: c(0x44, 0x47, 0x5a),
        ansi: [
            c(0x21, 0x22, 0x2c),
            c(0xff, 0x55, 0x55),
            c(0x50, 0xfa, 0x7b),
            c(0xf1, 0xfa, 0x8c),
            c(0xbd, 0x93, 0xf9),
            c(0xff, 0x79, 0xc6),
            c(0x8b, 0xe9, 0xfd),
            c(0xf8, 0xf8, 0xf2),
            c(0x62, 0x72, 0xa4),
            c(0xff, 0x6e, 0x6e),
            c(0x69, 0xff, 0x94),
            c(0xff, 0xff, 0xa5),
            c(0xd6, 0xac, 0xff),
            c(0xff, 0x92, 0xdf),
            c(0xa4, 0xff, 0xff),
            c(0xff, 0xff, 0xff),
        ],
    },
    Builtin {
        name: "Nord",
        appearance: Appearance::Dark,
        fg: c(0xd8, 0xde, 0xe9),
        bg: c(0x2e, 0x34, 0x40),
        cursor: c(0xec, 0xef, 0xf4),
        selection: c(0x4c, 0x56, 0x6a),
        ansi: [
            c(0x3b, 0x42, 0x52),
            c(0xbf, 0x61, 0x6a),
            c(0xa3, 0xbe, 0x8c),
            c(0xeb, 0xcb, 0x8b),
            c(0x81, 0xa1, 0xc1),
            c(0xb4, 0x8e, 0xad),
            c(0x88, 0xc0, 0xd0),
            c(0xe5, 0xe9, 0xf0),
            c(0x59, 0x63, 0x77),
            c(0xbf, 0x61, 0x6a),
            c(0xa3, 0xbe, 0x8c),
            c(0xeb, 0xcb, 0x8b),
            c(0x81, 0xa1, 0xc1),
            c(0xb4, 0x8e, 0xad),
            c(0x8f, 0xbc, 0xbb),
            c(0xec, 0xef, 0xf4),
        ],
    },
    Builtin {
        name: "Tokyo Night",
        appearance: Appearance::Dark,
        fg: c(0xc0, 0xca, 0xf5),
        bg: c(0x1a, 0x1b, 0x26),
        // Accent blue, not the fg lavender — a fg-coloured full-cell block reads as a
        // pale, heavy slab (visual-judge: "heavier than the glyphs"). The blue cursor
        // reads as a cursor and stays distinct from text.
        cursor: c(0x7a, 0xa2, 0xf7),
        selection: c(0x28, 0x34, 0x57),
        ansi: [
            c(0x15, 0x16, 0x1e),
            c(0xf7, 0x76, 0x8e),
            c(0x9e, 0xce, 0x6a),
            c(0xe0, 0xaf, 0x68),
            c(0x7a, 0xa2, 0xf7),
            c(0xbb, 0x9a, 0xf7),
            c(0x7d, 0xcf, 0xff),
            c(0xa9, 0xb1, 0xd6),
            // bright-black / "dim" tier lifted #414868 -> #7a82a8: the stock comment
            // grey was only ~2.8:1 on this very dark navy (below WCAG AA); now ~4.5:1
            // so de-emphasised/secondary text stays legible while still clearly dimmer.
            c(0x7a, 0x82, 0xa8),
            c(0xff, 0x89, 0x9d),
            c(0x9f, 0xe0, 0x44),
            c(0xfa, 0xba, 0x4a),
            c(0x8d, 0xb0, 0xff),
            c(0xc7, 0xa9, 0xff),
            c(0xa4, 0xda, 0xff),
            c(0xc0, 0xca, 0xf5),
        ],
    },
    Builtin {
        name: "Catppuccin Mocha",
        appearance: Appearance::Dark,
        fg: c(0xcd, 0xd6, 0xf4),
        bg: c(0x1e, 0x1e, 0x2e),
        cursor: c(0xf5, 0xe0, 0xdc),
        selection: c(0x58, 0x5b, 0x70),
        ansi: [
            c(0x45, 0x47, 0x5a),
            c(0xf3, 0x8b, 0xa8),
            c(0xa6, 0xe3, 0xa1),
            c(0xf9, 0xe2, 0xaf),
            c(0x89, 0xb4, 0xfa),
            c(0xf5, 0xc2, 0xe7),
            c(0x94, 0xe2, 0xd5),
            c(0xba, 0xc2, 0xde),
            c(0x58, 0x5b, 0x70),
            c(0xf7, 0xae, 0xc2),
            c(0xc2, 0xec, 0xbf),
            c(0xfc, 0xd6, 0x82),
            c(0xae, 0xcc, 0xfc),
            c(0xf3, 0x98, 0xda),
            c(0xb1, 0xea, 0xe1),
            c(0xa6, 0xad, 0xc8),
        ],
    },
    Builtin {
        name: "Gruvbox Dark",
        appearance: Appearance::Dark,
        fg: c(0xeb, 0xdb, 0xb2),
        bg: c(0x28, 0x28, 0x28),
        cursor: c(0xeb, 0xdb, 0xb2),
        selection: c(0x66, 0x5c, 0x54),
        ansi: [
            c(0x28, 0x28, 0x28),
            c(0xcc, 0x24, 0x1d),
            c(0x98, 0x97, 0x1a),
            c(0xd7, 0x99, 0x21),
            c(0x45, 0x85, 0x88),
            c(0xb1, 0x62, 0x86),
            c(0x68, 0x9d, 0x6a),
            c(0xa8, 0x99, 0x84),
            c(0x92, 0x83, 0x74),
            c(0xfb, 0x49, 0x34),
            c(0xb8, 0xbb, 0x26),
            c(0xfa, 0xbd, 0x2f),
            c(0x83, 0xa5, 0x98),
            c(0xd3, 0x86, 0x9b),
            c(0x8e, 0xc0, 0x7c),
            c(0xeb, 0xdb, 0xb2),
        ],
    },
    Builtin {
        name: "Solarized Dark",
        appearance: Appearance::Dark,
        fg: c(0x83, 0x94, 0x96),
        bg: c(0x00, 0x2b, 0x36),
        cursor: c(0x83, 0x94, 0x96),
        selection: c(0x07, 0x36, 0x42),
        ansi: [
            c(0x07, 0x36, 0x42),
            c(0xdc, 0x32, 0x2f),
            c(0x85, 0x99, 0x00),
            c(0xb5, 0x89, 0x00),
            c(0x26, 0x8b, 0xd2),
            c(0xd3, 0x36, 0x82),
            c(0x2a, 0xa1, 0x98),
            c(0xee, 0xe8, 0xd5),
            c(0x33, 0x5e, 0x69),
            c(0xcb, 0x4b, 0x16),
            c(0x58, 0x6e, 0x75),
            c(0x65, 0x7b, 0x83),
            c(0x83, 0x94, 0x96),
            c(0x6c, 0x71, 0xc4),
            c(0x93, 0xa1, 0xa1),
            c(0xfd, 0xf6, 0xe3),
        ],
    },
    Builtin {
        name: "One Dark",
        appearance: Appearance::Dark,
        fg: c(0xab, 0xb2, 0xbf),
        bg: c(0x21, 0x25, 0x2b),
        cursor: c(0xab, 0xb2, 0xbf),
        selection: c(0x32, 0x38, 0x44),
        ansi: [
            c(0x21, 0x25, 0x2b),
            c(0xe0, 0x6c, 0x75),
            c(0x98, 0xc3, 0x79),
            c(0xe5, 0xc0, 0x7b),
            c(0x61, 0xaf, 0xef),
            c(0xc6, 0x78, 0xdd),
            c(0x56, 0xb6, 0xc2),
            c(0xab, 0xb2, 0xbf),
            c(0x76, 0x76, 0x76),
            c(0xe0, 0x6c, 0x75),
            c(0x98, 0xc3, 0x79),
            c(0xe5, 0xc0, 0x7b),
            c(0x61, 0xaf, 0xef),
            c(0xc6, 0x78, 0xdd),
            c(0x56, 0xb6, 0xc2),
            c(0xab, 0xb2, 0xbf),
        ],
    },
];

/// Look up a built-in scheme by name (case-insensitive). `"Default"` resolves to
/// [`ColorScheme::default`]; every other name is searched in [`struct@BUILTINS`].
#[must_use]
pub fn builtin(name: &str) -> Option<ColorScheme> {
    if name.eq_ignore_ascii_case("default") {
        return Some(ColorScheme::default());
    }
    BUILTINS
        .iter()
        .find(|b| b.name.eq_ignore_ascii_case(name))
        .map(Builtin::to_scheme)
}

/// All built-in scheme names (for `--list-themes` and config diagnostics), with
/// `"Default"` first.
#[must_use]
pub fn builtin_names() -> Vec<&'static str> {
    let mut v = Vec::with_capacity(BUILTINS.len() + 1);
    v.push("Default");
    v.extend(BUILTINS.iter().map(|b| b.name));
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Default scheme's ANSI slots equal `ColorPalette::default_color` (one
    /// source) and its `to_color_palette()` resolves every index to the same RGB.
    #[test]
    fn default_scheme_matches_palette_defaults() {
        let s = ColorScheme::default();
        let p = s.to_color_palette();
        for i in 0u16..=255 {
            let idx = i as u8;
            assert_eq!(p.get(idx), ColorPalette::default_color(idx), "index {idx}");
        }
        // The blue lift is reflected (index 4 is the readable blue, not pure blue).
        assert_eq!(s.ansi[4], Rgb::new(59, 142, 234));
        assert_ne!(s.ansi[4], Rgb::new(0, 0, 238));
    }

    /// Default chrome projects to the historical renderer Theme values.
    #[test]
    fn default_theme_parts_are_historical() {
        let tp = ColorScheme::default().to_theme_parts();
        assert_eq!(tp.fg, 0x00D0_D0D0);
        assert_eq!(tp.bg, 0x0011_1318);
        assert_eq!(tp.cursor, 0x0050_FA7B);
        assert_eq!(tp.selection, 0x0026_4F78);
    }

    /// `builtin("default")` round-trips; an unknown name is `None`.
    #[test]
    fn builtin_lookup() {
        assert_eq!(builtin("default"), Some(ColorScheme::default()));
        assert_eq!(builtin("DEFAULT"), Some(ColorScheme::default()));
        assert!(builtin("no-such-theme-xyz").is_none());
        assert_eq!(builtin_names()[0], "Default");
    }

    /// Every built-in is internally well-formed (non-empty name, distinct from the
    /// reserved "Default" sentinel) and round-trips through `to_scheme`.
    #[test]
    fn builtins_well_formed() {
        for b in BUILTINS {
            assert!(!b.name.is_empty());
            assert!(
                !b.name.eq_ignore_ascii_case("default"),
                "{} shadows Default",
                b.name
            );
            let s = b.to_scheme();
            assert_eq!(s.name, b.name);
            assert_eq!(builtin(b.name).as_ref(), Some(&s));
        }
    }

    /// Golden spot-checks of actual palette VALUES (not just structure), so a
    /// corrupted/typo'd source constant can't pass vacuously — including the
    /// deliberate Nord/Catppuccin dark-selection substitutions. Verified against
    /// github.com/mbadolato/iTerm2-Color-Schemes.
    #[test]
    fn builtin_palette_values_are_canonical() {
        let g = |name: &str| builtin(name).expect("builtin exists");
        // Dracula: bg, fg, ANSI red, ANSI bright-blue.
        assert_eq!(g("Dracula").background, Rgb::new(0x28, 0x2a, 0x36));
        assert_eq!(g("Dracula").foreground, Rgb::new(0xf8, 0xf8, 0xf2));
        assert_eq!(g("Dracula").ansi[1], Rgb::new(0xff, 0x55, 0x55));
        assert_eq!(g("Dracula").ansi[12], Rgb::new(0xd6, 0xac, 0xff));
        // Nord: bg + the substituted dark selection (#4c566a, not the white #eceff4).
        assert_eq!(g("Nord").background, Rgb::new(0x2e, 0x34, 0x40));
        assert_eq!(g("Nord").selection, Some(Rgb::new(0x4c, 0x56, 0x6a)));
        // Catppuccin Mocha: the substituted dark selection (#585b70).
        assert_eq!(
            g("Catppuccin Mocha").selection,
            Some(Rgb::new(0x58, 0x5b, 0x70))
        );
        // Tokyo Night + Solarized Dark + Gruvbox + One Dark anchors.
        assert_eq!(g("Tokyo Night").background, Rgb::new(0x1a, 0x1b, 0x26));
        assert_eq!(g("Solarized Dark").ansi[1], Rgb::new(0xdc, 0x32, 0x2f));
        assert_eq!(g("Solarized Dark").background, Rgb::new(0x00, 0x2b, 0x36));
        assert_eq!(g("Gruvbox Dark").foreground, Rgb::new(0xeb, 0xdb, 0xb2));
        assert_eq!(g("One Dark").ansi[4], Rgb::new(0x61, 0xaf, 0xef));
    }
}
