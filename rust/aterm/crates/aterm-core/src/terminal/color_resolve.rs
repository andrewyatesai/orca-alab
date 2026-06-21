// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Centralized color resolution for terminal cells.
//!
//! Resolves raw cell colors (default/indexed/RGB) into final RGB values
//! with all style attributes applied: bold-to-bright, dim, inverse,
//! DECSCNM (reverse video), and hidden.
//!
//! This is the single source of truth for color resolution. All frontends
//! (bridge, FFI/Swift, GPU) should use these functions instead of
//! reimplementing attribute handling.

use crate::grid::{Cell, CellExtra, CellFlags, PackedColors};
use aterm_types::{ColorPalette, DIM_FACTOR, Rgb};

/// Resolve the foreground RGB color for a cell, applying all style attributes.
///
/// Applies in order: raw lookup -> bold-to-bright -> dim -> inverse -> hidden.
///
/// # Arguments
///
/// * `cell` - The cell to resolve colors for
/// * `extra` - Extended cell data (needed for RGB color lookup)
/// * `palette` - The color palette for indexed color resolution
/// * `default_fg` - Default foreground color (from terminal settings)
/// * `default_bg` - Default background color (from terminal settings)
/// * `reverse_video` - Terminal-level DECSCNM mode (DECSET mode 5)
#[must_use]
pub fn resolve_fg_color(
    cell: &Cell,
    extra: Option<&CellExtra>,
    palette: &ColorPalette,
    default_fg: Rgb,
    default_bg: Rgb,
    reverse_video: bool,
) -> Rgb {
    let fg_rgb = extra.and_then(CellExtra::fg_rgb);
    let bg_rgb = extra.and_then(CellExtra::bg_rgb);
    let (fg, _) = resolve_both(
        *cell,
        fg_rgb,
        bg_rgb,
        palette,
        default_fg,
        default_bg,
        reverse_video,
    );
    fg
}

/// Resolve the background RGB color for a cell, applying all style attributes.
///
/// See [`resolve_fg_color`] for attribute application order.
#[must_use]
pub fn resolve_bg_color(
    cell: &Cell,
    extra: Option<&CellExtra>,
    palette: &ColorPalette,
    default_fg: Rgb,
    default_bg: Rgb,
    reverse_video: bool,
) -> Rgb {
    let fg_rgb = extra.and_then(CellExtra::fg_rgb);
    let bg_rgb = extra.and_then(CellExtra::bg_rgb);
    let (_, bg) = resolve_both(
        *cell,
        fg_rgb,
        bg_rgb,
        palette,
        default_fg,
        default_bg,
        reverse_video,
    );
    bg
}

/// Resolve the foreground RGB color from pre-resolved RGB values.
///
/// Use this when RGB values are already retrieved from the unified grid lookup
/// (ring buffer + HashMap) rather than from `CellExtra` alone.
#[must_use]
pub fn resolve_fg_color_raw(
    cell: &Cell,
    fg_rgb: Option<[u8; 3]>,
    bg_rgb: Option<[u8; 3]>,
    palette: &ColorPalette,
    default_fg: Rgb,
    default_bg: Rgb,
    reverse_video: bool,
) -> Rgb {
    let (fg, _) = resolve_both(
        *cell,
        fg_rgb,
        bg_rgb,
        palette,
        default_fg,
        default_bg,
        reverse_video,
    );
    fg
}

/// Resolve the background RGB color from pre-resolved RGB values.
///
/// See [`resolve_fg_color_raw`] for details.
#[must_use]
pub fn resolve_bg_color_raw(
    cell: &Cell,
    fg_rgb: Option<[u8; 3]>,
    bg_rgb: Option<[u8; 3]>,
    palette: &ColorPalette,
    default_fg: Rgb,
    default_bg: Rgb,
    reverse_video: bool,
) -> Rgb {
    let (_, bg) = resolve_both(
        *cell,
        fg_rgb,
        bg_rgb,
        palette,
        default_fg,
        default_bg,
        reverse_video,
    );
    bg
}

/// Resolve both foreground and background colors for a cell.
///
/// Returns `(fg, bg)` with all style attributes applied.
///
/// **Note:** This uses `CellExtra` for RGB lookup, which only checks the
/// HashMap. Prefer [`resolve_colors_raw`] with pre-resolved RGB values from
/// `grid.fg_rgb_at()` / `grid.bg_rgb_at()` to include ring buffer lookups.
#[must_use]
pub fn resolve_colors(
    cell: &Cell,
    extra: Option<&CellExtra>,
    palette: &ColorPalette,
    default_fg: Rgb,
    default_bg: Rgb,
    reverse_video: bool,
) -> (Rgb, Rgb) {
    let fg_rgb = extra.and_then(CellExtra::fg_rgb);
    let bg_rgb = extra.and_then(CellExtra::bg_rgb);
    resolve_both(
        *cell,
        fg_rgb,
        bg_rgb,
        palette,
        default_fg,
        default_bg,
        reverse_video,
    )
}

/// Resolve both foreground and background colors from pre-resolved RGB values.
///
/// Use this when RGB values are already retrieved from the unified grid lookup
/// (ring buffer + HashMap) via `grid.fg_rgb_at()` / `grid.bg_rgb_at()`.
#[must_use]
pub fn resolve_colors_raw(
    cell: &Cell,
    fg_rgb: Option<[u8; 3]>,
    bg_rgb: Option<[u8; 3]>,
    palette: &ColorPalette,
    default_fg: Rgb,
    default_bg: Rgb,
    reverse_video: bool,
) -> (Rgb, Rgb) {
    resolve_both(
        *cell,
        fg_rgb,
        bg_rgb,
        palette,
        default_fg,
        default_bg,
        reverse_video,
    )
}

/// Core resolution logic — returns (fg, bg) tuple.
fn resolve_both(
    cell: Cell,
    fg_rgb: Option<[u8; 3]>,
    bg_rgb: Option<[u8; 3]>,
    palette: &ColorPalette,
    default_fg: Rgb,
    default_bg: Rgb,
    reverse_video: bool,
) -> (Rgb, Rgb) {
    let flags = cell.flags();
    let colors = cell.colors();

    // 1. Raw color lookup (default -> indexed -> RGB)
    let (mut fg, mut bg) = raw_resolve(colors, fg_rgb, bg_rgb, palette, default_fg, default_bg);

    // 2. Bold-to-bright: indexed 0-7 -> 8-15 when BOLD (not DIM)
    // Standard terminal behavior per ECMA-48: when both BOLD and DIM are
    // set, dim wins — no bright promotion.
    if flags.contains(CellFlags::BOLD) && !flags.contains(CellFlags::DIM) && colors.fg_is_indexed()
    {
        let idx = colors.fg_index();
        if idx < 8 {
            fg = palette.get(idx + 8);
        }
    }

    // 3. Dim: multiply fg channels by DIM_FACTOR
    if flags.contains(CellFlags::DIM) {
        fg = apply_dim(fg);
    }

    // 4. Inverse: XOR with DECSCNM
    // Cell INVERSE flag and terminal reverse_video cancel each other out.
    let effective_inverse = flags.contains(CellFlags::INVERSE) != reverse_video;
    if effective_inverse {
        std::mem::swap(&mut fg, &mut bg);
    }

    // 5. Hidden: fg = bg (after inverse)
    if flags.contains(CellFlags::HIDDEN) {
        fg = bg;
    }

    (fg, bg)
}

/// Apply dim factor to a color by multiplying each channel.
#[inline]
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "DIM_FACTOR in [0,1] — product of u8 * factor fits in u8"
)]
fn apply_dim(color: Rgb) -> Rgb {
    // DIM_FACTOR is 0.5 and inputs are u8 (0-255), so result is always 0-127.
    Rgb {
        r: (f32::from(color.r) * DIM_FACTOR) as u8,
        g: (f32::from(color.g) * DIM_FACTOR) as u8,
        b: (f32::from(color.b) * DIM_FACTOR) as u8,
    }
}

/// Resolve raw cell colors from packed representation.
///
/// Handles the three-tier resolution: default -> indexed palette -> RGB.
/// The `fg_rgb` and `bg_rgb` parameters are pre-resolved from the unified
/// grid lookup (ring buffer + HashMap) or from CellExtra directly.
fn raw_resolve(
    colors: PackedColors,
    fg_rgb: Option<[u8; 3]>,
    bg_rgb: Option<[u8; 3]>,
    palette: &ColorPalette,
    default_fg: Rgb,
    default_bg: Rgb,
) -> (Rgb, Rgb) {
    let fg = if colors.fg_is_default() {
        default_fg
    } else if colors.fg_is_indexed() {
        palette.get(colors.fg_index())
    } else {
        // RGB — use pre-resolved value from unified lookup
        fg_rgb.map_or(default_fg, |[r, g, b]| Rgb { r, g, b })
    };

    let bg = if colors.bg_is_default() {
        default_bg
    } else if colors.bg_is_indexed() {
        palette.get(colors.bg_index())
    } else {
        bg_rgb.map_or(default_bg, |[r, g, b]| Rgb { r, g, b })
    };

    (fg, bg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::{Cell, CellExtra, CellFlags, PackedColor};

    const DEFAULT_FG: Rgb = Rgb {
        r: 255,
        g: 255,
        b: 255,
    };
    const DEFAULT_BG: Rgb = Rgb { r: 0, g: 0, b: 0 };

    fn palette() -> ColorPalette {
        ColorPalette::new()
    }

    fn cell_with_flags(flags: CellFlags) -> Cell {
        let mut cell = Cell::default();
        cell.set_flags(flags);
        cell
    }

    fn cell_with_indexed_fg(index: u8, flags: CellFlags) -> Cell {
        let mut cell = Cell::default();
        cell.set_fg(PackedColor::indexed(index));
        cell.set_flags(flags);
        cell
    }

    fn cell_with_rgb_fg(r: u8, g: u8, b: u8, flags: CellFlags) -> (Cell, CellExtra) {
        let mut cell = Cell::default();
        cell.set_fg(PackedColor::rgb(r, g, b));
        cell.set_flags(flags);
        let mut extra = CellExtra::default();
        extra.set_fg_rgb(Some([r, g, b]));
        (cell, extra)
    }

    #[test]
    fn test_no_flags_returns_default_colors() {
        let cell = Cell::default();
        let pal = palette();
        let (fg, bg) = resolve_colors(&cell, None, &pal, DEFAULT_FG, DEFAULT_BG, false);
        assert_eq!(fg, DEFAULT_FG);
        assert_eq!(bg, DEFAULT_BG);
    }

    #[test]
    fn test_dim_ansi_color() {
        let cell = cell_with_indexed_fg(1, CellFlags::DIM);
        let pal = palette();
        let fg = resolve_fg_color(&cell, None, &pal, DEFAULT_FG, DEFAULT_BG, false);
        let raw_red = pal.get(1);
        assert_eq!(fg.r, (f32::from(raw_red.r) * 0.5) as u8);
        assert_eq!(fg.g, (f32::from(raw_red.g) * 0.5) as u8);
        assert_eq!(fg.b, (f32::from(raw_red.b) * 0.5) as u8);
    }

    #[test]
    fn test_dim_true_rgb() {
        let (cell, extra) = cell_with_rgb_fg(200, 100, 50, CellFlags::DIM);
        let pal = palette();
        let fg = resolve_fg_color(&cell, Some(&extra), &pal, DEFAULT_FG, DEFAULT_BG, false);
        assert_eq!(
            fg,
            Rgb {
                r: 100,
                g: 50,
                b: 25
            }
        );
    }

    #[test]
    fn test_bold_promotes_ansi_to_bright() {
        let cell = cell_with_indexed_fg(3, CellFlags::BOLD);
        let pal = palette();
        let fg = resolve_fg_color(&cell, None, &pal, DEFAULT_FG, DEFAULT_BG, false);
        assert_eq!(fg, pal.get(11));
    }

    #[test]
    fn test_bold_no_change_already_bright() {
        let cell = cell_with_indexed_fg(10, CellFlags::BOLD);
        let pal = palette();
        let fg = resolve_fg_color(&cell, None, &pal, DEFAULT_FG, DEFAULT_BG, false);
        assert_eq!(fg, pal.get(10));
    }

    #[test]
    fn test_bold_no_change_true_rgb() {
        let (cell, extra) = cell_with_rgb_fg(100, 200, 50, CellFlags::BOLD);
        let pal = palette();
        let fg = resolve_fg_color(&cell, Some(&extra), &pal, DEFAULT_FG, DEFAULT_BG, false);
        assert_eq!(
            fg,
            Rgb {
                r: 100,
                g: 200,
                b: 50
            }
        );
    }

    #[test]
    fn test_bold_dim_dim_wins() {
        let cell = cell_with_indexed_fg(1, CellFlags::BOLD.union(CellFlags::DIM));
        let pal = palette();
        let fg = resolve_fg_color(&cell, None, &pal, DEFAULT_FG, DEFAULT_BG, false);
        let expected = apply_dim(pal.get(1));
        assert_eq!(fg, expected);
    }

    #[test]
    fn test_inverse_swaps_colors() {
        let cell = cell_with_flags(CellFlags::INVERSE);
        let pal = palette();
        let (fg, bg) = resolve_colors(&cell, None, &pal, DEFAULT_FG, DEFAULT_BG, false);
        assert_eq!(fg, DEFAULT_BG);
        assert_eq!(bg, DEFAULT_FG);
    }

    #[test]
    fn test_decscnm_reverses() {
        let cell = Cell::default();
        let pal = palette();
        let (fg, bg) = resolve_colors(&cell, None, &pal, DEFAULT_FG, DEFAULT_BG, true);
        assert_eq!(fg, DEFAULT_BG);
        assert_eq!(bg, DEFAULT_FG);
    }

    #[test]
    fn test_decscnm_plus_inverse_cancel() {
        let cell = cell_with_flags(CellFlags::INVERSE);
        let pal = palette();
        let (fg, bg) = resolve_colors(&cell, None, &pal, DEFAULT_FG, DEFAULT_BG, true);
        assert_eq!(fg, DEFAULT_FG);
        assert_eq!(bg, DEFAULT_BG);
    }

    #[test]
    fn test_hidden_fg_equals_bg() {
        let cell = cell_with_flags(CellFlags::HIDDEN);
        let pal = palette();
        let (fg, bg) = resolve_colors(&cell, None, &pal, DEFAULT_FG, DEFAULT_BG, false);
        assert_eq!(fg, bg);
        assert_eq!(fg, DEFAULT_BG);
    }

    #[test]
    fn test_hidden_plus_inverse() {
        let cell = cell_with_flags(CellFlags::HIDDEN.union(CellFlags::INVERSE));
        let pal = palette();
        let (fg, bg) = resolve_colors(&cell, None, &pal, DEFAULT_FG, DEFAULT_BG, false);
        assert_eq!(fg, DEFAULT_FG);
        assert_eq!(bg, DEFAULT_FG);
    }

    #[test]
    fn test_all_flags_precedence() {
        let flags = CellFlags::BOLD
            .union(CellFlags::DIM)
            .union(CellFlags::INVERSE)
            .union(CellFlags::HIDDEN);
        let cell = cell_with_indexed_fg(2, flags);
        let pal = palette();
        let (fg, bg) = resolve_colors(&cell, None, &pal, DEFAULT_FG, DEFAULT_BG, false);
        let dimmed = apply_dim(pal.get(2));
        assert_eq!(fg, dimmed);
        assert_eq!(bg, dimmed);
    }

    #[test]
    fn test_custom_default_colors() {
        let custom_fg = Rgb {
            r: 200,
            g: 200,
            b: 200,
        };
        let custom_bg = Rgb {
            r: 30,
            g: 30,
            b: 30,
        };
        let cell = Cell::default();
        let pal = palette();
        let (fg, bg) = resolve_colors(&cell, None, &pal, custom_fg, custom_bg, false);
        assert_eq!(fg, custom_fg);
        assert_eq!(bg, custom_bg);
    }

    #[test]
    fn test_extended_palette_color() {
        let cell = cell_with_indexed_fg(128, CellFlags::empty());
        let pal = palette();
        let fg = resolve_fg_color(&cell, None, &pal, DEFAULT_FG, DEFAULT_BG, false);
        assert_eq!(fg, pal.get(128));
        assert_eq!(
            fg,
            Rgb {
                r: 175,
                g: 0,
                b: 215
            }
        );
    }
}
