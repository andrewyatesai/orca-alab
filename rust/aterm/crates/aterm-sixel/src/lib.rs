// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0
// Author: Andrew Yates

//! Sixel (DEC graphics) DCS decoder.
//!
//! Parses the `DCS Ps q <sixel-data> ST` body that VT300-class terminals use to
//! paint raster graphics, and produces a packed-RGBA [`SixelImage`] that the
//! terminal engine routes into its existing inline-image placement/blit path
//! (the same one OSC 1337 `File=` uses). The engine carries no image codec, so
//! this crate is the ONLY place sixel pixels are materialized; it has no
//! dependencies and is pure `std`.
//!
//! ## Wire format handled (first correct increment)
//!
//! - **Data bytes** `0x3F..=0x7E`: `value = byte - 0x3F` is a 6-bit column of
//!   pixels (LSB = topmost) painted in the current color at the current x, then
//!   x advances by 1.
//! - **`#` color introducer**: `#Pc` selects register `Pc`; `#Pc;Pu;Px;Py;Pz`
//!   DEFINES register `Pc` — `Pu==2` is RGB with `Px,Py,Pz` in `0..=100`
//!   (scaled to `0..=255`), `Pu==1` is HLS (`Px`=hue 0..360, `Py`=lightness
//!   0..100, `Pz`=saturation 0..100) converted to RGB.
//! - **`!Pn` DECGRI**: the next single data byte is repeated `Pn` times.
//! - **`"Pan;Pad;Ph;Pv` DECGRA**: raster attributes — `Ph`/`Pv` declare the
//!   image width/height, used to pre-size the buffer (clamped to
//!   [`SIXEL_MAX_DIMENSION`]). The aspect-ratio `Pan`/`Pad` are parsed but not
//!   applied (1:1 pixels).
//! - **`$`**: graphics carriage return — x back to 0, y band unchanged.
//! - **`-`**: graphics new-line — advance y by one 6-px band, x back to 0.
//!
//! ## Deliberately deferred (documented, not blockers for correctness)
//!
//! - HLS rounding edge-cases (a standard integer HLS→RGB is used).
//! - P2 background-select transparency semantics: unset pixels are left fully
//!   transparent (`A == 0`) so the cell background shows through the engine's
//!   straight-alpha-over blit. Full `P2==1` vs `P2==0` device-default fill is
//!   not modeled.
//! - DECGRA aspect-ratio (`Pan`/`Pad`) non-1:1 pixel scaling.
//! - More than [`MAX_COLOR_REGISTERS`] registers; private/animation color maps.
//! - Sub-band partial scrolling semantics beyond a whole 6-px band.

#![forbid(unsafe_code)]

/// Maximum number of color registers a sixel stream may define/select.
///
/// Matches the VT340 family + the limit XTSMGRAPHICS reports. Selecting or
/// defining a register `>= MAX_COLOR_REGISTERS` is clamped to the last index so
/// a hostile stream can never grow the palette unbounded.
pub const MAX_COLOR_REGISTERS: usize = 1024;

/// Maximum sixel image dimension (pixels) on either axis. A hard clamp on the
/// raster buffer so a hostile `"` raster declaration or a long run of data/`-`
/// bands cannot allocate unboundedly. Matches the value XTSMGRAPHICS reports.
pub const SIXEL_MAX_DIMENSION: usize = 4096;

/// Packed `0x00RRGGBB` for a fully-transparent pixel (alpha tracked separately).
const TRANSPARENT: u32 = 0;

/// A decoded sixel raster image: packed RGBA, ready for inline-image placement.
///
/// `pixels()` is row-major `width * height` packed `0xAARRGGBB` u32s (alpha in
/// the top byte). Unset pixels are fully transparent (`0x00000000`).
#[derive(Debug, Clone)]
pub struct SixelImage {
    width: usize,
    height: usize,
    /// Row-major packed `0xAARRGGBB`, length == `width * height`.
    pixels: Vec<u32>,
    /// Grid cursor row at hook time (for placement).
    cursor_row: u16,
    /// Grid cursor column at hook time (for placement).
    cursor_col: u16,
}

impl SixelImage {
    /// Image width in pixels.
    #[must_use]
    pub fn width(&self) -> usize {
        self.width
    }

    /// Image height in pixels.
    #[must_use]
    pub fn height(&self) -> usize {
        self.height
    }

    /// Packed `0xAARRGGBB` pixels, row-major, length `width * height`.
    #[must_use]
    pub fn pixels(&self) -> &[u32] {
        &self.pixels
    }

    /// Cursor row at the time the sixel sequence was hooked.
    #[must_use]
    pub fn cursor_row(&self) -> u16 {
        self.cursor_row
    }

    /// Cursor column at the time the sixel sequence was hooked.
    #[must_use]
    pub fn cursor_col(&self) -> u16 {
        self.cursor_col
    }

    /// Number of grid rows this image spans given a cell height in pixels.
    /// Always at least 1 for a non-empty image.
    #[must_use]
    pub fn rows_spanned(&self, cell_h: u16) -> usize {
        let cell_h = usize::from(cell_h).max(1);
        self.height
            .div_ceil(cell_h)
            .max(usize::from(self.height > 0))
    }

    /// Number of grid columns this image spans given a cell width in pixels.
    /// Always at least 1 for a non-empty image.
    #[must_use]
    pub fn cols_spanned(&self, cell_w: u16) -> usize {
        let cell_w = usize::from(cell_w).max(1);
        self.width.div_ceil(cell_w).max(usize::from(self.width > 0))
    }
}

/// Incremental sixel DCS decoder.
///
/// Lifecycle mirrors the parser's DCS callbacks: [`hook`](Self::hook) at the
/// final byte, [`put`](Self::put) per data byte, [`unhook`](Self::unhook) at ST
/// (yielding the image), or [`abort`](Self::abort) on cancel/interrupt.
#[derive(Debug)]
pub struct SixelDecoder {
    /// `true` between `hook` and `unhook`/`abort`.
    active: bool,

    /// Color registers as packed `0x00RRGGBB`.
    palette: Vec<u32>,
    /// Currently selected color register.
    current_color: usize,

    /// Numeric-parameter accumulator for `#`/`!`/`"` introducers.
    params: Vec<u32>,
    /// Which introducer we are collecting parameters for.
    mode: ParamMode,

    /// Packed `0x00RRGGBB` raster, row-major over `alloc_width`.
    raster: Vec<u32>,
    /// Per-pixel "set" mask parallel to `raster` (for transparency).
    set_mask: Vec<bool>,
    /// Allocated raster width (stride). Grows on demand up to the clamp.
    alloc_width: usize,
    /// Allocated raster height. Grows in 6-px bands up to the clamp.
    alloc_height: usize,

    /// Current write column (x).
    x: usize,
    /// Top pixel row of the current 6-px band (y).
    band_top: usize,
    /// Exclusive max x written (for final width).
    max_x: usize,
    /// Exclusive max y written (for final height).
    max_y: usize,

    /// Declared raster width from `"` DECGRA (0 = none).
    declared_w: usize,
    /// Declared raster height from `"` DECGRA (0 = none).
    declared_h: usize,
    /// Pending DECGRI repeat count for the next data byte (0 = none).
    pending_repeat: u32,
    /// Grid cursor `(row, col)` captured at `hook` time, for placement.
    pending_cursor: (u16, u16),
}

/// Which numeric-parameter introducer the decoder is mid-collecting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParamMode {
    /// No pending introducer; digits/`;` are ignored as stray.
    None,
    /// `#` — color select/define.
    Color,
    /// `!` — DECGRI repeat; the next data byte is repeated.
    Repeat,
    /// `"` — DECGRA raster attributes.
    Raster,
}

impl Default for SixelDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl SixelDecoder {
    /// A fresh, inactive decoder with the default VT340 16-color palette.
    #[must_use]
    pub fn new() -> Self {
        Self {
            active: false,
            palette: default_palette(),
            current_color: 0,
            params: Vec::new(),
            mode: ParamMode::None,
            raster: Vec::new(),
            set_mask: Vec::new(),
            alloc_width: 0,
            alloc_height: 0,
            x: 0,
            band_top: 0,
            max_x: 0,
            max_y: 0,
            declared_w: 0,
            declared_h: 0,
            pending_repeat: 0,
            pending_cursor: (0, 0),
        }
    }

    /// Begin a sixel sequence. `params` are the DCS numeric params
    /// (`P1`=aspect, `P2`=background-select, `P3`=horizontal-grid; only used
    /// for documented defaults). `cursor_row`/`cursor_col` are the grid cursor
    /// at hook time, carried into the produced image for placement.
    pub fn hook(&mut self, params: &[u16], cursor_row: u16, cursor_col: u16) {
        // Reset transient decode state; keep the default palette fresh so a
        // reused decoder does not inherit colors from a previous image.
        self.palette = default_palette();
        self.current_color = 0;
        self.params.clear();
        self.mode = ParamMode::None;
        self.raster.clear();
        self.set_mask.clear();
        self.alloc_width = 0;
        self.alloc_height = 0;
        self.x = 0;
        self.band_top = 0;
        self.max_x = 0;
        self.max_y = 0;
        self.declared_w = 0;
        self.declared_h = 0;
        self.pending_repeat = 0;
        self.pending_cursor = (cursor_row, cursor_col);
        // P1/P2/P3 are accepted but not load-bearing in this increment.
        let _ = params;
        self.active = true;
    }

    /// Feed one sixel data byte.
    pub fn put(&mut self, byte: u8) {
        if !self.active {
            return;
        }
        match byte {
            b'#' => {
                self.start_params(ParamMode::Color);
            }
            b'!' => {
                self.start_params(ParamMode::Repeat);
            }
            b'"' => {
                self.start_params(ParamMode::Raster);
            }
            b'0'..=b'9' => {
                let d = u32::from(byte - b'0');
                if let Some(slot) = self.params.last_mut() {
                    *slot = slot.saturating_mul(10).saturating_add(d);
                } else {
                    self.params.push(d);
                }
            }
            b';' => {
                // Commit the current slot and open a new one (default 0).
                self.params.push(0);
            }
            b'$' => {
                self.finish_params();
                self.x = 0;
                // DECGRI `!Pn` applies ONLY to the immediately-following sixel data
                // byte; a graphics-CR in between cancels a pending repeat (else
                // `!3$~` would wrongly widen the next band). #adversarial-stream.
                self.pending_repeat = 0;
            }
            b'-' => {
                self.finish_params();
                self.x = 0;
                self.band_top = self.band_top.saturating_add(6);
                // Same as `$`: a graphics-NL cancels a pending `!Pn` repeat.
                self.pending_repeat = 0;
            }
            0x3F..=0x7E => {
                let bits = byte - 0x3F;
                self.finish_params();
                self.emit_sixel(bits, self.pending_repeat.max(1));
                self.pending_repeat = 0;
            }
            _ => {
                // Any other byte (C0 controls, 8-bit, stray) is ignored as data
                // per the parser model; the decoder never panics on it.
            }
        }
    }

    /// Finalize the sequence and return the image, if one was produced.
    ///
    /// Returns `None` when the decoder was never hooked or the raster is empty
    /// (no painted pixels and no declared geometry). Always deactivates.
    #[must_use]
    pub fn unhook(&mut self) -> Option<SixelImage> {
        if !self.active {
            return None;
        }
        self.finish_params();
        self.active = false;

        // Final dimensions: painted extent, falling back to the declared raster
        // attributes when nothing was painted inside the declared box.
        let width = self.max_x.max(self.declared_w).min(SIXEL_MAX_DIMENSION);
        let height = self.max_y.max(self.declared_h).min(SIXEL_MAX_DIMENSION);
        if width == 0 || height == 0 {
            // Drop buffers and report nothing for a degenerate image.
            self.release();
            return None;
        }

        // Compose the final packed-RGBA buffer of exactly width*height.
        let stride = self.alloc_width;
        let mut pixels = Vec::with_capacity(width * height);
        for y in 0..height {
            for x in 0..width {
                let packed = if x < stride && y < self.alloc_height {
                    let idx = y * stride + x;
                    if self.set_mask.get(idx).copied().unwrap_or(false) {
                        // Opaque set pixel: force alpha to 0xFF.
                        0xFF00_0000 | (self.raster[idx] & 0x00FF_FFFF)
                    } else {
                        TRANSPARENT
                    }
                } else {
                    TRANSPARENT
                };
                pixels.push(packed);
            }
        }
        let (cursor_row, cursor_col) = self.pending_cursor;
        self.release();
        Some(SixelImage {
            width,
            height,
            pixels,
            cursor_row,
            cursor_col,
        })
    }

    /// Abort the current sequence, dropping all buffers without allocating a
    /// copy. Used when a DCS is interrupted (parser reset, CAN, budget blown).
    pub fn abort(&mut self) {
        self.active = false;
        self.release();
    }

    /// Live pixel-buffer byte size, for the engine's DCS memory budget.
    ///
    /// Counts the packed raster plus the parallel set-mask so the budget tracks
    /// the true allocation a hostile stream forces.
    #[must_use]
    pub fn pixel_alloc_bytes(&self) -> usize {
        self.raster.len() * core::mem::size_of::<u32>() + self.set_mask.len()
    }

    // --- internals ---------------------------------------------------------

    /// Drop the raster/mask/param buffers (keep palette small; it is reset on
    /// the next hook). Leaves the decoder ready for reuse.
    fn release(&mut self) {
        self.raster = Vec::new();
        self.set_mask = Vec::new();
        self.params = Vec::new();
        self.alloc_width = 0;
        self.alloc_height = 0;
        self.x = 0;
        self.band_top = 0;
        self.declared_w = 0;
        self.declared_h = 0;
        self.pending_repeat = 0;
    }

    /// Begin collecting params for a new introducer, finishing any prior one.
    fn start_params(&mut self, mode: ParamMode) {
        self.finish_params();
        self.params.clear();
        self.mode = mode;
    }

    /// Apply the just-collected params for the pending introducer. `#` selects/
    /// defines a color, `"` declares raster geometry, and `!` stashes the repeat
    /// count in `pending_repeat` for the upcoming data byte. Idempotent; clears
    /// `mode` to `None`.
    fn finish_params(&mut self) {
        match self.mode {
            ParamMode::Color => self.apply_color(),
            ParamMode::Raster => self.apply_raster(),
            ParamMode::Repeat => self.apply_repeat(),
            ParamMode::None => {}
        }
        self.mode = ParamMode::None;
        self.params.clear();
    }

    /// `#Pc` (select) or `#Pc;Pu;Px;Py;Pz` (define).
    fn apply_color(&mut self) {
        let Some(&pc) = self.params.first() else {
            return;
        };
        let reg = (pc as usize).min(MAX_COLOR_REGISTERS - 1);
        if self.params.len() >= 5 {
            let pu = self.params[1];
            let px = self.params[2];
            let py = self.params[3];
            let pz = self.params[4];
            let rgb = match pu {
                1 => hls_to_rgb(px, py, pz),
                // 2 (RGB) and any other value default to RGB-percent.
                _ => rgb_percent(px, py, pz),
            };
            self.palette[reg] = rgb;
        }
        self.current_color = reg;
    }

    /// `"Pan;Pad;Ph;Pv` — declare raster geometry to pre-size the buffer.
    fn apply_raster(&mut self) {
        // params: [Pan, Pad, Ph, Pv]; we use Ph (width) and Pv (height).
        let ph = self.params.get(2).copied().unwrap_or(0) as usize;
        let pv = self.params.get(3).copied().unwrap_or(0) as usize;
        self.declared_w = ph.min(SIXEL_MAX_DIMENSION);
        self.declared_h = pv.min(SIXEL_MAX_DIMENSION);
        if self.declared_w > 0 && self.declared_h > 0 {
            self.ensure_capacity(
                self.declared_w.saturating_sub(1),
                self.declared_h.saturating_sub(1),
            );
        }
    }

    /// `!Pn` — stash the repeat count for the next data byte.
    fn apply_repeat(&mut self) {
        self.pending_repeat = self.params.first().copied().unwrap_or(0);
    }

    /// Paint `count` columns of the 6-bit `bits` pattern starting at `self.x`.
    fn emit_sixel(&mut self, bits: u8, count: u32) {
        // Clamp the run so a hostile DECGRI cannot blow past the dimension cap.
        let max_run = SIXEL_MAX_DIMENSION.saturating_sub(self.x);
        let count = (count as usize).min(max_run);
        if count == 0 {
            // Even a clamped-to-zero run must not advance forever; bail.
            return;
        }
        if bits == 0 {
            // Empty column: still advances x (sixel columns are positional).
            self.x = (self.x + count).min(SIXEL_MAX_DIMENSION);
            self.max_x = self.max_x.max(self.x.min(SIXEL_MAX_DIMENSION));
            return;
        }
        let color = self.palette.get(self.current_color).copied().unwrap_or(0) & 0x00FF_FFFF;
        let end_y = self.band_top + 6;
        // Ensure capacity for the widest x and tallest band we touch.
        self.ensure_capacity((self.x + count).saturating_sub(1), end_y.saturating_sub(1));
        let stride = self.alloc_width;
        if stride == 0 {
            return;
        }
        for col in 0..count {
            let px = self.x + col;
            if px >= self.alloc_width {
                break;
            }
            for row in 0..6 {
                if bits & (1 << row) == 0 {
                    continue;
                }
                let py = self.band_top + row;
                if py >= self.alloc_height {
                    break;
                }
                let idx = py * stride + px;
                if idx < self.raster.len() {
                    self.raster[idx] = color;
                    self.set_mask[idx] = true;
                    self.max_x = self.max_x.max(px + 1);
                    self.max_y = self.max_y.max(py + 1);
                }
            }
        }
        self.x = (self.x + count).min(SIXEL_MAX_DIMENSION);
        self.max_x = self.max_x.max(self.x.min(SIXEL_MAX_DIMENSION));
    }

    /// Grow the raster/mask so coordinates up to `(want_x, want_y)` inclusive
    /// are addressable, clamped to [`SIXEL_MAX_DIMENSION`].
    fn ensure_capacity(&mut self, want_x: usize, want_y: usize) {
        let need_w = (want_x + 1).min(SIXEL_MAX_DIMENSION);
        let need_h = (want_y + 1).min(SIXEL_MAX_DIMENSION);
        if need_w <= self.alloc_width && need_h <= self.alloc_height {
            return;
        }
        let new_w = need_w.max(self.alloc_width);
        let new_h = need_h.max(self.alloc_height);
        // Reallocate row-major, copying old rows into the wider stride.
        let new_len = new_w.checked_mul(new_h).unwrap_or(0);
        if new_len == 0 {
            return;
        }
        let mut new_raster = vec![0u32; new_len];
        let mut new_mask = vec![false; new_len];
        let old_stride = self.alloc_width;
        for y in 0..self.alloc_height {
            let src = y * old_stride;
            let dst = y * new_w;
            // old row width <= new_w by construction.
            new_raster[dst..dst + old_stride].copy_from_slice(&self.raster[src..src + old_stride]);
            new_mask[dst..dst + old_stride].copy_from_slice(&self.set_mask[src..src + old_stride]);
        }
        self.raster = new_raster;
        self.set_mask = new_mask;
        self.alloc_width = new_w;
        self.alloc_height = new_h;
    }
}

/// Scale an `0..=100` percent component to `0..=255`.
fn scale_pct(v: u32) -> u32 {
    let v = v.min(100);
    (v * 255 + 50) / 100
}

/// Pack an RGB-percent triple (`0..=100` each) into `0x00RRGGBB`.
fn rgb_percent(r: u32, g: u32, b: u32) -> u32 {
    (scale_pct(r) << 16) | (scale_pct(g) << 8) | scale_pct(b)
}

/// Convert DEC HLS (`h`=0..360, `l`=0..100, `s`=0..100) to packed `0x00RRGGBB`.
///
/// DEC's hue origin differs from the usual HSL (0° = blue, increasing
/// counter-clockwise); we use the standard HSL formula with DEC's hue offset so
/// the common primaries land correctly enough for the first increment.
fn hls_to_rgb(h: u32, l: u32, s: u32) -> u32 {
    let h = (h % 360) as f64;
    let l = (l.min(100) as f64) / 100.0;
    let s = (s.min(100) as f64) / 100.0;
    // DEC hue 0 points "up" (blue); rotate so the standard formula matches.
    let hue = (h + 240.0) % 360.0;
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - (((hue / 60.0) % 2.0) - 1.0).abs());
    let m = l - c / 2.0;
    let (r1, g1, b1) = match (hue / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let to_u8 = |v: f64| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u32;
    (to_u8(r1) << 16) | (to_u8(g1) << 8) | to_u8(b1)
}

/// The VT340 default 16-color sixel palette as packed `0x00RRGGBB`.
///
/// Values follow the canonical DEC color map (color 0 = black, 1 = blue,
/// 2 = red, …) scaled from the device RGB-percent specification. Registers
/// beyond 15 default to black until defined.
fn default_palette() -> Vec<u32> {
    // (r%, g%, b%) per DEC VT340 color map.
    const DEC: [(u32, u32, u32); 16] = [
        (0, 0, 0),    // 0  black
        (20, 20, 80), // 1  blue
        (80, 13, 13), // 2  red
        (20, 80, 20), // 3  green
        (80, 20, 80), // 4  magenta
        (20, 80, 80), // 5  cyan
        (80, 80, 20), // 6  yellow
        (53, 53, 53), // 7  gray 50%
        (26, 26, 26), // 8  gray 25%
        (33, 33, 60), // 9  blue*
        (60, 26, 26), // 10 red*
        (33, 60, 33), // 11 green*
        (60, 33, 60), // 12 magenta*
        (33, 60, 60), // 13 cyan*
        (60, 60, 33), // 14 yellow*
        (80, 80, 80), // 15 gray 75%
    ];
    let mut pal = vec![0u32; MAX_COLOR_REGISTERS];
    for (i, &(r, g, b)) in DEC.iter().enumerate() {
        pal[i] = rgb_percent(r, g, b);
    }
    pal
}

#[cfg(test)]
mod tests;
