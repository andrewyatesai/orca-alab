// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! aterm CPU rasterizer — turns the terminal grid into an RGBA framebuffer.
//!
//! This is the renderer core: headless, deterministic, no window. It is *also*
//! the `read_image` pixel-introspection oracle (ATERM_DESIGN §8) and the
//! offscreen renderer the self-proving harness uses. A windowed frontend
//! (`aterm-gui`) presents the same framebuffer; nothing about rendering depends
//! on a display existing.
//!
//! First slice: monospace glyphs from a TTF (fontdue) on a themed fg/bg, plus
//! the DECSCUSR-shaped cursor (block/underline/bar/hollow, with a blink-phase
//! gate). Per-cell colours layer on next (the grid exposes fg_rgb/bg_rgb).

use std::collections::HashMap;

use aterm_core::grid::LineSize;
use aterm_core::terminal::{CursorStyle, RenderCell, Terminal, UnderlineStyle};

pub mod procedural;

/// Colours as 0x00RR_GGBB: default foreground/background, the block cursor
/// fill, and the selection-highlight background (painted under selected cells
/// in place of the cell bg; the glyph keeps its own foreground).
#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub fg: u32,
    pub bg: u32,
    pub cursor: u32,
    pub selection: u32,
}

impl Default for Theme {
    fn default() -> Self {
        // a calm dark theme; selection is a muted steel blue that keeps the
        // default 0xD0D0D0 foreground readable.
        Theme { fg: 0x00D0_D0D0, bg: 0x0011_1318, cursor: 0x0050_FA7B, selection: 0x0026_4F78 }
    }
}

// `Frame` / `RenderInput` (and `RenderInput`'s `cluster_at`/`combining_at`
// helpers) moved to `aterm-render-api` — the injected-Rasterizer seam
// (ATERM_DESIGN WS-F). Re-exported here so existing
// `aterm_render::{Frame, RenderInput}` call sites are unchanged; `Rasterizer` is
// the trait this CPU renderer implements (see `impl Rasterizer for Renderer`).
pub use aterm_render_api::{Frame, RenderInput, RenderView, Rasterizer};

/// Which glyph source a [`GlyphKey`] rasterizes from.
///
/// A `Renderer` owns two font faces (primary monospace + lazy Unicode
/// fallback) plus the fontless [`procedural`] source; future sources (a
/// colour-emoji face) become new variants so they share the same cache and
/// atlas plumbing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FaceId {
    /// The primary monospace face (also serves `.notdef` for misses).
    Primary,
    /// The broad-coverage Unicode fallback face (CJK, symbols, math).
    Fallback,
    /// No font at all: box-drawing / block / braille coverage synthesized
    /// from the cell geometry by [`procedural`] — cell-exact, hard 0/255, so
    /// strokes meet seamlessly across cells and CPU==GPU is bit-identical.
    /// `ATERM_NO_PROCEDURAL_GLYPHS=1` disables this source (font dispatch).
    Procedural,
    /// Apple Color Emoji (`sbix` colour bitmaps): 32-bit RGBA glyphs the mono
    /// faces can't draw (🚀 😀). Consulted only when Primary AND Fallback both
    /// miss a code point that the colour-emoji face covers.
    ColorEmoji,
}

/// The pixel class of a glyph image: what one cache/atlas texel holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GlyphClass {
    /// 8-bit alpha coverage, tinted by the cell foreground at blit time.
    Mono,
    /// 32-bit RGBA colour emoji addressed by Unicode scalar (`ch_or_id` is a
    /// code point): a single-codepoint emoji resolved through the colour face.
    Rgba,
    /// 32-bit RGBA colour emoji addressed by GLYPH ID (`ch_or_id` is an opaque
    /// colour-font glyph id, not a code point): a multi-codepoint grapheme
    /// cluster (ZWJ / skin-tone / keycap) already SHAPED to one glyph.
    RgbaGid,
}

/// Style variant bits for a glyph, part of its cache identity.
///
/// PLACEHOLDER this slice: every key carries [`StyleBits::REGULAR`]; the
/// bold/italic bits exist so styled rasterization (synthetic or real face
/// variants) can land later without another cache-key migration.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StyleBits(pub u8);

impl StyleBits {
    /// No style variation — the only value produced today.
    pub const REGULAR: StyleBits = StyleBits(0);
    pub const BOLD: StyleBits = StyleBits(1 << 0);
    pub const ITALIC: StyleBits = StyleBits(1 << 1);

    /// Whether every bit of `other` is set in `self`.
    pub fn contains(self, other: StyleBits) -> bool {
        self.0 & other.0 == other.0
    }
}

/// Cache/atlas identity of one rasterized glyph image.
///
/// This is the long-term key for everything glyph-shaped: which face it came
/// from, what kind of pixels it holds, which character (or, for procedural
/// sources, which glyph id), at which style and quantized pixel size. Two keys
/// are equal iff their rasterizations are byte-identical, so it is safe to key
/// caches and atlases by it. `Ord` is derived so key sets iterate
/// deterministically (atlas packing order is stable frame to frame).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GlyphKey {
    pub source: FaceId,
    pub glyph_class: GlyphClass,
    /// The Unicode scalar value for character glyphs; an opaque per-source
    /// glyph id for future procedural sources.
    pub ch_or_id: u32,
    pub style: StyleBits,
    /// Pixel size in 26.6 fixed point ([`GlyphKey::quantize_px`]), so the key
    /// stays `Eq + Hash` and one cache can host multiple sizes.
    pub px_q: u32,
}

impl GlyphKey {
    /// Quantize a pixel size into the key's 26.6 fixed-point form.
    pub fn quantize_px(px: f32) -> u32 {
        (px * 64.0).round() as u32
    }

    /// Key for a coverage (Mono) glyph of character `ch` from `source`.
    pub fn mono_char(source: FaceId, ch: char, style: StyleBits, px_q: u32) -> GlyphKey {
        GlyphKey { source, glyph_class: GlyphClass::Mono, ch_or_id: ch as u32, style, px_q }
    }

    /// Key for a colour (Rgba) glyph of character `ch` from `source` (the
    /// colour-emoji face). The image carries its own colours, so the cell
    /// foreground is irrelevant — `style` is fixed REGULAR.
    pub fn rgba_char(source: FaceId, ch: char, px_q: u32) -> GlyphKey {
        GlyphKey { source, glyph_class: GlyphClass::Rgba, ch_or_id: ch as u32, style: StyleBits::REGULAR, px_q }
    }

    /// Key for a colour (RgbaGid) glyph addressed by colour-font glyph id — a
    /// grapheme cluster (ZWJ / skin-tone / keycap) already shaped to one glyph.
    pub fn rgba_gid(source: FaceId, gid: u16, px_q: u32) -> GlyphKey {
        GlyphKey { source, glyph_class: GlyphClass::RgbaGid, ch_or_id: gid as u32, style: StyleBits::REGULAR, px_q }
    }

    /// The character this key rasterizes, when `ch_or_id` is a code point.
    /// `None` for [`GlyphClass::RgbaGid`], whose `ch_or_id` is a glyph id.
    pub fn chr(&self) -> Option<char> {
        if self.glyph_class == GlyphClass::RgbaGid {
            return None;
        }
        char::from_u32(self.ch_or_id)
    }
}

/// One rasterized glyph: bitmap bytes + placement, the value a [`GlyphKey`]
/// resolves to. `xmin`/`ymin` are fontdue's placement offsets (the blit anchors
/// at `cell_x + xmin`, `cell_y + baseline - height - ymin`), `advance` the
/// horizontal advance in pixels.
#[derive(Clone, Debug)]
pub enum GlyphImage {
    /// 8-bit alpha coverage, row-major, `width * height` bytes; the renderer
    /// tints it with the cell foreground at blit time.
    Mono { width: usize, height: usize, xmin: i32, ymin: i32, advance: f32, bytes: Vec<u8> },
    /// 32-bit RGBA colour, row-major, `width * height * 4` bytes (colour
    /// emoji). PLACEHOLDER this slice: nothing produces a non-empty one.
    Rgba { width: usize, height: usize, xmin: i32, ymin: i32, advance: f32, bytes: Vec<u8> },
}

impl GlyphImage {
    pub fn width(&self) -> usize {
        match self {
            GlyphImage::Mono { width, .. } | GlyphImage::Rgba { width, .. } => *width,
        }
    }

    pub fn height(&self) -> usize {
        match self {
            GlyphImage::Mono { height, .. } | GlyphImage::Rgba { height, .. } => *height,
        }
    }

    pub fn xmin(&self) -> i32 {
        match self {
            GlyphImage::Mono { xmin, .. } | GlyphImage::Rgba { xmin, .. } => *xmin,
        }
    }

    pub fn ymin(&self) -> i32 {
        match self {
            GlyphImage::Mono { ymin, .. } | GlyphImage::Rgba { ymin, .. } => *ymin,
        }
    }

    pub fn advance(&self) -> f32 {
        match self {
            GlyphImage::Mono { advance, .. } | GlyphImage::Rgba { advance, .. } => *advance,
        }
    }

    /// The raw bitmap bytes (1 byte/texel for `Mono`, 4 for `Rgba`).
    pub fn bytes(&self) -> &[u8] {
        match self {
            GlyphImage::Mono { bytes, .. } | GlyphImage::Rgba { bytes, .. } => bytes,
        }
    }
}

/// Monospace CPU rasterizer.
///
/// `font` is the primary monospace face used for Latin/box-drawing. `fallback`
/// is a broad-coverage face (CJK, symbols, math) consulted only when the primary
/// has no glyph for a code point — so 日本語, math symbols, and the like render
/// instead of going blank. Glyph dispatch is per-char and cached.
pub struct Renderer {
    font: fontdue::Font,
    /// Broad-coverage fallback face, loaded LAZILY: a full Unicode font (e.g.
    /// Arial Unicode, 50k glyphs) costs ~370 MB once fontdue parses it, so it is
    /// NOT loaded until a code point actually misses the primary face. Sessions
    /// that only show Latin/box-drawing never pay it (idle RSS ~70 MB, not ~450).
    fallback: Option<fontdue::Font>,
    /// Candidate fallback font paths, tried on first miss; emptied once consumed.
    fallback_paths: Vec<String>,
    /// Apple Color Emoji font bytes, loaded LAZILY on the first emoji miss (a
    /// large `sbix` font; sessions without emoji never pay it). Stored as raw
    /// bytes because a `ttf_parser::Face` borrows them — a fresh Face is parsed
    /// per emoji rasterization, which is rare and off the hot path.
    color_font: Option<Vec<u8>>,
    /// Candidate colour-emoji font paths, tried on first emoji; emptied once consumed.
    color_font_paths: Vec<String>,
    px: f32,
    /// `px` in the key's 26.6 fixed-point form, computed once at construction.
    px_q: u32,
    cell_w: usize,
    cell_h: usize,
    baseline: i32,
    theme: Theme,
    /// The glyph cache, keyed by full rasterization identity.
    glyphs: HashMap<GlyphKey, GlyphImage>,
    /// Per-char key resolve cache (primary-vs-fallback dispatch happens once
    /// per char, not once per blit — the hot path stays two cheap lookups).
    keys: HashMap<char, GlyphKey>,
    /// Separate resolve cache for EMOJI-presentation cells (a VS16-widened base
    /// like `❤️`): the same char resolves to a DIFFERENT key here (the colour
    /// face is preferred over the mono primary), so it can't share `keys`.
    emoji_keys: HashMap<char, GlyphKey>,
    /// Shaping cache: an emoji grapheme cluster (ZWJ / skin-tone / keycap) ->
    /// the single colour-font glyph id rustybuzz shapes it to (`None` = the
    /// cluster does not shape to one colour glyph, so fall back to the base).
    /// Keyed by the cluster string so each unique cluster shapes at most once.
    cluster_gids: HashMap<Box<str>, Option<u16>>,
    /// Resolve cache for BOLD/ITALIC cells, keyed by `(char, style)` since the
    /// same char has a distinct synthetic-styled glyph per weight/slant. The
    /// unstyled hot path keeps using `keys` (plain `char`).
    styled_keys: HashMap<(char, StyleBits), GlyphKey>,
    /// Blink phase consulted ONLY for the `Blinking*` cursor styles: `true`
    /// (the default) draws the cursor, `false` skips it for the frame. Steady
    /// styles ignore it. A windowed frontend toggles this ~every 530ms.
    cursor_blink_phase: bool,
    /// When set, the cursor is drawn in THIS style instead of the terminal's
    /// own DECSCUSR style — the windowed frontend forces [`CursorStyle::HollowBlock`]
    /// while the window is unfocused (standard terminal behavior).
    cursor_style_override: Option<CursorStyle>,
    /// Whether box-drawing/block/braille chars dispatch to the [`procedural`]
    /// source instead of a font ($ATERM_NO_PROCEDURAL_GLYPHS, read once at
    /// construction).
    procedural: bool,
    /// Damage-tracking cache for [`render_input`](Self::render_input): the
    /// previous frame's pixels plus enough of the previous input to decide,
    /// row by row, what changed. `None` until the first frame (or after any
    /// state that invalidates reuse — see [`RenderCache`]). The output is
    /// byte-identical to a full repaint; only the WORK differs (unchanged rows
    /// keep their cached pixels).
    cache: Option<RenderCache>,
}

/// Cached state for the damage-tracking fast path in
/// [`Renderer::render_input`].
///
/// Holds the last frame's framebuffer (reused in place — no per-frame
/// allocation) plus the last [`RenderInput`] and the renderer state that the
/// cursor/blink overlay reads (blink phase + cursor-style override), so the
/// next frame can compute the dirty row set and the dirty-gate. Invalidated to
/// a full render whenever any precondition for safe reuse is violated (dims
/// change, scrollback/selection change, a double-HEIGHT row anywhere).
struct RenderCache {
    /// The previous frame's framebuffer (`width * height` packed pixels).
    pixels: Vec<u32>,
    width: usize,
    height: usize,
    /// The previous frame's input snapshot, for per-row equality + the gate.
    input: RenderInput,
    /// The blink phase that previous frame was drawn with (the cursor overlay
    /// is suppressed for `Blinking*` styles when this is `false`).
    cursor_blink_phase: bool,
    /// The cursor-style override the previous frame was drawn with.
    cursor_style_override: Option<CursorStyle>,
}

/// Font paths to try, most-preferred first; override with $ATERM_FONT.
const FONT_CANDIDATES: &[&str] = &[
    "/System/Library/Fonts/SFNSMono.ttf",
    "/System/Library/Fonts/Supplemental/Andale Mono.ttf",
    "/System/Library/Fonts/Supplemental/Courier New.ttf",
];

/// Broad-coverage fallback faces (CJK + symbols), most-preferred first.
/// Override with $ATERM_FALLBACK_FONT.
const FALLBACK_CANDIDATES: &[&str] = &[
    "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
    "/System/Library/Fonts/Apple Symbols.ttf",
];

/// Colour-emoji faces (sbix bitmaps), most-preferred first. Override with
/// `$ATERM_EMOJI_FONT`. A `.ttc` collection: face index 0.
const COLOR_EMOJI_CANDIDATES: &[&str] = &[
    "/System/Library/Fonts/Apple Color Emoji.ttc",
];

/// Env escape hatch for the [`procedural`] glyph source: box-drawing / block /
/// braille cells are synthesized from the cell geometry by default (cell-exact,
/// seam-free — see the module docs); set `ATERM_NO_PROCEDURAL_GLYPHS=1` to
/// restore font glyphs for those ranges. Read once per renderer, at
/// construction, like $ATERM_FONT / $ATERM_FALLBACK_FONT above.
const NO_PROCEDURAL_ENV: &str = "ATERM_NO_PROCEDURAL_GLYPHS";

/// The ordered fallback-font candidate paths ($ATERM_FALLBACK_FONT first), to be
/// loaded lazily on the first primary-face miss.
fn fallback_candidate_paths() -> Vec<String> {
    let mut paths: Vec<String> = Vec::new();
    if let Ok(p) = std::env::var("ATERM_FALLBACK_FONT") {
        paths.push(p);
    }
    paths.extend(FALLBACK_CANDIDATES.iter().map(|s| (*s).to_string()));
    paths
}

/// The ordered colour-emoji candidate paths ($ATERM_EMOJI_FONT first), loaded
/// lazily the first time a code point misses both mono faces.
fn color_emoji_candidate_paths() -> Vec<String> {
    let mut paths: Vec<String> = Vec::new();
    if let Ok(p) = std::env::var("ATERM_EMOJI_FONT") {
        paths.push(p);
    }
    paths.extend(COLOR_EMOJI_CANDIDATES.iter().map(|s| (*s).to_string()));
    paths
}

impl Renderer {
    /// Build from explicit font bytes at a given pixel size.
    pub fn from_bytes(bytes: &[u8], px: f32, theme: Theme) -> Result<Self, String> {
        let font = fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
            .map_err(|e| e.to_string())?;
        let lm = font
            .horizontal_line_metrics(px)
            .ok_or("font has no horizontal line metrics")?;
        // Monospace: every advance is equal; measure a representative glyph.
        let adv = font.metrics('M', px).advance_width;
        let cell_w = adv.ceil().max(1.0) as usize;
        let cell_h = lm.new_line_size.ceil().max(1.0) as usize;
        let baseline = lm.ascent.round() as i32;
        Ok(Renderer {
            font,
            fallback: None,
            fallback_paths: Vec::new(),
            color_font: None,
            color_font_paths: Vec::new(),
            px,
            px_q: GlyphKey::quantize_px(px),
            cell_w,
            cell_h,
            baseline,
            theme,
            glyphs: HashMap::new(),
            keys: HashMap::new(),
            emoji_keys: HashMap::new(),
            cluster_gids: HashMap::new(),
            styled_keys: HashMap::new(),
            cursor_blink_phase: true,
            cursor_style_override: None,
            procedural: std::env::var_os(NO_PROCEDURAL_ENV).is_none(),
            cache: None,
        })
    }

    /// Install a broad-coverage fallback face from explicit bytes (eagerly).
    /// Glyphs absent in the primary font are rasterized from this face instead
    /// of going blank. Prefer the lazy path (`from_system`) unless you have a
    /// reason to pay the parse cost upfront.
    pub fn set_fallback_bytes(&mut self, bytes: &[u8]) -> Result<(), String> {
        let f = fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
            .map_err(|e| e.to_string())?;
        self.fallback = Some(f);
        self.fallback_paths.clear();
        Ok(())
    }

    /// Build from the first available system monospace font ($ATERM_FONT first).
    /// The Unicode fallback ($ATERM_FALLBACK_FONT first) is recorded but loaded
    /// LAZILY on the first code point that misses the primary face.
    pub fn from_system(px: f32, theme: Theme) -> Option<Self> {
        let mut paths: Vec<String> = Vec::new();
        if let Ok(p) = std::env::var("ATERM_FONT") {
            paths.push(p);
        }
        paths.extend(FONT_CANDIDATES.iter().map(|s| s.to_string()));
        for p in paths {
            if let Ok(bytes) = std::fs::read(&p)
                && let Ok(mut r) = Self::from_bytes(&bytes, px, theme)
            {
                r.fallback_paths = fallback_candidate_paths();
                r.color_font_paths = color_emoji_candidate_paths();
                return Some(r);
            }
        }
        None
    }

    /// Lazily load the first available fallback face the first time it's needed.
    /// After this runs once, `fallback_paths` is empty so we never re-try.
    fn ensure_fallback(&mut self) {
        if self.fallback.is_some() || self.fallback_paths.is_empty() {
            return;
        }
        let paths = std::mem::take(&mut self.fallback_paths);
        for p in paths {
            if let Ok(bytes) = std::fs::read(&p)
                && let Ok(f) = fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
            {
                self.fallback = Some(f);
                return;
            }
        }
    }

    /// Lazily load the colour-emoji font bytes the first time one is needed.
    /// After this runs once, `color_font_paths` is empty so we never re-try.
    /// Only the bytes are kept (a `ttf_parser::Face` borrows them); a Face is
    /// parsed per emoji rasterization.
    fn ensure_color_font(&mut self) {
        if self.color_font.is_some() || self.color_font_paths.is_empty() {
            return;
        }
        let paths = std::mem::take(&mut self.color_font_paths);
        for p in paths {
            if let Ok(bytes) = std::fs::read(&p) {
                // Validate it parses as a colour (sbix) face before keeping it.
                if ttf_parser::Face::parse(&bytes, 0).is_ok() {
                    self.color_font = Some(bytes);
                    return;
                }
            }
        }
    }

    /// Whether the colour-emoji face has a glyph for `ch` (loads it lazily).
    fn color_font_has(&mut self, ch: char) -> bool {
        self.ensure_color_font();
        let Some(bytes) = self.color_font.as_ref() else {
            return false;
        };
        let Ok(face) = ttf_parser::Face::parse(bytes, 0) else {
            return false;
        };
        // A glyph is colour-renderable only if it has an sbix raster image, not
        // just a (possibly .notdef) glyph id.
        face.glyph_index(ch)
            .and_then(|gid| face.glyph_raster_image(gid, u16::MAX))
            .is_some()
    }

    pub fn cell_size(&self) -> (usize, usize) {
        (self.cell_w, self.cell_h)
    }

    /// Pixel size of a `rows`x`cols` grid.
    pub fn frame_size(&self, rows: usize, cols: usize) -> (usize, usize) {
        (cols * self.cell_w, rows * self.cell_h)
    }

    /// The font baseline (pixels from the cell top to the glyph baseline).
    pub fn baseline(&self) -> i32 {
        self.baseline
    }

    /// The pixel size this renderer rasterizes glyphs at.
    pub fn px(&self) -> f32 {
        self.px
    }

    /// Set the cursor blink phase: `false` skips drawing the cursor for the
    /// frame, but ONLY for the `Blinking*` DECSCUSR styles (steady styles are
    /// unaffected). Defaults to `true`; a windowed frontend toggles it ~530ms.
    pub fn set_cursor_blink_phase(&mut self, on: bool) {
        self.cursor_blink_phase = on;
    }

    /// The current cursor blink phase (see [`Self::set_cursor_blink_phase`]).
    pub fn cursor_blink_phase(&self) -> bool {
        self.cursor_blink_phase
    }

    /// Force the cursor to be drawn as `style` regardless of the terminal's
    /// DECSCUSR style (`None` restores it). The windowed frontend sets
    /// `Some(HollowBlock)` while unfocused, the standard terminal behavior.
    pub fn set_cursor_style_override(&mut self, style: Option<CursorStyle>) {
        self.cursor_style_override = style;
    }

    /// The active cursor style override (see [`Self::set_cursor_style_override`]).
    pub fn cursor_style_override(&self) -> Option<CursorStyle> {
        self.cursor_style_override
    }

    /// Resolve `ch` to its glyph cache key at this renderer's size: the same
    /// fallback-aware dispatch the blit path always used (primary face owns the
    /// glyph unless it has none — `.notdef` == index 0 — and only then is the
    /// fallback lazily loaded and consulted), memoized per char so the hot path
    /// pays the face lookups once, not once per blit.
    pub fn glyph_key(&mut self, ch: char) -> GlyphKey {
        if let Some(&key) = self.keys.get(&ch) {
            return key;
        }
        // Procedural box-drawing/block/braille interception happens BEFORE any
        // face lookup: those cells must be cell-exact and seam-free, which no
        // font guarantees ($ATERM_NO_PROCEDURAL_GLYPHS opts back into fonts).
        let key = if self.procedural && procedural::covers(ch) {
            GlyphKey::mono_char(FaceId::Procedural, ch, StyleBits::REGULAR, self.px_q)
        } else if self.font.lookup_glyph_index(ch) != 0 {
            GlyphKey::mono_char(FaceId::Primary, ch, StyleBits::REGULAR, self.px_q)
        } else {
            self.ensure_fallback();
            if self.fallback.as_ref().is_some_and(|fb| fb.lookup_glyph_index(ch) != 0) {
                GlyphKey::mono_char(FaceId::Fallback, ch, StyleBits::REGULAR, self.px_q)
            } else if self.color_font_has(ch) {
                // Mono faces miss it but the colour-emoji face has an sbix
                // bitmap (🚀 😀): a 32-bit RGBA glyph, not foreground-tinted.
                GlyphKey::rgba_char(FaceId::ColorEmoji, ch, self.px_q)
            } else {
                // No face covers it: the primary face renders `.notdef`.
                GlyphKey::mono_char(FaceId::Primary, ch, StyleBits::REGULAR, self.px_q)
            }
        };
        self.keys.insert(ch, key);
        key
    }

    /// Like [`glyph_key`](Self::glyph_key) but for a BOLD/ITALIC cell: the same
    /// face dispatch, with `style` carried on the resulting key so the rasterizer
    /// synthesizes the weight/slant. Procedural (cell-exact box-drawing) and
    /// colour-emoji glyphs ignore `style` — they have no synthetic variant.
    /// `REGULAR` short-circuits to the plain unstyled cache.
    pub fn glyph_key_styled(&mut self, ch: char, style: StyleBits) -> GlyphKey {
        if style == StyleBits::REGULAR {
            return self.glyph_key(ch);
        }
        if let Some(&key) = self.styled_keys.get(&(ch, style)) {
            return key;
        }
        let base = self.glyph_key(ch);
        let key = if base.glyph_class == GlyphClass::Mono && base.source != FaceId::Procedural {
            GlyphKey { style, ..base }
        } else {
            base
        };
        self.styled_keys.insert((ch, style), key);
        key
    }

    /// Resolve `ch` for a cell that requested EMOJI presentation (a VS16-widened
    /// base char — see [`RenderCell::emoji_presentation`]). Prefers the
    /// colour-emoji face: `❤️` (U+2764 + VS16) must render in colour even though
    /// the mono primary/fallback faces DO have a black-heart glyph (which the
    /// ordinary [`glyph_key`](Self::glyph_key) would pick). Falls back to the
    /// normal text dispatch if the colour font lacks the glyph. Memoized
    /// separately from `keys` so the same char can hold both presentations.
    pub fn glyph_key_emoji(&mut self, ch: char) -> GlyphKey {
        if let Some(&key) = self.emoji_keys.get(&ch) {
            return key;
        }
        let key = if self.color_font_has(ch) {
            GlyphKey::rgba_char(FaceId::ColorEmoji, ch, self.px_q)
        } else {
            // No colour glyph for this char — honour the ordinary text dispatch.
            self.glyph_key(ch)
        };
        self.emoji_keys.insert(ch, key);
        key
    }

    /// Shape an emoji grapheme cluster (ZWJ / skin-tone / keycap) to a single
    /// colour-font glyph id with rustybuzz, cached. Returns `None` unless the
    /// cluster shapes to exactly ONE glyph that has an `sbix` colour bitmap —
    /// so a non-emoji or unsupported cluster cleanly declines (caller falls back
    /// to the base codepoint). The colour font is the AAT-shaped face (Apple
    /// Color Emoji uses `morx`), which rustybuzz handles natively.
    pub fn shape_cluster(&mut self, cluster: &str) -> Option<u16> {
        if let Some(&gid) = self.cluster_gids.get(cluster) {
            return gid;
        }
        let gid = self.shape_cluster_uncached(cluster);
        self.cluster_gids.insert(cluster.into(), gid);
        gid
    }

    fn shape_cluster_uncached(&mut self, cluster: &str) -> Option<u16> {
        self.ensure_color_font();
        let bytes = self.color_font.as_ref()?;
        let face = rustybuzz::Face::from_slice(bytes, 0)?;
        let mut buf = rustybuzz::UnicodeBuffer::new();
        buf.push_str(cluster);
        let shaped = rustybuzz::shape(&face, &[], buf);
        let infos = shaped.glyph_infos();
        // Exactly one glyph means the cluster fully ligated to a single emoji.
        if infos.len() != 1 {
            return None;
        }
        let gid = u16::try_from(infos[0].glyph_id).ok()?;
        if gid == 0 {
            return None; // .notdef — not a real emoji glyph
        }
        // Must actually carry a colour bitmap, else there's nothing to draw in
        // colour and the base-codepoint fallback is the honest result.
        let tt = ttf_parser::Face::parse(bytes, 0).ok()?;
        tt.glyph_raster_image(ttf_parser::GlyphId(gid), self.cell_h.max(1) as u16)?;
        Some(gid)
    }

    /// Resolve an emoji cluster to its colour glyph key (shaped glyph id), or
    /// `None` if the cluster does not shape to a single colour glyph.
    pub fn glyph_key_cluster(&mut self, cluster: &str) -> Option<GlyphKey> {
        let gid = self.shape_cluster(cluster)?;
        Some(GlyphKey::rgba_gid(FaceId::ColorEmoji, gid, self.px_q))
    }

    /// The rasterized image for `key`, cached. External rasterizers (the GPU
    /// atlas) consume the exact bytes the CPU blit path uses, so their output
    /// can match pixel-for-pixel without duplicating the font logic/fallback.
    pub fn glyph_image(&mut self, key: GlyphKey) -> &GlyphImage {
        if !self.glyphs.contains_key(&key) {
            let img = self.rasterize(key);
            self.glyphs.insert(key, img);
        }
        &self.glyphs[&key]
    }

    /// Rasterize `key` from its source face. Keys made by [`Self::glyph_key`]
    /// always carry this renderer's own `px_q`; rasterization uses the exact
    /// `px` float the renderer was built with (no quantization round-trip), so
    /// metrics are bit-identical to a direct `fontdue` rasterization.
    fn rasterize(&mut self, key: GlyphKey) -> GlyphImage {
        debug_assert_eq!(
            key.px_q,
            self.px_q,
            "GlyphKey.px_q must match the renderer's own size this slice"
        );
        match key.glyph_class {
            GlyphClass::Mono => {
                let ch = key.chr().unwrap_or(char::REPLACEMENT_CHARACTER);
                // A Procedural key whose char IS in the procedural ranges is
                // cell-exact (the bitmap IS the cell, so the blit anchor
                // `(cell_x + xmin, cell_y + baseline - height - ymin)` lands on
                // the cell's top-left corner). A Procedural key for a char
                // outside those ranges can only be hand-built; fail safe to the
                // primary face below rather than panicking. The key's `px_q`
                // stands in for (cell_w, cell_h): both are pure functions of
                // this renderer's px and face.
                if key.source == FaceId::Procedural
                    && let Some(bytes) = procedural::coverage(ch, self.cell_w, self.cell_h)
                {
                    return GlyphImage::Mono {
                        width: self.cell_w,
                        height: self.cell_h,
                        xmin: 0,
                        ymin: self.baseline - self.cell_h as i32,
                        advance: self.cell_w as f32,
                        bytes,
                    };
                }
                let face = match key.source {
                    // ColorEmoji never carries a Mono class (it rasterizes via
                    // the Rgba arm), but cover it: fail safe to the primary face.
                    FaceId::Primary | FaceId::Procedural | FaceId::ColorEmoji => &self.font,
                    FaceId::Fallback => {
                        self.ensure_fallback();
                        self.fallback.as_ref().unwrap_or(&self.font)
                    }
                };
                let (m, bytes) = face.rasterize(ch, self.px);
                // Synthetic bold/italic: thicken / shear the coverage. REGULAR
                // keys pass straight through (the common path).
                let (width, bytes) =
                    apply_synthetic_style(key.style, m.width, m.height, bytes, self.px);
                GlyphImage::Mono {
                    width,
                    height: m.height,
                    xmin: m.xmin,
                    ymin: m.ymin,
                    advance: m.advance_width,
                    bytes,
                }
            }
            GlyphClass::Rgba => {
                let ch = key.chr().unwrap_or(char::REPLACEMENT_CHARACTER);
                // Empty image on any failure (missing/undecodable bitmap): the
                // blit treats a 0-sized Rgba glyph as a no-op, same as a space.
                self.rasterize_color_emoji(ch).unwrap_or_else(empty_rgba)
            }
            GlyphClass::RgbaGid => {
                // `ch_or_id` is a shaped colour-font glyph id (a cluster).
                let gid = ttf_parser::GlyphId(key.ch_or_id as u16);
                self.rasterize_color_emoji_gid(gid).unwrap_or_else(empty_rgba)
            }
        }
    }

    /// Rasterize a single-codepoint colour emoji: map `ch` to its glyph id in
    /// the colour face, then pull + scale the bitmap. `None` if the face/glyph
    /// is missing.
    fn rasterize_color_emoji(&mut self, ch: char) -> Option<GlyphImage> {
        self.ensure_color_font();
        let bytes = self.color_font.as_ref()?;
        let gid = ttf_parser::Face::parse(bytes, 0).ok()?.glyph_index(ch)?;
        self.rasterize_color_emoji_gid(gid)
    }

    /// Rasterize a colour-emoji glyph BY glyph id (a cluster already shaped to
    /// one glyph): pull the `sbix` PNG bitmap from the colour-emoji face, decode
    /// it to RGBA8, and scale it (preserving aspect) to fit a 2-cell-wide box —
    /// emoji are full-width. Returns `None` if the bitmap is missing/undecodable.
    fn rasterize_color_emoji_gid(&mut self, gid: ttf_parser::GlyphId) -> Option<GlyphImage> {
        self.ensure_color_font();
        let bytes = self.color_font.as_ref()?;
        let face = ttf_parser::Face::parse(bytes, 0).ok()?;
        // Ask for a strike at least as tall as the cell so we DOWNscale (sharper)
        // rather than upscale; Apple strikes are 20/32/40/48/64/96/160 px.
        let raster = face.glyph_raster_image(gid, self.cell_h.max(1) as u16)?;
        if !matches!(raster.format, ttf_parser::RasterImageFormat::PNG) {
            return None;
        }
        let decoder = png::Decoder::new(raster.data);
        let mut reader = decoder.read_info().ok()?;
        let mut buf = vec![0u8; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buf).ok()?;
        if info.bit_depth != png::BitDepth::Eight {
            return None;
        }
        let (src_w, src_h) = (info.width as usize, info.height as usize);
        let src = to_rgba8(&buf[..info.buffer_size()], info.color_type, src_w, src_h)?;

        // Target: a wide cell = 2 cells. Fit the (square) emoji into
        // (2*cell_w) x cell_h, preserving aspect, centred.
        let box_w = (2 * self.cell_w).max(1);
        let box_h = self.cell_h.max(1);
        let scale = (box_w as f32 / src_w as f32).min(box_h as f32 / src_h as f32);
        let dst_w = ((src_w as f32 * scale).round() as usize).clamp(1, box_w);
        let dst_h = ((src_h as f32 * scale).round() as usize).clamp(1, box_h);
        let dst = bilinear_rgba(&src, src_w, src_h, dst_w, dst_h);

        // Centre horizontally in the 2-cell box; centre vertically in the cell.
        // The blit anchors at `cell_y + baseline - height - ymin`, so to land the
        // glyph `top_inset` px below the cell top we set ymin accordingly.
        let xmin = ((box_w - dst_w) / 2) as i32;
        let top_inset = ((box_h as i32 - dst_h as i32) / 2).max(0);
        let ymin = self.baseline - dst_h as i32 - top_inset;
        Some(GlyphImage::Rgba { width: dst_w, height: dst_h, xmin, ymin, advance: box_w as f32, bytes: dst })
    }

    /// Snapshot everything the renderers read from `term` for a `rows`x`cols`
    /// frame into a plain [`RenderInput`] — a fast, READ-ONLY clone of the
    /// visible rows plus the cursor/selection scalars. The windowed frontend
    /// calls this under the `Terminal` lock, then renders without it.
    ///
    /// This is an associated function (it borrows only `&Terminal`, not the
    /// renderer) so a caller can lock, extract, drop the guard, and only then
    /// borrow the renderer mutably for [`Self::render_input`]. Renderer-owned
    /// state (blink phase, cursor-style override) is deliberately NOT captured
    /// here; it is consulted in `render_input`.
    pub fn extract(term: &Terminal, rows: usize, cols: usize) -> RenderInput {
        // Allocate an empty snapshot, then fill it via the shared in-place path
        // so `extract` and `extract_into` produce byte-identical output. (This
        // path still allocates a fresh `RenderInput`; the reuse win is in
        // `extract_into`, which the windowed frontend calls on a kept scratch.)
        let mut scratch = RenderInput::default();
        Self::extract_into(&mut scratch, term, rows, cols);
        scratch
    }

    /// Like [`extract`](Self::extract), but REFILLS a caller-owned `scratch`
    /// [`RenderInput`] in place instead of allocating a fresh one each frame.
    ///
    /// The three per-row container Vecs of Vecs (`cells`, `clusters`,
    /// `combining`) are resized to `rows` REUSING their existing inner per-row
    /// Vecs in place (truncating if shorter, pushing fresh empty Vecs if
    /// longer), then each row's inner Vec is `clear()`ed + refilled by the
    /// matching aterm-core `*_row_into` accessor. So when the grid dimensions
    /// are stable (the common case: same window, frame after frame) NEITHER the
    /// outer Vecs NOR the inner per-row Vecs reallocate. `line_sizes` is
    /// `.clear()`ed (its elements are `Copy`, no inner allocation). The data is
    /// byte-for-byte identical to what [`extract`](Self::extract) produces —
    /// the `*_row_into` accessors share the one code path with `*_row`.
    ///
    /// Per-frame allocation of the four containers AND the per-row inner Vecs is
    /// now elided. What still allocates is the owned cluster/mark CONTENT
    /// (`Box<str>` per emoji cluster, `Box<[char]>` per combining cell) the
    /// `*_row_into` accessors push — that is per-cluster owned data, only
    /// present for emoji/diacritic cells, and not removable without a grid
    /// borrow API. Plain ASCII rows push none of those, so they are
    /// allocation-free in steady state.
    ///
    /// IMPORTANT: do NOT `.clear()` the outer container Vecs — that drops the
    /// inner per-row Vecs, throwing away their grown capacity and forcing a
    /// fresh allocation per row next frame. Resize-in-place is what preserves
    /// the inner buffers.
    ///
    /// The scratch is the INCOMING snapshot for [`render_input`](Self::render_input);
    /// it is distinct from that method's own damage cache (`self.cache.input`,
    /// the PREVIOUS frame's snapshot), so the two never alias.
    pub fn extract_into(scratch: &mut RenderInput, term: &Terminal, rows: usize, cols: usize) {
        scratch.rows = rows;
        scratch.cols = cols;

        // Resize the outer Vec-of-Vecs to `rows`, KEEPING the existing inner
        // per-row Vecs (their grown capacity), then refill each in place via the
        // `*_row_into` accessor (which clears + repushes). `resize_with` truncates
        // when `rows` shrank (dropping the surplus inner Vecs) and appends fresh
        // empty Vecs when `rows` grew; the `0..len` already-present rows keep
        // their buffers untouched until the per-row `*_into` clear+refill below.
        scratch.cells.resize_with(rows, Vec::new);
        for (r, cells) in scratch.cells.iter_mut().enumerate() {
            term.render_row_into(r, cells);
        }

        scratch.clusters.resize_with(rows, Vec::new);
        for (r, clusters) in scratch.clusters.iter_mut().enumerate() {
            term.cluster_row_into(r, clusters);
        }

        scratch.combining.resize_with(rows, Vec::new);
        for (r, combining) in scratch.combining.iter_mut().enumerate() {
            term.combining_row_into(r, combining);
        }

        scratch.line_sizes.clear();
        scratch.line_sizes.extend((0..rows).map(|r| {
            u16::try_from(r)
                .ok()
                .and_then(|vr| term.grid().row(vr))
                .map_or(LineSize::SingleWidth, |row| row.line_size())
        }));

        let cur = term.cursor();
        scratch.cursor_row = cur.row as usize;
        scratch.cursor_col = cur.col as usize;
        scratch.cursor_visible = term.cursor_visible();
        scratch.cursor_style = term.cursor_style();
        scratch.display_offset = term.grid().display_offset() as i32;
        // `clone_from` reuses the destination's existing allocation where the
        // selection's owned data permits, instead of dropping + reallocating.
        scratch.selection.clone_from(term.text_selection());
    }

    /// Render the terminal's visible `rows`x`cols` grid to a framebuffer.
    ///
    /// Each cell is painted with its own resolved colours: the cell rect is
    /// filled with the cell's background, then the glyph is blitted in the
    /// cell's foreground. Empty trailing cells keep the theme background.
    /// Cells inside the active text selection get `theme.selection` as their
    /// background instead (foreground/glyph unchanged); the cursor block is
    /// layered on top of everything, selection included.
    pub fn render(&mut self, term: &Terminal, rows: usize, cols: usize) -> Frame {
        let input = Self::extract(term, rows, cols);
        self.render_input(&input)
    }

    /// Render a previously [`extract`](Self::extract)ed [`RenderInput`] to a
    /// framebuffer — identical pixels to [`render`](Self::render), but with no
    /// `&Terminal` borrow, so the caller can render after releasing the lock.
    ///
    /// Damage-tracked: the output is byte-identical to a full repaint, but only
    /// the rows that actually changed (plus the old/new cursor rows) are
    /// re-rendered, the rest are reused from [`Self::cache`], and a frame with
    /// nothing to change at all returns the cached pixels untouched. The slow
    /// full-repaint path is taken on the first frame and whenever a precondition
    /// for safe reuse is violated (see [`Self::full_render`]); it produces the
    /// same pixels either way — only the WORK differs.
    ///
    /// This returns an OWNED [`Frame`], so it CLONES the damage cache's
    /// framebuffer (the snapshot / `read_image` / test path, which needs to keep
    /// the pixels past the next render). The per-frame presentation hot path
    /// should call [`render_input_cached`](Self::render_input_cached) instead,
    /// which hands back a borrow and elides this clone; both share the one
    /// rendering code path below, so they are byte-identical by construction.
    pub fn render_input(&mut self, input: &RenderInput) -> Frame {
        // ONE rendering code path: do the damage render into the cache, then
        // clone the borrowed result into an owned Frame. The clone is the price
        // of ownership; the hot path avoids it via `render_input_cached`.
        let view = self.render_input_cached(input);
        Frame { width: view.width(), height: view.height(), pixels: view.pixels().to_vec() }
    }

    /// Render a pre-extracted snapshot like [`render_input`](Self::render_input)
    /// but return a [`RenderView`] BORROWING the renderer's persistent damage
    /// cache — NO per-frame `Frame` clone, NO per-frame `Vec` allocation. This is
    /// the per-frame PRESENTATION hot path: a windowed frontend copies the borrow
    /// straight into its surface, so the only full-framebuffer copy per frame is
    /// that surface copy (cache→Frame is gone).
    ///
    /// The pixels are byte-identical to [`render_input`](Self::render_input) —
    /// indeed `render_input` is implemented as this method plus a clone, so there
    /// is exactly one rendering code path and no way for the two to drift.
    ///
    /// The returned borrow is tied to `&mut self`: it is valid until the next
    /// call that mutates the renderer (the next render reuses the same cache
    /// buffer in place).
    pub fn render_input_cached(&mut self, input: &RenderInput) -> RenderView<'_> {
        let (rows, cols) = (input.rows, input.cols);
        let (w, h) = self.frame_size(rows, cols);

        // Decide whether the cached frame can be reused. The full-render path is
        // taken (and the cache rebuilt) on the first frame, on any geometry /
        // scrollback / selection change, or on any double-HEIGHT row — DECDHL
        // top/bottom halves clip a 2× glyph across two row bands, so a single
        // dirty row can't be repainted in isolation without risking a seam.
        let reusable = match &self.cache {
            Some(c) => {
                c.width == w
                    && c.height == h
                    && c.input.rows == rows
                    && c.input.cols == cols
                    && c.input.display_offset == input.display_offset
                    && c.input.selection == input.selection
                    && !any_double_height(&c.input.line_sizes)
                    && !any_double_height(&input.line_sizes)
            }
            None => false,
        };
        if !reusable {
            self.full_render(input, w, h);
            return self.cached_view(w, h);
        }

        // DAMAGED PATH. Reuse the cached framebuffer in place — no allocation.
        // Take the cache out so the per-row helpers can borrow `self` mutably
        // (glyph rasterization caches mutate `self`); restored before return.
        let mut cache = self.cache.take().expect("reusable implies Some");
        debug_assert_eq!(cache.pixels.len(), w * h);

        // The dirty row set: any row whose render-relevant inputs differ from
        // the cached frame, UNION the previous and current cursor rows (only
        // when the cursor is/was actually shown — an invisible cursor paints
        // nothing, so its row needs no repaint on that account).
        let prev = &cache.input;
        let mut dirty = vec![false; rows];
        let mut any_dirty = false;
        for r in 0..rows {
            if row_differs(input, prev, r) {
                dirty[r] = true;
                any_dirty = true;
            }
        }
        // Cursor: where it was last frame and where it is this frame. Mark each
        // for repaint so the old cursor is erased and the new one drawn. Use the
        // SAME shown-test the overlay uses (style override + blink phase).
        let prev_style = cache.cursor_style_override.unwrap_or(prev.cursor_style);
        let prev_shown = prev.cursor_row < rows
            && prev.cursor_col < cols
            && prev.cursor_visible
            && cursor_shown(prev_style, cache.cursor_blink_phase);
        let cur_style = self.cursor_style_override.unwrap_or(input.cursor_style);
        let cur_shown = input.cursor_row < rows
            && input.cursor_col < cols
            && input.cursor_visible
            && cursor_shown(cur_style, self.cursor_blink_phase);
        // The cursor cell's own glyph/colours feed the block "cut-out", but a
        // change there is already a row-content change caught by `row_differs`,
        // so we only track the cursor's position / style / shown-ness here.
        let cursor_changed = prev_shown != cur_shown
            || (cur_shown
                && (prev.cursor_row != input.cursor_row
                    || prev.cursor_col != input.cursor_col
                    || prev_style != cur_style));
        if prev_shown {
            mark(&mut dirty, prev.cursor_row);
        }
        if cur_shown {
            mark(&mut dirty, input.cursor_row);
        }

        // DIRTY-GATE: nothing to draw — no dirty rows, the cursor is in the same
        // place/state, and the blink phase is unchanged — so the cached pixels
        // are already exactly this frame. Hand them back with zero rendering.
        //
        // The decision delegates to the shared `is_unchanged_frame` predicate so
        // the CPU gate and the GPU dirty-gate are ONE source of truth and cannot
        // diverge. `dirty`/`cursor_changed` above remain the implementation of
        // the MISS path (which rows to repaint); the predicate is a strict
        // function of the same inputs, so its verdict matches `!any_dirty &&
        // !cursor_changed && blink/override unchanged` exactly. (Asserted in
        // debug builds.)
        let gate_hit = is_unchanged_frame(
            prev,
            cache.cursor_blink_phase,
            cache.cursor_style_override,
            input,
            self.cursor_blink_phase,
            self.cursor_style_override,
        );
        debug_assert_eq!(
            gate_hit,
            !any_dirty
                && !cursor_changed
                && cache.cursor_blink_phase == self.cursor_blink_phase
                && cache.cursor_style_override == self.cursor_style_override,
            "is_unchanged_frame must agree with the inline dirty/cursor gate"
        );
        if gate_hit {
            self.cache = Some(cache);
            return self.cached_view(w, h);
        }

        // Re-render each dirty row into the reused framebuffer: first restore its
        // band to the theme background (replicating the `vec![bg]` the full path
        // starts from for that band), then run the IDENTICAL passes 1/2/3.
        for r in 0..rows {
            if dirty[r] {
                self.fill_band_bg(&mut cache.pixels, w, h, r);
                self.render_row(&mut cache.pixels, w, h, input, r);
            }
        }
        // Cursor overlay — the EXACT same code the full path runs.
        self.draw_cursor(&mut cache.pixels, w, h, input);

        // Refresh the cache to this frame's state, then borrow it back out.
        cache.input = input.clone();
        cache.cursor_blink_phase = self.cursor_blink_phase;
        cache.cursor_style_override = self.cursor_style_override;
        self.cache = Some(cache);
        self.cached_view(w, h)
    }

    /// Borrow the current damage cache as a [`RenderView`]. Only called right
    /// after a render has populated `self.cache`, so the `expect` cannot fire.
    fn cached_view(&self, w: usize, h: usize) -> RenderView<'_> {
        let pixels = &self.cache.as_ref().expect("cache populated by render").pixels;
        RenderView::Borrowed { width: w, height: h, pixels }
    }

    /// TEST/BENCH SCAFFOLDING ONLY — drop the damage-tracking cache so the very
    /// next [`render_input`](Self::render_input) takes the full-repaint path (as
    /// if it were the first frame). This does NOT change normal rendering: the
    /// cache is rebuilt on that next frame exactly as construction leaves it
    /// (`cache: None`), and the pixels produced are byte-identical either way —
    /// only the WORK differs. It exists so a benchmark can measure the OLD
    /// pre-optimization behavior (full repaint every frame) on one warm renderer:
    /// call it between frames to defeat row-reuse. It is `#[doc(hidden)]` and
    /// carries no semantic meaning for production callers, which never reset the
    /// cache mid-session.
    #[doc(hidden)]
    pub fn reset_damage_cache(&mut self) {
        self.cache = None;
    }

    /// The full-repaint path: allocate a fresh background-filled framebuffer,
    /// render every row, draw the cursor, then cache the result for the next
    /// frame's damage tracking. Byte-identical to the original `render_input`.
    /// Leaves the rendered framebuffer in `self.cache`; the caller borrows it
    /// back out (via [`cached_view`](Self::cached_view)) — no clone here.
    fn full_render(&mut self, input: &RenderInput, w: usize, h: usize) {
        let rows = input.rows;
        let mut pixels = vec![self.theme.bg; w * h];
        for r in 0..rows {
            // The whole buffer is already bg-filled, so each row's band starts
            // from the theme background exactly as the damaged path arranges.
            self.render_row(&mut pixels, w, h, input, r);
        }
        self.draw_cursor(&mut pixels, w, h, input);

        self.cache = Some(RenderCache {
            pixels,
            width: w,
            height: h,
            input: input.clone(),
            cursor_blink_phase: self.cursor_blink_phase,
            cursor_style_override: self.cursor_style_override,
        });
    }

    /// Fill row `r`'s full pixel band — `y in [r*cell_h, (r+1)*cell_h)`,
    /// `x in [0, w)` — with the theme background. In the damaged path this
    /// re-establishes the `vec![bg]` starting state for the band before its
    /// passes run, so the row is repainted from scratch exactly as a full render
    /// would.
    fn fill_band_bg(&self, pixels: &mut [u32], w: usize, h: usize, r: usize) {
        let y0 = r * self.cell_h;
        let y1 = ((r + 1) * self.cell_h).min(h);
        if y0 >= y1 {
            return;
        }
        pixels[y0 * w..y1 * w].fill(self.theme.bg);
    }

    /// Render one row `r` of `input` into `pixels` — passes 1 (per-cell bg /
    /// selection fill), 2 (glyph + combining-mark blit), 3 (underline / strike /
    /// overline). The row's band is assumed already filled with the theme
    /// background (the full path pre-fills the whole buffer; the damaged path
    /// calls [`Self::fill_band_bg`] first). Shared verbatim by both paths so
    /// they can never drift.
    fn render_row(&mut self, pixels: &mut [u32], w: usize, h: usize, input: &RenderInput, r: usize) {
        let cols = input.cols;
        let selection = &input.selection;
        // Selection rows are live-screen coords; the viewport may be scrolled
        // back, so viewport row r shows live row (r - display_offset).
        let display_offset = input.display_offset;
        let cells = &input.cells[r];
        let y0 = r * self.cell_h;
        let sel_row = r as i32 - display_offset;
        // DEC line size (DECDWL/DECDHL): `cw` is the on-screen cell advance,
        // `scale`/`anchor_y` drive the glyph's NEAREST enlargement + clip.
        let line_size = input.line_sizes[r];
        let cw = row_cell_w(line_size, self.cell_w);
        let (scale, anchor_y) = row_scale(line_size, y0, self.cell_h);
        // Pass 1: fill every cell rect with its background colour. Done
        // before any glyph so a wide glyph (which overflows into its
        // continuation column) isn't clobbered by that column's fill.
        // Selected cells take the theme's selection background instead.
        for (c, cell) in cells.iter().take(cols).enumerate() {
            // A lead cell is wide iff the NEXT cell is its continuation.
            let is_wide_lead = cells.get(c + 1).is_some_and(|n| n.wide);
            let bg = if selection.contains_cell(sel_row, c as u16, is_wide_lead, cell.wide) {
                self.theme.selection
            } else {
                rgb_to_u32(cell.bg)
            };
            self.fill_rect(pixels, w, h, c * cw, y0, cw, self.cell_h, bg);
        }
        // Pass 2: blit each glyph in the cell's foreground over the fills.
        // Wide continuation columns carry no glyph of their own.
        let row_clusters = &input.clusters[r];
        let row_combining = &input.combining[r];
        for (c, cell) in cells.iter().take(cols).enumerate() {
            if !cell.wide && cell.ch != ' ' && !cell.ch.is_control() {
                let cluster = cluster_for(row_clusters, c);
                let key = self.resolve_cell_key(cluster, cell);
                self.blit(pixels, w, (c * cw) as i32, anchor_y, key, rgb_to_u32(cell.fg), scale);
                // Overlay combining diacritics (é, ñ, …) on the base. A
                // combining mark's own metrics assume the pen sits at the
                // base's advance (a large negative left bearing backs it up
                // over the base), so blitting at the cell origin drops it far
                // left. In a monospace cell the base glyph is centred in its
                // advance, so we centre the mark's INK in the cell — putting
                // the accent over the base, on CPU and GPU identically.
                if cluster.is_none() {
                    if let Some(marks) = combining_for(row_combining, c) {
                        for &m in marks {
                            let mk = self.glyph_key(m);
                            let (gw, xmin) =
                                { let mi = self.glyph_image(mk); (mi.width(), mi.xmin()) };
                            let cx = mark_cell_x(c, cw, gw, xmin, scale);
                            self.blit(pixels, w, cx, anchor_y, mk, rgb_to_u32(cell.fg), scale);
                        }
                    }
                }
            }
        }
        // Pass 3: line decorations (underline / strikethrough / overline)
        // OVER the glyphs. The lead of a wide glyph draws across both cells.
        for (c, cell) in cells.iter().take(cols).enumerate() {
            if cell.wide
                || (matches!(cell.underline, UnderlineStyle::None)
                    && !cell.strikethrough
                    && !cell.overline)
            {
                continue;
            }
            let x = c * cw;
            let dw = if cells.get(c + 1).is_some_and(|n| n.wide) { 2 * cw } else { cw };
            let ucolor = rgb_to_u32(cell.underline_color.unwrap_or(cell.fg));
            for [rx, ry, rw, rh] in
                underline_rects(cell.underline, x, y0, dw, self.cell_h, self.baseline)
            {
                self.fill_rect(pixels, w, h, rx, ry, rw, rh, ucolor);
            }
            let fgc = rgb_to_u32(cell.fg);
            for [rx, ry, rw, rh] in strike_overline_rects(
                cell.strikethrough,
                cell.overline,
                x,
                y0,
                dw,
                self.cell_h,
                self.baseline,
            ) {
                self.fill_rect(pixels, w, h, rx, ry, rw, rh, fgc);
            }
        }
    }

    /// Draw the cursor overlay onto `pixels` — the EXACT same logic the original
    /// `render_input` ran after the row loop, factored out so the full and
    /// damaged paths share it verbatim.
    ///
    /// The cursor is shaped by DECSCUSR (`input.cursor_style`) or the frontend's
    /// override (unfocused windows force HollowBlock). The block styles fill the
    /// cell with the cursor colour and re-draw the glyph in the cell's own
    /// background ("cut out"); underline/bar/hollow paint only their strip /
    /// outline OVER the normally drawn glyph. Nothing is drawn when DECTCEM hides
    /// the cursor, the style is Hidden, or a Blinking* style is in its off phase.
    fn draw_cursor(&mut self, pixels: &mut [u32], w: usize, h: usize, input: &RenderInput) {
        let (rows, cols) = (input.rows, input.cols);
        let (cr, cc) = (input.cursor_row, input.cursor_col);
        let style = self.cursor_style_override.unwrap_or(input.cursor_style);
        if cr < rows
            && cc < cols
            && input.cursor_visible
            && cursor_shown(style, self.cursor_blink_phase)
        {
            // The cursor row may itself be a DEC double-size line.
            let line_size = input.line_sizes[cr];
            let cw = row_cell_w(line_size, self.cell_w);
            let (scale, anchor_y) = row_scale(line_size, cr * self.cell_h, self.cell_h);
            let (x0, y0) = (cc * cw, cr * self.cell_h);
            for [rx, ry, rw, rh] in cursor_rects(style, x0, y0, cw, self.cell_h) {
                self.fill_rect(pixels, w, h, rx, ry, rw, rh, self.theme.cursor);
            }
            if matches!(style, CursorStyle::BlinkingBlock | CursorStyle::SteadyBlock) {
                let cells = &input.cells[cr];
                if let Some(cell) = cells.get(cc) {
                    if !cell.wide && cell.ch != ' ' && !cell.ch.is_control() {
                        let cluster = cluster_for(&input.clusters[cr], cc);
                        let key = self.resolve_cell_key(cluster, cell);
                        self.blit(pixels, w, x0 as i32, anchor_y, key, rgb_to_u32(cell.bg), scale);
                    }
                }
            }
        }
    }

    /// Fill an arbitrary pixel rectangle with a solid colour, clipped to the
    /// frame.
    #[allow(clippy::too_many_arguments)]
    fn fill_rect(
        &self,
        px: &mut [u32],
        w: usize,
        h: usize,
        x0: usize,
        y0: usize,
        rw: usize,
        rh: usize,
        color: u32,
    ) {
        for y in y0..(y0 + rh).min(h) {
            for x in x0..(x0 + rw).min(w) {
                px[y * w + x] = color;
            }
        }
    }

    /// Resolve a cell to its glyph key: a shaped emoji CLUSTER (ZWJ / skin-tone
    /// / keycap) takes priority, then a VS16 emoji-presentation base, then the
    /// ordinary text dispatch. `cluster` is the cell's grapheme-cluster string
    /// when [`RenderInput::clusters`] holds one for it. Public so the GPU atlas
    /// builder resolves keys through the EXACT same dispatch (CPU/GPU parity).
    pub fn resolve_cell_key(&mut self, cluster: Option<&str>, cell: &RenderCell) -> GlyphKey {
        if let Some(cl) = cluster
            && let Some(k) = self.glyph_key_cluster(cl)
        {
            return k;
        }
        if cell.emoji_presentation {
            self.glyph_key_emoji(cell.ch)
        } else {
            self.glyph_key_styled(cell.ch, cell_style(cell))
        }
    }

    /// Blit one glyph into the framebuffer for the already-resolved `key`,
    /// blending `color` over the existing pixels (coverage for Mono, straight
    /// src-alpha for colour glyphs). [`Scale`] enlarges the glyph by NEAREST
    /// replication — 2× wide for a DECDWL row, 2× both with a dest-row clip for
    /// a DECDHL half-row — so the GPU's nearest-sampled quad matches exactly.
    /// `cell_y` is `i32` because a DECDHL bottom half anchors one row up.
    // Each argument is a distinct rendering input (framebuffer, geometry, glyph,
    // colour, scale); bundling them into a struct would only obscure the hot
    // blit call sites without removing any parameter.
    #[allow(clippy::too_many_arguments)]
    fn blit(
        &mut self,
        px: &mut [u32],
        stride: usize,
        cell_x: i32,
        cell_y: i32,
        key: GlyphKey,
        color: u32,
        scale: Scale,
    ) {
        let baseline = self.baseline;
        let img = self.glyph_image(key);
        let (width, height, xmin, ymin) = (img.width(), img.height(), img.xmin(), img.ymin());
        if width == 0 || height == 0 {
            return;
        }
        let (xs, ys) = (scale.xs.max(1), scale.ys.max(1));
        let (clip_y0, clip_y1) = (scale.clip_y0, scale.clip_y1);
        let gx0 = cell_x + xmin * xs as i32;
        // Baseline scales with the vertical factor (a 2× cell has a 2× ascent).
        let gy0 = cell_y + ys as i32 * (baseline - height as i32 - ymin);
        let px_len = px.len();
        // Absolute (x, y) -> framebuffer index, clipped to the frame AND the
        // dest-row window (the DECDHL top/bottom half).
        let fb_at = |x: i32, y: i32| -> Option<usize> {
            if x < 0 || y < clip_y0 || y >= clip_y1 || y < 0 || x as usize >= stride {
                return None;
            }
            let idx = y as usize * stride + x as usize;
            (idx < px_len).then_some(idx)
        };
        // Each source pixel spans `xs` × `ys` destination pixels (nearest).
        match img {
            // Coverage glyph: tint the cell foreground by per-texel coverage.
            GlyphImage::Mono { bytes, .. } => {
                for j in 0..height {
                    for i in 0..width {
                        let cov = bytes[j * width + i];
                        if cov == 0 {
                            continue;
                        }
                        for sy in 0..ys {
                            let y = gy0 + (j * ys + sy) as i32;
                            for sx in 0..xs {
                                if let Some(idx) = fb_at(gx0 + (i * xs + sx) as i32, y) {
                                    px[idx] = blend(px[idx], color, cov);
                                }
                            }
                        }
                    }
                }
            }
            // Colour emoji: alpha-over the glyph's OWN colours (straight alpha;
            // the cell foreground is irrelevant for colour glyphs).
            GlyphImage::Rgba { bytes, .. } => {
                for j in 0..height {
                    for i in 0..width {
                        let p = (j * width + i) * 4;
                        let a = bytes[p + 3];
                        if a == 0 {
                            continue;
                        }
                        let rgb = ((bytes[p] as u32) << 16)
                            | ((bytes[p + 1] as u32) << 8)
                            | (bytes[p + 2] as u32);
                        for sy in 0..ys {
                            let y = gy0 + (j * ys + sy) as i32;
                            for sx in 0..xs {
                                if let Some(idx) = fb_at(gx0 + (i * xs + sx) as i32, y) {
                                    px[idx] = blend(px[idx], rgb, a);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Whether a cursor of `style` is drawn at this blink phase: `Hidden` never,
/// the `Blinking*` styles only while the phase is on, steady styles always.
/// Shared by the CPU and GPU renderers so their suppression rules agree.
pub fn cursor_shown(style: CursorStyle, blink_phase: bool) -> bool {
    match style {
        CursorStyle::Hidden => false,
        CursorStyle::BlinkingBlock | CursorStyle::BlinkingUnderline | CursorStyle::BlinkingBar => {
            blink_phase
        }
        _ => true,
    }
}

/// The pixel rects (`[x, y, w, h]`) a cursor of `style` paints in the theme's
/// cursor colour, for the cell at `(x0, y0)` of size `cell_w` x `cell_h`:
///
/// * block — the whole cell (the caller re-draws the glyph over it, "cut out"),
/// * underline — the bottom strip, `max(2, cell_h/8)` px tall,
/// * bar — the left strip, `max(2, cell_w/8)` px wide,
/// * hollow block — a `max(1, cell_h/16)` px outline rectangle,
/// * hidden — nothing.
///
/// Shared by the CPU and GPU renderers so their cursor geometry is identical.
pub fn cursor_rects(
    style: CursorStyle,
    x0: usize,
    y0: usize,
    cell_w: usize,
    cell_h: usize,
) -> Vec<[usize; 4]> {
    match style {
        CursorStyle::BlinkingBlock | CursorStyle::SteadyBlock => {
            vec![[x0, y0, cell_w, cell_h]]
        }
        CursorStyle::BlinkingUnderline | CursorStyle::SteadyUnderline => {
            let t = (cell_h / 8).max(2).min(cell_h);
            vec![[x0, y0 + cell_h - t, cell_w, t]]
        }
        CursorStyle::BlinkingBar | CursorStyle::SteadyBar => {
            let t = (cell_w / 8).max(2).min(cell_w);
            vec![[x0, y0, t, cell_h]]
        }
        CursorStyle::HollowBlock => {
            let t = (cell_h / 16).max(1).min(cell_w.min(cell_h));
            let mid = cell_h.saturating_sub(2 * t);
            vec![
                [x0, y0, cell_w, t],
                [x0, y0 + cell_h - t, cell_w, t],
                [x0, y0 + t, t, mid],
                [x0 + cell_w - t, y0 + t, t, mid],
            ]
        }
        // Hidden (and, fail-safe, any future variant: the enum is
        // non-exhaustive) paints nothing.
        _ => Vec::new(),
    }
}

/// The pixel rects (`[x, y, w, h]`) for a cell's UNDERLINE decoration, drawn in
/// the underline colour. Patterned styles (curly/dotted/dashed) are emitted as
/// a list of short straight rects so the CPU and GPU draw byte-identical pixels
/// (no per-path antialiasing to diverge). `baseline` is the ascent in pixels
/// from the cell top; `w` is the glyph width (2 cells for a wide glyph).
pub fn underline_rects(
    style: UnderlineStyle,
    x0: usize,
    y0: usize,
    w: usize,
    cell_h: usize,
    baseline: i32,
) -> Vec<[usize; 4]> {
    if matches!(style, UnderlineStyle::None) || w == 0 || cell_h == 0 {
        return Vec::new();
    }
    let t = (cell_h / 15).max(1);
    let base = baseline.max(0) as usize;
    // A hair below the baseline, kept fully inside the cell.
    let uy = (y0 + base + t).min(y0 + cell_h.saturating_sub(t));
    match style {
        UnderlineStyle::None => Vec::new(),
        UnderlineStyle::Single => vec![[x0, uy, w, t]],
        UnderlineStyle::Double => {
            let gap = (2 * t).max(2);
            let top = uy.saturating_sub(gap);
            vec![[x0, top, w, t], [x0, uy, w, t]]
        }
        UnderlineStyle::Curly => {
            // Square-wave squiggle: alternating up/down segments. Recognisable
            // as "wavy" and parity-safe (identical rects on both renderers).
            let amp = t;
            let seg = (cell_h / 6).max(2);
            let mut rects = Vec::new();
            let (mut x, mut up) = (x0, false);
            while x < x0 + w {
                let sw = seg.min(x0 + w - x);
                let yy = if up { uy.saturating_sub(amp) } else { uy };
                rects.push([x, yy, sw, t]);
                x += sw;
                up = !up;
            }
            rects
        }
        UnderlineStyle::Dotted => {
            let dot = t.max(1);
            let step = (2 * dot).max(2);
            let mut rects = Vec::new();
            let mut x = x0;
            while x < x0 + w {
                rects.push([x, uy, dot.min(x0 + w - x), t]);
                x += step;
            }
            rects
        }
        UnderlineStyle::Dashed => {
            let dash = (w / 3).max(1);
            let step = dash + (dash / 2).max(1);
            let mut rects = Vec::new();
            let mut x = x0;
            while x < x0 + w {
                rects.push([x, uy, dash.min(x0 + w - x), t]);
                x += step;
            }
            rects
        }
    }
}

/// The pixel rects for a cell's STRIKETHROUGH (through the glyph middle) and
/// OVERLINE (along the cell top) decorations, drawn in the foreground colour.
pub fn strike_overline_rects(
    strikethrough: bool,
    overline: bool,
    x0: usize,
    y0: usize,
    w: usize,
    cell_h: usize,
    baseline: i32,
) -> Vec<[usize; 4]> {
    if w == 0 || cell_h == 0 {
        return Vec::new();
    }
    let t = (cell_h / 15).max(1);
    let mut rects = Vec::new();
    if strikethrough {
        // Through the x-height middle: a third of the ascent above the baseline.
        let base = baseline.max(0) as usize;
        let sy = (y0 + base.saturating_sub((base / 3).max(1))).min(y0 + cell_h.saturating_sub(t));
        rects.push([x0, sy, w, t]);
    }
    if overline {
        rects.push([x0, y0, w, t]);
    }
    rects
}

/// Find the emoji-cluster string for column `col` in a row's sparse cluster
/// list (`RenderInput::clusters[row]`). Rows almost never have clusters, so the
/// linear scan is over an empty/tiny slice on the hot path.
fn cluster_for(row_clusters: &[(usize, Box<str>)], col: usize) -> Option<&str> {
    row_clusters.iter().find(|(c, _)| *c == col).map(|(_, s)| s.as_ref())
}

/// Find the combining marks for column `col` in a row's sparse combining list.
fn combining_for(row_combining: &[(usize, Box<[char]>)], col: usize) -> Option<&[char]> {
    row_combining.iter().find(|(c, _)| *c == col).map(|(_, m)| m.as_ref())
}

/// Whether any row's DEC line size is a double-HEIGHT band (DECDHL top/bottom).
/// Such a row's 2× glyph is clipped across two adjacent row bands, so a single
/// dirty row can't be repainted in isolation — the damage path falls back to a
/// full render when this holds for either the cached or the incoming frame.
fn any_double_height(line_sizes: &[LineSize]) -> bool {
    line_sizes
        .iter()
        .any(|ls| matches!(ls, LineSize::DoubleHeightTop | LineSize::DoubleHeightBottom))
}

/// Whether row `r`'s render-relevant inputs differ between two frames: the
/// resolved cells, the sparse cluster / combining-mark lists, or the DEC line
/// size. (Selection and display-offset are frame-global and gate the whole
/// damaged path, so they are not re-checked per row.) Equality here is exact, so
/// an unchanged row is provably safe to reuse from the cache.
fn row_differs(a: &RenderInput, b: &RenderInput, r: usize) -> bool {
    a.cells[r] != b.cells[r]
        || a.clusters[r] != b.clusters[r]
        || a.combining[r] != b.combining[r]
        || a.line_sizes[r] != b.line_sizes[r]
}

/// Mark row `r` dirty if it is in range (a no-op for an out-of-range cursor row).
fn mark(dirty: &mut [bool], r: usize) {
    if let Some(slot) = dirty.get_mut(r) {
        *slot = true;
    }
}

/// THE full-frame gate-hit predicate: is `input` (to be drawn at `cur_blink_phase`
/// / `cur_cursor_style_override`) PIXEL-IDENTICAL to a frame previously rendered
/// from `prev_input` (at `prev_blink_phase` / `prev_cursor_style_override`)?
///
/// This is the SINGLE source of truth for "nothing changed, reuse the prior
/// pixels" — both the CPU damage path ([`Renderer::render_input_cached`]) and the
/// GPU dirty-gate ([`aterm_gpu::GpuRenderer`]) call it, so they cannot diverge.
/// It encodes EXACTLY the conditions under which `render_input_cached` returns
/// the cached frame untouched:
///
/// 1. REUSABLE — same geometry (rows/cols, hence pixel dims), same scrollback
///    offset, same selection, and NO double-HEIGHT row in either frame (a DECDHL
///    glyph spans two row bands, so per-row reuse is unsafe).
/// 2. NO DIRTY ROW — every row's render-relevant inputs ([`row_differs`]) match.
/// 3. CURSOR UNCHANGED — the cursor's shown-ness, position, and effective style
///    are identical (using the SAME shown-test the overlay uses), AND the blink
///    phase and style override the frame was drawn with are unchanged.
///
/// When all three hold the cached pixels ARE this frame, byte-for-byte, so a
/// renderer may re-present them with zero rendering work.
#[must_use]
pub fn is_unchanged_frame(
    prev_input: &RenderInput,
    prev_blink_phase: bool,
    prev_cursor_style_override: Option<CursorStyle>,
    input: &RenderInput,
    cur_blink_phase: bool,
    cur_cursor_style_override: Option<CursorStyle>,
) -> bool {
    let (rows, cols) = (input.rows, input.cols);

    // 1. REUSABLE: geometry / scrollback / selection equal, no double-height.
    // Equal rows/cols ⇒ equal pixel dims (dims are a pure fn of rows/cols and the
    // renderer's fixed cell metrics), so comparing rows/cols matches the
    // width/height check in `render_input_cached`'s `reusable`.
    let reusable = prev_input.rows == rows
        && prev_input.cols == cols
        && prev_input.display_offset == input.display_offset
        && prev_input.selection == input.selection
        && !any_double_height(&prev_input.line_sizes)
        && !any_double_height(&input.line_sizes);
    if !reusable {
        return false;
    }

    // 2. NO DIRTY ROW: any render-relevant per-row difference forces a repaint.
    for r in 0..rows {
        if row_differs(input, prev_input, r) {
            return false;
        }
    }

    // 3. CURSOR / BLINK / OVERRIDE UNCHANGED. Use the SAME shown-test the overlay
    // uses (effective style = override ?? DECSCUSR, gated by blink phase).
    let prev_style = prev_cursor_style_override.unwrap_or(prev_input.cursor_style);
    let prev_shown = prev_input.cursor_row < rows
        && prev_input.cursor_col < cols
        && prev_input.cursor_visible
        && cursor_shown(prev_style, prev_blink_phase);
    let cur_style = cur_cursor_style_override.unwrap_or(input.cursor_style);
    let cur_shown = input.cursor_row < rows
        && input.cursor_col < cols
        && input.cursor_visible
        && cursor_shown(cur_style, cur_blink_phase);
    let cursor_changed = prev_shown != cur_shown
        || (cur_shown
            && (prev_input.cursor_row != input.cursor_row
                || prev_input.cursor_col != input.cursor_col
                || prev_style != cur_style));

    !cursor_changed
        && prev_blink_phase == cur_blink_phase
        && prev_cursor_style_override == cur_cursor_style_override
}

/// How a glyph is enlarged for a DEC line-size row: `xs`/`ys` are NEAREST
/// replication factors and `[clip_y0, clip_y1)` is the destination-row window
/// (used by DECDHL to show only the top or bottom half of the doubled glyph).
/// Public so the GPU renderer scales glyph quads through the SAME geometry.
#[derive(Clone, Copy)]
pub struct Scale {
    pub xs: usize,
    pub ys: usize,
    pub clip_y0: i32,
    pub clip_y1: i32,
}

impl Scale {
    /// Ordinary single-size row: no scaling, no clip.
    pub const NORMAL: Scale = Scale { xs: 1, ys: 1, clip_y0: i32::MIN, clip_y1: i32::MAX };
}

/// The on-screen scale + glyph anchor for a row of `line_size` at top pixel
/// `y0` with cell height `ch`: returns `(Scale, anchor_y)`. DECDWL is 2× wide;
/// DECDHL is 2× both with the dest clipped to this row — its bottom half anchors
/// one row up so the lower portion of the doubled glyph lands here.
pub fn row_scale(line_size: LineSize, y0: usize, ch: usize) -> (Scale, i32) {
    let (y0, ch) = (y0 as i32, ch as i32);
    match line_size {
        LineSize::DoubleWidth => (Scale { xs: 2, ys: 1, ..Scale::NORMAL }, y0),
        LineSize::DoubleHeightTop => {
            (Scale { xs: 2, ys: 2, clip_y0: y0, clip_y1: y0 + ch }, y0)
        }
        LineSize::DoubleHeightBottom => {
            (Scale { xs: 2, ys: 2, clip_y0: y0, clip_y1: y0 + ch }, y0 - ch)
        }
        // SingleWidth and any future variant: ordinary single-size.
        _ => (Scale::NORMAL, y0),
    }
}

/// The on-screen cell advance (px) for a row of `line_size` — doubled for any
/// double-width/height row, single otherwise.
pub fn row_cell_w(line_size: LineSize, cell_w: usize) -> usize {
    if matches!(line_size, LineSize::SingleWidth) { cell_w } else { cell_w * 2 }
}

/// Destination rect + atlas UV for the VISIBLE part of an atlas glyph under
/// `scale`, for the GPU. `cell_left` is the cell's left pixel (column × row cell
/// width); `anchor_y` the (DECDHL-shifted) cell top; `baseline` the renderer
/// ascent; `ax/ay/gw/gh/xmin/ymin` the atlas slot; `aw/ah` the atlas size.
/// `None` when the clip leaves nothing. The NEAREST sampling of this quad
/// reproduces the CPU [`blit`]'s integer x/y replicate + clip exactly.
#[allow(clippy::too_many_arguments)]
#[must_use]
pub fn glyph_quad(
    cell_left: f32,
    anchor_y: i32,
    baseline: i32,
    scale: Scale,
    ax: u32,
    ay: u32,
    gw: u32,
    gh: u32,
    xmin: i32,
    ymin: i32,
    aw: f32,
    ah: f32,
) -> Option<([f32; 4], [f32; 4])> {
    let (xs, ys) = (scale.xs.max(1), scale.ys.max(1));
    let gx0 = cell_left + (xmin * xs as i32) as f32;
    let gy0 = (anchor_y + ys as i32 * (baseline - gh as i32 - ymin)) as f32;
    let full_h = (gh as usize * ys) as f32;
    let vy0 = gy0.max(scale.clip_y0 as f32);
    let vy1 = (gy0 + full_h).min(scale.clip_y1 as f32);
    if vy1 <= vy0 {
        return None;
    }
    let vh = vy1 - vy0;
    let v_top = (vy0 - gy0) / ys as f32; // source pixels from the glyph top
    let rect = [gx0, vy0, gw as f32 * xs as f32, vh];
    let uv = [ax as f32 / aw, (ay as f32 + v_top) / ah, gw as f32 / aw, (vh / ys as f32) / ah];
    Some((rect, uv))
}

/// Horizontal cell origin (the `cell_x`/`cell_left` to pass to [`blit`] /
/// [`glyph_quad`]) that CENTRES a combining mark's ink in its cell: column `c`,
/// on-screen cell advance `rcw`, mark ink `gw` px wide (atlas width) with left
/// bearing `xmin`, under `scale` (which doubles widths on DEC double-size rows).
/// Pure integer arithmetic, shared by the CPU and GPU paths so the mark lands on
/// the identical pixel in both — preserving CPU/GPU parity.
#[must_use]
pub fn mark_cell_x(c: usize, rcw: usize, gw: usize, xmin: i32, scale: Scale) -> i32 {
    let xs = scale.xs.max(1) as i32;
    let cell_left = (c * rcw) as i32;
    cell_left + (rcw as i32 - gw as i32 * xs) / 2 - xmin * xs
}

/// The synthetic-style bits for a cell (SGR 1 bold / SGR 3 italic).
fn cell_style(cell: &RenderCell) -> StyleBits {
    let mut bits = 0u8;
    if cell.bold {
        bits |= StyleBits::BOLD.0;
    }
    if cell.italic {
        bits |= StyleBits::ITALIC.0;
    }
    StyleBits(bits)
}

/// Horizontally dilate a coverage bitmap by `e` px (synthetic BOLD): each output
/// column is the max of the source column and its `e` left neighbours, widening
/// every stroke. Returns the new bytes + width (height and the left bearing are
/// unchanged; the glyph just overflows ~`e` px to the right, as bold does).
fn embolden(cov: &[u8], w: usize, h: usize, e: usize) -> (Vec<u8>, usize) {
    if e == 0 || w == 0 || h == 0 {
        return (cov.to_vec(), w);
    }
    let nw = w + e;
    let mut out = vec![0u8; nw * h];
    for y in 0..h {
        let row = &cov[y * w..y * w + w];
        let orow = &mut out[y * nw..y * nw + nw];
        for (x, slot) in orow.iter_mut().enumerate() {
            let mut m = 0u8;
            for k in 0..=e {
                if let Some(&v) = x.checked_sub(k).and_then(|i| row.get(i)) {
                    m = m.max(v);
                }
            }
            *slot = m;
        }
    }
    (out, nw)
}

/// Shear a coverage bitmap forward (synthetic ITALIC): each row is shifted right
/// proportional to its height above the bitmap bottom, so the top leans right.
/// Returns the new bytes + width (height + left bearing unchanged).
fn slant(cov: &[u8], w: usize, h: usize, shear: f32) -> (Vec<u8>, usize) {
    if w == 0 || h == 0 {
        return (cov.to_vec(), w);
    }
    let max_off = (((h - 1) as f32) * shear).round() as usize;
    let nw = w + max_off;
    let mut out = vec![0u8; nw * h];
    for y in 0..h {
        let off = ((((h - 1 - y) as f32) * shear).round() as usize).min(max_off);
        for x in 0..w {
            out[y * nw + (x + off)] = cov[y * w + x];
        }
    }
    (out, nw)
}

/// Apply synthetic BOLD (embolden) then ITALIC (slant) to a freshly rasterized
/// mono coverage bitmap, returning the possibly-widened `(width, bytes)`. The
/// advance and left bearing stay the original so cell layout is unchanged.
fn apply_synthetic_style(style: StyleBits, w: usize, h: usize, bytes: Vec<u8>, px: f32) -> (usize, Vec<u8>) {
    let (mut w, mut bytes) = (w, bytes);
    if style.contains(StyleBits::BOLD) {
        let e = (px / 18.0).round().max(1.0) as usize;
        let (b, nw) = embolden(&bytes, w, h, e);
        bytes = b;
        w = nw;
    }
    if style.contains(StyleBits::ITALIC) {
        let (b, nw) = slant(&bytes, w, h, 0.2);
        bytes = b;
        w = nw;
    }
    (w, bytes)
}

/// A zero-sized colour glyph: the blit treats it as a no-op (like a space).
/// Returned when a colour-emoji bitmap is missing or undecodable.
fn empty_rgba() -> GlyphImage {
    GlyphImage::Rgba { width: 0, height: 0, xmin: 0, ymin: 0, advance: 0.0, bytes: Vec::new() }
}

/// Pack an `[r, g, b]` triple into the framebuffer's `0x00RRGGBB` format.
fn rgb_to_u32([r, g, b]: [u8; 3]) -> u32 {
    (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)
}

/// Blend `fg` over `bg` by coverage `t` (0..=255), per channel.
fn blend(bg: u32, fg: u32, t: u8) -> u32 {
    let t = t as u32;
    let mix = |bg: u32, fg: u32| -> u32 { (bg * (255 - t) + fg * t) / 255 };
    let br = (bg >> 16) & 0xff;
    let bgc = (bg >> 8) & 0xff;
    let bb = bg & 0xff;
    let fr = (fg >> 16) & 0xff;
    let fgc = (fg >> 8) & 0xff;
    let fb = fg & 0xff;
    (mix(br, fr) << 16) | (mix(bgc, fgc) << 8) | mix(bb, fb)
}

/// Convert a decoded 8-bit PNG buffer to packed RGBA8. Handles the colour types
/// Apple Color Emoji's sbix PNGs use (RGBA, RGB); anything else returns `None`.
fn to_rgba8(buf: &[u8], color_type: png::ColorType, w: usize, h: usize) -> Option<Vec<u8>> {
    let n = w.checked_mul(h)?;
    match color_type {
        png::ColorType::Rgba => (buf.len() >= n * 4).then(|| buf[..n * 4].to_vec()),
        png::ColorType::Rgb => {
            if buf.len() < n * 3 {
                return None;
            }
            let mut out = Vec::with_capacity(n * 4);
            for px in buf[..n * 3].chunks_exact(3) {
                out.extend_from_slice(&[px[0], px[1], px[2], 255]);
            }
            Some(out)
        }
        _ => None,
    }
}

/// Bilinearly resample a packed RGBA8 image to `dw`x`dh`. Used once per emoji to
/// fit its sbix bitmap into the cell box; emoji are infrequent, so clarity beats
/// raw speed here.
fn bilinear_rgba(src: &[u8], sw: usize, sh: usize, dw: usize, dh: usize) -> Vec<u8> {
    let mut out = vec![0u8; dw * dh * 4];
    if sw == 0 || sh == 0 {
        return out;
    }
    let fx = sw as f32 / dw as f32;
    let fy = sh as f32 / dh as f32;
    for dy in 0..dh {
        let sy = ((dy as f32 + 0.5) * fy - 0.5).max(0.0);
        let y0 = sy.floor() as usize;
        let y1 = (y0 + 1).min(sh - 1);
        let wy = sy - y0 as f32;
        for dx in 0..dw {
            let sx = ((dx as f32 + 0.5) * fx - 0.5).max(0.0);
            let x0 = sx.floor() as usize;
            let x1 = (x0 + 1).min(sw - 1);
            let wx = sx - x0 as f32;
            let di = (dy * dw + dx) * 4;
            for c in 0..4 {
                let p00 = src[(y0 * sw + x0) * 4 + c] as f32;
                let p10 = src[(y0 * sw + x1) * 4 + c] as f32;
                let p01 = src[(y1 * sw + x0) * 4 + c] as f32;
                let p11 = src[(y1 * sw + x1) * 4 + c] as f32;
                let top = p00 + (p10 - p00) * wx;
                let bot = p01 + (p11 - p01) * wx;
                out[di + c] = (top + (bot - top) * wy).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn renderer() -> Option<Renderer> {
        Renderer::from_system(16.0, Theme::default())
    }

    #[test]
    fn blend_endpoints() {
        assert_eq!(blend(0x000000, 0xffffff, 0), 0x000000);
        assert_eq!(blend(0x000000, 0xffffff, 255), 0xffffff);
        // halfway is grey-ish
        let mid = blend(0x000000, 0xffffff, 128);
        assert!((mid & 0xff) > 0x70 && (mid & 0xff) < 0x90);
    }

    #[test]
    fn renders_grid_to_correct_dimensions_and_draws_text() {
        let Some(mut r) = renderer() else {
            eprintln!("SKIP: no system mono font found");
            return;
        };
        let (cw, ch) = r.cell_size();
        assert!(cw > 0 && ch > 0);

        let mut term = Terminal::new(2, 4);
        term.process(b"Hi");
        let frame = r.render(&term, 2, 4);

        // dimensions exactly grid * cell
        assert_eq!(frame.width, 4 * cw);
        assert_eq!(frame.height, 2 * ch);
        assert_eq!(frame.pixels.len(), frame.width * frame.height);

        // the background colour is present (empty cells)
        assert!(frame.pixels.iter().any(|&p| p == Theme::default().bg));
        // 'H' and 'i' drew foreground-ish pixels somewhere in row 0
        let fg = Theme::default().fg;
        let drew_glyph = frame
            .pixels
            .iter()
            .any(|&p| p != Theme::default().bg && p != Theme::default().cursor && near(p, fg));
        assert!(drew_glyph, "expected rasterized glyph pixels");
        // the cursor block is drawn (cursor colour present)
        assert!(frame.pixels.iter().any(|&p| p == Theme::default().cursor));
    }

    fn near(a: u32, b: u32) -> bool {
        let d = |x: u32, y: u32| (x as i32 - y as i32).abs();
        d((a >> 16) & 0xff, (b >> 16) & 0xff) < 0x60
            && d((a >> 8) & 0xff, (b >> 8) & 0xff) < 0x60
            && d(a & 0xff, b & 0xff) < 0x60
    }

    #[test]
    fn renders_per_cell_foreground_and_background_colors() {
        let Some(mut r) = renderer() else {
            eprintln!("SKIP: no system mono font found");
            return;
        };
        let (cw, ch) = r.cell_size();

        let mut term = Terminal::new(2, 8);
        // 'R' in red foreground, 'G' on a green background.
        term.process(b"\x1b[31mR\x1b[0m\x1b[42mG\x1b[0m");
        let frame = r.render(&term, 2, 8);

        // Cell (0,0) holds the red 'R': its glyph must paint red-dominant pixels.
        let r_cell_red = cell_pixels(&frame, 0, 0, cw, ch)
            .any(|p| channels(p).0 > channels(p).1 && channels(p).0 > channels(p).2);
        assert!(r_cell_red, "expected red-dominant glyph pixels in the 'R' cell");

        // Cell (0,1) has a green background fill: it must be green-dominant.
        let g_cell_green = cell_pixels(&frame, 1, 0, cw, ch)
            .any(|p| channels(p).1 > channels(p).0 && channels(p).1 > channels(p).2);
        assert!(g_cell_green, "expected green-dominant background in the 'G' cell");
    }

    /// (r, g, b) channels of a packed `0x00RRGGBB` pixel.
    fn channels(p: u32) -> (u32, u32, u32) {
        ((p >> 16) & 0xff, (p >> 8) & 0xff, p & 0xff)
    }

    /// Iterate the pixels of the cell at (`col`, `row`) in `cw`x`ch` cells.
    fn cell_pixels(
        frame: &Frame,
        col: usize,
        row: usize,
        cw: usize,
        ch: usize,
    ) -> impl Iterator<Item = u32> + '_ {
        let (x0, y0) = (col * cw, row * ch);
        (y0..y0 + ch).flat_map(move |y| {
            (x0..x0 + cw).map(move |x| frame.pixels[y * frame.width + x])
        })
    }

    /// The bytes of the first readable primary-font candidate ($ATERM_FONT
    /// first), i.e. the file `Renderer::from_system` would load.
    fn system_font_bytes() -> Option<Vec<u8>> {
        let mut paths: Vec<String> = Vec::new();
        if let Ok(p) = std::env::var("ATERM_FONT") {
            paths.push(p);
        }
        paths.extend(FONT_CANDIDATES.iter().map(|s| s.to_string()));
        paths.iter().find_map(|p| std::fs::read(p).ok())
    }

    /// The glyph cache must serve the EXACT bytes/metrics a direct fontdue
    /// rasterization at the renderer's `px` produces — no quantization
    /// round-trip, no metric re-rounding. This is the byte-level contract the
    /// GPU atlas (and the parity suite) stands on.
    #[test]
    fn glyph_image_matches_direct_fontdue_rasterization() {
        let Some(bytes) = system_font_bytes() else {
            eprintln!("SKIP: no system mono font found");
            return;
        };
        let px = 16.0;
        let mut r = Renderer::from_bytes(&bytes, px, Theme::default()).expect("renderer");
        let font = fontdue::Font::from_bytes(bytes.as_slice(), fontdue::FontSettings::default())
            .expect("font");
        for ch in ['M', 'a', '0', '%', ' '] {
            let key = r.glyph_key(ch);
            assert_eq!(key.source, FaceId::Primary);
            assert_eq!(key.glyph_class, GlyphClass::Mono);
            assert_eq!(key.ch_or_id, ch as u32);
            assert_eq!(key.style, StyleBits::REGULAR);
            assert_eq!(key.px_q, GlyphKey::quantize_px(px));
            let img = r.glyph_image(key).clone();
            let (m, direct) = font.rasterize(ch, px);
            assert_eq!(
                (img.width(), img.height(), img.xmin(), img.ymin()),
                (m.width, m.height, m.xmin, m.ymin),
                "metrics differ for {ch:?}"
            );
            assert_eq!(img.advance(), m.advance_width, "advance differs for {ch:?}");
            assert_eq!(img.bytes(), direct.as_slice(), "coverage bytes differ for {ch:?}");
        }
    }

    /// Key resolution is memoized and stable: the same char yields the same
    /// key, and a char the primary face covers resolves to it without ever
    /// loading the fallback.
    #[test]
    fn glyph_key_is_cached_and_primary_chars_skip_fallback() {
        let Some(mut r) = renderer() else {
            eprintln!("SKIP: no system mono font found");
            return;
        };
        let k1 = r.glyph_key('M');
        let k2 = r.glyph_key('M');
        assert_eq!(k1, k2);
        assert_eq!(k1.source, FaceId::Primary);
        assert!(r.fallback.is_none(), "ASCII lookup must not load the fallback face");
        assert_eq!(r.keys.len(), 1);
    }

    /// A char the primary face misses dispatches to the fallback face (when
    /// one exists on this system), and its image carries real coverage.
    #[test]
    fn cjk_glyph_key_dispatches_to_fallback_face() {
        let Some(mut r) = renderer() else {
            eprintln!("SKIP: no system mono font found");
            return;
        };
        let key = r.glyph_key('日');
        if r.fallback.is_none() {
            eprintln!("SKIP: no system fallback font found");
            return;
        }
        assert_eq!(key.source, FaceId::Fallback);
        let img = r.glyph_image(key);
        assert!(img.width() > 0 && img.height() > 0, "CJK glyph rasterized empty");
        assert!(img.bytes().iter().any(|&c| c > 0), "CJK glyph has no coverage");
    }

    /// Box-drawing/block/braille chars intercept BEFORE any face lookup:
    /// they resolve to [`FaceId::Procedural`] (never loading the fallback),
    /// and their images are cell-exact hard-coverage bitmaps whose placement
    /// offsets anchor the blit at the cell's top-left corner.
    #[test]
    fn procedural_chars_dispatch_before_font_lookup() {
        let Some(mut r) = renderer() else {
            eprintln!("SKIP: no system mono font found");
            return;
        };
        let (cw, ch) = r.cell_size();
        let baseline = r.baseline();
        for c in ['─', '│', '┼', '╋', '╬', '╭', '╳', '█', '▚', '░', '\u{28FF}'] {
            let key = r.glyph_key(c);
            assert_eq!(key.source, FaceId::Procedural, "{c:?} must be procedural");
            let img = r.glyph_image(key).clone();
            assert_eq!((img.width(), img.height()), (cw, ch), "{c:?} must fill the cell");
            assert_eq!(img.xmin(), 0, "{c:?} anchors at the cell's left edge");
            // blit row anchor: cell_y + baseline - height - ymin == cell_y.
            assert_eq!(baseline - img.height() as i32 - img.ymin(), 0, "{c:?} anchors at the cell top");
            assert!(
                img.bytes().iter().all(|&b| b == 0 || b == 255),
                "{c:?} must be hard 0/255 coverage"
            );
            assert!(img.bytes().iter().any(|&b| b == 255), "{c:?} must draw something");
        }
        assert!(r.fallback.is_none(), "procedural dispatch must not load the fallback face");
    }

    /// The Rgba placeholder is wired but produces nothing yet: an Rgba key
    /// resolves to an empty image (and does not panic).
    #[test]
    fn rgba_keys_resolve_to_colour_emoji_images() {
        let Some(mut r) = renderer() else {
            eprintln!("SKIP: no system mono font found");
            return;
        };
        // 🙂 (U+1F642) is in Apple Color Emoji's sbix table.
        let key = GlyphKey::rgba_char(FaceId::ColorEmoji, '🙂', GlyphKey::quantize_px(r.px()));
        let img = r.glyph_image(key).clone();
        assert!(matches!(img, GlyphImage::Rgba { .. }));
        if r.color_font.is_none() {
            eprintln!("SKIP: no colour-emoji font on this system");
            return;
        }
        // A real colour glyph: non-empty, sized RGBA with some opaque texels.
        assert!(img.width() > 0 && img.height() > 0, "colour emoji glyph is empty");
        assert_eq!(img.bytes().len(), img.width() * img.height() * 4);
        assert!(
            img.bytes().chunks_exact(4).any(|p| p[3] > 0),
            "colour emoji glyph is fully transparent"
        );
        // Wide: emoji advance is the 2-cell box.
        assert_eq!(img.advance(), (2 * r.cell_w) as f32);
    }

    /// Dispatch: a code point only the colour-emoji face covers gets an Rgba key.
    #[test]
    fn emoji_dispatches_to_colour_face() {
        let Some(mut r) = renderer() else {
            eprintln!("SKIP: no system mono font found");
            return;
        };
        let key = r.glyph_key('🚀');
        if r.color_font.is_none() {
            eprintln!("SKIP: no colour-emoji font on this system");
            return;
        }
        assert_eq!(key.source, FaceId::ColorEmoji);
        assert_eq!(key.glyph_class, GlyphClass::Rgba);
    }

    /// Emoji grapheme clusters (ZWJ family, skin-tone, keycap) shape to a single
    /// colour glyph: `glyph_key_cluster` returns an `RgbaGid` key whose image is
    /// a non-empty, non-transparent colour bitmap. A non-emoji cluster declines.
    #[test]
    fn emoji_clusters_shape_to_colour_glyphs() {
        let Some(mut r) = renderer() else {
            eprintln!("SKIP: no system mono font");
            return;
        };
        if !r.color_font_has('\u{1F44D}') {
            eprintln!("SKIP: no colour-emoji font on this system");
            return;
        }
        for (name, s) in [
            ("family", "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}"),
            ("keycap-1", "\u{31}\u{FE0F}\u{20E3}"),
            ("thumb-skin", "\u{1F44D}\u{1F3FD}"),
            ("flag-US", "\u{1F1FA}\u{1F1F8}"),
        ] {
            let key = r.glyph_key_cluster(s).unwrap_or_else(|| panic!("{name} should shape to a glyph"));
            assert_eq!(key.glyph_class, GlyphClass::RgbaGid, "{name} key is glyph-id-addressed");
            assert_eq!(key.source, FaceId::ColorEmoji, "{name} uses the colour face");
            let img = r.glyph_image(key).clone();
            assert!(
                matches!(img, GlyphImage::Rgba { .. }) && img.width() > 0 && img.height() > 0,
                "{name} colour glyph is empty"
            );
            assert!(
                img.bytes().chunks_exact(4).any(|p| p[3] > 0),
                "{name} colour glyph is fully transparent"
            );
        }
        // A non-emoji "cluster" (Latin base + combining acute) has no colour
        // glyph, so shaping declines and the caller falls back to the base.
        assert!(r.glyph_key_cluster("e\u{0301}").is_none(), "Latin diacritic must not shape to colour");
    }

    /// Synthetic bold/italic: a BOLD key keeps the Mono class but carries the
    /// BOLD style and rasterizes to HEAVIER coverage (more ink) than regular;
    /// ITALIC widens the bitmap (the shear extends it right). Both differ from
    /// the regular glyph's bytes.
    #[test]
    fn bold_italic_synthesize_distinct_glyphs() {
        let Some(mut r) = renderer() else {
            eprintln!("SKIP: no system mono font");
            return;
        };
        let ink = |img: &GlyphImage| -> u32 { img.bytes().iter().map(|&b| u32::from(b)).sum() };

        let reg = r.glyph_key('M');
        assert_eq!(reg.style, StyleBits::REGULAR);
        let reg_img = r.glyph_image(reg).clone();

        let bold = r.glyph_key_styled('M', StyleBits::BOLD);
        assert_eq!(bold.glyph_class, GlyphClass::Mono);
        assert!(bold.style.contains(StyleBits::BOLD));
        assert_ne!(reg, bold, "bold key differs from regular");
        let bold_img = r.glyph_image(bold).clone();
        assert!(ink(&bold_img) > ink(&reg_img), "bold glyph should have more ink than regular");

        let ital = r.glyph_key_styled('M', StyleBits::ITALIC);
        let ital_img = r.glyph_image(ital).clone();
        assert!(ital_img.width() >= reg_img.width(), "italic shear widens the bitmap");
        assert_ne!(reg_img.bytes(), ital_img.bytes(), "italic glyph differs from regular");

        // REGULAR style short-circuits to the plain unstyled key.
        assert_eq!(r.glyph_key_styled('M', StyleBits::REGULAR), reg);
    }

    /// C-1: `render_input_cached` reuses the renderer's owned damage-cache pixel
    /// buffer across frames (no per-frame reallocation on a steady-size grid) and
    /// produces byte-identical pixels to the allocating `render_input` (parity
    /// preserved). This pins BOTH the buffer-reuse win (the borrow hot path) and
    /// the single-code-path invariant (`render_input` == cached + clone).
    #[test]
    fn render_input_cached_reuses_buffer_and_matches_render_input() {
        let Some(mut r) = renderer() else {
            eprintln!("SKIP: no system mono font found");
            return;
        };
        let mut term = Terminal::new(3, 12);
        term.process(b"reuse me\x1b[31m!\x1b[0m");
        let input = Renderer::extract(&term, 3, 12);

        // Frame 1 via the borrowing cached path: capture the buffer's heap pointer.
        let (ptr1, expected) = {
            let f = r.render_input_cached(&input);
            (f.pixels().as_ptr(), f.pixels().to_vec())
        };
        // Frame 2 (same dims): the buffer must be the SAME allocation — reused in
        // place, not freshly allocated — and the pixels identical.
        let ptr2 = {
            let f = r.render_input_cached(&input);
            assert_eq!(f.pixels(), expected.as_slice(), "reused-buffer frame differs frame-to-frame");
            f.pixels().as_ptr()
        };
        assert_eq!(ptr1, ptr2, "steady-size frame must REUSE the pixel allocation");

        // And the borrowing path is byte-identical to the allocating render_input.
        let owned = r.render_input(&input);
        assert_eq!(owned.pixels, expected, "render_input_cached must match render_input pixels");
        assert_eq!((owned.width, owned.height), (input.cols * r.cell_w, input.rows * r.cell_h));
    }

    /// C-1: `extract_into` refilling a reused `RenderInput` yields the SAME
    /// snapshot (and therefore the same pixels) as a fresh `extract`.
    #[test]
    fn extract_into_matches_fresh_extract() {
        let Some(mut r) = renderer() else {
            eprintln!("SKIP: no system mono font found");
            return;
        };
        let mut term = Terminal::new(2, 8);
        term.process(b"AB\x1b[42mC\x1b[0m");

        let fresh = Renderer::extract(&term, 2, 8);
        let mut reused = RenderInput::empty();
        // Prime the reused buffer at a DIFFERENT size first, to prove refill
        // (truncate/extend) lands on the right shape regardless of prior state.
        Renderer::extract_into(&mut reused, &term, 1, 4);
        Renderer::extract_into(&mut reused, &term, 2, 8);

        assert_eq!(r.render_input(&fresh).pixels, r.render_input(&reused).pixels,
            "extract_into must produce pixels identical to a fresh extract");
    }

    /// VS16 presentation: `❤️` (U+2764 + VS16) must resolve to the COLOUR face
    /// even though the mono primary/fallback faces carry a black-heart glyph.
    /// `glyph_key` (text) picks the mono glyph; `glyph_key_emoji` (the path the
    /// blit takes for an emoji-presentation cell) picks the colour glyph. The
    /// two presentations of the same char must produce DIFFERENT keys.
    #[test]
    fn vs16_emoji_presentation_prefers_colour_face() {
        let Some(mut r) = renderer() else {
            eprintln!("SKIP: no system mono font found");
            return;
        };
        let heart = '\u{2764}'; // ❤ HEAVY BLACK HEART (text default)
        // color_font_has triggers the lazy colour-font load and gates the test
        // (the text dispatch below would NOT load it — the mono primary covers ❤).
        if !r.color_font_has(heart) {
            eprintln!("SKIP: no colour-emoji glyph for ❤ on this system");
            return;
        }
        let text_key = r.glyph_key(heart);
        let emoji_key = r.glyph_key_emoji(heart);
        assert_eq!(emoji_key.source, FaceId::ColorEmoji, "VS16 heart must use the colour face");
        assert_eq!(emoji_key.glyph_class, GlyphClass::Rgba, "VS16 heart must be an Rgba glyph");
        assert_ne!(text_key, emoji_key, "text and emoji presentations must differ");
        // The text presentation is a mono glyph (the black heart from a text font).
        assert_eq!(text_key.glyph_class, GlyphClass::Mono, "bare ❤ should stay mono");
        // And the colour glyph actually rasterizes to a non-empty colour bitmap.
        let img = r.glyph_image(emoji_key).clone();
        assert!(
            matches!(img, GlyphImage::Rgba { .. }) && img.width() > 0 && img.height() > 0,
            "VS16 heart colour glyph is empty"
        );
    }
}

// The CPU renderer as the injected `Rasterizer` (ATERM_DESIGN WS-F). Forwards to
// the inherent methods via UFCS so the trait and inherent `render`/`render_input`
// names cannot collide.
impl Rasterizer for Renderer {
    fn cell_size(&self) -> (usize, usize) {
        Renderer::cell_size(self)
    }
    fn render(&mut self, term: &Terminal, rows: usize, cols: usize) -> Frame {
        Renderer::render(self, term, rows, cols)
    }
    fn render_input(&mut self, input: &RenderInput) -> Frame {
        Renderer::render_input(self, input)
    }
    fn render_input_cached(&mut self, input: &RenderInput) -> RenderView<'_> {
        // Override the default (which clones into a Frame): the CPU renderer can
        // hand back a borrow of its damage cache, eliding the per-frame clone.
        Renderer::render_input_cached(self, input)
    }
    fn set_cursor_blink_phase(&mut self, on: bool) {
        Renderer::set_cursor_blink_phase(self, on)
    }
    fn set_cursor_style_override(&mut self, style: Option<CursorStyle>) {
        Renderer::set_cursor_style_override(self, style)
    }
}
