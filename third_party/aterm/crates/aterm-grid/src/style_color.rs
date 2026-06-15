// Copyright 2026 The aterm Authors
// Author: The aterm Authors
// SPDX-License-Identifier: Apache-2.0

//! RGBA color type with ANSI 256-color palette support.

/// RGBA color.
///
/// Note: Default is black (0,0,0,255), not zero values.
/// Use `Color::DEFAULT_FG` for white or `Color::DEFAULT_BG` for black explicitly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct Color {
    /// Red component.
    pub r: u8,
    /// Green component.
    pub g: u8,
    /// Blue component.
    pub b: u8,
    /// Alpha component.
    pub a: u8,
}

impl Default for Color {
    /// Default color is black (used for background).
    fn default() -> Self {
        Self::DEFAULT_BG
    }
}

impl Color {
    /// Create a new opaque color.
    #[must_use]
    #[inline]
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    /// Default foreground color (white).
    pub const DEFAULT_FG: Self = Self::new(255, 255, 255);

    /// Default background color (black).
    pub const DEFAULT_BG: Self = Self::new(0, 0, 0);

    /// Get RGB components as a tuple.
    #[must_use]
    #[inline]
    pub const fn to_rgb(self) -> (u8, u8, u8) {
        (self.r, self.g, self.b)
    }

    /// Create a color from an ANSI 256-color index.
    ///
    /// Color indices map as follows:
    /// - 0-7: Standard colors (black, red, green, yellow, blue, magenta, cyan, white)
    /// - 8-15: Bright colors (bright versions of 0-7)
    /// - 16-231: 6x6x6 color cube
    /// - 232-255: Grayscale (dark to light)
    ///
    /// Uses a pre-computed lookup table (1KB, L1d-resident) instead of
    /// per-call arithmetic. Eliminates 3 divisions + 3 modulos for the
    /// 216-color cube range (indices 16-231).
    #[must_use]
    #[inline]
    pub const fn from_ansi_256(index: u8) -> Self {
        ANSI_256_TABLE[index as usize]
    }

    /// Compute an ANSI 256-color value (const fn, used to build the static table).
    #[doc(hidden)]
    pub const fn compute_ansi_256(index: u8) -> Self {
        // Standard ANSI colors (xterm defaults)
        const ANSI_16: [(u8, u8, u8); 16] = [
            (0, 0, 0),       // 0: Black
            (205, 0, 0),     // 1: Red
            (0, 205, 0),     // 2: Green
            (205, 205, 0),   // 3: Yellow
            (0, 0, 238),     // 4: Blue
            (205, 0, 205),   // 5: Magenta
            (0, 205, 205),   // 6: Cyan
            (229, 229, 229), // 7: White
            (127, 127, 127), // 8: Bright Black (Gray)
            (255, 0, 0),     // 9: Bright Red
            (0, 255, 0),     // 10: Bright Green
            (255, 255, 0),   // 11: Bright Yellow
            (92, 92, 255),   // 12: Bright Blue
            (255, 0, 255),   // 13: Bright Magenta
            (0, 255, 255),   // 14: Bright Cyan
            (255, 255, 255), // 15: Bright White
        ];

        if index < 16 {
            let (r, g, b) = ANSI_16[index as usize];
            Self::new(r, g, b)
        } else if index < 232 {
            // 6x6x6 color cube (indices 16-231)
            let idx = index - 16;
            let r = if idx / 36 == 0 {
                0
            } else {
                55 + (idx / 36) * 40
            };
            let g = if (idx % 36) / 6 == 0 {
                0
            } else {
                55 + ((idx % 36) / 6) * 40
            };
            let b = if idx.is_multiple_of(6) {
                0
            } else {
                55 + (idx % 6) * 40
            };
            Self::new(r, g, b)
        } else {
            // Grayscale (indices 232-255)
            let gray = 8 + (index - 232) * 10;
            Self::new(gray, gray, gray)
        }
    }
}

/// Pre-computed ANSI 256-color palette (1KB, compile-time evaluated).
///
/// Eliminates per-call division/modulo arithmetic for the 216-color cube
/// (indices 16-231). Array index == palette index for O(1) lookup.
static ANSI_256_TABLE: [Color; 256] = {
    let mut table = [Color::new(0, 0, 0); 256];
    let mut i: u16 = 0;
    while i < 256 {
        table[i as usize] = Color::compute_ansi_256(i as u8);
        i += 1;
    }
    table
};
