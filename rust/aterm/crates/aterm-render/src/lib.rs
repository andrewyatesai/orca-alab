// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Andrew Yates

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
// A-3: the CPU renderer no longer borrows `&Terminal` — it consumes only the
// engine-built `RenderInput`. `Terminal` is imported solely in the test module
// (which builds terminals + calls `Terminal::cell_frame` to feed the renderer).
use aterm_core::terminal::{CursorStyle, RenderCell, UnderlineStyle};

mod colr;
pub mod ligature_shaping;
pub mod procedural;

pub use aterm_types::text_shaping::{LigatureMode, TextShapingConfig};
pub use ligature_shaping::ColumnGlyph;

/// An interned parsed fallback face: its source bytes paired with the parsed
/// `fontdue::Font`, so identical injected fonts share one ~370MB parse.
type InternedFace = (std::sync::Arc<Vec<u8>>, std::sync::Arc<fontdue::Font>);

thread_local! {
    /// Dedup large RAW font bytes (Apple Color Emoji ~180MB, Noto CJK ~100MB) across
    /// every `Renderer`/embedder in this address space. In the wasm renderer all
    /// terminal panes share ONE linear memory and inject the SAME OS fonts, so
    /// without interning each pane held its own copy. Share one `Arc` keyed by
    /// content; bounded by the handful of distinct fonts ever injected.
    static FONT_BYTES_INTERN: std::cell::RefCell<Vec<std::sync::Arc<Vec<u8>>>> =
        const { std::cell::RefCell::new(Vec::new()) };
    /// Dedup PARSED fallback faces (a broad Unicode fallback is ~370MB once fontdue
    /// parses it) keyed by their source bytes, so N panes injecting the same fallback
    /// share ONE parsed face instead of paying ~370MB each.
    static PARSED_FONT_INTERN: std::cell::RefCell<Vec<InternedFace>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Return a shared `Arc` for `bytes`, reusing an already-interned identical blob so
/// N panes injecting the same font cost one copy, not N. The byte-equality check
/// only runs against same-length entries and only at injection time. `pub` so the
/// GPU-web embedder can intern its reinit-retention byte copies too.
pub fn intern_font_bytes(bytes: Vec<u8>) -> std::sync::Arc<Vec<u8>> {
    FONT_BYTES_INTERN.with(|cell| {
        let mut store = cell.borrow_mut();
        if let Some(existing) = store
            .iter()
            .find(|a| a.len() == bytes.len() && a.as_slice() == bytes.as_slice())
        {
            return existing.clone();
        }
        let arc = std::sync::Arc::new(bytes);
        store.push(arc.clone());
        arc
    })
}

/// Parse a fallback `fontdue::Font` from `bytes`, sharing ONE parsed instance across
/// all Renderers for identical bytes (a broad Unicode fallback is ~370MB parsed, so
/// without this every pane paid it). Content-keyed by byte equality.
fn intern_parsed_font(bytes: &[u8]) -> Result<std::sync::Arc<fontdue::Font>, String> {
    PARSED_FONT_INTERN.with(|cell| {
        {
            let store = cell.borrow();
            if let Some((_, font)) = store
                .iter()
                .find(|(src, _)| src.len() == bytes.len() && src.as_slice() == bytes)
            {
                return Ok(font.clone());
            }
        }
        let parsed = fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
            .map_err(|e| e.to_string())?;
        let font = std::sync::Arc::new(parsed);
        cell.borrow_mut()
            .push((std::sync::Arc::new(bytes.to_vec()), font.clone()));
        Ok(font)
    })
}

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
        Theme {
            fg: 0x00D0_D0D0,
            bg: 0x0011_1318,
            cursor: 0x0050_FA7B,
            // Darkened from #264F78 so selected COLOURED text stays readable
            // (the highlight recedes behind the fg instead of clashing). Must stay
            // in sync with aterm_types::scheme SELECTION_DEFAULT (asserted there).
            selection: 0x0033_415E,
        }
    }
}

// `Frame` / `RenderInput` (and `RenderInput`'s `cluster_at`/`combining_at`
// helpers) moved to `aterm-render-api` — the injected-Rasterizer seam
// (ATERM_DESIGN WS-F). Re-exported here so existing
// `aterm_render::{Frame, RenderInput}` call sites are unchanged; `Rasterizer` is
// the trait this CPU renderer implements (see `impl Rasterizer for Renderer`).
pub use aterm_render_api::{Frame, Rasterizer, RenderInput, RenderView};

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
    /// A monochrome SYMBOL fallback (e.g. STIX Two Math), consulted after
    /// [`FaceId::Fallback`] and BEFORE [`FaceId::ColorEmoji`]. It carries the
    /// many `Emoji=Yes, Emoji_Presentation=No` symbols (media controls ⏸⏹⏺,
    /// misc-technical glyphs) that the primary/fallback mono faces miss but that
    /// must still render as TEXT, not colour — so they reach a real monochrome
    /// glyph here instead of `.notdef`, without ever touching the colour face.
    SymbolFallback,
    /// No font at all: box-drawing / block / braille coverage synthesized
    /// from the cell geometry by [`procedural`] — cell-exact, hard 0/255, so
    /// strokes meet seamlessly across cells and CPU==GPU is bit-identical.
    /// `ATERM_NO_PROCEDURAL_GLYPHS=1` disables this source (font dispatch).
    Procedural,
    /// Apple Color Emoji (`sbix` colour bitmaps): 32-bit RGBA glyphs the mono
    /// faces can't draw (🚀 😀). Consulted only when every mono face misses a code
    /// point AND that code point actually WANTS emoji presentation (Unicode
    /// `Emoji_Presentation=Yes`, or an explicit VS16) — never on raw coverage, so
    /// a default-text symbol the colour font happens to cover does NOT land here.
    ColorEmoji,
    /// A MONOCHROMATIZED colour glyph: the `sbix` bitmap's alpha silhouette, drawn
    /// as foreground-tinted coverage (a [`GlyphClass::Mono`] image), NOT the colour
    /// bitmap. The absolute last resort for a default-TEXT code point that no mono
    /// face on the system covers but the colour font does (⏺ on a machine without
    /// STIX/Apple Symbols): it guarantees a visible, theme-coloured glyph instead
    /// of `.notdef` tofu, while still never rendering in colour.
    ColorEmojiMono,
    /// A RUNTIME-DISCOVERED fallback face (M3 FONT-DISCOVERY): a system font found
    /// at query time to cover a code point that NONE of the configured faces
    /// (primary / broad / symbol / colour) carry. On macOS the cover is resolved
    /// through CoreText (`CTFontCreateForString` over the system cascade list); on
    /// other platforms by scanning the candidate font directories. The exact face
    /// is keyed per code point in [`Renderer::runtime_fallback`] — a `Mono`
    /// coverage glyph like the other mono faces — so a script the bundled fallback
    /// fonts miss (e.g. an uninstalled-by-default CJK or Indic block, or an emoji on
    /// a machine whose Apple Color Emoji path moved) reaches a real glyph instead
    /// of `.notdef` tofu. Consulted ONLY after `select_face` would otherwise give
    /// up, so it never shadows the verified configured-face policy.
    RuntimeFallback,
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
    /// 8-bit alpha coverage addressed by PRIMARY-FACE GLYPH ID (`ch_or_id` is an
    /// opaque primary-font glyph id, not a code point): a programming LIGATURE
    /// (`=>`, `!=`, …) that rustybuzz shaped from a run of cells. Rasterized via
    /// fontdue `rasterize_indexed` with the SAME stem-darken + synthetic style as
    /// [`GlyphClass::Mono`], so CPU and GPU stay byte-identical.
    MonoGid,
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
        GlyphKey {
            source,
            glyph_class: GlyphClass::Mono,
            ch_or_id: ch as u32,
            style,
            px_q,
        }
    }

    /// Key for a colour (Rgba) glyph of character `ch` from `source` (the
    /// colour-emoji face). The image carries its own colours, so the cell
    /// foreground is irrelevant — `style` is fixed REGULAR.
    pub fn rgba_char(source: FaceId, ch: char, px_q: u32) -> GlyphKey {
        GlyphKey {
            source,
            glyph_class: GlyphClass::Rgba,
            ch_or_id: ch as u32,
            style: StyleBits::REGULAR,
            px_q,
        }
    }

    /// Key for a colour (RgbaGid) glyph addressed by colour-font glyph id — a
    /// grapheme cluster (ZWJ / skin-tone / keycap) already shaped to one glyph.
    pub fn rgba_gid(source: FaceId, gid: u16, px_q: u32) -> GlyphKey {
        GlyphKey {
            source,
            glyph_class: GlyphClass::RgbaGid,
            ch_or_id: gid as u32,
            style: StyleBits::REGULAR,
            px_q,
        }
    }

    /// Key for a coverage (MonoGid) glyph addressed by PRIMARY-FACE glyph id — a
    /// programming ligature (`=>`, `!=`, …) rustybuzz shaped from a cell run. The
    /// run's SGR `style` is part of the key so a bold/italic run ligates distinctly.
    pub fn mono_gid(gid: u16, style: StyleBits, px_q: u32) -> GlyphKey {
        GlyphKey {
            source: FaceId::Primary,
            glyph_class: GlyphClass::MonoGid,
            ch_or_id: gid as u32,
            style,
            px_q,
        }
    }

    /// The character this key rasterizes, when `ch_or_id` is a code point.
    /// `None` for [`GlyphClass::RgbaGid`] / [`GlyphClass::MonoGid`], whose
    /// `ch_or_id` is a glyph id.
    pub fn chr(&self) -> Option<char> {
        if matches!(self.glyph_class, GlyphClass::RgbaGid | GlyphClass::MonoGid) {
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
    Mono {
        width: usize,
        height: usize,
        xmin: i32,
        ymin: i32,
        advance: f32,
        bytes: Vec<u8>,
    },
    /// 32-bit RGBA colour, row-major, `width * height * 4` bytes (colour
    /// emoji). PLACEHOLDER this slice: nothing produces a non-empty one.
    Rgba {
        width: usize,
        height: usize,
        xmin: i32,
        ymin: i32,
        advance: f32,
        bytes: Vec<u8>,
    },
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

/// Cache key for a shaped run: the run's text plus its style bits.
type ShapedRunKey = (Box<str>, StyleBits);
/// A shaped run's result: the per-character primary-glyph ids, or `None` when the
/// run did not ligate (shaping changed nothing, so the plain per-cell path is used).
type ShapedRunGlyphs = Option<Box<[u16]>>;

/// Monospace CPU rasterizer.
///
/// `font` is the primary monospace face used for Latin/box-drawing. `fallback`
/// is a broad-coverage face (CJK, symbols, math) consulted only when the primary
/// has no glyph for a code point — so 日本語, math symbols, and the like render
/// instead of going blank. Glyph dispatch is per-char and cached.
pub struct Renderer {
    font: fontdue::Font,
    /// Raw PRIMARY-face bytes, retained so a `rustybuzz::Face` can be built for
    /// run shaping (ligatures). `None` when no bytes were available (e.g. a font
    /// loaded only by path that failed to re-read) — ligatures then cleanly
    /// decline and the per-cell path is used. A `rustybuzz::Face` borrows these,
    /// so a fresh face is parsed per shaping miss (rare; shaped runs are cached).
    rb_primary_bytes: Option<Vec<u8>>,
    /// Whether the primary face advertises a `liga`/`calt` `GSUB` feature, computed
    /// ONCE at construction. A font with neither can emit no substitution under
    /// those features, so its shaped run would always equal the per-cell cmap
    /// glyphs — when this is `false` the planner skips run coalescing + rustybuzz
    /// entirely (byte-identical output, no per-frame shaping). Lives on the shared
    /// `Renderer` so the CPU and GPU planners (both call [`Renderer::row_glyph_plan`])
    /// make the SAME decision, preserving CPU==GPU parity.
    has_ligature_features: bool,
    /// Text-shaping config (ligature mode + font features). DEFAULT is
    /// `LigatureMode::Enabled`, but a run is only ligated when a `rustybuzz::Face`
    /// builds AND the run actually shapes to different glyphs; otherwise the
    /// per-cell path is byte-identical to before. Threaded into both renderers so
    /// CPU and GPU shape identically.
    shaping: aterm_types::text_shaping::TextShapingConfig,
    /// The rustybuzz feature array applied to every ligature shaping run,
    /// RESOLVED ONCE from [`Self::shaping`] (the base `liga`+`calt` pair plus the
    /// user's OpenType `font_features`). Rebuilt only when the shaping config
    /// changes ([`Self::set_text_shaping`]) — NEVER per row/run/cell — so the
    /// per-run hot loop borrows this slice without allocating or scanning the
    /// `font_features` Vec. When the user supplied no features this is exactly the
    /// two-element `[liga, calt]` base list, so the common path is unchanged.
    /// See [`ligature_shaping::build_feature_list`].
    resolved_features: Vec<rustybuzz::Feature>,
    /// Shaped-run cache: a `(run string, style)` -> the per-character shaped
    /// primary-glyph ids (or `None` when the run did not ligate, i.e. shaping
    /// changed nothing, so the caller uses the plain per-cell path). Keyed by the
    /// run so each distinct run shapes at most once. See [`ligature_shaping`].
    shaped_runs: HashMap<ShapedRunKey, ShapedRunGlyphs>,
    /// Broad-coverage fallback CHAIN, most-preferred first, loaded LAZILY: a full
    /// Unicode font (e.g. Arial Unicode, 50k glyphs) costs ~370 MB once fontdue
    /// parses it, so the chain is NOT populated until a code point actually misses
    /// the primary face. Sessions that only show Latin/box-drawing never pay it
    /// (idle RSS ~70 MB, not ~450). A single bundled fallback rarely covers every
    /// non-Latin script at once (a CJK face lacks Arabic/Devanagari/Thai/Hebrew),
    /// so [`FaceId::Fallback`] tries each chain entry IN ORDER and the first that
    /// has the glyph wins — the per-char winner is memoized in [`Self::fallback_pick`].
    fallback_chain: Vec<std::sync::Arc<fontdue::Font>>,
    /// Per-char index into [`Self::fallback_chain`] for the entry that covered the
    /// char, recorded by [`Self::fallback_has`] so the rasterizer (which sees only
    /// a [`GlyphKey`]'s code point) recovers WHICH chain face to draw from — exactly
    /// how [`RuntimeFallback`] keys its per-code-point decision. Bounded by the live
    /// char set; cleared on font/size rebuilds with the other resolve caches.
    fallback_pick: HashMap<char, usize>,
    /// Candidate fallback font paths, tried on first miss; emptied once consumed.
    fallback_paths: Vec<String>,
    /// Monochrome SYMBOL fallback face ([`FaceId::SymbolFallback`]), loaded
    /// LAZILY the first time a code point misses BOTH the primary and broad
    /// fallback faces. Covers the default-text symbols (⏸⏹⏺ and friends) that
    /// would otherwise have no monochrome glyph anywhere on the system.
    symbol_fallback: Option<std::sync::Arc<fontdue::Font>>,
    /// Candidate symbol-fallback font paths, tried on first symbol miss; emptied once consumed.
    symbol_fallback_paths: Vec<String>,
    /// Apple Color Emoji font bytes, loaded LAZILY on the first emoji miss (a
    /// large `sbix` font; sessions without emoji never pay it). Stored as raw
    /// bytes because a `ttf_parser::Face` borrows them — a fresh Face is parsed
    /// per emoji rasterization, which is rare and off the hot path.
    color_font: Option<std::sync::Arc<Vec<u8>>>,
    /// Candidate colour-emoji font paths, tried on first emoji; emptied once consumed.
    color_font_paths: Vec<String>,
    /// Runtime per-codepoint font fallback (M3 FONT-DISCOVERY): when a code point
    /// misses EVERY configured face (primary / broad / symbol / colour),
    /// [`Renderer::glyph_key`] asks this resolver for a system font that covers it
    /// (CoreText on macOS, candidate-directory scan elsewhere). The decision is
    /// cached per code point so repeated lookups are O(1) and bounded. Empty and
    /// untouched on the common path — a code point a configured face covers never
    /// reaches it, so there is no behaviour or perf change for ordinary text.
    runtime_fallback: RuntimeFallback,
    px: f32,
    /// `px` in the key's 26.6 fixed-point form, computed once at construction.
    px_q: u32,
    cell_w: usize,
    cell_h: usize,
    /// Interior padding in px on EVERY edge: the grid is inset by `pad` from the
    /// framebuffer's top-left and the framebuffer is `pad` larger on each side, so
    /// text never sits flush against the window edge. The border is filled with
    /// the theme background. `0` = no padding (the historical behavior; every
    /// render test that asserts `cols·cell_w × rows·cell_h` dims keeps that). The
    /// SAME renderer (hence the SAME `pad`) backs the on-screen present and the
    /// `image`/snapshot introspection, so both are pixel-identical by construction.
    pad: usize,
    baseline: i32,
    theme: Theme,
    /// Per-renderer stem-darkening LUT, derived from `theme` at construction (see
    /// [`stem_gamma_for_theme`]). The coverage warp that makes the renderer's
    /// sRGB-space coverage lerp approximate true linear-light antialiasing for the
    /// theme's fg-over-bg. Rebuilt whenever the renderer is (every theme/font/zoom
    /// change builds a fresh `Renderer`), so it never goes stale. Applied to the
    /// SHARED coverage bytes, so CPU and GPU stay byte-identical.
    stem_lut: [u8; 256],
    /// Optional explicit foreground for SELECTED text (theme `selectionForeground`).
    /// `None` keeps the default behaviour — floor the cell's SGR fg against the
    /// selection bg for WCAG-legible contrast; `Some(rgb)` paints all selected
    /// glyphs in this colour. GPU mirrors this identically (parity).
    selection_fg: Option<u32>,
    /// Whether the pane is UNFOCUSED: when `true`, selected cells take the dimmer
    /// [`Self::selection_inactive_bg`] instead of `theme.selection` — xterm's
    /// `selectionInactiveBackground` semantics. Default `false` (focused = active
    /// selection bg). The active-selection band is unchanged when focused.
    selection_inactive: bool,
    /// Background painted under selected cells while UNFOCUSED (0x00RRGGBB).
    /// `None` derives a sensible default from `theme.selection` blended toward
    /// `theme.bg` (see [`derive_inactive_selection_bg`]); `Some(rgb)` is the host's
    /// explicit `selectionInactiveBackground`. Only consulted when
    /// [`Self::selection_inactive`] is `true`. GPU mirrors this identically.
    selection_inactive_bg: Option<u32>,
    /// The glyph cache, keyed by full rasterization identity.
    glyphs: HashMap<GlyphKey, GlyphImage>,
    /// Per-char key resolve cache (primary-vs-fallback dispatch happens once
    /// per char, not once per blit — the hot path stays two cheap lookups).
    keys: HashMap<char, GlyphKey>,
    /// Per-char PRIMARY-face glyph-id cache, resolved through the font's UNICODE
    /// cmap via ttf-parser (see [`primary_unicode_gid`](Renderer::primary_unicode_gid)).
    /// fontdue's own char lookup prefers a legacy (1,0) Mac Roman cmap subtable on
    /// some Apple fonts (Menlo/Monaco `.ttc`), which mis-maps the whole Latin-1
    /// range (`·`→`∑`, `é`→`È`, …); resolving the id ourselves and rasterizing it
    /// by glyph id sidesteps that. `None` = the primary face has no Unicode glyph.
    primary_gid_cache: HashMap<char, Option<u16>>,
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
}

/// Per-window CPU damage-cache state — the analog of the GPU's `WindowGpu` (S5).
///
/// The shared [`Renderer`] holds only glyph/metrics/cursor state that every
/// window re-applies identically; the DAMAGE-CACHE state, by contrast, is keyed
/// only on `(w, h)` + the previous [`RenderInput`] with NO window identity, so
/// two windows sharing one `Renderer` would diff against each other's cached
/// input → a wrong dirty set or a false dirty-gate hit handing one window the
/// other's pixels. Holding it per-window (one `WindowCpu` each) keeps each
/// window's damage tracking isolated. Threaded into
/// [`Renderer::render_input_cached`] et al. as `&mut WindowCpu`.
#[derive(Default)]
pub struct WindowCpu {
    /// Damage-tracking cache for [`render_input_cached`](Renderer::render_input_cached):
    /// the previous frame's pixels plus enough of the previous input to decide,
    /// row by row, what changed. `None` until the first frame (or after any
    /// state that invalidates reuse — see [`RenderCache`]). The output is
    /// byte-identical to a full repaint; only the WORK differs (unchanged rows
    /// keep their cached pixels).
    pub(crate) cache: Option<RenderCache>,
    /// Persistent dirty-row scratch for [`render_input_cached`](Renderer::render_input_cached):
    /// the `&mut Vec<bool>` handed to [`compute_dirty_rows`] each frame. Resident
    /// across frames (resized + reset in place) so a stable-dimension changed
    /// frame allocates no per-call dirty Vec. The flags it holds — and thus the
    /// damage decision — are byte-identical to the old per-call `vec![false; rows]`.
    pub(crate) dirty_scratch: Vec<bool>,
    /// Decoded inline-image cache (iTerm2 OSC 1337 `File=`), keyed by the image
    /// payload's `Arc` pointer identity + the footprint pixel size it was scaled
    /// for. Each entry is the RGBA8 image already resampled to exactly
    /// `cols*cell_w × rows*cell_h` px, so a covered cell paints a 1:1 tile. Empty
    /// in the common (image-free) case; bounded to a small LRU so a stream of
    /// distinct images cannot grow it without bound. PER-WINDOW so window B's
    /// images never leak into window A.
    pub(crate) image_cache: ImageCache,
}

impl WindowCpu {
    pub fn new() -> Self {
        Self::default()
    }

    /// Drop the damage cache so the NEXT `render_input_cached` is a full repaint.
    /// Needed after an appearance change that is NOT cell content — theme, palette,
    /// or font — since the dirty-row diff tracks content only and would otherwise
    /// leave the selection band, idle cursor, padding, or recoloured cells stale.
    /// (Mirrors `WindowGpu::invalidate_present` for the CPU presentation path.)
    pub fn invalidate(&mut self) {
        self.cache = None;
    }
}

/// Decoded-image LRU for the CPU renderer's inline-image pass. Keyed by the
/// payload `Arc` pointer + the footprint pixel size; the value is the RGBA8
/// image resampled to that size (so per-cell blits are 1:1 nearest copies).
#[derive(Default)]
struct ImageCache {
    /// `(arc_ptr, fp_w, fp_h) -> (rgba bytes, fp_w, fp_h)`, MRU at the back.
    entries: Vec<((usize, usize, usize), DecodedImage)>,
}

/// One decoded + footprint-scaled inline image: straight RGBA8, row-major.
struct DecodedImage {
    /// Footprint pixel width (`cols * cell_w`).
    w: usize,
    /// Footprint pixel height (`rows * cell_h`).
    h: usize,
    /// `w * h * 4` straight-alpha RGBA bytes, or empty if the decode failed
    /// (a cached negative result: the image draws nothing but is not re-decoded).
    rgba: Vec<u8>,
}

impl ImageCache {
    /// Maximum distinct decoded images retained. A modest cap: inline images are
    /// large, and a terminal rarely shows many simultaneously.
    const MAX: usize = 8;

    /// Look up a decoded image by key, promoting it to MRU on a hit.
    fn get(&mut self, key: (usize, usize, usize)) -> Option<&DecodedImage> {
        let idx = self.entries.iter().position(|(k, _)| *k == key)?;
        let entry = self.entries.remove(idx);
        self.entries.push(entry);
        self.entries.last().map(|(_, v)| v)
    }

    /// Insert a freshly decoded image, evicting the LRU entry past the cap.
    fn put(&mut self, key: (usize, usize, usize), value: DecodedImage) {
        if self.entries.len() >= Self::MAX {
            self.entries.remove(0);
        }
        self.entries.push((key, value));
    }
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
pub(crate) struct RenderCache {
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

/// Font paths to try, most-preferred first; override with $ATERM_FONT (or the
/// `font_family` config, which wins ahead of all of these). Menlo leads: it is the
/// classic macOS Terminal/Xcode coding face and rasterizes with a fuller, better-
/// fitted stem under our (CoreText-free) fontdue path — in a frontier-LLM visual
/// judging pass (codex+claude) Menlo scored highest (9/10) for crispness and
/// attractiveness, beating the system SF Mono file, which ships as the thin
/// "SF NS Mono Light" instance (SFNSMono.ttf) and reads faint light-on-dark. SF
/// Mono stays as the next candidate (users who prefer it set `font_family`/
/// $ATERM_FONT), then Monaco. The historical Andale Mono / Courier New entries
/// stay LAST so a machine missing the nicer faces still finds a mono font — the
/// no-font fallback (a `None` from `from_system*`) is unchanged.
const FONT_CANDIDATES: &[&str] = &[
    // macOS
    "/System/Library/Fonts/Menlo.ttc",
    "/System/Library/Fonts/SFNSMono.ttf",
    "/System/Library/Fonts/Monaco.ttf",
    "/System/Library/Fonts/Supplemental/Andale Mono.ttf",
    "/System/Library/Fonts/Supplemental/Courier New.ttf",
    // Linux (Debian/Ubuntu): a real system monospace before the embedded DejaVu.
    "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
    "/usr/share/fonts/truetype/liberation/LiberationMono-Regular.ttf",
    "/usr/share/fonts/opentype/noto/NotoSansMono-Regular.ttf",
];

/// The bundled last-resort monospace face (FONT-EMBED): DejaVu Sans Mono, under the
/// Bitstream Vera + DejaVu licenses (`assets/DejaVuSansMono.LICENSE.txt`). Used only
/// when every system/configured candidate is absent, so a host with no usable font
/// still renders text. Compiled in only with the default `embedded-font` feature.
#[cfg(feature = "embedded-font")]
pub(crate) fn embedded_font() -> &'static [u8] {
    include_bytes!("../assets/DejaVuSansMono.ttf")
}

/// Broad-coverage fallback faces (CJK + symbols), most-preferred first. Both macOS
/// and Linux paths are listed; the first one that EXISTS is loaded, so a host only
/// ever pays for its own platform. Override with $ATERM_FALLBACK_FONT. The Linux
/// entries lead with `DroidSansFallbackFull` (TrueType `glyf` — guaranteed
/// fontdue-rasterizable, broad CJK) before `NotoSansCJK` (CFF) so the broad face is
/// always one that actually renders; `fallback_has` is a cmap-only probe, so a face
/// whose glyphs fontdue cannot draw would otherwise show blank. Any code point these
/// miss still reaches a real glyph via the recursive runtime fallback scan.
const FALLBACK_CANDIDATES: &[&str] = &[
    // macOS
    "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
    "/System/Library/Fonts/Apple Symbols.ttf",
    // Linux (Debian/Ubuntu default install)
    "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
];

/// Monochrome SYMBOL fallback faces, most-preferred first. Override with
/// `$ATERM_SYMBOL_FONT`. STIX Two Math is the broadest monochrome symbol face
/// shipped with macOS — crucially it is (with the colour-only Apple Color Emoji
/// and the tofu LastResort) one of the only system faces carrying U+23F8..23FA
/// (⏸⏹⏺). It is consulted only AFTER the primary + broad fallback miss, so it
/// never shadows their coverage; it exists purely to give default-text symbols a
/// real monochrome glyph instead of `.notdef`, keeping them off the colour face.
const SYMBOL_FALLBACK_CANDIDATES: &[&str] = &[
    // macOS
    "/System/Library/Fonts/Supplemental/STIXTwoMath.otf",
    "/System/Library/Fonts/STIXTwoMath.otf",
    "/System/Library/Fonts/Apple Symbols.ttf",
    // Linux: Noto Sans Symbols 2 carries the monochrome media/technical glyphs
    // (⏸⏹⏺ U+23F8..23FA and friends); DejaVu Sans is the broad-symbol backstop.
    "/usr/share/fonts/truetype/noto/NotoSansSymbols2-Regular.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
];

/// Colour-emoji faces (bitmap strikes), most-preferred first. Override with
/// `$ATERM_EMOJI_FONT`. Apple Color Emoji is `sbix`; Noto Color Emoji is `CBDT/CBLC`
/// — both are PNG bitmap strikes read uniformly through ttf-parser's
/// `glyph_raster_image` ([`Renderer::color_font_has`]), so the Linux entry renders
/// colour emoji exactly as the macOS one does. The first path that EXISTS wins.
const COLOR_EMOJI_CANDIDATES: &[&str] = &[
    "/System/Library/Fonts/Apple Color Emoji.ttc",
    "/usr/share/fonts/truetype/noto/NotoColorEmoji.ttf",
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

/// The ordered symbol-fallback candidate paths ($ATERM_SYMBOL_FONT first),
/// loaded lazily the first time a code point misses the primary + broad fallback.
fn symbol_fallback_candidate_paths() -> Vec<String> {
    let mut paths: Vec<String> = Vec::new();
    if let Ok(p) = std::env::var("ATERM_SYMBOL_FONT") {
        paths.push(p);
    }
    paths.extend(SYMBOL_FALLBACK_CANDIDATES.iter().map(|s| (*s).to_string()));
    paths
}

/// The ordered colour-emoji candidate paths ($ATERM_EMOJI_FONT first), loaded
/// lazily the first time a code point misses both mono faces.
fn color_emoji_candidate_paths() -> Vec<String> {
    let mut paths: Vec<String> = Vec::new();
    if let Ok(p) = std::env::var("ATERM_EMOJI_FONT") {
        paths.push(p);
    }
    // Prefer a user-installed colour emoji font (no root needed). A COLR(v1) build
    // renders ALL emoji — including ZWJ family/couple sequences — in colour, where a
    // stock CBDT Noto renders those (Unicode-deprecated) sequences as monochrome
    // silhouettes. Scan the per-user font dirs for any *emoji* face so dropping one
    // into ~/.local/share/fonts is enough; sorted for a deterministic choice.
    if let Some(home) = std::env::var_os("HOME") {
        let home = std::path::PathBuf::from(home);
        for dir in [".local/share/fonts", ".fonts"] {
            let Ok(rd) = std::fs::read_dir(home.join(dir)) else {
                continue;
            };
            let mut hits: Vec<String> = rd
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.extension()
                        .and_then(|e| e.to_str())
                        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "ttf" | "otf" | "ttc"))
                        && p.file_name()
                            .and_then(|n| n.to_str())
                            .is_some_and(|n| n.to_ascii_lowercase().contains("emoji"))
                })
                .filter_map(|p| p.to_str().map(String::from))
                .collect();
            hits.sort();
            paths.extend(hits);
        }
    }
    paths.extend(COLOR_EMOJI_CANDIDATES.iter().map(|s| (*s).to_string()));
    paths
}

/// Runtime per-codepoint font-fallback resolver (M3 FONT-DISCOVERY).
///
/// The configured faces ([`FONT_CANDIDATES`] / [`FALLBACK_CANDIDATES`] /
/// [`SYMBOL_FALLBACK_CANDIDATES`] / [`COLOR_EMOJI_CANDIDATES`]) cover the common
/// scripts, but a machine can show a code point none of them carry — an
/// uninstalled-by-default CJK/Indic block, a rarely-bundled script, an emoji on a
/// host whose Apple Color Emoji path moved. This resolver is the LAST step before
/// `.notdef` tofu: given such a code point it finds a SYSTEM font that covers it
/// (CoreText on macOS, a candidate-directory scan elsewhere), loads it once, and
/// caches the per-code-point decision so repeated lookups are O(1).
///
/// It is consulted ONLY when [`select_face`] would otherwise give up, so the
/// verified configured-face policy is never shadowed and the common path (a code
/// point a configured face covers) never touches this struct.
#[derive(Default)]
struct RuntimeFallback {
    /// Distinct runtime-discovered faces, parsed once and reused. Indexed by the
    /// `usize` stored in [`Self::decisions`]; parallel to [`Self::face_paths`].
    faces: Vec<fontdue::Font>,
    /// The file path each entry of [`Self::faces`] was loaded from, so a second
    /// code point routed to the same font reuses the already-parsed face instead
    /// of re-reading/parsing it.
    face_paths: Vec<String>,
    /// The per-code-point decision cache: `Some(i)` = covered by `faces[i]`,
    /// `None` = no system font covers it (a real miss, rendered as `.notdef`).
    /// Bounded to [`Self::MAX_DECISIONS`]; the cache is cleared wholesale once it
    /// fills (a coarse bound — the decision is cheap to recompute and a terminal's
    /// working set of fallback code points is tiny).
    decisions: HashMap<char, Option<usize>>,
}

impl RuntimeFallback {
    /// Cap on cached code-point decisions. Generous — distinct code points needing
    /// runtime fallback are rare — but finite, so an adversarial stream of unique
    /// code points cannot grow the map without bound.
    const MAX_DECISIONS: usize = 4096;

    /// Resolve `ch` to a runtime-fallback face index, memoized. Returns `Some(i)`
    /// when `self.faces[i]` covers `ch`, `None` when no system font does (so the
    /// caller renders `.notdef`). The first lookup for a code point does the
    /// platform query + font load; every later lookup is a single map hit.
    fn resolve(&mut self, ch: char) -> Option<usize> {
        if let Some(&decision) = self.decisions.get(&ch) {
            return decision;
        }
        let decision = self.discover(ch);
        // Coarse bound: clear wholesale once full so the map can never grow past
        // the cap. The just-computed decision is always inserted afterward, so the
        // freshly requested code point stays cached.
        if self.decisions.len() >= Self::MAX_DECISIONS {
            self.decisions.clear();
        }
        self.decisions.insert(ch, decision);
        decision
    }

    /// Discover (without caching) a system font that can actually RENDER `ch` as a
    /// monochrome glyph. Gathers an ordered list of candidate font paths (the
    /// CoreText hint first on macOS, then the candidate-directory scan), and
    /// accepts the FIRST one fontdue can rasterize `ch` from to a non-empty bitmap.
    /// Returns the index of the loaded face in [`Self::faces`] (reusing an
    /// already-loaded path), or `None` when nothing renders it.
    ///
    /// The "non-empty raster" gate is load-bearing: CoreText's first pick for a
    /// script can be a font fontdue cannot draw — PingFang (CFF2 variable outlines)
    /// for CJK, Apple Color Emoji (`sbix` bitmaps, no outlines) for emoji, the
    /// universal LastResort tofu font for noncharacters. A nonzero glyph index does
    /// NOT imply fontdue can render it, so we verify by rasterizing and fall through
    /// to the next candidate (e.g. Arial Unicode, a plain TTF) when it can't.
    fn discover(&mut self, ch: char) -> Option<usize> {
        for path in runtime_fallback_candidate_paths(ch) {
            // Reuse an already-loaded face for this path (a previous code point
            // routed here): avoids re-reading/parsing the same large font.
            if let Some(i) = self.face_paths.iter().position(|p| *p == path) {
                if face_can_render(&self.faces[i], ch) {
                    return Some(i);
                }
                continue;
            }
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            let Ok(face) = fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
            else {
                continue;
            };
            if !face_can_render(&face, ch) {
                continue;
            }
            self.faces.push(face);
            self.face_paths.push(path);
            return Some(self.faces.len() - 1);
        }
        None
    }

    /// Borrow the discovered face for code point `ch`, if one is cached. Used by
    /// the rasterizer, which only sees a [`GlyphKey`]'s code point and must
    /// recover WHICH face to rasterize from. `None` is the fail-safe (the caller
    /// falls back to `.notdef`); in practice the decision was cached by
    /// [`Renderer::glyph_key`] before the key was ever rasterized.
    fn face_for(&self, ch: char) -> Option<&fontdue::Font> {
        let &Some(i) = self.decisions.get(&ch)? else {
            return None;
        };
        self.faces.get(i)
    }
}

/// The probe size at which [`face_can_render`] tests a face: the resolver only
/// needs a yes/no, and a small size keeps the trial rasterization cheap.
const RUNTIME_FALLBACK_PROBE_PX: f32 = 16.0;

/// Whether `face` can produce a NON-EMPTY monochrome raster for `ch`. A nonzero
/// `cmap` glyph index is necessary but NOT sufficient: fontdue is an outline
/// rasterizer, so a font whose glyph is a CFF2 variable outline (PingFang) or a
/// colour bitmap (Apple Color Emoji) reports the glyph yet rasterizes to a 0×0 /
/// all-zero bitmap. This gate is what lets [`RuntimeFallback::discover`] reject
/// those and move to the next candidate. (A space-like glyph would also be
/// empty, but the resolver is only ever asked about non-space code points the
/// configured faces missed.)
fn face_can_render(face: &fontdue::Font, ch: char) -> bool {
    if face.lookup_glyph_index(ch) == 0 {
        return false;
    }
    let (m, bytes) = face.rasterize(ch, RUNTIME_FALLBACK_PROBE_PX);
    m.width > 0 && m.height > 0 && bytes.iter().any(|&c| c > 0)
}

/// Ordered candidate font paths to try for code point `ch`, most-preferred first
/// (M3 FONT-DISCOVERY). On macOS the CoreText pick leads (its system cascade
/// list), then the candidate-directory scan; on other platforms only the scan.
/// The caller ([`RuntimeFallback::discover`]) accepts the first path that can
/// actually render `ch`, so leading with a font fontdue cannot draw (e.g. PingFang
/// for CJK) is harmless — the scan's plain-TTF result (Arial Unicode) follows.
fn runtime_fallback_candidate_paths(ch: char) -> Vec<String> {
    let mut paths: Vec<String> = Vec::new();
    #[cfg(target_os = "macos")]
    {
        // CoreText returns the LastResort tofu font for a genuinely-uncovered code
        // point (noncharacters etc.); drop it so the resolver can return None
        // gracefully rather than claiming a fake cover.
        if let Some(p) = macos_coretext::font_path_for_char(ch)
            && !path_is_last_resort(&p)
        {
            paths.push(p);
        }
    }
    runtime_fallback_scan_candidates(ch, &mut paths);
    paths
}

/// Whether `path` is the universal LastResort tofu font (by file stem). It
/// "covers" every code point with a placeholder box, so accepting it would defeat
/// the resolver's graceful-`None` contract for uncovered code points.
fn path_is_last_resort(path: &str) -> bool {
    std::path::Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("LastResort"))
}

/// Cross-platform (and macOS-CoreText-miss) discovery: scan the candidate font
/// directories for every font whose `cmap` covers `ch`, appending their paths to
/// `out` (deduplicated against entries already present, e.g. the CoreText pick).
/// Dependency-free and deterministic (directories + files scanned in a fixed,
/// sorted order). On Linux this is also where a future fontconfig integration
/// would slot in — see the NOTE; we deliberately do NOT link a fontconfig C
/// dependency here.
///
/// The universal LastResort font (macOS `LastResort.otf`) is SKIPPED: it covers
/// every code point with a tofu placeholder, so accepting it would make the
/// resolver claim coverage for genuinely-uncovered code points (noncharacters,
/// unassigned planes) — defeating the "returns None gracefully" contract.
///
/// NOTE (Linux / fontconfig TODO): a real Linux deployment wants
/// `FcCharSetHasChar` over the fontconfig database for accurate, fast coverage
/// queries. This scan is a correct-but-narrow stand-in: it only sees fonts in the
/// [`FONT_DIRS`] it knows, parses each candidate to test coverage (slower than a
/// charset index), and does not understand fontconfig's family aliasing. It is
/// sufficient for the bundled macOS faces and any font dropped into those dirs.
/// One system font's cmap coverage: its path plus the sorted, coalesced inclusive
/// codepoint ranges its `cmap` maps. Built ONCE (see [`font_coverage_index`]) so the
/// per-codepoint runtime fallback is a cheap range probe instead of a re-read+parse
/// of every font on the render thread.
struct FontCoverage {
    path: String,
    ranges: Vec<(u32, u32)>,
}

impl FontCoverage {
    /// Whether the font's cmap covers `cp` — binary search over the sorted ranges.
    fn covers(&self, cp: u32) -> bool {
        self.ranges
            .binary_search_by(|&(lo, hi)| {
                if cp < lo {
                    std::cmp::Ordering::Greater
                } else if cp > hi {
                    std::cmp::Ordering::Less
                } else {
                    std::cmp::Ordering::Equal
                }
            })
            .is_ok()
    }
}

/// Extract a font's covered codepoints from its `cmap` as sorted, coalesced inclusive
/// ranges (compact: a CJK face's ~tens-of-thousands of codepoints collapse to a few
/// hundred ranges). `None` when the face has no usable Unicode cmap.
fn font_cmap_ranges(bytes: &[u8]) -> Option<Vec<(u32, u32)>> {
    let face = ttf_parser::Face::parse(bytes, 0).ok()?;
    let cmap = face.tables().cmap?;
    let mut cps: Vec<u32> = Vec::new();
    for sub in cmap.subtables {
        if sub.is_unicode() {
            sub.codepoints(|cp| cps.push(cp));
        }
    }
    if cps.is_empty() {
        return None;
    }
    cps.sort_unstable();
    cps.dedup();
    let mut ranges: Vec<(u32, u32)> = Vec::with_capacity(64);
    for cp in cps {
        match ranges.last_mut() {
            Some(last) if cp == last.1 + 1 => last.1 = cp,
            _ => ranges.push((cp, cp)),
        }
    }
    Some(ranges)
}

/// The process-wide font cmap-coverage index, built LAZILY on first use: every system
/// font under [`font_search_dirs`] read + parsed ONCE, its coverage stored as ranges.
/// This replaces the old per-codepoint re-read+parse of all ~260 fonts (a ~1s
/// render-thread freeze on each new uncovered codepoint) — the build is paid once,
/// then [`runtime_fallback_scan_candidates`] is an in-memory range probe. The
/// LastResort tofu font is excluded (it "covers" everything). Only the small range
/// tables are retained; the font bytes are dropped after extraction.
fn font_coverage_index() -> &'static [FontCoverage] {
    static INDEX: std::sync::OnceLock<Vec<FontCoverage>> = std::sync::OnceLock::new();
    INDEX.get_or_init(|| {
        let mut idx: Vec<FontCoverage> = Vec::new();
        for path in font_files() {
            let Some(p) = path.to_str().map(str::to_string) else {
                continue;
            };
            if path_is_last_resort(&p) {
                continue;
            }
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            if let Some(ranges) = font_cmap_ranges(&bytes) {
                idx.push(FontCoverage { path: p, ranges });
            }
        }
        idx
    })
}

/// Cross-platform (and macOS-CoreText-miss) discovery: the fonts whose `cmap` covers
/// `ch`, appended to `out` (deduped against entries already present, e.g. the
/// CoreText pick). Backed by the one-time [`font_coverage_index`], so this is an
/// in-memory range probe per font — NO disk read or parse per call. The caller
/// re-parses the chosen path with fontdue and applies `face_can_render`, so a font
/// whose cmap maps `ch` to a glyph fontdue cannot draw is still rejected downstream.
fn runtime_fallback_scan_candidates(ch: char, out: &mut Vec<String>) {
    let cp = ch as u32;
    for fc in font_coverage_index() {
        if fc.covers(cp) && !out.contains(&fc.path) {
            out.push(fc.path.clone());
        }
    }
}

/// macOS CoreText runtime font discovery (M3 FONT-DISCOVERY). Given a code point
/// no configured face covers, ask CoreText which font the system would use for it
/// and recover that font's file path so the renderer can load + rasterize it.
///
/// This is hand-rolled FFI: the workspace has no `core-text` crate, so we declare
/// exactly the CoreText / CoreFoundation symbols this query needs. The frameworks
/// are stock macOS system libraries linked via `#[link(... kind = "framework")]`
/// (the same pattern the GUI's window-capture FFI uses) — NOT a new crate
/// dependency. Every object we `Create`/`Copy` is released exactly once.
#[cfg(target_os = "macos")]
mod macos_coretext {
    use std::ffi::c_void;

    /// Opaque CoreFoundation / CoreText object pointers (never dereferenced in
    /// Rust — handed back to the frameworks or released).
    type CFTypeRef = *const c_void;
    type CFStringRef = *const c_void;
    type CFURLRef = *const c_void;
    type CTFontRef = *const c_void;
    type CFAllocatorRef = *const c_void;

    /// `CFStringEncoding` for UTF-8 (`kCFStringEncodingUTF8`), per CFString.h.
    const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
    /// `CFURLPathStyle` POSIX (`kCFURLPOSIXPathStyle`), per CFURL.h.
    const K_CF_URL_POSIX_PATH_STYLE: isize = 0;

    // SAFETY (whole block): these are the standard, stable CoreFoundation /
    // CoreText C entry points with the signatures published in Apple's headers.
    // `font_path_for_char` upholds each contract: every `Create`/`Copy` result is
    // released exactly once; pointers passed in are live objects we created; the
    // UTF-16 unit count handed to `CTFontCreateForString` matches the buffer.
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFStringCreateWithBytes(
            alloc: CFAllocatorRef,
            bytes: *const u8,
            num_bytes: isize,
            encoding: u32,
            is_external: bool,
        ) -> CFStringRef;
        fn CFStringGetLength(s: CFStringRef) -> isize;
        fn CFStringGetCString(
            s: CFStringRef,
            buffer: *mut u8,
            buffer_size: isize,
            encoding: u32,
        ) -> bool;
        fn CFRelease(cf: CFTypeRef);
    }

    #[link(name = "CoreText", kind = "framework")]
    unsafe extern "C" {
        /// Make a base font at a given size (we only need it as the seed for
        /// `CTFontCreateForString`; family/size are irrelevant to which fallback
        /// CoreText picks for the code point).
        fn CTFontCreateWithName(name: CFStringRef, size: f64, matrix: *const c_void) -> CTFontRef;
        /// THE query: given a base font + a string range, return the font CoreText
        /// would substitute to render that range — i.e. the system cascade-list
        /// font covering the code point. This is the documented per-string fallback
        /// resolver (`CTFontCopyDefaultCascadeListForLanguages` underlies it).
        fn CTFontCreateForString(
            current: CTFontRef,
            string: CFStringRef,
            range: CFRange,
        ) -> CTFontRef;
        /// The font's file URL (CoreText fonts are file-backed), or NULL.
        fn CTFontCopyAttribute(font: CTFontRef, attribute: CFStringRef) -> CFTypeRef;
        /// `kCTFontURLAttribute` — the constant string key for the font's URL
        /// attribute. Declared as an extern static (a `CFStringRef` global).
        static kCTFontURLAttribute: CFStringRef;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        /// Turn a `CFURLRef` into its POSIX file-system path as a `CFStringRef`.
        fn CFURLCopyFileSystemPath(url: CFURLRef, path_style: isize) -> CFStringRef;
    }

    /// A `CFRange` (location + length), matching CoreFoundation's layout.
    #[repr(C)]
    struct CFRange {
        location: isize,
        length: isize,
    }

    /// Build a `CFStringRef` from a Rust `&str` (UTF-8). Caller releases it.
    /// Returns NULL on failure.
    /// SAFETY: see the extern block's block-level note; `s.as_ptr()`/`s.len()`
    /// describe a valid UTF-8 buffer for the duration of the call.
    unsafe fn cfstring(s: &str) -> CFStringRef {
        unsafe {
            CFStringCreateWithBytes(
                std::ptr::null(),
                s.as_ptr(),
                s.len() as isize,
                K_CF_STRING_ENCODING_UTF8,
                false,
            )
        }
    }

    /// Read a `CFStringRef` back into a Rust `String` (UTF-8), or `None`.
    /// SAFETY: `s` must be a live `CFStringRef`.
    unsafe fn cfstring_to_string(s: CFStringRef) -> Option<String> {
        if s.is_null() {
            return None;
        }
        unsafe {
            // Worst-case UTF-8 is 4 bytes/unit; +1 for the NUL CFStringGetCString
            // appends. A font path is short, so this buffer is comfortably sized.
            let len = CFStringGetLength(s);
            let cap = (len as usize).saturating_mul(4).saturating_add(1).max(8);
            let mut buf = vec![0u8; cap];
            if !CFStringGetCString(s, buf.as_mut_ptr(), cap as isize, K_CF_STRING_ENCODING_UTF8) {
                return None;
            }
            let nul = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
            buf.truncate(nul);
            String::from_utf8(buf).ok()
        }
    }

    /// Resolve the file path of the font CoreText would use to render `ch`.
    ///
    /// Seeds an arbitrary base font, asks `CTFontCreateForString` for the
    /// substituted font over `ch`'s UTF-16 range, then reads that font's
    /// `kCTFontURLAttribute` URL and converts it to a POSIX path. `None` if any
    /// step fails (CoreText could not find a cover, the font isn't file-backed,
    /// etc.) — the caller then tries the generic candidate scan.
    pub fn font_path_for_char(ch: char) -> Option<String> {
        let mut u16buf = [0u16; 2];
        let utf16 = ch.encode_utf16(&mut u16buf);
        let utf16_len = utf16.len() as isize;
        let s = ch.to_string();
        // SAFETY: every Create/Copy below is released exactly once on all paths via
        // the `release` helpers; we never deref the opaque pointers. See the extern
        // block's block-level SAFETY note.
        unsafe {
            let cf = cfstring(&s);
            if cf.is_null() {
                return None;
            }
            // A neutral base font ("Helvetica" exists on every Mac); size is
            // irrelevant to which fallback CoreText selects.
            let base_name = cfstring("Helvetica");
            let base = if base_name.is_null() {
                std::ptr::null()
            } else {
                CTFontCreateWithName(base_name, 12.0, std::ptr::null())
            };
            if !base_name.is_null() {
                CFRelease(base_name);
            }
            // `base` may be NULL; CTFontCreateForString tolerates a NULL current
            // font (uses a system default) per the headers.
            let range = CFRange {
                location: 0,
                length: utf16_len,
            };
            let substituted = CTFontCreateForString(base, cf, range);
            if !base.is_null() {
                CFRelease(base);
            }
            CFRelease(cf);
            if substituted.is_null() {
                return None;
            }
            let url = CTFontCopyAttribute(substituted, kCTFontURLAttribute) as CFURLRef;
            CFRelease(substituted);
            if url.is_null() {
                return None;
            }
            let path_cf = CFURLCopyFileSystemPath(url, K_CF_URL_POSIX_PATH_STYLE);
            CFRelease(url as CFTypeRef);
            let path = cfstring_to_string(path_cf);
            if !path_cf.is_null() {
                CFRelease(path_cf);
            }
            path
        }
    }
}

/// The face-selection POLICY for a code point's ordinary (non-VS16) text
/// dispatch, extracted as a pure function of coverage facts so it can be
/// exhaustively verified independently of font I/O (see
/// `crates/aterm-render/tests/presentation_gate.rs`).
///
/// Inputs are the per-face coverage facts [`Renderer::glyph_key`] probes lazily,
/// in priority order, plus `wants_emoji` — the Unicode `Emoji_Presentation`
/// property ([`aterm_grapheme::is_emoji_presentation`]). Priority:
/// procedural → primary mono → broad mono fallback → symbol mono fallback →
/// colour emoji (GATED) → primary `.notdef`.
///
/// # Invariant (proven)
///
/// `select_face(..) == FaceId::ColorEmoji` **implies** `wants_emoji`. A
/// default-text code point therefore NEVER resolves to the colour face here,
/// even when the colour font is the only face that covers it — the bug this
/// gate fixes (⏺ U+23FA: `Emoji=Yes`, `Emoji_Presentation=No`). This matches
/// the reference terminals: iTerm2 gates on `emojiWithDefaultEmojiPresentation`
/// set membership, Ghostty on `uucode.get(.is_emoji_presentation, cp)` — both
/// the default-presentation property, never raw font coverage. The explicit
/// VS16 / emoji-presentation request is handled by the separate, intentionally
/// unconditional [`Renderer::glyph_key_emoji`].
#[must_use]
pub fn select_face(
    procedural: bool,
    primary_has: bool,
    fallback_has: bool,
    symbol_has: bool,
    color_has: bool,
    wants_emoji: bool,
) -> FaceId {
    if procedural {
        FaceId::Procedural
    } else if primary_has {
        FaceId::Primary
    } else if fallback_has {
        FaceId::Fallback
    } else if symbol_has {
        FaceId::SymbolFallback
    } else if color_has && wants_emoji {
        // The colour (RGBA) face is reachable ONLY for a code point that actually
        // wants emoji presentation. This `&& wants_emoji` is the whole fix.
        FaceId::ColorEmoji
    } else if color_has {
        // A default-TEXT code point that no mono face covers but the colour font
        // does (⏺ on a machine without STIX/Apple Symbols): render the colour
        // glyph's MONOCHROME silhouette, foreground-tinted — a visible glyph, not
        // tofu, and still never the colour bitmap.
        FaceId::ColorEmojiMono
    } else {
        // Nothing covers it anywhere: the primary face renders `.notdef`.
        FaceId::Primary
    }
}

/// The font directories scanned by [`resolve_font_family`] / [`list_fonts`] / the
/// runtime fallback, in lookup order (per-user fonts shadow system ones). BOTH the
/// macOS and the Linux locations are listed unconditionally: a directory that does
/// not exist on the host is simply skipped by `read_dir`, so one list serves every
/// platform with no `cfg`. Relative entries are joined with `$HOME` (per-user
/// fonts). On Linux the system trees are NESTED (`…/truetype/<vendor>/x.ttf`), so
/// the scan descends — see [`font_files`].
const FONT_DIRS: &[&str] = &[
    // --- per-user (joined with $HOME), most-preferred first ---
    "Library/Fonts",      // macOS user fonts
    ".fonts",             // Linux user fonts (legacy ~/.fonts)
    ".local/share/fonts", // Linux user fonts (XDG ~/.local/share/fonts)
    // --- macOS system ---
    "/Library/Fonts",
    "/System/Library/Fonts",
    "/System/Library/Fonts/Supplemental",
    // --- Linux system ---
    "/usr/share/fonts",
    "/usr/local/share/fonts",
    "/run/host/usr/share/fonts", // flatpak host fonts
];

/// Font file extensions [`resolve_font_family`] will load (TrueType / OpenType /
/// collections — fontdue reads face 0 of a `.ttc`/`.otc`).
const FONT_EXTS: &[&str] = &["ttf", "otf", "ttc", "otc"];

/// Resolve a font FAMILY name (e.g. `"JetBrains Mono"`) to a font FILE path by
/// scanning the standard macOS font directories for a file whose stem matches
/// the family. The match is case- and separator-insensitive (`"JetBrains Mono"`,
/// `"JetBrainsMono"`, `"jetbrains-mono"` all hit `JetBrainsMono.ttf`), and a
/// file whose stem STARTS WITH the family (so `"Menlo"` finds `Menlo.ttc` and
/// not `Menlo Bold.ttf` only if exact is absent) is accepted as a fallback. An
/// empty / whitespace family, or no match, returns `None` so the caller falls
/// back to `$ATERM_FONT` then the built-in candidates.
///
/// This is a dependency-free resolver (no CoreText link); it deliberately does
/// NOT consult the system font registry, so it is deterministic and testable.
/// A user who needs an arbitrary registered face can still point `$ATERM_FONT`
/// (or the family value) at an explicit path.
#[must_use]
pub fn resolve_font_family(family: &str) -> Option<String> {
    // An explicit path in the family value short-circuits the scan (lets the
    // user name a file directly, like `$ATERM_FONT`).
    let trimmed = family.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.contains('/') && std::path::Path::new(trimmed).is_file() {
        return Some(trimmed.to_string());
    }
    let want = normalize_family(trimmed);
    if want.is_empty() {
        return None;
    }
    // Two passes: an EXACT stem match wins over a prefix match across all dirs (in
    // [`FONT_DIRS`] order, so per-user fonts shadow system ones), so `"Menlo"`
    // prefers `Menlo.ttc` to `Menlo Bold.ttf`. [`font_files`] walks the nested
    // Linux tree recursively, so a family installed at `…/truetype/<vendor>/` is
    // found, not just one sitting at a top-level dir.
    let mut prefix_hit: Option<String> = None;
    for path in font_files() {
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let norm = normalize_family(stem);
        if norm == want {
            return path.to_str().map(str::to_string);
        }
        if prefix_hit.is_none() && norm.starts_with(&want) {
            prefix_hit = path.to_str().map(str::to_string);
        }
    }
    prefix_hit
}

/// The font directories to scan, with `$HOME/Library/Fonts` expanded. Public so
/// diagnostics (`aterm list-fonts`) can report the exact directories the resolver
/// scans, rather than re-deriving them.
#[must_use]
pub fn font_search_dirs() -> Vec<std::path::PathBuf> {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    FONT_DIRS
        .iter()
        .filter_map(|d| {
            let p = std::path::Path::new(d);
            if p.is_absolute() {
                Some(p.to_path_buf())
            } else {
                home.as_ref().map(|h| h.join(d))
            }
        })
        .collect()
}

/// Maximum directory recursion depth for the font-dir scan. macOS keeps its faces
/// one level deep, but Linux NESTS them (`/usr/share/fonts/truetype/<vendor>/x.ttf`),
/// so the scan must descend. A small bound covers every real layout. Symlinked
/// directories are NEVER descended (see [`font_files`]), so the depth bound is a
/// belt-and-braces cap on a pathologically deep REAL tree, not the loop guard.
const FONT_SCAN_MAX_DEPTH: usize = 8;

/// Recursively collect font FILES (extension in [`FONT_EXTS`]) under every directory
/// in [`font_search_dirs`], descending up to [`FONT_SCAN_MAX_DEPTH`] levels. The
/// order is deterministic: directories are visited in [`FONT_DIRS`] order (so
/// per-user fonts precede system fonts), and entries within each directory are
/// sorted. Unreadable directories are skipped, never an error.
///
/// This is the ONE place the on-disk font layout is walked. [`resolve_font_family`],
/// [`list_fonts`], and the runtime fallback ([`runtime_fallback_scan_candidates`])
/// all consume it, so Linux's nested tree and macOS's flat one are handled
/// uniformly — depth-0 files (the macOS layout) are still found first.
fn font_files() -> Vec<std::path::PathBuf> {
    fn is_font(p: &std::path::Path) -> bool {
        p.extension()
            .and_then(|e| e.to_str())
            .is_some_and(|ext| FONT_EXTS.iter().any(|e| ext.eq_ignore_ascii_case(e)))
    }
    fn walk(dir: &std::path::Path, depth: usize, out: &mut Vec<std::path::PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let mut entries: Vec<std::fs::DirEntry> = entries.flatten().collect();
        entries.sort_by_key(std::fs::DirEntry::path);
        for entry in entries {
            let p = entry.path();
            // `file_type()` reports the entry WITHOUT following a final symlink. We
            // descend ONLY real directories — a symlinked directory is never followed,
            // so a self/loop symlink (e.g. in ~/.fonts) cannot blow the walk up
            // (real dirs form an acyclic tree). A symlink that points at a font FILE is
            // still honoured (font trees legitimately symlink faces).
            let Ok(ft) = entry.file_type() else { continue };
            if ft.is_dir() {
                if depth < FONT_SCAN_MAX_DEPTH {
                    walk(&p, depth + 1, out);
                }
            } else if ft.is_symlink() {
                // Resolve the link target's kind once; follow file targets, skip dir
                // targets (loop-safe). `is_file()` traverses the link.
                if p.is_file() && is_font(&p) {
                    out.push(p);
                }
            } else if is_font(&p) {
                out.push(p);
            }
        }
    }
    let mut out = Vec::new();
    for dir in font_search_dirs() {
        walk(&dir, 0, &mut out);
    }
    out
}

/// Normalize a family name / file stem for comparison: lowercase, with ASCII
/// whitespace, `-` and `_` removed, so `"JetBrains Mono"`, `"JetBrainsMono"`,
/// and `"jetbrains-mono"` collapse to the same key.
fn normalize_family(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_ascii_whitespace() && *c != '-' && *c != '_')
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Basic metrics for a resolved font face — the data behind `aterm show-face`.
/// Cell metrics are at [`FaceInfo::PROBE_PX`], a representative monospace size; a
/// caller can scale linearly. This is a read-only introspection summary, not a
/// rendering handle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FaceInfo {
    /// Absolute path to the resolved font file.
    pub path: String,
    /// Advance width of `'M'` in px at [`Self::PROBE_PX`] (a cell width for a
    /// monospace face; just one glyph's advance for a proportional one).
    pub cell_width: usize,
    /// Cell height in px at [`Self::PROBE_PX`] (line height from font metrics).
    pub cell_height: usize,
    /// Baseline offset in px (ascent) at [`Self::PROBE_PX`].
    pub baseline: i32,
    /// Number of glyphs in the face (from the font's glyph table).
    pub glyph_count: usize,
}

impl FaceInfo {
    /// The fixed probe size the reported metrics are measured at.
    pub const PROBE_PX: f32 = 16.0;
}

/// Enumerate the available font files as FAMILY STEMS (file stems like `"Menlo"`,
/// `"SFNSMono"`) from the system font directories ([`FONT_DIRS`], matched by
/// [`FONT_EXTS`]), de-duplicated and sorted for deterministic, scriptable output.
/// User and system fonts merge into one set. Returns whatever it finds — an
/// unreadable directory is skipped, never an error. NOT filtered to monospace.
///
/// This is the data behind `aterm list-fonts`. A stem is an *approximation* of a
/// family name: weight/style variants shipped as separate files list separately
/// (`Arial`, `Arial Bold`, …), and the faces inside a `.ttc`/`.otc` collection are
/// not enumerated individually. Each listed stem does resolve via
/// [`resolve_font_family`].
#[must_use]
pub fn list_fonts() -> Vec<String> {
    let mut families: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for path in font_files() {
        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            families.insert(stem.to_string());
        }
    }
    families.into_iter().collect()
}

/// Resolve a font `family` to its [`FaceInfo`] (path + cell metrics + glyph
/// count), or `None` if the family does not resolve or the file cannot be parsed.
/// Uses the SAME [`resolve_font_family`] the renderer uses, so the reported path
/// is the one aterm would actually load. Metrics are at [`FaceInfo::PROBE_PX`].
///
/// Device-pixel cell width from a monospace advance. Round (not ceil): ceil
/// systematically over-widens the cell by up to ~1px, which at low dpr drops whole
/// columns vs the count terminals fit by rounding — a wide table would wrap.
#[inline]
fn cell_w_from_advance(advance: f32) -> usize {
    (advance.round() as usize).max(1)
}

/// This is the data behind `aterm show-face <family>`.
#[must_use]
pub fn face_info(family: &str) -> Option<FaceInfo> {
    let path = resolve_font_family(family)?;
    let bytes = std::fs::read(&path).ok()?;
    let font =
        fontdue::Font::from_bytes(bytes.as_slice(), fontdue::FontSettings::default()).ok()?;
    let lm = font.horizontal_line_metrics(FaceInfo::PROBE_PX)?;
    let adv = font.metrics('M', FaceInfo::PROBE_PX).advance_width;
    // Glyph count from the font's glyph table (ttf-parser is already a dependency,
    // used for the colour-emoji sbix path).
    let glyph_count = ttf_parser::Face::parse(bytes.as_slice(), 0)
        .map(|f| f.number_of_glyphs() as usize)
        .unwrap_or(0);
    Some(FaceInfo {
        path,
        cell_width: cell_w_from_advance(adv),
        cell_height: lm.new_line_size.ceil().max(1.0) as usize,
        baseline: lm.ascent.round() as i32,
        glyph_count,
    })
}

#[cfg(test)]
mod font_enum_tests {
    use super::{FaceInfo, face_info, list_fonts, resolve_font_family};

    #[test]
    fn list_is_sorted_and_deduplicated() {
        let fonts = list_fonts();
        // Sorted ascending and no duplicates (BTreeSet guarantees both; assert it
        // so a future switch to a different container can't silently regress the
        // scriptable, deterministic contract).
        for w in fonts.windows(2) {
            assert!(w[0] < w[1], "not sorted/unique: {:?} then {:?}", w[0], w[1]);
        }
    }

    #[test]
    fn face_info_resolves_a_listed_font() {
        // Pick any listed family that actually resolves, and assert its metrics are
        // sane. Skips cleanly on a host with no enumerable/resolvable fonts.
        let Some(family) = list_fonts()
            .into_iter()
            .find(|f| resolve_font_family(f).is_some())
        else {
            eprintln!("SKIP: no resolvable system font");
            return;
        };
        let info = face_info(&family).expect("a resolvable family yields face_info");
        assert!(!info.path.is_empty(), "path must be set");
        assert!(info.cell_width >= 1 && info.cell_height >= 1, "{info:?}");
        assert!(info.glyph_count > 0, "a real face has glyphs: {info:?}");
        let _ = FaceInfo::PROBE_PX;
    }

    #[test]
    fn face_info_none_for_bogus_family() {
        assert!(face_info("definitely-not-a-real-font-xyzzy").is_none());
    }
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
        let cell_w = cell_w_from_advance(adv);
        let cell_h = lm.new_line_size.ceil().max(1.0) as usize;
        let baseline = lm.ascent.round() as i32;
        Ok(Renderer {
            font,
            // Retain the primary bytes so run shaping can build a rustybuzz::Face.
            rb_primary_bytes: Some(bytes.to_vec()),
            // One-time GSUB probe: fonts with no liga/calt feature can never ligate,
            // so the planner short-circuits shaping for them (byte-identical, faster).
            has_ligature_features: ligature_shaping::font_has_ligature_features(bytes),
            shaping: aterm_types::text_shaping::TextShapingConfig::default(),
            // Default config has no user features, so this resolves to the base
            // `[liga, calt]` pair — identical to the pre-feature shaping behaviour.
            resolved_features: ligature_shaping::build_feature_list(&[]),
            shaped_runs: HashMap::new(),
            fallback_chain: Vec::new(),
            fallback_pick: HashMap::new(),
            fallback_paths: Vec::new(),
            symbol_fallback: None,
            symbol_fallback_paths: Vec::new(),
            color_font: None,
            color_font_paths: Vec::new(),
            runtime_fallback: RuntimeFallback::default(),
            px,
            px_q: GlyphKey::quantize_px(px),
            cell_w,
            cell_h,
            pad: 0,
            baseline,
            theme,
            stem_lut: build_stem_lut(stem_gamma_for_theme(theme.fg, theme.bg)),
            selection_fg: None,
            selection_inactive: false,
            selection_inactive_bg: None,
            glyphs: HashMap::new(),
            primary_gid_cache: HashMap::new(),
            keys: HashMap::new(),
            emoji_keys: HashMap::new(),
            cluster_gids: HashMap::new(),
            styled_keys: HashMap::new(),
            cursor_blink_phase: true,
            cursor_style_override: None,
            procedural: std::env::var_os(NO_PROCEDURAL_ENV).is_none(),
        })
    }

    /// RESET the broad-coverage fallback chain to a SINGLE face from explicit bytes
    /// (eagerly). Glyphs absent in the primary font are rasterized from this face
    /// instead of going blank. Prefer the lazy path (`from_system`) unless you have
    /// a reason to pay the parse cost upfront. To add MORE fallbacks for scripts one
    /// face misses, follow with [`add_fallback_bytes`](Self::add_fallback_bytes).
    pub fn set_fallback_bytes(&mut self, bytes: &[u8]) -> Result<(), String> {
        // Interned: a ~370MB parsed broad fallback is shared across panes injecting
        // the same bytes (every terminal pane injects the same OS fallback).
        let font = intern_parsed_font(bytes)?;
        self.fallback_chain.clear();
        self.fallback_chain.push(font);
        // The per-char winners were keyed to the OLD chain order; drop them so the
        // next miss re-probes against the reset chain.
        self.fallback_pick.clear();
        self.keys.clear();
        self.fallback_paths.clear();
        Ok(())
    }

    /// APPEND a broad-coverage fallback face to the chain (eagerly). The chain is
    /// consulted in append order: [`FaceId::Fallback`] tries each entry and the
    /// first that has the glyph wins, so a CJK fallback (no Arabic/Devanagari/Thai/
    /// Hebrew) appended first still reaches a script-specific face appended after it
    /// instead of rendering tofu. Coverage is the real fontdue cmap probe
    /// (`lookup_glyph_index != 0`), same as the single-face path.
    pub fn add_fallback_bytes(&mut self, bytes: &[u8]) -> Result<(), String> {
        let font = intern_parsed_font(bytes)?;
        self.fallback_chain.push(font);
        // A char that previously resolved to `.notdef` (no chain face covered it)
        // might now be covered by the new entry; re-probe on the next miss.
        self.fallback_pick.clear();
        self.keys.clear();
        self.fallback_paths.clear();
        Ok(())
    }

    /// Install a monochrome SYMBOL fallback face from explicit bytes (eagerly).
    /// Consulted only after the primary + broad fallback miss; mirrors
    /// [`set_fallback_bytes`] for the symbol slot. Clearing the candidate paths
    /// stops the lazy system scan from later overwriting the injected face.
    pub fn set_symbol_fallback_bytes(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.symbol_fallback = Some(intern_parsed_font(bytes)?);
        self.symbol_fallback_paths.clear();
        Ok(())
    }

    /// Install the colour-emoji (sbix) font from explicit bytes (eagerly).
    /// Mirrors how [`ensure_color_font`] populates `color_font`, so the existing
    /// ColorEmoji colour path renders the injected emoji face. Clearing the
    /// candidate paths stops the lazy system scan from later overwriting it. Bytes
    /// are validated as a parseable face (a `ttf_parser::Face` borrows them per
    /// rasterization), so a bad blob fails loudly instead of yielding tofu later.
    pub fn set_color_font_bytes(&mut self, bytes: Vec<u8>) -> Result<(), String> {
        ttf_parser::Face::parse(&bytes, 0).map_err(|e| e.to_string())?;
        self.color_font = Some(intern_font_bytes(bytes));
        self.color_font_paths.clear();
        Ok(())
    }

    /// Build from the first available system monospace font ($ATERM_FONT first).
    /// The Unicode fallback ($ATERM_FALLBACK_FONT first) is recorded but loaded
    /// LAZILY on the first code point that misses the primary face.
    pub fn from_system(px: f32, theme: Theme) -> Option<Self> {
        Self::from_system_with_family(None, px, theme)
    }

    /// Like [`Renderer::from_system`], but tries a configured font FAMILY name
    /// FIRST. The candidate order is: the resolved family file (if `family` names
    /// one that exists), then `$ATERM_FONT`, then the built-in [`FONT_CANDIDATES`].
    /// A `None` family — or a family that resolves to nothing — reduces EXACTLY to
    /// `from_system`, so the default (and a typo'd family) is byte-identical.
    pub fn from_system_with_family(family: Option<&str>, px: f32, theme: Theme) -> Option<Self> {
        let mut paths: Vec<String> = Vec::new();
        if let Some(p) = family.and_then(resolve_font_family) {
            paths.push(p);
        }
        if let Ok(p) = std::env::var("ATERM_FONT") {
            paths.push(p);
        }
        paths.extend(FONT_CANDIDATES.iter().map(|s| s.to_string()));
        for p in paths {
            if let Ok(bytes) = std::fs::read(&p)
                && let Ok(mut r) = Self::from_bytes(&bytes, px, theme)
            {
                r.fallback_paths = fallback_candidate_paths();
                r.symbol_fallback_paths = symbol_fallback_candidate_paths();
                r.color_font_paths = color_emoji_candidate_paths();
                return Some(r);
            }
        }
        // FONT-EMBED: last resort — a bundled monospace face so text still renders
        // when the host has NO usable system font (e.g. a stripped Linux container,
        // where every candidate path above is absent). Only compiled in with the
        // default `embedded-font` feature.
        #[cfg(feature = "embedded-font")]
        if let Ok(mut r) = Self::from_bytes(embedded_font(), px, theme) {
            r.fallback_paths = fallback_candidate_paths();
            r.symbol_fallback_paths = symbol_fallback_candidate_paths();
            r.color_font_paths = color_emoji_candidate_paths();
            return Some(r);
        }
        None
    }

    /// Lazily seed the fallback chain with the first available system face the
    /// first time it's needed. After this runs once, `fallback_paths` is empty so
    /// we never re-try. Only seeds the lazy SYSTEM face; explicit
    /// `add_fallback_bytes` entries are already in the chain and untouched.
    fn ensure_fallback(&mut self) {
        if !self.fallback_chain.is_empty() || self.fallback_paths.is_empty() {
            return;
        }
        let paths = std::mem::take(&mut self.fallback_paths);
        for p in paths {
            if let Ok(bytes) = std::fs::read(&p)
                && let Ok(font) = intern_parsed_font(&bytes)
            {
                self.fallback_chain.push(font);
                return;
            }
        }
    }

    /// Whether ANY chain face has a (non-`.notdef`) glyph for `ch`, recording the
    /// FIRST covering entry's index in [`Self::fallback_pick`] so the rasterizer can
    /// recover which face to draw from. Probes the real fontdue cmap in chain order;
    /// loads the lazy system face on first use.
    fn fallback_has(&mut self, ch: char) -> bool {
        self.ensure_fallback();
        for (i, font) in self.fallback_chain.iter().enumerate() {
            if font.lookup_glyph_index(ch) != 0 {
                self.fallback_pick.insert(ch, i);
                return true;
            }
        }
        false
    }

    /// Lazily load the first available symbol-fallback face the first time a code
    /// point misses both the primary and broad fallback faces. After this runs
    /// once, `symbol_fallback_paths` is empty so we never re-try.
    fn ensure_symbol_fallback(&mut self) {
        if self.symbol_fallback.is_some() || self.symbol_fallback_paths.is_empty() {
            return;
        }
        let paths = std::mem::take(&mut self.symbol_fallback_paths);
        for p in paths {
            if let Ok(bytes) = std::fs::read(&p)
                && let Ok(font) = intern_parsed_font(&bytes)
            {
                self.symbol_fallback = Some(font);
                return;
            }
        }
    }

    /// Whether the symbol-fallback face has a (non-`.notdef`) glyph for `ch`
    /// (loads it lazily).
    fn symbol_fallback_has(&mut self, ch: char) -> bool {
        self.ensure_symbol_fallback();
        self.symbol_fallback
            .as_ref()
            .is_some_and(|f| f.lookup_glyph_index(ch) != 0)
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
                    self.color_font = Some(intern_font_bytes(bytes));
                    return;
                }
            }
        }
    }

    /// Whether the colour-emoji face has a glyph for `ch` (loads it lazily).
    fn color_font_has(&mut self, ch: char) -> bool {
        self.ensure_color_font();
        let Some(bytes) = self.color_font.as_deref() else {
            return false;
        };
        let Ok(face) = ttf_parser::Face::parse(bytes, 0) else {
            return false;
        };
        // A glyph is colour-renderable if the face can DRAW it in colour: a CBDT/
        // sbix raster strike OR a COLR (vector) paint. The COLR branch is what lets
        // a COLR-only font (Twemoji, modern COLRv1 Noto) render emoji in colour at
        // all — without it `select_face` declines every glyph and falls to mono.
        face.glyph_index(ch).is_some_and(|gid| {
            face.glyph_raster_image(gid, u16::MAX).is_some() || face.is_color_glyph(gid)
        })
    }

    pub fn cell_size(&self) -> (usize, usize) {
        (self.cell_w, self.cell_h)
    }

    /// Replace the default fg/bg/cursor/selection theme live (host theme change),
    /// so a pane re-themes WITHOUT being rebuilt. Glyphs are coverage masks coloured
    /// at blit time from the cell's SGR colour or this theme, so no glyph-cache
    /// invalidation is needed — the next render paints with the new colours.
    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
    }

    /// Re-rasterize at a new pixel size — for a host DPI / devicePixelRatio change,
    /// so a pane moved to a different-density display rebuilds its cell metrics
    /// instead of staying frozen at the construction-dpr size (which would mis-size
    /// the grid). Re-derives cell_w/cell_h/baseline from the font's metrics at `px`
    /// and drops the glyph caches (they were rasterized at the old px). No-op if the
    /// size is unchanged or the font lacks metrics at `px`.
    pub fn set_px(&mut self, px: f32) {
        if (px - self.px).abs() < 0.01 {
            return;
        }
        let Some(lm) = self.font.horizontal_line_metrics(px) else {
            return;
        };
        let adv = self.font.metrics('M', px).advance_width;
        self.px = px;
        self.px_q = GlyphKey::quantize_px(px);
        self.cell_w = cell_w_from_advance(adv);
        self.cell_h = lm.new_line_size.ceil().max(1.0) as usize;
        self.baseline = lm.ascent.round() as i32;
        // Glyphs were rasterized at the old px; drop the caches so they re-rasterize.
        self.glyphs.clear();
        self.keys.clear();
        self.emoji_keys.clear();
        self.styled_keys.clear();
        self.cluster_gids.clear();
        self.shaped_runs.clear();
        // `fallback_pick` keys off `keys`-resolved chars; keep it consistent. The
        // chain faces themselves are size-independent (rasterized at `px` on demand).
        self.fallback_pick.clear();
    }

    /// Set the explicit selected-text foreground (theme `selectionForeground`), or
    /// `None` to restore the WCAG contrast-floor default. Applied at blit time like
    /// the theme, so no glyph-cache invalidation is needed.
    pub fn set_selection_fg(&mut self, fg: Option<u32>) {
        self.selection_fg = fg;
    }

    /// The current explicit selected-text foreground override (or `None` for the
    /// contrast-floor default). The GPU renderer reads this off its wrapped CPU
    /// face so both paths resolve selected-glyph colour identically.
    #[must_use]
    pub fn selection_fg(&self) -> Option<u32> {
        self.selection_fg
    }

    /// Mark the pane FOCUSED (`false`) or UNFOCUSED (`true`). When unfocused,
    /// selected cells paint with the inactive selection bg (xterm
    /// `selectionInactiveBackground`) instead of the active `theme.selection`.
    /// Appearance-only, applied at fill time — no glyph-cache invalidation.
    pub fn set_selection_inactive(&mut self, inactive: bool) {
        self.selection_inactive = inactive;
    }

    /// Whether the pane is currently treated as unfocused for selection theming.
    #[must_use]
    pub fn selection_inactive(&self) -> bool {
        self.selection_inactive
    }

    /// Set the inactive (unfocused) selection background (0x00RRGGBB), or `None` to
    /// derive it from the active selection bg (see [`derive_inactive_selection_bg`]).
    /// Only consulted while [`set_selection_inactive`](Self::set_selection_inactive)
    /// is `true`. Applied at fill time, so no glyph-cache invalidation is needed.
    pub fn set_selection_inactive_bg(&mut self, bg: Option<u32>) {
        self.selection_inactive_bg = bg;
    }

    /// The configured inactive selection background override, or `None` (derived).
    #[must_use]
    pub fn selection_inactive_bg(&self) -> Option<u32> {
        self.selection_inactive_bg
    }

    /// The selection background to fill selected cells with THIS frame: the active
    /// `theme.selection` when focused, else the inactive bg (the host override, or a
    /// value derived from `theme.selection`/`theme.bg`). The SINGLE source of truth
    /// for the selection-band colour — both the CPU fill and the GPU encode read it,
    /// so focused/unfocused selection stays byte-identical across paths.
    #[must_use]
    pub fn effective_selection_bg(&self) -> u32 {
        if self.selection_inactive {
            self.selection_inactive_bg.unwrap_or_else(|| {
                derive_inactive_selection_bg(self.theme.selection, self.theme.bg)
            })
        } else {
            self.theme.selection
        }
    }

    /// The current interior padding (px per edge). `0` is the historical no-pad
    /// behavior. See the `pad` field; the GPU mirror reads this to stay identical.
    pub fn pad(&self) -> usize {
        self.pad
    }

    /// Set the interior padding (px per edge). Invalidates the damage cache so the
    /// next render repaints into the newly-sized framebuffer (the cached pixels are
    /// a different dimension once `pad` changes). Idempotent for an unchanged value.
    pub fn set_pad(&mut self, pad: usize) {
        // NOTE: cache invalidation on a pad change is handled per-window:
        // `pad` changes the framebuffer dims (`2·pad` larger per axis), and
        // `render_input_cached`'s `c.width == w && c.height == h` guard forces a
        // FullRepaint whenever the cached dims no longer match. The damage cache
        // now lives in `WindowCpu` (per-window), so the shared `Renderer` cannot
        // (and need not) clear it here.
        self.pad = pad;
    }

    /// Pixel size of a `rows`x`cols` grid, INCLUDING `2·pad` of interior padding
    /// (one `pad` on each of the four edges). With `pad == 0` this is the original
    /// `cols·cell_w × rows·cell_h`.
    pub fn frame_size(&self, rows: usize, cols: usize) -> (usize, usize) {
        (
            cols * self.cell_w + 2 * self.pad,
            rows * self.cell_h + 2 * self.pad,
        )
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
    /// Resolve `ch` to a PRIMARY-face glyph id through the font's UNICODE cmap,
    /// using ttf-parser (which selects a Unicode subtable) rather than fontdue's
    /// own char lookup. fontdue prefers a legacy `(1,0)` Mac Roman cmap subtable
    /// when one is present (Apple `.ttc` faces like Menlo/Monaco), which mis-maps
    /// the entire Latin-1 block — `·`(U+00B7)→`∑`, `é`→`È`, `®`→`Æ`, … — because
    /// Mac Roman byte 0xB7 is `∑`, 0xE9 is `È`, and so on. Resolving the id
    /// ourselves and rasterizing it BY ID (`rasterize_indexed`) makes glyph choice
    /// faithful to the Unicode scalar in the cell. Memoized per char; `None` (incl.
    /// a `.notdef`/0 mapping) means the primary face has no glyph and the normal
    /// fallback chain takes over. Verified exhaustively by the glyph-fidelity
    /// conformance test (`render_glyph_resolution_is_unicode_faithful`).
    fn primary_unicode_gid(&mut self, ch: char) -> Option<u16> {
        if let Some(&g) = self.primary_gid_cache.get(&ch) {
            return g;
        }
        let g = self
            .rb_primary_bytes
            .as_deref()
            .and_then(|b| ttf_parser::Face::parse(b, 0).ok())
            .and_then(|f| f.glyph_index(ch))
            .map(|gid| gid.0)
            .filter(|&gid| gid != 0);
        self.primary_gid_cache.insert(ch, g);
        g
    }

    pub fn glyph_key(&mut self, ch: char) -> GlyphKey {
        if let Some(&key) = self.keys.get(&ch) {
            return key;
        }
        // Probe coverage lazily, in priority order — each fact is computed only
        // when every higher-priority face has already missed, so the heavy
        // fallback / symbol / colour faces are never loaded for a char the
        // primary face covers. Procedural box-drawing/block/braille interception
        // stays FIRST: those cells must be cell-exact and seam-free, which no
        // font guarantees ($ATERM_NO_PROCEDURAL_GLYPHS opts back into fonts).
        let procedural = self.procedural && procedural::covers(ch);
        // Primary coverage + glyph id come from the UNICODE cmap (ttf-parser), NOT
        // fontdue's char lookup, which mis-selects a Mac Roman subtable on Apple
        // `.ttc` fonts (see `primary_unicode_gid`). The id is carried on the key so
        // the glyph is rasterized BY ID, faithful to the cell's Unicode scalar.
        let primary_gid = if procedural {
            None
        } else {
            self.primary_unicode_gid(ch)
        };
        let primary_has = primary_gid.is_some();
        // The fallback CHAIN is tried in order; `fallback_has` records WHICH entry
        // covered `ch` (in `fallback_pick`) so the rasterizer recovers it later.
        let fallback_has = !procedural && !primary_has && self.fallback_has(ch);
        let symbol_has =
            !procedural && !primary_has && !fallback_has && self.symbol_fallback_has(ch);
        // Colour coverage is probed when every mono face missed. Whether it
        // produces a colour glyph or a monochromatized one is decided by
        // `select_face` from `wants_emoji` (the Unicode `Emoji_Presentation`
        // property) — so a default-text symbol (⏺) the colour font covers never
        // resolves to the colour face. The POLICY lives in the pure,
        // exhaustively-verified `select_face`: ONE provable colour-vs-text place.
        let wants_emoji = aterm_grapheme::is_emoji_presentation(ch);
        let color_has =
            !procedural && !primary_has && !fallback_has && !symbol_has && self.color_font_has(ch);
        let face = select_face(
            procedural,
            primary_has,
            fallback_has,
            symbol_has,
            color_has,
            wants_emoji,
        );
        // M3 FONT-DISCOVERY: `select_face` returns `FaceId::Primary` for the
        // genuinely-covered primary glyph AND for the give-up `.notdef` case (no
        // configured face covers the char). Distinguish them: it is a give-up
        // ONLY when the char isn't procedural and the primary truly lacks it.
        // In that give-up case — and ONLY then — try the runtime resolver before
        // settling for `.notdef`. The common path (any face covered it) never
        // reaches this, so behaviour/perf for ordinary text is unchanged.
        let face = if face == FaceId::Primary && !procedural && !primary_has {
            match self.runtime_fallback.resolve(ch) {
                Some(_) => FaceId::RuntimeFallback,
                None => FaceId::Primary, // a real miss — render `.notdef`.
            }
        } else {
            face
        };
        let key = match face {
            // The colour face carries a 32-bit RGBA sbix bitmap (🚀 😀); every
            // other outcome — including the monochromatized colour silhouette
            // (`ColorEmojiMono`) and the runtime-discovered fallback — is a
            // foreground-tinted Mono coverage mask.
            FaceId::ColorEmoji => GlyphKey::rgba_char(FaceId::ColorEmoji, ch, self.px_q),
            // Primary glyph: address it BY the Unicode-resolved glyph id so the
            // rasterizer (`rasterize_indexed`) bypasses fontdue's Mac-Roman char
            // lookup. Only the genuinely-covered case has a gid here; the give-up
            // `.notdef` (primary_gid == None) falls through to `mono_char`.
            FaceId::Primary if primary_gid.is_some() => {
                GlyphKey::mono_gid(primary_gid.unwrap(), StyleBits::REGULAR, self.px_q)
            }
            source => GlyphKey::mono_char(source, ch, StyleBits::REGULAR, self.px_q),
        };
        self.keys.insert(ch, key);
        key
    }

    /// Resolve `ch` to a runtime-discovered fallback face's index (M3
    /// FONT-DISCOVERY), or `None` if no system font covers it. Memoized in
    /// [`Self::runtime_fallback`]. Exposed for tests/diagnostics: production code
    /// reaches it only via [`Self::glyph_key`]'s give-up path.
    #[doc(hidden)]
    pub fn runtime_fallback_resolves(&mut self, ch: char) -> bool {
        self.runtime_fallback.resolve(ch).is_some()
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
        // Procedural (cell-exact) and ColorEmojiMono (an emoji silhouette, no real
        // weight/slant) ignore synthetic styling; every other coverage glyph gets
        // it — including primary glyphs now addressed by id (`MonoGid`), so bold/
        // italic still applies to Latin-1 & co. after the Unicode-id routing.
        let key = if matches!(base.glyph_class, GlyphClass::Mono | GlyphClass::MonoGid)
            && base.source != FaceId::Procedural
            && base.source != FaceId::ColorEmojiMono
        {
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
        let bytes = self.color_font.as_deref()?;
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
        // The colour face must actually be able to DRAW this glyph, else there's
        // nothing to render in colour and the base-codepoint fallback is the honest
        // result. Accept EITHER a bitmap strike OR a COLR (vector) paint:
        //  - Raster (CBDT/sbix): probe the LARGEST strike (`u16::MAX`). Apple Color
        //    Emoji's sbix has a ppem dead-zone (~33–52) where COMPOSITE glyphs (ZWJ
        //    families/sequences) carry no strike — a cell-ppem request would return
        //    None and we'd wrongly decline the cluster, collapsing 👨‍👩‍👧‍👦 to 👨.
        //    The largest strike always has it; the rasterizer downscales regardless.
        //  - COLR: a COLR-only font (Twemoji, modern COLRv1 Noto) has NO raster
        //    strike at all, so without this branch every ZWJ cluster emoji on such a
        //    font fell back to .notdef mono. `rasterize_color_emoji_gid` already
        //    paints COLR via `rasterize_colr`; this lets the cluster reach it.
        // Mirrors `color_font_has`.
        let tt = ttf_parser::Face::parse(bytes, 0).ok()?;
        let g = ttf_parser::GlyphId(gid);
        if tt.glyph_raster_image(g, u16::MAX).is_none() && !tt.is_color_glyph(g) {
            return None;
        }
        Some(gid)
    }

    /// Resolve an emoji cluster to its colour glyph key (shaped glyph id), or
    /// `None` if the cluster does not shape to a single colour glyph.
    pub fn glyph_key_cluster(&mut self, cluster: &str) -> Option<GlyphKey> {
        let gid = self.shape_cluster(cluster)?;
        Some(GlyphKey::rgba_gid(FaceId::ColorEmoji, gid, self.px_q))
    }

    /// Install the text-shaping config (ligature mode + features). DEFAULT is
    /// `LigatureMode::Enabled`; set `Disabled` to render strictly per-cell (the
    /// pre-ligature behaviour). Clears the shaped-run cache so a mode flip takes
    /// effect on the next frame.
    pub fn set_text_shaping(&mut self, shaping: aterm_types::text_shaping::TextShapingConfig) {
        self.shaping = shaping;
        // Resolve the rustybuzz feature array ONCE here (not per row/run/cell):
        // the base `liga`+`calt` pair plus the user's OpenType features for the
        // PRIMARY face (`font_id == 0`, the face this shaper covers). Done off the
        // hot path so the per-run loop just borrows the prebuilt slice.
        self.resolved_features = Self::resolve_features(&self.shaping);
        self.shaped_runs.clear();
    }

    /// Flatten the shaping config's per-font `font_features` into the rustybuzz
    /// feature array for the PRIMARY face (`font_id == 0`, the only face the
    /// ligature shaper drives). When no primary-face features are configured this
    /// returns just the base `[liga, calt]` pair, so the empty-features (common)
    /// path is byte-identical to the pre-feature renderer. Called only when the
    /// config changes — never on the per-frame hot path.
    fn resolve_features(
        shaping: &aterm_types::text_shaping::TextShapingConfig,
    ) -> Vec<rustybuzz::Feature> {
        let primary: Vec<aterm_types::text_shaping::FontFeature> = shaping
            .font_features
            .iter()
            .filter(|set| set.font_id == 0)
            .flat_map(|set| set.features.iter().copied())
            .collect();
        ligature_shaping::build_feature_list(&primary)
    }

    /// The current text-shaping config.
    pub fn text_shaping(&self) -> &aterm_types::text_shaping::TextShapingConfig {
        &self.shaping
    }

    /// The rustybuzz feature array currently applied to ligature shaping runs
    /// (base `liga`+`calt` plus the resolved primary-face user `font_features`).
    /// Exposed for the WIRE-FONTFEAT integration tests, which assert the user's
    /// configured features actually reach the shaping call rather than being
    /// silently dropped. Not part of the rendering contract.
    #[doc(hidden)]
    #[must_use]
    pub fn resolved_features_for_test(&self) -> &[rustybuzz::Feature] {
        &self.resolved_features
    }

    /// Whether the primary face advertises a `liga`/`calt` `GSUB` feature (probed
    /// once at construction). `false` means the font cannot ligate, so the planner
    /// short-circuits shaping to the per-cell path. Exposed for tests/diagnostics.
    #[must_use]
    pub fn has_ligature_features(&self) -> bool {
        self.has_ligature_features
    }

    /// Whether ligatures are GLOBALLY off (`LigatureMode::Disabled`). The
    /// `CursorDisabled` mode is row-local and handled at the column level (the
    /// cursor's run is forced per-cell), so it does NOT disable globally.
    fn ligatures_globally_off(&self) -> bool {
        matches!(
            self.shaping.ligature_mode,
            aterm_types::text_shaping::LigatureMode::Disabled
        )
    }

    /// Build the per-column glyph plan for row `r` of `input`: which columns a
    /// ligature owns (drawn as a `mono_gid` glyph at the column origin) and which
    /// fall back to the ordinary per-cell dispatch. `break_cols` lists columns
    /// that must stay per-cell this frame (the cursor column under
    /// `CursorDisabled`, so the cursor never sits on a ligature glyph). When
    /// ligatures are globally off the plan is all `PerCell` (byte-identical to the
    /// pre-ligature path). SHARED by both renderers so they place identical glyphs.
    pub fn row_glyph_plan(
        &mut self,
        input: &RenderInput,
        r: usize,
        break_cols: &[usize],
        out: &mut Vec<ColumnGlyph>,
    ) {
        let cols = input.cols;
        let cells = &input.cells[r];
        // Short-circuit to the all-PerCell plan when ligatures are off, no primary
        // bytes are retained, OR the primary font has no liga/calt GSUB feature (it
        // cannot ligate, so shaping would reproduce the per-cell cmap glyphs — skip
        // it). This guard is on the SHARED seam, so CPU and GPU decide identically.
        if self.ligatures_globally_off()
            || self.rb_primary_bytes.is_none()
            || !self.has_ligature_features
        {
            out.clear();
            out.resize(cols, ColumnGlyph::PerCell);
            return;
        }
        let row_clusters = &input.clusters[r];
        let row_images = &input.images[r];
        // A column may join a run iff it is a plain shapeable cell AND is not a
        // per-frame break column (cursor under CursorDisabled).
        let mut shapeable = vec![false; cols];
        for (c, sh) in shapeable.iter_mut().enumerate().take(cols.min(cells.len())) {
            let has_cluster = cluster_for(row_clusters, c).is_some();
            let img = image_covers(row_images, c);
            *sh = ligature_shaping::cell_is_shapeable(&cells[c], has_cluster, img)
                && !break_cols.contains(&c);
        }
        // `plan_row_runs` borrows `cells` and shapes via a closure; the shape
        // closure needs `&mut self`, so collect runs first, shape them, then plan.
        // Two-phase to satisfy the borrow checker without cloning the grid: pass a
        // closure that captures a shaping buffer keyed off a RefCell-free plan by
        // shaping inline using a raw-bytes copy is avoided — instead we resolve
        // each run through the cache via an owned bytes handle.
        let style_of = |c: usize| cell_style(&cells[c]);
        // Shape via the cache: clone the rb bytes handle out so the closure does
        // not borrow `self` while `plan_row_runs` borrows `cells`.
        let rb = self.rb_primary_bytes.clone();
        let mut newly_shaped: Vec<(ShapedRunKey, ShapedRunGlyphs)> = Vec::new();
        let cache = &self.shaped_runs;
        // Borrow the feature array resolved once at config time — no per-run alloc
        // or scan of `font_features`. Empty user features => the base [liga, calt].
        let features = &self.resolved_features;
        ligature_shaping::plan_row_runs(
            cells,
            cols,
            &shapeable,
            style_of,
            |run, run_chars, style| {
                let ck = (Box::<str>::from(run), style);
                if let Some(c) = cache.get(&ck) {
                    return c.clone();
                }
                let res = rb.as_ref().and_then(|b| {
                    ligature_shaping::shape_ligature_run(b, run, run_chars, true, features)
                });
                newly_shaped.push((ck, res.clone()));
                res
            },
            out,
        );
        // Persist freshly shaped runs into the cache for later frames.
        for (k, v) in newly_shaped {
            self.shaped_runs.entry(k).or_insert(v);
        }
    }

    /// The glyph key for a ligated column: a `mono_gid` coverage glyph at the
    /// run's SGR `style`. Shared so the GPU atlas keys it identically to the CPU.
    pub fn ligature_key(&self, gid: u16, style: StyleBits) -> GlyphKey {
        GlyphKey::mono_gid(gid, style, self.px_q)
    }

    /// The columns of row `r` forced PER-CELL this frame for ligature purposes
    /// (the cursor cell under `LigatureMode::CursorDisabled`). The GPU planner
    /// calls this so its break set matches the CPU's — preserving parity.
    pub fn ligature_break_cols_for_row(&self, input: &RenderInput, r: usize) -> Vec<usize> {
        ligature_break_cols(input, r, &self.shaping)
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

    /// Pre-rasterize printable ASCII (U+0020..=U+007E) into the glyph cache so the
    /// FIRST frame's atlas build pulls warm `GlyphImage`s instead of rasterizing on
    /// the hot path. Intended to run OFF the critical path (the GPU backend spawns
    /// it on the same background font thread that builds the renderer), so it adds
    /// no serial cold-start time. Produces byte-identical glyphs to on-demand
    /// rasterization — it only fills the cache early; rendered output is unchanged.
    /// Box-drawing/block are procedural (free) and so are not warmed here.
    pub fn prewarm_ascii(&mut self) {
        for ch in '\u{20}'..='\u{7E}' {
            let key = self.glyph_key(ch);
            let _ = self.glyph_image(key);
        }
    }

    /// Rasterize `key` from its source face. Keys made by [`Self::glyph_key`]
    /// always carry this renderer's own `px_q`; rasterization uses the exact
    /// `px` float the renderer was built with (no quantization round-trip), so
    /// metrics are bit-identical to a direct `fontdue` rasterization.
    fn rasterize(&mut self, key: GlyphKey) -> GlyphImage {
        debug_assert_eq!(
            key.px_q, self.px_q,
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
                // A monochromatized colour glyph: the sbix bitmap's alpha
                // silhouette as foreground-tinted coverage (the last-resort
                // default-text path). Falls through to the primary `.notdef` if
                // the colour face/bitmap is gone.
                if key.source == FaceId::ColorEmojiMono
                    && let Some(img) = self.rasterize_color_emoji_mono(ch)
                {
                    return img;
                }
                let face = match key.source {
                    // ColorEmoji never carries a Mono class (it rasterizes via the
                    // Rgba arm); ColorEmojiMono is handled just above. Cover both:
                    // fail safe to the primary face (`.notdef`).
                    FaceId::Primary
                    | FaceId::Procedural
                    | FaceId::ColorEmoji
                    | FaceId::ColorEmojiMono => &self.font,
                    FaceId::Fallback => {
                        self.ensure_fallback();
                        // Recover the chain entry that covered `ch` (recorded by
                        // `fallback_has` when this key was built). Fail safe to the
                        // first chain face, then the primary (`.notdef`).
                        self.fallback_pick
                            .get(&ch)
                            .and_then(|&i| self.fallback_chain.get(i))
                            .or_else(|| self.fallback_chain.first())
                            .map(|f| f.as_ref())
                            .unwrap_or(&self.font)
                    }
                    FaceId::SymbolFallback => {
                        self.ensure_symbol_fallback();
                        self.symbol_fallback.as_deref().unwrap_or(&self.font)
                    }
                    // A runtime-discovered fallback face (M3 FONT-DISCOVERY): the
                    // per-code-point decision was cached by `glyph_key` before this
                    // key was built, so `face_for` recovers the exact face. Fail
                    // safe to the primary (`.notdef`) if the decision is somehow
                    // absent (it never is on the path that produces this key).
                    FaceId::RuntimeFallback => {
                        self.runtime_fallback.face_for(ch).unwrap_or(&self.font)
                    }
                };
                let (m, mut bytes) = face.rasterize(ch, self.px);
                // CRISPNESS: stem-darken the coverage before it is blended.
                // The blend is `out = fg*cov + bg*(1-cov)` in raw sRGB (the CPU
                // `blend` and the GPU `ALPHA_BLENDING`, kept byte-identical). On a
                // dark background that linear-in-sRGB mix makes the antialiased
                // EDGE texels (mid coverage) optically too faint, so light-on-dark
                // stems read thin/muddy. Applying a sub-1 gamma to the coverage
                // here lifts those mid texels, thickening the perceived stem the
                // way macOS font-smoothing / Ghostty's `font-thicken` do. Because
                // it edits the SHARED coverage bytes (the GPU atlas pulls these
                // exact bytes), CPU and GPU stay identical — no blend-math change.
                stem_darken(&mut bytes, &self.stem_lut);
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
                self.rasterize_color_emoji_gid(gid)
                    .unwrap_or_else(empty_rgba)
            }
            GlyphClass::MonoGid => {
                // `ch_or_id` is a PRIMARY-face glyph id (a shaped ligature). Same
                // fontdue raster + stem-darken + synthetic-style pipeline as the
                // Mono arm, only addressed by glyph id, so the SHARED coverage
                // bytes keep CPU and GPU byte-identical.
                let (m, mut bytes) = self.font.rasterize_indexed(key.ch_or_id as u16, self.px);
                stem_darken(&mut bytes, &self.stem_lut);
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
        }
    }

    /// Rasterize `ch` from the colour face but as a MONOCHROME coverage glyph:
    /// pull the `sbix` PNG, scale it into a SINGLE cell (these are width-1
    /// default-text symbols, not 2-cell emoji), and keep only the ALPHA channel as
    /// 8-bit coverage. The blit then tints that silhouette with the cell
    /// foreground — so ⏺ shows as a theme-coloured circle, never the colour
    /// bitmap. `None` (→ `.notdef`) if the face/glyph/bitmap is missing.
    ///
    /// The result is an ordinary [`GlyphImage::Mono`] cached by its [`GlyphKey`],
    /// so the GPU atlas pulls the exact same bytes — CPU/GPU stay bit-identical,
    /// like every other mono glyph.
    fn rasterize_color_emoji_mono(&mut self, ch: char) -> Option<GlyphImage> {
        self.ensure_color_font();
        let bytes = self.color_font.as_deref()?;
        let face = ttf_parser::Face::parse(bytes, 0).ok()?;
        let gid = face.glyph_index(ch)?;
        // Largest strike (`u16::MAX`) — see `rasterize_color_emoji_gid` for the
        // sbix ppem dead-zone rationale; we downscale to the cell either way.
        let raster = face.glyph_raster_image(gid, u16::MAX)?;
        if !matches!(raster.format, ttf_parser::RasterImageFormat::PNG) {
            return None;
        }
        let (src, src_w, src_h) = decode_png_rgba8(raster.data)?;

        // Fit the (square) glyph into ONE cell, preserving aspect, centred.
        let box_w = self.cell_w.max(1);
        let box_h = self.cell_h.max(1);
        let scale = (box_w as f32 / src_w as f32).min(box_h as f32 / src_h as f32);
        let dst_w = ((src_w as f32 * scale).round() as usize).clamp(1, box_w);
        let dst_h = ((src_h as f32 * scale).round() as usize).clamp(1, box_h);
        let rgba = bilinear_rgba(&src, src_w, src_h, dst_w, dst_h);
        // Coverage = the alpha channel (the glyph's opacity/silhouette).
        let coverage: Vec<u8> = rgba.chunks_exact(4).map(|px| px[3]).collect();

        let xmin = ((box_w - dst_w) / 2) as i32;
        let top_inset = ((box_h as i32 - dst_h as i32) / 2).max(0);
        let ymin = self.baseline - dst_h as i32 - top_inset;
        Some(GlyphImage::Mono {
            width: dst_w,
            height: dst_h,
            xmin,
            ymin,
            advance: box_w as f32,
            bytes: coverage,
        })
    }

    /// Rasterize a single-codepoint colour emoji: map `ch` to its glyph id in
    /// the colour face, then pull + scale the bitmap. `None` if the face/glyph
    /// is missing.
    fn rasterize_color_emoji(&mut self, ch: char) -> Option<GlyphImage> {
        self.ensure_color_font();
        let bytes = self.color_font.as_deref()?;
        let gid = ttf_parser::Face::parse(bytes, 0).ok()?.glyph_index(ch)?;
        self.rasterize_color_emoji_gid(gid)
    }

    /// Rasterize a colour-emoji glyph BY glyph id (a cluster already shaped to
    /// one glyph): pull the `sbix` PNG bitmap from the colour-emoji face, decode
    /// it to RGBA8, and scale it (preserving aspect) to fit a 2-cell-wide box —
    /// emoji are full-width. Returns `None` if the bitmap is missing/undecodable.
    fn rasterize_color_emoji_gid(&mut self, gid: ttf_parser::GlyphId) -> Option<GlyphImage> {
        self.ensure_color_font();
        let bytes = self.color_font.as_deref()?;
        let face = ttf_parser::Face::parse(bytes, 0).ok()?;
        // Ask for the LARGEST strike (`u16::MAX`) so we always DOWNscale (sharper)
        // and never hit the sbix ppem dead-zone (~33–52) where COMPOSITE glyphs
        // (ZWJ family/sequence emoji like 👨‍👩‍👧‍👦) carry no strike and a
        // cell-ppem request would return None — collapsing the cluster to its base
        // codepoint. Apple strikes are 20/32/40/48/64/96/160 px.
        // `glyph_raster_image` covers sbix (Apple) AND CBDT/CBLC (Noto on Linux).
        let Some(raster) = face
            .glyph_raster_image(gid, u16::MAX)
            .filter(|r| matches!(r.format, ttf_parser::RasterImageFormat::PNG))
        else {
            // No bitmap strike: try a COLR (vector) color glyph (Twemoji/Segoe/
            // Noto-COLRv1). Rasterized to a 2-cell box, placed full-height.
            let box_w = (2 * self.cell_w).max(1);
            let box_h = self.cell_h.max(1);
            let rgba = crate::colr::rasterize_colr(&face, gid, box_w, box_h)?;
            return Some(GlyphImage::Rgba {
                width: box_w,
                height: box_h,
                xmin: 0,
                ymin: self.baseline - box_h as i32,
                advance: box_w as f32,
                bytes: rgba,
            });
        };
        let (src, src_w, src_h) = decode_png_rgba8(raster.data)?;

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
        Some(GlyphImage::Rgba {
            width: dst_w,
            height: dst_h,
            xmin,
            ymin,
            advance: box_w as f32,
            bytes: dst,
        })
    }

    /// Render a [`RenderInput`] snapshot (built by the engine via
    /// [`aterm_core::terminal::Terminal::cell_frame_into`]) to a framebuffer — no
    /// `&Terminal` borrow, so the caller can render after releasing the lock.
    ///
    /// As of REARCH A-3 the renderer is a PURE consumer of the snapshot: it never
    /// reaches into `Terminal`. The engine emits the snapshot; this is where it is
    /// painted.
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
    /// Wholesale-clear the monotonically-growing glyph caches past a generous cap.
    /// They memoize rasterized bitmaps + key/shape lookups and otherwise only grow,
    /// so a long-lived pane rendering a huge distinct-glyph set (CJK + emoji + many
    /// ligature shapes) would accrete unbounded CPU RAM. After a clear the next
    /// render re-memoizes only the ~thousands of currently-visible glyphs, so len
    /// drops far below the cap and growth restarts slowly — no per-frame thrash.
    /// Mirrors the existing RuntimeFallback decision-cap.
    fn evict_glyph_caches_if_large(&mut self) {
        const GLYPH_CACHE_CAP: usize = 16_384;
        if self.glyphs.len() <= GLYPH_CACHE_CAP {
            return;
        }
        self.glyphs.clear();
        self.keys.clear();
        self.emoji_keys.clear();
        self.styled_keys.clear();
        self.cluster_gids.clear();
        self.shaped_runs.clear();
    }

    pub fn render_input(&mut self, input: &RenderInput) -> Frame {
        // ONE rendering code path: do the damage render into the cache, then
        // clone the borrowed result into an owned Frame. The clone is the price
        // of ownership; the hot path avoids it via `render_input_cached`.
        //
        // The owned-`Frame` path is the snapshot / `read_image` / test path: it
        // returns a fresh owned buffer every call, so a window's PERSISTENT damage
        // cache buys it nothing (it never re-presents a borrow). It therefore uses
        // a THROWAWAY `WindowCpu` — a full repaint into a discarded cache —
        // byte-identical to the borrow path; only cache REUSE (not the pixels) is
        // forgone. The per-frame presentation hot path holds a persistent
        // `WindowCpu` and calls `render_input_cached(wc, ..)` directly.
        let mut wc = WindowCpu::new();
        let view = self.render_input_cached(&mut wc, input);
        Frame {
            width: view.width(),
            height: view.height(),
            pixels: view.pixels().to_vec(),
        }
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
    pub fn render_input_cached<'a>(
        &mut self,
        wc: &'a mut WindowCpu,
        input: &RenderInput,
    ) -> RenderView<'a> {
        // Bound the otherwise-unbounded glyph caches before they're consulted below.
        self.evict_glyph_caches_if_large();
        let (rows, cols) = (input.rows, input.cols);
        let (w, h) = self.frame_size(rows, cols);

        // Decide whether the cached frame can be reused, and if so which rows are
        // dirty, via the ONE shared `compute_dirty_rows` — the SAME function the
        // GPU scissored repaint consults, so the CPU and GPU dirty sets cannot
        // diverge. The full-render path is taken (and the cache rebuilt) on the
        // first frame (no cache) and on any `FullRepaint` verdict (geometry /
        // scrollback / selection change, or any double-HEIGHT row — DECDHL
        // top/bottom halves clip a 2× glyph across two row bands, so a single
        // dirty row can't be repainted in isolation without risking a seam).
        //
        // `compute_dirty_rows` compares rows/cols (which fix the pixel dims) for
        // its reusable precheck; that subsumes the old explicit `c.width == w &&
        // c.height == h` check, since `w`/`h` are a pure function of rows/cols and
        // the renderer's fixed cell metrics.
        // Take the persistent dirty scratch out of `wc` (swapping in an empty
        // Vec — no allocation) so `compute_dirty_rows` can borrow it mutably while
        // `&wc.cache` is read in the same match, and the per-row repaint loop
        // below can borrow `self` mutably for rasterization. It is restored into
        // `wc.dirty_scratch` before every return (capacity retained for reuse).
        let mut dirty = std::mem::take(&mut wc.dirty_scratch);
        let decision = match &wc.cache {
            // The cached pixel dims must also match `(w, h)`. `compute_dirty_rows`
            // already requires equal rows/cols (which fix the dims under the fixed
            // cell metrics), but guard the cached buffer explicitly so a stale-dims
            // cache can never reach the in-place repaint below.
            Some(c) if c.width == w && c.height == h => compute_dirty_rows(
                &c.input,
                input,
                c.cursor_blink_phase,
                c.cursor_style_override,
                self.cursor_blink_phase,
                self.cursor_style_override,
                &mut dirty,
            ),
            _ => DirtyDecision::FullRepaint,
        };
        let dirty_rows = match decision {
            DirtyDecision::FullRepaint => {
                wc.dirty_scratch = dirty;
                self.full_render(wc, input, w, h);
                return Self::cached_view(wc, w, h);
            }
            DirtyDecision::Rows(d) => d,
        };

        // DAMAGED PATH. Reuse the cached framebuffer in place — no allocation.
        // Take the cache out so the per-row helpers can borrow `self` mutably
        // (glyph rasterization caches mutate `self`); restored before return. The
        // per-row dirty flags now live in the `dirty` scratch local (restored to
        // `wc.dirty_scratch` before return).
        let mut cache = wc.cache.take().expect("reusable implies Some");
        debug_assert_eq!(cache.pixels.len(), w * h);

        // DIRTY-GATE: nothing to draw — no dirty rows, the cursor is in the same
        // place/state, and the blink/override is unchanged — so the cached pixels
        // are already exactly this frame. Hand them back with zero rendering. The
        // verdict is `DirtyRows::is_gate_hit`, which is exactly the shared
        // `is_unchanged_frame` predicate (both derive from `compute_dirty_rows`),
        // so the CPU gate and the GPU dirty-gate are ONE source of truth.
        let gate_hit = !dirty_rows.any_dirty
            && !dirty_rows.cursor_changed
            && !dirty_rows.blink_or_override_changed;
        debug_assert_eq!(
            gate_hit,
            is_unchanged_frame(
                &cache.input,
                cache.cursor_blink_phase,
                cache.cursor_style_override,
                input,
                self.cursor_blink_phase,
                self.cursor_style_override,
            ),
            "DirtyRows::is_gate_hit must agree with is_unchanged_frame"
        );
        if gate_hit {
            wc.cache = Some(cache);
            wc.dirty_scratch = dirty;
            return Self::cached_view(wc, w, h);
        }

        // Re-render each dirty row into the reused framebuffer: first restore its
        // band to the theme background (replicating the `vec![bg]` the full path
        // starts from for that band), then run the IDENTICAL passes 1/2/3.
        for (r, &is_dirty) in dirty.iter().enumerate() {
            if is_dirty {
                self.fill_band_bg(&mut cache.pixels, w, h, r);
                // `cache` is taken out of `wc`, so `&mut wc.image_cache` is a
                // disjoint borrow here (no RefCell needed). This borrow ends at
                // the loop's end, before `Self::cached_view(wc, …)` reborrows wc.
                self.render_row(&mut wc.image_cache, &mut cache.pixels, w, h, input, r);
            }
        }
        // Cursor overlay — the EXACT same code the full path runs.
        self.draw_cursor(&mut cache.pixels, w, h, input);

        // Refresh the cache to this frame's state, then borrow it back out.
        cache.input = input.clone();
        cache.cursor_blink_phase = self.cursor_blink_phase;
        cache.cursor_style_override = self.cursor_style_override;
        wc.cache = Some(cache);
        wc.dirty_scratch = dirty;
        Self::cached_view(wc, w, h)
    }

    /// Borrow the given window's damage cache as a [`RenderView`]. Only called
    /// right after a render has populated `wc.cache`, so the `expect` cannot fire.
    /// An associated fn (not `&self`) so the returned borrow ties to `wc`, not to
    /// the shared `Renderer` — letting one `Renderer` serve many `WindowCpu`.
    fn cached_view(wc: &WindowCpu, w: usize, h: usize) -> RenderView<'_> {
        let pixels = &wc.cache.as_ref().expect("cache populated by render").pixels;
        RenderView::Borrowed {
            width: w,
            height: h,
            pixels,
        }
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
    pub fn reset_damage_cache(wc: &mut WindowCpu) {
        wc.cache = None;
    }

    /// The full-repaint path: allocate a fresh background-filled framebuffer,
    /// render every row, draw the cursor, then cache the result for the next
    /// frame's damage tracking. Byte-identical to the original `render_input`.
    /// Leaves the rendered framebuffer in `wc.cache`; the caller borrows it
    /// back out (via [`cached_view`](Self::cached_view)) — no clone here.
    fn full_render(&mut self, wc: &mut WindowCpu, input: &RenderInput, w: usize, h: usize) {
        let rows = input.rows;
        let mut pixels = vec![self.theme.bg; w * h];
        // Take the per-window image cache out of `wc` so `render_row` can borrow it
        // mutably while `self` is borrowed for rasterization (and `wc.cache` is
        // written below). Restored before return.
        let mut ic = std::mem::take(&mut wc.image_cache);
        for r in 0..rows {
            // The whole buffer is already bg-filled, so each row's band starts
            // from the theme background exactly as the damaged path arranges.
            self.render_row(&mut ic, &mut pixels, w, h, input, r);
        }
        self.draw_cursor(&mut pixels, w, h, input);
        wc.image_cache = ic;

        wc.cache = Some(RenderCache {
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
        // The band starts `pad` px down (the top padding is part of the bg border,
        // filled once by the full render and never a row's responsibility). Each
        // band spans the FULL width, so the left/right padding columns are
        // re-established as bg here too — exactly what the full path's `vec![bg]`
        // start leaves for an unwritten edge.
        let y0 = self.pad + r * self.cell_h;
        let y1 = (self.pad + (r + 1) * self.cell_h).min(h);
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
    fn render_row(
        &mut self,
        ic: &mut ImageCache,
        pixels: &mut [u32],
        w: usize,
        h: usize,
        input: &RenderInput,
        r: usize,
    ) {
        let cols = input.cols;
        let selection = &input.selection;
        // Selection rows are live-screen coords; the viewport may be scrolled
        // back, so viewport row r shows live row (r - display_offset).
        let display_offset = input.display_offset;
        let cells = &input.cells[r];
        // Interior padding insets the grid by `pad` on every edge: a row's top is
        // `pad + r·cell_h`, and a column's left is `pad + c·cw`. With `pad == 0`
        // these collapse to the original `r·cell_h` / `c·cw` (byte-identical). The
        // pad offset flows into `anchor_y` (via `row_scale(y0, …)`) and every x
        // origin below, so glyphs, decorations, images and the cursor all shift
        // together; the freed border is the theme bg the buffer starts filled with.
        let (pad_x, pad_y) = (self.pad, self.pad);
        let y0 = pad_y + r * self.cell_h;
        let sel_row = r as i32 - display_offset;
        // DEC line size (DECDWL/DECDHL): `cw` is the on-screen cell advance,
        // `scale`/`anchor_y` drive the glyph's NEAREST enlargement + clip.
        let line_size = input.line_sizes[r];
        let cw = row_cell_w(line_size, self.cell_w);
        let (scale, anchor_y) = row_scale(line_size, y0, self.cell_h);
        // Pass 1: fill every cell rect with its background colour. Done
        // before any glyph so a wide glyph (which overflows into its
        // continuation column) isn't clobbered by that column's fill.
        // Selected cells take the (active or inactive-while-unfocused) selection bg.
        let sel_bg = self.effective_selection_bg();
        for (c, cell) in cells.iter().take(cols).enumerate() {
            // A lead cell is wide iff the NEXT cell is its continuation.
            let is_wide_lead = cells.get(c + 1).is_some_and(|n| n.wide);
            let bg = if selection.contains_cell(sel_row, c as u16, is_wide_lead, cell.wide) {
                sel_bg
            } else {
                rgb_to_u32(cell.bg)
            };
            self.fill_rect(pixels, w, h, pad_x + c * cw, y0, cw, self.cell_h, bg);
        }
        // Pass 1b: inline images (iTerm2 OSC 1337 `File=`) OVER the cell bg, BEFORE
        // glyphs — an image-covered cell paints its image tile and skips its glyph
        // (the bg still showed through any transparent pixels). Empty (and so a
        // no-op) for every image-free row, keeping the text path byte-identical.
        let row_images = &input.images[r];
        if !row_images.is_empty() {
            for &(c, ref image) in row_images {
                if c >= cols {
                    continue;
                }
                self.blit_image_cell(ic, pixels, w, h, pad_x + c * cw, y0, image);
            }
        }
        // Ligature plan for this row: which columns a programming ligature owns
        // (drawn as a shaped `mono_gid` glyph) vs the ordinary per-cell path. The
        // cursor column under `CursorDisabled` is forced per-cell so the cursor
        // never lands on a ligature glyph. Computed once per row, SHARED with the
        // GPU path so both place identical glyphs at identical columns.
        let break_cols = ligature_break_cols(input, r, &self.shaping);
        let mut plan: Vec<ColumnGlyph> = Vec::new();
        self.row_glyph_plan(input, r, &break_cols, &mut plan);
        // Pass 2: blit each glyph in the cell's foreground over the fills.
        // Wide continuation columns carry no glyph of their own. An image-covered
        // cell skips its glyph entirely (the image owns the cell) — this is the
        // image-vs-glyph precedence rule the GPU path mirrors via `drawable`.
        let row_clusters = &input.clusters[r];
        let row_combining = &input.combining[r];
        for (c, cell) in cells.iter().take(cols).enumerate() {
            if !image_covers(row_images, c) && !cell.wide && cell.ch != ' ' && !cell.ch.is_control()
            {
                let cluster = cluster_for(row_clusters, c);
                // A ligature-owned column draws the shaped primary glyph (the lead
                // cells of a ligature get the empty placeholder, the final cell the
                // wide ligature glyph); all other columns use per-cell dispatch.
                let key = match plan.get(c).copied().unwrap_or(ColumnGlyph::PerCell) {
                    ColumnGlyph::Ligated(gid) => self.ligature_key(gid, cell_style(cell)),
                    ColumnGlyph::PerCell => self.resolve_cell_key(cluster, cell),
                };
                // Selected cells floor their glyph fg against the selection bg so
                // colour-on-selection stays legible (GPU mirrors this identically).
                let fg = if selection.contains_cell(
                    sel_row,
                    c as u16,
                    cells.get(c + 1).is_some_and(|n| n.wide),
                    cell.wide,
                ) {
                    // An explicit theme selectionForeground wins; otherwise floor
                    // the SGR fg against the ACTUAL selection bg painted this frame
                    // (active or inactive) for legible contrast.
                    self.selection_fg
                        .unwrap_or_else(|| floor_selection_fg(rgb_to_u32(cell.fg), sel_bg))
                } else {
                    rgb_to_u32(cell.fg)
                };
                self.blit(pixels, w, (pad_x + c * cw) as i32, anchor_y, key, fg, scale);
                // Overlay combining diacritics (é, ñ, …) on the base. A
                // combining mark's own metrics assume the pen sits at the
                // base's advance (a large negative left bearing backs it up
                // over the base), so blitting at the cell origin drops it far
                // left. In a monospace cell the base glyph is centred in its
                // advance, so we centre the mark's INK in the cell — putting
                // the accent over the base, on CPU and GPU identically.
                if cluster.is_none()
                    && let Some(marks) = combining_for(row_combining, c)
                {
                    for &m in marks {
                        let mk = self.glyph_key(m);
                        let (gw, xmin) = {
                            let mi = self.glyph_image(mk);
                            (mi.width(), mi.xmin())
                        };
                        let cx = mark_cell_x(c, cw, gw, xmin, scale) + pad_x as i32;
                        // Combining marks keep the raw fg (unfloored) on BOTH paths
                        // so the GPU combining loop stays pixel-equivalent.
                        self.blit(pixels, w, cx, anchor_y, mk, rgb_to_u32(cell.fg), scale);
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
            let x = pad_x + c * cw;
            let dw = if cells.get(c + 1).is_some_and(|n| n.wide) {
                2 * cw
            } else {
                cw
            };
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
            // The cursor row may itself be a DEC double-size line. Inset by `pad`
            // exactly like `render_row`, so the cursor lands on its (padded) cell.
            let (pad_x, pad_y) = (self.pad, self.pad);
            let line_size = input.line_sizes[cr];
            let cw = row_cell_w(line_size, self.cell_w);
            let (scale, anchor_y) = row_scale(line_size, pad_y + cr * self.cell_h, self.cell_h);
            let (x0, y0) = (pad_x + cc * cw, pad_y + cr * self.cell_h);
            for [rx, ry, rw, rh] in cursor_rects(style, x0, y0, cw, self.cell_h) {
                self.fill_rect(pixels, w, h, rx, ry, rw, rh, self.theme.cursor);
            }
            if matches!(style, CursorStyle::BlinkingBlock | CursorStyle::SteadyBlock) {
                let cell = input.cells[cr].get(cc).copied();
                if let Some(cell) = cell
                    && !cell.wide
                    && cell.ch != ' '
                    && !cell.ch.is_control()
                {
                    // The cut-out re-draws whatever this cell actually shows in
                    // the cell bg colour. Consult the SAME ligature plan as the
                    // base pass so a block cursor over a ligature glyph cuts out
                    // the ligature glyph (not the per-cell char) — CPU/GPU parity.
                    let break_cols = ligature_break_cols(input, cr, &self.shaping);
                    let mut plan: Vec<ColumnGlyph> = Vec::new();
                    self.row_glyph_plan(input, cr, &break_cols, &mut plan);
                    let key = match plan.get(cc).copied().unwrap_or(ColumnGlyph::PerCell) {
                        ColumnGlyph::Ligated(gid) => self.ligature_key(gid, cell_style(&cell)),
                        ColumnGlyph::PerCell => {
                            let cluster = cluster_for(&input.clusters[cr], cc);
                            self.resolve_cell_key(cluster, &cell)
                        }
                    };
                    self.blit(
                        pixels,
                        w,
                        x0 as i32,
                        anchor_y,
                        key,
                        rgb_to_u32(cell.bg),
                        scale,
                    );
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

    /// Composite one inline-image cell's tile into the framebuffer (iTerm2 OSC
    /// 1337). `x0`/`y0` are the cell's top-left pixel; the cell paints the
    /// `cell_w × cell_h` tile of the footprint-scaled image at offset
    /// `(cell_col*cell_w, cell_row*cell_h)`, straight-alpha-blended over whatever
    /// the background pass already filled (so a transparent PNG shows the cell
    /// bg). The decoded+scaled image is cached by the payload `Arc` identity +
    /// footprint size, so it is decoded at most once per distinct placement.
    #[allow(clippy::too_many_arguments)]
    fn blit_image_cell(
        &self,
        ic: &mut ImageCache,
        px: &mut [u32],
        w: usize,
        h: usize,
        x0: usize,
        y0: usize,
        image: &aterm_core::grid::extra::ImageRef,
    ) {
        let cw = self.cell_w;
        let ch = self.cell_h;
        let fp_w = image.image.cols as usize * cw;
        let fp_h = image.image.rows as usize * ch;
        let key = (std::sync::Arc::as_ptr(&image.image) as usize, fp_w, fp_h);
        // Decode + scale on the first cell of a new placement; reuse thereafter.
        // The decoded cache is the PER-WINDOW `WindowCpu::image_cache`, threaded
        // in as `ic` (so the shared `Renderer` needs only `&self` here).
        if ic.get(key).is_none() {
            let rgba =
                decode_image_to_footprint(&image.image.bytes, image.image.format, fp_w, fp_h)
                    .unwrap_or_default();
            ic.put(
                key,
                DecodedImage {
                    w: fp_w,
                    h: fp_h,
                    rgba,
                },
            );
        }
        let Some(decoded) = ic.get(key) else { return };
        if decoded.rgba.is_empty() || decoded.w == 0 {
            // Decode failed (cached negative): draw nothing, bg shows through.
            return;
        }
        // Source origin for THIS cell's tile within the footprint-scaled image.
        let sx0 = image.cell_col as usize * cw;
        let sy0 = image.cell_row as usize * ch;
        for dy in 0..ch {
            let py = y0 + dy;
            if py >= h {
                break;
            }
            let sy = sy0 + dy;
            if sy >= decoded.h {
                break;
            }
            for dx in 0..cw {
                let pxx = x0 + dx;
                if pxx >= w {
                    break;
                }
                let sx = sx0 + dx;
                if sx >= decoded.w {
                    break;
                }
                let si = (sy * decoded.w + sx) * 4;
                let (r, g, b, a) = (
                    decoded.rgba[si],
                    decoded.rgba[si + 1],
                    decoded.rgba[si + 2],
                    decoded.rgba[si + 3],
                );
                let di = py * w + pxx;
                // Straight-alpha over: out = src*a + dst*(1-a). `blend` mixes by
                // coverage t/255, exactly the form we want for the src colour.
                px[di] = blend(px[di], rgb_to_u32([r, g, b]), a);
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
    row_clusters
        .iter()
        .find(|(c, _)| *c == col)
        .map(|(_, s)| s.as_ref())
}

/// Find the combining marks for column `col` in a row's sparse combining list.
fn combining_for(row_combining: &[(usize, Box<[char]>)], col: usize) -> Option<&[char]> {
    row_combining
        .iter()
        .find(|(c, _)| *c == col)
        .map(|(_, m)| m.as_ref())
}

/// Whether column `col` is covered by an inline image in a row's sparse image
/// list (`RenderInput::images[row]`). An image-covered cell skips its glyph — the
/// image-vs-glyph precedence rule shared with the GPU path. Rows almost never
/// have images, so the linear scan is over an empty slice on the hot path.
fn image_covers(row_images: &[(usize, aterm_core::grid::extra::ImageRef)], col: usize) -> bool {
    // Only an image drawn OVER the text (z_index >= 0, the default) hides the cell's
    // glyph. A Kitty `z < 0` image draws BEHIND the text, so the glyph still paints
    // on top (and the cell stays ligature-shapeable). Matches the GPU path's
    // `RenderInput::image_hides_glyph_at`.
    row_images
        .iter()
        .any(|(c, r)| *c == col && r.image.z_index >= 0)
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
/// resolved cells, the sparse cluster / combining-mark lists, the DEC line size,
/// or the row's inline-image placements. (Selection and display-offset are
/// frame-global and gate the whole damaged path, so they are not re-checked per
/// row.) Equality here is exact, so an unchanged row is provably safe to reuse
/// from the cache.
///
/// The image comparison matters now that BOTH renderers draw image PIXELS (the
/// GPU pixel pass): an image cell's content lives in the sparse `images` list,
/// NOT the `RenderCell`, so without this clause an image that appeared/changed
/// without a cell edit would not mark its rows dirty and the dirty/gate paths
/// would skip repainting it. The common ASCII row carries an empty `images[r]`,
/// so this is a cheap `Vec::is_empty`-fast `==` there (no allocation, no scan).
fn row_differs(a: &RenderInput, b: &RenderInput, r: usize) -> bool {
    a.cells[r] != b.cells[r]
        || a.clusters[r] != b.clusters[r]
        || a.combining[r] != b.combining[r]
        || a.line_sizes[r] != b.line_sizes[r]
        || a.images[r] != b.images[r]
}

/// Mark row `r` dirty if it is in range (a no-op for an out-of-range cursor row).
fn mark(dirty: &mut [bool], r: usize) {
    if let Some(slot) = dirty.get_mut(r) {
        *slot = true;
    }
}

/// The verdict of [`compute_dirty_rows`]: whether the frame can reuse the prior
/// frame's pixels row-by-row, and if so, exactly which rows differ.
///
/// This is THE single source of truth for the dirty row set, shared by the CPU
/// damage path ([`Renderer::render_input_cached`]) and the GPU scissored repaint
/// ([`aterm_gpu::GpuRenderer::present_input`]) so the two CANNOT diverge: a row
/// the CPU repaints is exactly a row the GPU re-encodes, and a row either skips
/// is provably pixel-identical to the prior frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DirtyDecision {
    /// The frame is NOT reusable: geometry / scrollback / selection changed, or
    /// either frame has a double-HEIGHT row (a DECDHL glyph spans two row bands,
    /// so per-row reuse risks a seam). Caller must do a FULL repaint — the always-
    /// correct path. (The "first frame, no prior" case is handled by the caller,
    /// which passes no prior input and treats it as `FullRepaint`.)
    FullRepaint,
    /// The frame IS reusable. `dirty[r]` is true iff row `r` must be repainted:
    /// any render-relevant per-row difference UNION the previous/current cursor
    /// rows (when shown). All-false with `!blink_or_override_changed` means the
    /// frame is pixel-identical to the prior one (a gate hit — nothing to draw).
    Rows(DirtyRows),
}

/// The reusable-frame auxiliary flags both the CPU gate and the GPU scissor read.
/// Fields mirror the inline computation that used to live in
/// [`Renderer::render_input_cached`].
///
/// The per-row `dirty` flags themselves do NOT live here: [`compute_dirty_rows`]
/// writes them into a CALLER-OWNED `&mut Vec<bool>` scratch (resized + reset to
/// false, reusing its capacity) so a stable-dimension changed frame allocates no
/// dirty Vec. Each present path keeps that scratch resident across frames; this
/// struct carries only the small `Copy` verdict flags alongside it. The dirty set
/// (and therefore the decision logic) is byte-identical to the old owned-`Vec`
/// form — only the allocation lifetime changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirtyRows {
    /// Whether ANY row's render-relevant inputs differ (cursor rows excluded —
    /// this is `row_differs` only). Mirrors the old `any_dirty`.
    pub any_dirty: bool,
    /// Whether the cursor's shown-ness / position / effective style changed.
    pub cursor_changed: bool,
    /// Whether the blink phase OR the cursor-style override changed between the
    /// two frames. Even with no dirty rows + no cursor change, a blink/override
    /// flip means the prior frame was drawn under a DIFFERENT renderer state, so
    /// the gate must NOT fire (the conservative path re-renders — a no-op here,
    /// but it keeps the gate predicate exact). Mirrors the old equality checks.
    pub blink_or_override_changed: bool,
}

impl DirtyRows {
    /// True iff the frame is pixel-identical to the prior one: no dirty row, no
    /// cursor change, and the same blink/override state. This is exactly the
    /// CPU/GPU gate-hit condition.
    #[must_use]
    pub fn is_gate_hit(&self) -> bool {
        !self.any_dirty && !self.cursor_changed && !self.blink_or_override_changed
    }
}

/// THE shared dirty-row computation. Decides whether `input` (to be drawn at
/// `cur_blink_phase` / `cur_cursor_style_override`) can reuse the frame previously
/// rendered from `prev_input` (at `prev_blink_phase` / `prev_cursor_style_
/// override`) row-by-row, and if so, exactly which rows differ.
///
/// The result is consumed by BOTH renderers:
///   * the CPU damage path repaints the `dirty` rows into its cached framebuffer,
///   * the GPU scissored path re-encodes ONLY the `dirty` rows into the persistent
///     offscreen (LoadOp::Load + a scissor over the dirty band(s)),
///
/// so they share one dirty set and cannot drift. `FullRepaint` is the always-safe
/// fallback (whole-frame Clear + all rows), taken whenever reuse is unsafe.
///
/// Caller responsibility: the FIRST frame (no prior input) is treated as
/// `FullRepaint` by the caller — this function assumes a prior frame exists.
///
/// `dirty` is a CALLER-OWNED scratch buffer the per-row repaint flags are written
/// into: it is resized to `rows` and reset to `false` (reusing its existing
/// capacity), so a stable-dimension changed frame allocates nothing here. On a
/// [`DirtyDecision::Rows`] verdict `dirty[r]` is the repaint flag for row `r`
/// (the same set the old owned-`Vec` form returned); on `FullRepaint` the scratch
/// is left untouched (the caller repaints every row). Each present path keeps the
/// scratch resident across frames so the dirty Vec is allocated at most once.
pub fn compute_dirty_rows(
    prev_input: &RenderInput,
    input: &RenderInput,
    prev_blink_phase: bool,
    prev_cursor_style_override: Option<CursorStyle>,
    cur_blink_phase: bool,
    cur_cursor_style_override: Option<CursorStyle>,
    dirty: &mut Vec<bool>,
) -> DirtyDecision {
    let (rows, cols) = (input.rows, input.cols);

    // REUSABLE precheck — IDENTICAL to `render_input_cached`'s `reusable` and to
    // `is_unchanged_frame`'s clause 1: same geometry (rows/cols fix the pixel
    // dims), same scrollback offset, same selection, and NO double-HEIGHT row in
    // either frame. A double-height (DECDHL) glyph clips a 2× glyph across two row
    // bands, so a single dirty row can't be repainted in isolation without risking
    // a seam — any double-height row forces the full path. (DECDWL double-WIDTH is
    // safe: it stays within one row band, so it rides the normal per-row path.)
    let reusable = prev_input.rows == rows
        && prev_input.cols == cols
        && prev_input.display_offset == input.display_offset
        && prev_input.selection == input.selection
        && !any_double_height(&prev_input.line_sizes)
        && !any_double_height(&input.line_sizes);
    if !reusable {
        return DirtyDecision::FullRepaint;
    }

    // The dirty row set: any row whose render-relevant inputs differ from the
    // prior frame, UNION the previous and current cursor rows (only when the
    // cursor is/was actually shown — an invisible cursor paints nothing). This is
    // byte-for-byte the computation `render_input_cached` used inline.
    //
    // Reset the caller-owned scratch to `rows` × `false`, REUSING its capacity:
    // `clear()` keeps the backing allocation, then `resize(rows, false)` refills
    // (and grows only if `rows` exceeded the retained capacity). A stable-dims
    // frame allocates nothing — byte-identical to the old `vec![false; rows]`.
    dirty.clear();
    dirty.resize(rows, false);
    let mut any_dirty = false;
    for (r, d) in dirty.iter_mut().enumerate() {
        if row_differs(input, prev_input, r) {
            *d = true;
            any_dirty = true;
        }
    }
    // Cursor: where it was last frame and where it is this frame. Use the SAME
    // shown-test the overlay uses (effective style = override ?? DECSCUSR, gated
    // by DECTCEM + blink phase).
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
    if prev_shown {
        mark(dirty, prev_input.cursor_row);
    }
    if cur_shown {
        mark(dirty, input.cursor_row);
    }

    let blink_or_override_changed = prev_blink_phase != cur_blink_phase
        || prev_cursor_style_override != cur_cursor_style_override;

    DirtyDecision::Rows(DirtyRows {
        any_dirty,
        cursor_changed,
        blink_or_override_changed,
    })
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
    // Delegate to the ONE shared dirty-row computation so this predicate and the
    // CPU/GPU dirty sets cannot diverge: the frame is unchanged iff it is reusable
    // (clause 1) AND every row matches with no cursor/blink/override change (a
    // gate hit). `FullRepaint` (not reusable) is never an unchanged frame.
    //
    // `compute_dirty_rows` now writes its per-row flags into a caller scratch.
    // This predicate inspects only the verdict flags (`is_gate_hit`), never the
    // rows, so it borrows a thread-local scratch (reused across calls — no
    // per-call allocation) purely to satisfy the signature. The flags — and thus
    // the predicate — are byte-identical to the prior owned-`Vec` form.
    thread_local! {
        static UNCHANGED_SCRATCH: std::cell::RefCell<Vec<bool>> =
            const { std::cell::RefCell::new(Vec::new()) };
    }
    UNCHANGED_SCRATCH.with_borrow_mut(|dirty| {
        match compute_dirty_rows(
            prev_input,
            input,
            prev_blink_phase,
            prev_cursor_style_override,
            cur_blink_phase,
            cur_cursor_style_override,
            dirty,
        ) {
            DirtyDecision::FullRepaint => false,
            DirtyDecision::Rows(d) => d.is_gate_hit(),
        }
    })
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
    pub const NORMAL: Scale = Scale {
        xs: 1,
        ys: 1,
        clip_y0: i32::MIN,
        clip_y1: i32::MAX,
    };
}

/// The on-screen scale + glyph anchor for a row of `line_size` at top pixel
/// `y0` with cell height `ch`: returns `(Scale, anchor_y)`. DECDWL is 2× wide;
/// DECDHL is 2× both with the dest clipped to this row — its bottom half anchors
/// one row up so the lower portion of the doubled glyph lands here.
pub fn row_scale(line_size: LineSize, y0: usize, ch: usize) -> (Scale, i32) {
    let (y0, ch) = (y0 as i32, ch as i32);
    match line_size {
        LineSize::DoubleWidth => (
            Scale {
                xs: 2,
                ys: 1,
                ..Scale::NORMAL
            },
            y0,
        ),
        LineSize::DoubleHeightTop => (
            Scale {
                xs: 2,
                ys: 2,
                clip_y0: y0,
                clip_y1: y0 + ch,
            },
            y0,
        ),
        LineSize::DoubleHeightBottom => (
            Scale {
                xs: 2,
                ys: 2,
                clip_y0: y0,
                clip_y1: y0 + ch,
            },
            y0 - ch,
        ),
        // SingleWidth and any future variant: ordinary single-size.
        _ => (Scale::NORMAL, y0),
    }
}

/// The on-screen cell advance (px) for a row of `line_size` — doubled for any
/// double-width/height row, single otherwise.
pub fn row_cell_w(line_size: LineSize, cell_w: usize) -> usize {
    if matches!(line_size, LineSize::SingleWidth) {
        cell_w
    } else {
        cell_w * 2
    }
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
    let uv = [
        ax as f32 / aw,
        (ay as f32 + v_top) / ah,
        gw as f32 / aw,
        (vh / ys as f32) / ah,
    ];
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

/// Columns of row `r` that must stay PER-CELL this frame for ligature purposes:
/// under `LigatureMode::CursorDisabled` the cursor cell is excluded from any run
/// so the cursor never sits on a ligature glyph (matching the documented mode);
/// AND any column whose selected-state would make a ligature unsafe — a selected
/// cell, or a cell adjacent to a selected-state change. Without the selection
/// breaks a ligature glyph would paint across cells with DIFFERENT backgrounds
/// (selected steel-blue vs cell bg), so the highlight would be visibly wrong;
/// breaking keeps selected cells per-cell with the selection background.
/// Shared by both renderers so the break set — and therefore the plan — is
/// identical, preserving CPU/GPU parity.
fn ligature_break_cols(
    input: &RenderInput,
    r: usize,
    shaping: &aterm_types::text_shaping::TextShapingConfig,
) -> Vec<usize> {
    let mut cols: Vec<usize> = Vec::new();
    if matches!(
        shaping.ligature_mode,
        aterm_types::text_shaping::LigatureMode::CursorDisabled
    ) && input.cursor_visible
        && input.cursor_row == r
        && input.cursor_col < input.cols
    {
        cols.push(input.cursor_col);
    }
    // Break on selection so no shaped run spans a selection-highlight boundary.
    // Selection rows are live-screen coords (viewport row r shows live row
    // r - display_offset), matching `render_row`'s per-cell selection fill.
    if input.selection.has_selection() {
        let n = input.cols.min(input.cells.get(r).map_or(0, Vec::len));
        let row_cells = &input.cells[r];
        let sel_row = r as i32 - input.display_offset;
        let selected = |c: usize| {
            let cell = &row_cells[c];
            let is_wide_lead = row_cells.get(c + 1).is_some_and(|next| next.wide);
            input
                .selection
                .contains_cell(sel_row, c as u16, is_wide_lead, cell.wide)
        };
        let mut prev = false;
        for c in 0..n {
            let sel = selected(c);
            // A selected cell stays per-cell; a state change between adjacent
            // columns also breaks so the run never straddles the boundary.
            if sel || sel != prev {
                cols.push(c);
            }
            prev = sel;
        }
    }
    cols
}

/// The synthetic-style bits for a cell (SGR 1 bold / SGR 3 italic). Public so the
/// GPU instance builder keys ligature glyphs with the same style as the CPU.
pub fn cell_style(cell: &RenderCell) -> StyleBits {
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
fn apply_synthetic_style(
    style: StyleBits,
    w: usize,
    h: usize,
    bytes: Vec<u8>,
    px: f32,
) -> (usize, Vec<u8>) {
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

/// sRGB EOTF — gamma-encoded channel (0..1) → linear-light (0..1), the PROPER
/// piecewise curve (not a bare `pow(2.2)`, which is visibly wrong near black
/// where terminal text edges live).
fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Inverse sRGB OETF — linear-light (0..1) → gamma-encoded channel (0..1).
fn linear_to_srgb(l: f32) -> f32 {
    if l <= 0.003_130_8 {
        12.92 * l
    } else {
        1.055 * l.powf(1.0 / 2.4) - 0.055
    }
}

/// Gamma-encoded (Y′) luma of a `0x00RRGGBB` colour, 0..1 — a linear combination
/// of the sRGB-encoded channels (Rec.709 weights), which is exactly how the
/// renderer's per-channel sRGB-space coverage lerp combines luma.
fn luma_srgb(rgb: u32) -> f32 {
    let r = ((rgb >> 16) & 0xff) as f32 / 255.0;
    let g = ((rgb >> 8) & 0xff) as f32 / 255.0;
    let b = (rgb & 0xff) as f32 / 255.0;
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// Linear-light relative luminance of a `0x00RRGGBB` colour, 0..1.
fn luma_linear(rgb: u32) -> f32 {
    let r = srgb_to_linear(((rgb >> 16) & 0xff) as f32 / 255.0);
    let g = srgb_to_linear(((rgb >> 8) & 0xff) as f32 / 255.0);
    let b = srgb_to_linear((rgb & 0xff) as f32 / 255.0);
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// The coverage gamma for the shared stem-darkening LUT that makes the renderer's
/// sRGB-space coverage lerp `out = fg·c + bg·(1−c)` (the CPU `blend` and the GPU
/// `ALPHA_BLENDING`, kept byte-identical) approximate TRUE linear-light
/// antialiased blending of the theme's fg over bg.
///
/// Antialiasing is physically a *linear-light* coverage average, but the renderer
/// composites raw sRGB bytes. On a dark background that makes mid-coverage edge
/// texels optically far too faint (a 50%-covered white stem on black displays at
/// ~22% luminance, not ~50%), so light-on-dark text reads thin/washed-out — the
/// "basic/ugly" complaint. Warping coverage by `c → c^γ` before the lerp corrects
/// it: we pick the single γ whose warp reproduces, at half coverage, the displayed
/// luma a real linear-light blend would have. For the default dark theme γ≈0.50
/// (was a fixed 0.65 — too weak, so text stayed thin); for a light theme the same
/// fit yields γ>1, which correctly *thins* dark-on-light text the old fixed lift
/// wrongly over-darkened. Endpoints stay exact (0→0, 255→255). `1.0` is a no-op.
fn stem_gamma_for_theme(fg: u32, bg: u32) -> f32 {
    // Escape hatch / tuning knob: an explicit `ATERM_STEM_GAMMA` overrides the
    // computed value (clamped to the same safe range). <1 thickens text, >1 thins
    // it; `1.0` disables the warp entirely.
    if let Ok(v) = std::env::var("ATERM_STEM_GAMMA")
        && let Ok(g) = v.trim().parse::<f32>()
        && g.is_finite()
    {
        return g.clamp(0.30, 3.0);
    }
    let yfg_s = luma_srgb(fg);
    let ybg_s = luma_srgb(bg);
    // Displayed luma of a true linear-light blend at half coverage.
    let target = linear_to_srgb(0.5 * luma_linear(fg) + 0.5 * luma_linear(bg));
    let denom = yfg_s - ybg_s;
    if denom.abs() < 1e-3 {
        return 1.0; // fg≈bg in luma: degenerate/low-contrast — no warp.
    }
    // Coverage c′ the sRGB-space lerp needs to hit `target`; γ from c′ = 0.5^γ.
    let cprime = ((target - ybg_s) / denom).clamp(0.05, 0.95);
    (cprime.ln() / 0.5_f32.ln()).clamp(0.45, 2.4)
}

/// Build a per-value stem-darkening LUT: `LUT[c] = round(255·(c/255)^gamma)`.
/// A 256-entry table keeps the hot path a single byte lookup (no per-texel
/// `powf`). Endpoints are EXACT (`LUT[0] == 0`, `LUT[255] == 255`) so fully-empty
/// and fully-covered texels are untouched — only the antialiased fringe shifts.
fn build_stem_lut(gamma: f32) -> [u8; 256] {
    let mut t = [0u8; 256];
    for (c, slot) in t.iter_mut().enumerate() {
        let v = (c as f32 / 255.0).powf(gamma) * 255.0;
        *slot = v.round().clamp(0.0, 255.0) as u8;
    }
    t
}

/// Stem-darken a coverage bitmap in place: remap every texel through `lut` (the
/// renderer's theme-derived [`build_stem_lut`]). See the call site in `rasterize`
/// for why (crisper, correctly-weighted text under the sRGB coverage blend).
/// Endpoints are fixed, so a hard-edged glyph (all 0/255) is unchanged — only
/// antialiased texels move.
fn stem_darken(cov: &mut [u8], lut: &[u8; 256]) {
    for c in cov.iter_mut() {
        *c = lut[*c as usize];
    }
}

/// A zero-sized colour glyph: the blit treats it as a no-op (like a space).
/// Returned when a colour-emoji bitmap is missing or undecodable.
fn empty_rgba() -> GlyphImage {
    GlyphImage::Rgba {
        width: 0,
        height: 0,
        xmin: 0,
        ymin: 0,
        advance: 0.0,
        bytes: Vec::new(),
    }
}

/// Pack an `[r, g, b]` triple into the framebuffer's `0x00RRGGBB` format.
pub fn rgb_to_u32([r, g, b]: [u8; 3]) -> u32 {
    (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)
}

/// Per-value sRGB→linear lookup, built once: `LUT[c]` = the WCAG-linearized
/// value of the 8-bit channel `c`. A 256-entry table keeps `relative_luminance`
/// (hot in the selection-contrast floor, called per selected glyph) three table
/// reads instead of three `powf`. Endpoints are exact by construction.
fn srgb_linear_lut() -> &'static [f32; 256] {
    use std::sync::OnceLock;
    static LUT: OnceLock<[f32; 256]> = OnceLock::new();
    LUT.get_or_init(|| {
        let mut t = [0.0f32; 256];
        for (c, slot) in t.iter_mut().enumerate() {
            let n = c as f32 / 255.0;
            *slot = if n <= 0.03928 {
                n / 12.92
            } else {
                ((n + 0.055) / 1.055).powf(2.4)
            };
        }
        t
    })
}

/// sRGB relative luminance (WCAG) of a `0x00RRGGBB` colour.
fn relative_luminance(rgb: u32) -> f32 {
    let lut = srgb_linear_lut();
    let chan = |c: u32| -> f32 { lut[(c & 0xff) as usize] };
    0.2126 * chan(rgb >> 16) + 0.7152 * chan(rgb >> 8) + 0.0722 * chan(rgb)
}

/// WCAG contrast ratio (>= 1) between two `0x00RRGGBB` colours.
fn contrast_ratio(a: u32, b: u32) -> f32 {
    let (la, lb) = (relative_luminance(a), relative_luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// Floor a glyph foreground's contrast against the SELECTION background so selected
/// text stays legible even when its SGR colour is close to the selection colour
/// (xterm's minimumContrastRatio, applied to the selection band). Blends the fg
/// toward black/white — whichever the selection bg contrasts with — by the smallest
/// step that clears ~4.5:1, preserving hue where possible. Identical on the CPU and
/// GPU paths (same inputs) so selection rendering stays pixel-equivalent.
pub fn floor_selection_fg(fg: u32, selection_bg: u32) -> u32 {
    const MIN: f32 = 4.5;
    if contrast_ratio(fg, selection_bg) >= MIN {
        return fg;
    }
    let target: u32 = if relative_luminance(selection_bg) > 0.5 {
        0x0000_0000
    } else {
        0x00ff_ffff
    };
    let mix = |fg: u32, shift: u32, t: f32| -> u32 {
        let f = ((fg >> shift) & 0xff) as f32;
        let g = ((target >> shift) & 0xff) as f32;
        ((f + (g - f) * t).round() as u32) & 0xff
    };
    let mut step = 1u32;
    while step <= 10 {
        let t = step as f32 / 10.0;
        let cand = (mix(fg, 16, t) << 16) | (mix(fg, 8, t) << 8) | mix(fg, 0, t);
        if contrast_ratio(cand, selection_bg) >= MIN {
            return cand;
        }
        step += 1;
    }
    target
}

/// Derive a default INACTIVE (unfocused) selection background from the active
/// selection bg and the theme bg, when the host supplies no explicit
/// `selectionInactiveBackground`. xterm dims the band when the pane loses focus;
/// we reproduce that by blending the active selection colour HALFWAY toward the
/// theme background (a real computed midpoint, not a magic constant), so the band
/// stays visibly a selection yet recedes when unfocused. With the default theme
/// (`selection 0x33415E` over `bg 0x111318`) this yields `0x222A3B`. Pure +
/// deterministic, so the CPU fill and the GPU encode derive the identical colour.
#[must_use]
pub fn derive_inactive_selection_bg(active_selection: u32, theme_bg: u32) -> u32 {
    // 50% coverage of the active selection over the theme bg — the midpoint, the
    // dim xterm uses for an unfocused band. `blend(bg, fg, 128)` ~= halfway.
    blend(theme_bg, active_selection, 128)
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

/// Per-axis pixel cap for PNG decode. Mirrors the sixel renderer's
/// `SIXEL_MAX_DIMENSION` (4096): a remote peer can stream an inline-image PNG
/// (iTerm2 OSC 1337) whose IHDR declares enormous dimensions in a tiny file, so
/// we reject anything past this on either axis BEFORE allocating the output
/// buffer to avoid a multi-GB allocation DoS.
const IMAGE_MAX_DIMENSION: u32 = 4096;

/// Total-allocation cap handed to the `png` decoder (64 MiB). This bounds the
/// decoder's *intermediate* buffers; the `png` crate explicitly excludes our own
/// pre-allocated output buffer from this limit, which is why the dimension check
/// below is the load-bearing guard against the IHDR-dimension allocation bomb.
const PNG_DECODE_BYTE_LIMIT: usize = 64 * 1024 * 1024;

/// Decode an 8-bit PNG to packed RGBA8, returning `(rgba, width, height)`.
///
/// Hardened against the inline-image allocation-bomb DoS: a tiny PNG can declare
/// huge IHDR dimensions and force a multi-GB output buffer. We (1) cap the
/// decoder's intermediate allocations via `png::Limits`, and (2) reject either
/// axis past `IMAGE_MAX_DIMENSION` after parsing IHDR but BEFORE allocating the
/// output buffer. Non-8-bit depths and a pixel count that overflows or exceeds
/// the byte budget also bail (returning `None`) — never panic, never allocate.
fn decode_png_rgba8(bytes: &[u8]) -> Option<(Vec<u8>, usize, usize)> {
    let mut decoder = png::Decoder::new(bytes);
    decoder.set_limits(png::Limits {
        bytes: PNG_DECODE_BYTE_LIMIT,
    });
    // EXPAND palette (Indexed) → RGB(A) and apply tRNS transparency; STRIP_16 folds
    // any 16-bit channel to 8-bit. Noto Color Emoji's CBDT strikes are INDEXED PNGs
    // (palette + tRNS) — without EXPAND `to_rgba8` can't read them and every emoji
    // renders blank. RGB/RGBA PNGs (Apple sbix) are unaffected (EXPAND is a no-op
    // for them, save honouring a tRNS chunk).
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    // IHDR is parsed by `read_info`; reject oversized dims before the alloc.
    let (w, h) = (reader.info().width, reader.info().height);
    if w == 0 || h == 0 || w > IMAGE_MAX_DIMENSION || h > IMAGE_MAX_DIMENSION {
        return None;
    }
    // Bound the output allocation by the byte budget too, with checked math so a
    // pixel-count overflow bails instead of wrapping.
    let pixels = (w as usize).checked_mul(h as usize)?;
    let out_bytes = pixels.checked_mul(4)?;
    if out_bytes > PNG_DECODE_BYTE_LIMIT {
        return None;
    }
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    if info.bit_depth != png::BitDepth::Eight {
        return None;
    }
    let (src_w, src_h) = (info.width as usize, info.height as usize);
    let rgba = to_rgba8(&buf[..info.buffer_size()], info.color_type, src_w, src_h)?;
    Some((rgba, src_w, src_h))
}

/// Convert a decoded 8-bit PNG buffer to packed RGBA8. After [`decode_png_rgba8`]'s
/// EXPAND transform, palette images arrive as RGB/RGBA, so the cases here are RGBA
/// (Apple sbix, expanded Noto Color Emoji), RGB, and grayscale (±alpha) for any
/// monochrome strike; anything else returns `None`.
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
        png::ColorType::GrayscaleAlpha => {
            if buf.len() < n * 2 {
                return None;
            }
            let mut out = Vec::with_capacity(n * 4);
            for px in buf[..n * 2].chunks_exact(2) {
                out.extend_from_slice(&[px[0], px[0], px[0], px[1]]);
            }
            Some(out)
        }
        png::ColorType::Grayscale => {
            if buf.len() < n {
                return None;
            }
            let mut out = Vec::with_capacity(n * 4);
            for &g in &buf[..n] {
                out.extend_from_slice(&[g, g, g, 255]);
            }
            Some(out)
        }
        _ => None,
    }
}

/// Decode an inline-image payload (iTerm2 OSC 1337) to RGBA8 resampled to fill
/// the footprint pixel box `fp_w × fp_h`. The engine already chose the footprint
/// cell count (honoring aspect ratio), so here we simply scale the decoded image
/// to fill that box; each covered cell then paints a 1:1 tile of the result.
///
/// Returns `None` only for a format the renderer cannot decode (non-PNG) or a
/// corrupt/oversized PNG; the caller caches that as "draw nothing" so a bad image
/// degrades gracefully instead of re-decoding every frame.
///
/// Public so the GPU renderer's image pass decodes byte-identically: it uploads
/// this exact footprint RGBA into a texture and samples it NEAREST per cell, so a
/// covered cell's pixels match the CPU `blit_image_cell` 1:1 tile (the CPU/GPU
/// inline-image parity gate).
pub fn decode_image_to_footprint(
    bytes: &[u8],
    format: aterm_core::grid::extra::ImageFormat,
    fp_w: usize,
    fp_h: usize,
) -> Option<Vec<u8>> {
    if fp_w == 0 || fp_h == 0 {
        return None;
    }
    // Already-decoded RGBA8 (the sixel path): resample the stored raster to the
    // footprint directly — no container to decode. The engine guarantees the
    // byte layout (`[r, g, b, a]` per pixel, row-major over `width`), matching
    // `bilinear_rgba`'s input contract.
    if let aterm_core::grid::extra::ImageFormat::RawRgba8 { width, height } = format {
        let (w, h) = (width as usize, height as usize);
        if w == 0 || h == 0 || bytes.len() < w.checked_mul(h)?.checked_mul(4)? {
            return None;
        }
        return Some(bilinear_rgba(&bytes[..w * h * 4], w, h, fp_w, fp_h));
    }
    // Only PNG is decodable today; anything else degrades to nothing.
    if !matches!(format, aterm_core::grid::extra::ImageFormat::Png) {
        return None;
    }
    let (src, src_w, src_h) = decode_png_rgba8(bytes)?;
    Some(bilinear_rgba(&src, src_w, src_h, fp_w, fp_h))
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

// The CPU renderer as the injected `Rasterizer` (ATERM_DESIGN WS-F). Forwards to
// the inherent methods via UFCS so the trait and inherent `render_input` names
// cannot collide. The trait is `&Terminal`-free (A-3): the renderer consumes only
// the engine-built `RenderInput`.
impl Rasterizer for Renderer {
    fn cell_size(&self) -> (usize, usize) {
        Renderer::cell_size(self)
    }
    fn render_input(&mut self, input: &RenderInput) -> Frame {
        Renderer::render_input(self, input)
    }
    // `render_input_cached` is intentionally NOT overridden (mirror of S5's
    // `impl Rasterizer for GpuRenderer`): the inherent version returns a
    // `RenderView` borrowing a per-window `WindowCpu`'s damage cache, which the
    // `&Terminal`-/window-free trait signature can't thread. The trait's default
    // (`RenderView::Owned(self.render_input(input))`) is byte-identical and
    // object-safe; the CPU hot path calls the inherent
    // `render_input_cached(wc, ..)` directly, not via this trait.
    fn set_cursor_blink_phase(&mut self, on: bool) {
        Renderer::set_cursor_blink_phase(self, on)
    }
    fn set_cursor_style_override(&mut self, style: Option<CursorStyle>) {
        Renderer::set_cursor_style_override(self, style)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// END-TO-END regression for ZWJ-sequence emoji (family / couple): drive the
    /// REAL print path (`Terminal::process` → `cell_frame` → `render_input`) and
    /// assert the engine (1) GROUPS the multi-emoji ZWJ sequence into ONE cell so
    /// `cluster_row` emits the full string, and (2) resolves+rasterizes it through
    /// the COLOUR face (a non-empty `Rgba` glyph) rather than collapsing to a
    /// `.notdef` mono box.
    ///
    /// Part (2) guards the COLR coverage fix: the cluster gate
    /// (`shape_cluster_uncached`) now accepts a COLR (vector) glyph, not just a
    /// CBDT/sbix raster, so cluster emoji render on COLR-only fonts (Twemoji, modern
    /// COLRv1 Noto) instead of tofu. Whether the glyph is COLOURFUL or monochrome is
    /// a FONT property — the stock Noto CBDT renders these Unicode-deprecated
    /// sequences as monochrome silhouettes — so when the active colour font DOES
    /// carry colour for the cluster (saturated texels) we additionally assert that
    /// colour survives to the framebuffer cell (point a COLR font at it via
    /// `ATERM_EMOJI_FONT` to exercise that branch).
    #[cfg(target_os = "linux")]
    #[test]
    fn zwj_cluster_renders_through_colour_face() {
        use aterm_core::terminal::Terminal;
        let Some(mut r) = Renderer::from_system(40.0, Theme::default()) else {
            return;
        };
        let (cw, ch) = r.cell_size();
        for (name, seq) in [
            ("family", "👨\u{200d}👩\u{200d}👧"),
            ("couple", "👩\u{200d}\u{2764}\u{fe0f}\u{200d}👨"),
        ] {
            let mut term = Terminal::new(4, 20);
            term.process(seq.as_bytes());
            let input = term.cell_frame(4, 20);
            // (1) the engine grouped the sequence into ONE cell at lead col 0.
            let grouped = input.clusters[0]
                .iter()
                .find(|(c, _)| *c == 0)
                .map(|(_, s)| s.as_ref());
            assert_eq!(grouped, Some(seq), "{name}: engine must group the ZWJ sequence");
            // (2) it resolves through the COLOUR face — a non-empty Rgba glyph — not a
            //     mono `.notdef` box. (Pre-fix, a COLR-only colour font produced Mono.)
            let lead = &input.cells[0][0];
            let key = r.resolve_cell_key(Some(seq), lead);
            let GlyphImage::Rgba { bytes, .. } = r.glyph_image(key) else {
                // The active colour font has no glyph for this cluster at all; the
                // grouping check above is the font-independent guarantee.
                continue;
            };
            assert!(bytes.iter().any(|&b| b != 0), "{name}: cluster glyph must not be blank");
            // If the colour font carries COLOUR for the cluster, it must reach the cell.
            let sat = |b: &[u8]| {
                let (rr, gg, bb) = (b[0] as i32, b[1] as i32, b[2] as i32);
                b[3] > 0 && ((rr - gg).abs() > 24 || (gg - bb).abs() > 24 || (rr - bb).abs() > 24)
            };
            if bytes.chunks(4).filter(|px| sat(px)).count() > 16 {
                let frame = r.render_input(&input);
                let cell_sat = (0..ch.min(frame.height))
                    .flat_map(|y| (0..(2 * cw).min(frame.width)).map(move |x| (x, y)))
                    .filter(|&(x, y)| {
                        let p = frame.pixels[y * frame.width + x];
                        let (rr, gg, bb) =
                            ((p >> 16 & 0xff) as i32, (p >> 8 & 0xff) as i32, (p & 0xff) as i32);
                        (rr - gg).abs() > 24 || (gg - bb).abs() > 24 || (rr - bb).abs() > 24
                    })
                    .count();
                assert!(cell_sat > 8, "{name}: colour cluster must paint colour into the cell, got {cell_sat}");
            }
        }
    }
    // Tests build terminals and call `Terminal::cell_frame` to feed the renderer
    // (the engine-side snapshot path that replaced the old `Renderer::extract`).
    use aterm_core::terminal::Terminal;

    /// Regression (colour emoji on Linux): Noto Color Emoji's CBDT strikes are
    /// INDEXED (palette + tRNS) PNGs. The decoder MUST `EXPAND` them to RGBA, or
    /// every emoji rendered blank (the bug this guards). Portable: synthesizes a
    /// tiny palette PNG, no system font needed.
    #[test]
    fn decode_png_expands_indexed_palette_to_rgba() {
        let mut png_bytes = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut png_bytes, 2, 1);
            enc.set_color(png::ColorType::Indexed);
            enc.set_depth(png::BitDepth::Eight);
            // idx0 = red, idx1 = green; idx0 opaque, idx1 fully transparent.
            enc.set_palette(vec![255, 0, 0, 0, 255, 0]);
            enc.set_trns(vec![255, 0]);
            let mut w = enc.write_header().unwrap();
            w.write_image_data(&[0u8, 1u8]).unwrap();
        }
        let (rgba, w, h) = decode_png_rgba8(&png_bytes).expect("indexed PNG must decode to RGBA");
        assert_eq!((w, h), (2, 1));
        assert_eq!(&rgba[0..4], &[255, 0, 0, 255], "palette idx0 → opaque red");
        assert_eq!(rgba[7], 0, "palette idx1 (tRNS) → transparent");
    }

    /// Regression: the real Noto Color Emoji face must rasterize a non-blank colour
    /// glyph when installed (the end-to-end Linux emoji path). Skipped when the
    /// font is absent, so it never fails a host without it.
    #[cfg(target_os = "linux")]
    #[test]
    fn noto_color_emoji_rasterizes_nonblank_when_present() {
        let path = "/usr/share/fonts/truetype/noto/NotoColorEmoji.ttf";
        if !std::path::Path::new(path).is_file() {
            return;
        }
        let Some(mut r) = Renderer::from_system(16.0, Theme::default()) else {
            return;
        };
        match r.rasterize_color_emoji('😀') {
            Some(GlyphImage::Rgba { bytes, .. }) => {
                assert!(bytes.iter().any(|&b| b != 0), "colour emoji glyph must not be blank");
            }
            other => panic!("expected an RGBA colour glyph, got {other:?}"),
        }
    }

    fn renderer() -> Option<Renderer> {
        Renderer::from_system(16.0, Theme::default())
    }

    #[cfg(feature = "embedded-font")]
    #[test]
    fn embedded_font_is_valid_monospace_and_builds_a_renderer() {
        // FONT-EMBED: the bundled last-resort font must parse, cover ASCII, and
        // rasterize — so a host with no system font still renders text.
        let f = fontdue::Font::from_bytes(embedded_font(), fontdue::FontSettings::default())
            .expect("embedded DejaVu Sans Mono parses");
        assert_ne!(f.lookup_glyph_index('A'), 0, "embedded font covers 'A'");
        let (m, bitmap) = f.rasterize('A', 16.0);
        assert!(
            m.width > 0 && !bitmap.is_empty(),
            "embedded font rasterizes 'A' to coverage"
        );
        // And it drives a real Renderer (the path taken when every system candidate
        // is absent).
        assert!(
            Renderer::from_bytes(embedded_font(), 16.0, Theme::default()).is_ok(),
            "a Renderer builds from the embedded font"
        );
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
    fn decode_raw_rgba8_resamples_directly_to_footprint() {
        use aterm_core::grid::extra::ImageFormat;
        // A 2x2 opaque-red RGBA8 raster (the layout the sixel path produces:
        // [R, G, B, A] per pixel, row-major). Decoding to a 4x4 footprint must
        // succeed (no codec) and fill it with red, alpha 255.
        let mut raster = Vec::with_capacity(2 * 2 * 4);
        for _ in 0..4 {
            raster.extend_from_slice(&[0xFF, 0x00, 0x00, 0xFF]);
        }
        let out = decode_image_to_footprint(
            &raster,
            ImageFormat::RawRgba8 {
                width: 2,
                height: 2,
            },
            4,
            4,
        )
        .expect("RawRgba8 must decode without a codec");
        assert_eq!(out.len(), 4 * 4 * 4, "footprint is 4x4 RGBA");
        // Every footprint pixel is the source red, alpha preserved.
        for px in out.chunks_exact(4) {
            assert_eq!(px[0], 0xFF, "R");
            assert_eq!(px[1], 0x00, "G");
            assert_eq!(px[2], 0x00, "B");
            assert_eq!(px[3], 0xFF, "A");
        }
    }

    #[test]
    fn decode_raw_rgba8_rejects_short_buffer() {
        use aterm_core::grid::extra::ImageFormat;
        // Declared 4x4 (= 64 bytes) but only 4 bytes supplied: must return None
        // (cached as "draw nothing") rather than read out of bounds.
        let short = vec![0xFFu8; 4];
        assert!(
            decode_image_to_footprint(
                &short,
                ImageFormat::RawRgba8 {
                    width: 4,
                    height: 4
                },
                8,
                8
            )
            .is_none(),
            "a too-short RawRgba8 buffer must decode to None"
        );
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
        let frame = r.render_input(&term.cell_frame(2, 4));

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
        let frame = r.render_input(&term.cell_frame(2, 8));

        // Cell (0,0) holds the red 'R': its glyph must paint red-dominant pixels.
        let r_cell_red = cell_pixels(&frame, 0, 0, cw, ch)
            .any(|p| channels(p).0 > channels(p).1 && channels(p).0 > channels(p).2);
        assert!(
            r_cell_red,
            "expected red-dominant glyph pixels in the 'R' cell"
        );

        // Cell (0,1) has a green background fill: it must be green-dominant.
        let g_cell_green = cell_pixels(&frame, 1, 0, cw, ch)
            .any(|p| channels(p).1 > channels(p).0 && channels(p).1 > channels(p).2);
        assert!(
            g_cell_green,
            "expected green-dominant background in the 'G' cell"
        );
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
        (y0..y0 + ch)
            .flat_map(move |y| (x0..x0 + cw).map(move |x| frame.pixels[y * frame.width + x]))
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
            // Primary glyphs are addressed by their UNICODE-resolved glyph id
            // (MonoGid), bypassing fontdue's Mac-Roman char lookup; `ch_or_id` is
            // that gid. The reference rasterization is by the SAME id.
            assert_eq!(key.glyph_class, GlyphClass::MonoGid);
            let gid = r.primary_unicode_gid(ch).expect("primary covers ASCII");
            assert_eq!(key.ch_or_id, u32::from(gid));
            assert_eq!(key.style, StyleBits::REGULAR);
            assert_eq!(key.px_q, GlyphKey::quantize_px(px));
            let img = r.glyph_image(key).clone();
            let (m, direct) = font.rasterize_indexed(gid, px);
            assert_eq!(
                (img.width(), img.height(), img.xmin(), img.ymin()),
                (m.width, m.height, m.xmin, m.ymin),
                "metrics differ for {ch:?}"
            );
            assert_eq!(img.advance(), m.advance_width, "advance differs for {ch:?}");
            // Coverage is fontdue's bytes passed through the documented
            // stem-darkening LUT (and nothing else): placement/advance stay
            // bit-identical above; only the per-texel coverage values are lifted
            // by `stem_darken`. Asserting against the LUT-transformed reference
            // (rather than the raw bytes) keeps this test's intent — "the
            // renderer applies EXACTLY the documented transform" — while letting
            // the crispness pass through.
            let mut expected = direct.clone();
            stem_darken(&mut expected, &r.stem_lut);
            assert_eq!(
                img.bytes(),
                expected.as_slice(),
                "coverage bytes differ for {ch:?}"
            );
        }
    }

    // =========================================================================
    // GLYPH-FIDELITY: the formal guard that WOULD HAVE CAUGHT the Mac Roman
    // substitution bug (·→∑, é→È). The grid stores Unicode scalars; the renderer
    // must rasterize, for each cell, the font's UNICODE glyph for that scalar —
    // never a glyph from a different (Mac Roman) encoding. This is a REFINEMENT
    // property R ⊑ U (the resolver R refines the Unicode cmap U). The input
    // domain — Unicode scalars — is FINITE, so we discharge the refinement by
    // EXHAUSTIVE ENUMERATION over the whole domain, which is a complete proof,
    // NOT example-based sampling. The trusted oracle for U is ttf-parser's
    // Unicode-subtable cmap; the prove-and-catch test below adds independence by
    // demonstrating divergence from fontdue's (buggy) char lookup.
    // =========================================================================

    /// Walk EVERY Unicode scalar (0..=0x10FFFF, surrogates excluded) and prove the
    /// renderer's primary-face glyph for each covered scalar is exactly the
    /// Unicode-cmap glyph. Returns `(covered, divergence-from-fontdue)` for the
    /// caller's non-vacuity assertions.
    fn prove_faithful_over_all_scalars(bytes: &[u8], label: &str) -> (u64, u64) {
        let oracle = ttf_parser::Face::parse(bytes, 0).expect("ttf-parser parses the font");
        let fd = fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
            .expect("fontdue parses the font");
        let mut r = Renderer::from_bytes(bytes, 16.0, Theme::default()).expect("renderer");
        let (mut covered, mut fontdue_divergences) = (0u64, 0u64);
        for cp in 0u32..=0x10_FFFF {
            if (0xD800..=0xDFFF).contains(&cp) {
                continue; // surrogate halves are not scalar values
            }
            let c = char::from_u32(cp).expect("non-surrogate is a scalar");
            // The Unicode truth: the glyph the font's Unicode cmap maps c to.
            let Some(want) = oracle.glyph_index(c).map(|g| g.0).filter(|&g| g != 0) else {
                continue;
            };
            covered += 1;
            // Tally where fontdue's OWN char lookup (the pre-fix path) would have
            // chosen a different glyph — the Mac Roman substitution surface.
            let fd_gid = fd.lookup_glyph_index(c);
            if fd_gid != 0 && fd_gid != want {
                fontdue_divergences += 1;
            }
            // Box-drawing/block/braille are intentionally synthesized (procedural)
            // and emoji-presentation cells intentionally use the colour face; both
            // are faithful to the scalar's MEANING, not the primary text glyph, so
            // they're outside this text-fidelity property.
            if procedural::covers(c) || aterm_grapheme::is_emoji_presentation(c) {
                continue;
            }
            let key = r.glyph_key(c);
            // Completeness: a scalar the primary covers in Unicode must render from
            // the primary face (not be shunted to a fallback substitute)…
            assert_eq!(
                key.source,
                FaceId::Primary,
                "U+{cp:04X} ({c:?}) is covered by {label} but did not resolve to the primary face",
            );
            // …and Faithfulness: addressed BY the Unicode glyph id, never a char
            // routed through fontdue's Mac Roman cmap.
            assert_eq!(
                key.glyph_class,
                GlyphClass::MonoGid,
                "U+{cp:04X} ({c:?}) primary glyph must be Unicode-id-addressed ({label})",
            );
            assert_eq!(
                u16::try_from(key.ch_or_id).unwrap(),
                want,
                "UNFAITHFUL ({label}): U+{cp:04X} ({c:?}) rasterizes glyph id {} but the Unicode cmap says {want}",
                key.ch_or_id,
            );
        }
        assert!(
            covered > 100,
            "{label}: only {covered} covered scalars — proof would be vacuous",
        );
        eprintln!(
            "PROVEN ({label}): {covered} covered scalars ALL render their Unicode glyph \
             (fontdue's char lookup would have diverged on {fontdue_divergences})",
        );
        (covered, fontdue_divergences)
    }

    /// EXHAUSTIVE PROOF — for the shipping default font (and the bundled face),
    /// the renderer's char→glyph map refines the Unicode cmap over the ENTIRE
    /// scalar domain. A complete proof, not a sample. (This is the gate that was
    /// missing when `·` rendered as `∑`.)
    #[test]
    fn render_glyph_resolution_refines_unicode_cmap_exhaustive() {
        if let Some(bytes) = system_font_bytes() {
            prove_faithful_over_all_scalars(&bytes, "system default");
        } else {
            eprintln!("SKIP: no system mono font for the system-default proof");
        }
        #[cfg(feature = "embedded-font")]
        prove_faithful_over_all_scalars(embedded_font(), "embedded DejaVu Sans Mono");
    }

    /// PROVE-AND-CATCH (non-vacuity + independence): on a font carrying a legacy
    /// `(1,0)` Mac Roman cmap subtable (Apple Menlo/Monaco), fontdue's own char
    /// lookup — the path aterm shipped before — DIVERGES from the Unicode cmap
    /// across Latin-1 (Mac Roman 0xB7 is `∑`, 0xE9 is `È`, …). This proves (a) the
    /// bug class is real and the fidelity proof above is non-vacuous, and (b)
    /// aterm's resolution sides with UNICODE, not fontdue — so the proof FAILS on
    /// the old code and PASSES on the fixed code. Honest skip if no such font.
    #[test]
    fn glyph_fidelity_proof_catches_mac_roman_substitution() {
        let candidates = [
            "/System/Library/Fonts/Menlo.ttc",
            "/System/Library/Fonts/Monaco.ttf",
        ];
        let mut demonstrated = false;
        for path in candidates {
            let Ok(bytes) = std::fs::read(path) else {
                continue;
            };
            let Ok(oracle) = ttf_parser::Face::parse(&bytes, 0) else {
                continue;
            };
            let fd = fontdue::Font::from_bytes(bytes.as_slice(), fontdue::FontSettings::default())
                .expect("fontdue parses");
            // Latin-1 codepoints where fontdue's char lookup disagrees with Unicode.
            let mut diverged: Vec<(char, u16, u16)> = Vec::new();
            for cp in 0xA0u32..=0xFF {
                let c = char::from_u32(cp).unwrap();
                let uni = oracle.glyph_index(c).map(|g| g.0).unwrap_or(0);
                let fdg = fd.lookup_glyph_index(c);
                if uni != 0 && fdg != 0 && uni != fdg {
                    diverged.push((c, uni, fdg));
                }
            }
            if diverged.is_empty() {
                continue; // this font has no Mac Roman quirk; try the next
            }
            demonstrated = true;
            // (b) aterm sides with Unicode on EVERY diverging codepoint.
            let mut r = Renderer::from_bytes(&bytes, 16.0, Theme::default()).expect("renderer");
            for (c, uni, fdg) in &diverged {
                let key = r.glyph_key(*c);
                assert_eq!(key.source, FaceId::Primary, "U+{:04X}", *c as u32);
                assert_eq!(
                    key.glyph_class,
                    GlyphClass::MonoGid,
                    "U+{:04X} must be Unicode-id-addressed",
                    *c as u32,
                );
                assert_eq!(
                    u16::try_from(key.ch_or_id).unwrap(),
                    *uni,
                    "U+{:04X}: aterm must use the Unicode glyph {uni}, not fontdue's Mac-Roman {fdg}",
                    *c as u32,
                );
                assert_ne!(
                    u16::try_from(key.ch_or_id).unwrap(),
                    *fdg,
                    "U+{:04X}: aterm must NOT reproduce fontdue's Mac-Roman substitution",
                    *c as u32,
                );
            }
            let dot = diverged.iter().find(|(c, _, _)| *c == '\u{B7}');
            eprintln!(
                "CAUGHT ({path}): {} Latin-1 codepoints where fontdue diverges from Unicode; \
                 aterm renders every one via the Unicode cmap{}",
                diverged.len(),
                dot.map(|(_, u, f)| format!(
                    " — e.g. U+00B7 ‘·’: fontdue→gid{f} (∑), unicode→gid{u}"
                ))
                .unwrap_or_default(),
            );
        }
        if !demonstrated {
            eprintln!(
                "SKIP: no Mac-Roman-subtable font (Menlo/Monaco) present to demonstrate the catch",
            );
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
        assert!(
            r.fallback_chain.is_empty(),
            "ASCII lookup must not load the fallback face"
        );
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
        if r.fallback_chain.is_empty() {
            eprintln!("SKIP: no system fallback font found");
            return;
        }
        assert_eq!(key.source, FaceId::Fallback);
        let img = r.glyph_image(key);
        assert!(
            img.width() > 0 && img.height() > 0,
            "CJK glyph rasterized empty"
        );
        assert!(
            img.bytes().iter().any(|&c| c > 0),
            "CJK glyph has no coverage"
        );
    }

    /// MULTI-FALLBACK CHAIN: a glyph absent in the primary AND the FIRST appended
    /// fallback but present in a SECOND appended fallback resolves to the second
    /// face (a real fontdue-coverage win), not `.notdef`. Built deterministically:
    /// primary = bundled DejaVu Sans Mono (no Hebrew), fallback#1 = Apple Symbols
    /// (no Hebrew either), fallback#2 = SFHebrew (covers א U+05D0). The system
    /// fallbacks are gated so the test skips cleanly where they are absent.
    #[cfg(feature = "embedded-font")]
    #[test]
    fn second_chain_fallback_covers_glyph_first_misses() {
        const HEBREW_ALEF: char = '\u{05D0}'; // א
        // Primary: the bundled DejaVu Sans Mono — deterministically lacks Hebrew.
        let mut r = Renderer::from_bytes(embedded_font(), 16.0, Theme::default())
            .expect("embedded font builds a renderer");
        assert_eq!(
            r.primary_unicode_gid(HEBREW_ALEF),
            None,
            "DejaVu Sans Mono must not cover Hebrew (test precondition)"
        );
        // fallback#1 = Apple Symbols (no Hebrew); fallback#2 = SFHebrew (has א).
        let Ok(sym) = std::fs::read("/System/Library/Fonts/Apple Symbols.ttf") else {
            eprintln!("SKIP: Apple Symbols.ttf absent");
            return;
        };
        let Ok(heb) = std::fs::read("/System/Library/Fonts/SFHebrew.ttf") else {
            eprintln!("SKIP: SFHebrew.ttf absent");
            return;
        };
        // Verify the disjoint-coverage precondition with the REAL fontdue probe the
        // engine uses, so a future OS font change fails loudly instead of silently
        // weakening the test.
        let probe = |bytes: &[u8], ch: char| -> bool {
            fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
                .map(|f| f.lookup_glyph_index(ch) != 0)
                .unwrap_or(false)
        };
        if probe(&sym, HEBREW_ALEF) || !probe(&heb, HEBREW_ALEF) {
            eprintln!("SKIP: system fonts no longer have the disjoint Hebrew coverage");
            return;
        }
        r.set_fallback_bytes(&sym).expect("Apple Symbols parses");
        r.add_fallback_bytes(&heb).expect("SFHebrew parses");
        assert_eq!(r.fallback_chain.len(), 2, "chain holds both faces in order");

        let key = r.glyph_key(HEBREW_ALEF);
        // It must reach the FALLBACK face (not the primary `.notdef` give-up).
        assert_eq!(
            key.source,
            FaceId::Fallback,
            "Hebrew alef must resolve to the fallback chain, not .notdef"
        );
        // And specifically to the SECOND chain entry (index 1 = SFHebrew), proven
        // via the per-char pick the rasterizer recovers the face from.
        assert_eq!(
            r.fallback_pick.get(&HEBREW_ALEF).copied(),
            Some(1),
            "the SECOND appended fallback (index 1) must own the glyph"
        );
        // The rasterized glyph carries real coverage (not an empty/notdef bitmap).
        let img = r.glyph_image(key);
        assert!(
            img.width() > 0 && img.height() > 0 && img.bytes().iter().any(|&c| c > 0),
            "Hebrew alef from the second fallback rasterized empty"
        );
    }

    /// `set_fallback_bytes` RESETS the chain to a single face (back-compat for the
    /// existing single caller); `add_fallback_bytes` APPENDS. Proven on the chain.
    #[cfg(feature = "embedded-font")]
    #[test]
    fn set_resets_and_add_appends_the_fallback_chain() {
        let mut r = Renderer::from_bytes(embedded_font(), 16.0, Theme::default())
            .expect("embedded font builds a renderer");
        // Two distinct parseable faces: the bundled DejaVu and Apple Symbols (or, if
        // absent, fall back to using DejaVu twice via interning — still two pushes).
        let a = embedded_font().to_vec();
        let b =
            std::fs::read("/System/Library/Fonts/Apple Symbols.ttf").unwrap_or_else(|_| a.clone());
        r.set_fallback_bytes(&a).expect("face a parses");
        assert_eq!(r.fallback_chain.len(), 1, "set installs exactly one face");
        r.add_fallback_bytes(&b).expect("face b parses");
        assert_eq!(r.fallback_chain.len(), 2, "add appends a second face");
        // set RESETS back to one (drops the appended b).
        r.set_fallback_bytes(&a).expect("face a parses again");
        assert_eq!(
            r.fallback_chain.len(),
            1,
            "set resets the chain to one face"
        );
    }

    /// REGRESSION (the ⏺ bug): a default-TEXT code point that only the colour
    /// font covers must NOT resolve to the colour-emoji face. U+23FA BLACK CIRCLE
    /// FOR RECORD is `Emoji=Yes` but `Emoji_Presentation=No` — the exact glyph
    /// Claude Code prints before each line. On stock macOS no mono face has it,
    /// so the OLD coverage-only fallback rendered it as a colour Apple emoji.
    #[test]
    fn record_symbol_is_never_color_emoji() {
        let Some(mut r) = renderer() else {
            eprintln!("SKIP: no system mono font found");
            return;
        };
        let key = r.glyph_key('\u{23FA}'); // ⏺
        eprintln!(
            "⏺ U+23FA resolved to {:?} / {:?}",
            key.source, key.glyph_class
        );
        assert_ne!(
            key.source,
            FaceId::ColorEmoji,
            "⏺ (U+23FA, Emoji_Presentation=No) must not use the colour-emoji face"
        );
        assert_ne!(
            key.glyph_class,
            GlyphClass::Rgba,
            "⏺ must rasterize as a foreground-tinted mono glyph, not a colour bitmap"
        );
        // When a mono symbol face that covers ⏺ is installed (STIX Two Math on
        // stock macOS), it must be used — a real monochrome glyph, not `.notdef`.
        if r.symbol_fallback_has('\u{23FA}') {
            assert_eq!(
                key.source,
                FaceId::SymbolFallback,
                "⏺ should render via the mono symbol fallback when one covers it"
            );
            let img = r.glyph_image(key).clone();
            assert!(
                img.bytes().iter().any(|&b| b > 0),
                "⏺ symbol-fallback glyph must carry real coverage, not be blank"
            );
        }
        // The same holds for its siblings ⏸ ⏹ (U+23F8..23F9).
        for c in ['\u{23F8}', '\u{23F9}'] {
            assert_ne!(
                r.glyph_key(c).source,
                FaceId::ColorEmoji,
                "{c:?} (Emoji_Presentation=No) must not use the colour face"
            );
        }
    }

    /// CAVEAT-2 COVERAGE: when NO mono face on the system covers ⏺ (no STIX / no
    /// Apple Symbols), it must still render — as the monochromatized colour
    /// silhouette (`ColorEmojiMono`, a foreground-tinted Mono glyph with real
    /// coverage), never `.notdef` tofu and never the colour bitmap.
    #[test]
    fn record_symbol_monochromatizes_when_no_mono_symbol_face() {
        let Some(mut r) = renderer() else {
            eprintln!("SKIP: no system mono font found");
            return;
        };
        if !r.color_font_has('\u{23FA}') {
            eprintln!("SKIP: no colour-emoji font on this system");
            return;
        }
        // Force every mono symbol face to be unavailable (simulate a minimal
        // system without STIX Two Math / Apple Symbols).
        r.symbol_fallback = None;
        r.symbol_fallback_paths = vec!["/nonexistent/no-symbol-font.ttf".to_string()];

        let key = r.glyph_key('\u{23FA}');
        assert_eq!(
            key.source,
            FaceId::ColorEmojiMono,
            "⏺ with no mono glyph anywhere must monochromatize the colour glyph"
        );
        assert_eq!(
            key.glyph_class,
            GlyphClass::Mono,
            "the monochromatized ⏺ must be a Mono (foreground-tinted) glyph, not Rgba"
        );
        let img = r.glyph_image(key).clone();
        assert!(
            matches!(img, GlyphImage::Mono { .. }),
            "must be a Mono image"
        );
        assert!(
            img.bytes().iter().any(|&b| b > 0),
            "monochromatized ⏺ must carry real coverage, not be blank tofu"
        );
    }

    /// The dual of the regression: a genuine default-EMOJI code point
    /// (Emoji_Presentation=Yes) the mono faces miss MUST still reach the colour
    /// face — proving the gate did not over-correct into "never colour".
    #[test]
    fn default_emoji_still_uses_color_face() {
        let Some(mut r) = renderer() else {
            eprintln!("SKIP: no system mono font found");
            return;
        };
        // 🚀 U+1F680 is Emoji_Presentation=Yes and present in Apple Color Emoji.
        let key = r.glyph_key('\u{1F680}');
        if !r.color_font_has('\u{1F680}') {
            eprintln!("SKIP: no colour-emoji font on this system");
            return;
        }
        assert_eq!(
            key.source,
            FaceId::ColorEmoji,
            "🚀 (Emoji_Presentation=Yes) must still render in colour"
        );
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
        for c in ['─', '│', '┼', '╋', '╬', '╭', '╳', '█', '▚', '░', '\u{28FF}']
        {
            let key = r.glyph_key(c);
            assert_eq!(key.source, FaceId::Procedural, "{c:?} must be procedural");
            let img = r.glyph_image(key).clone();
            assert_eq!(
                (img.width(), img.height()),
                (cw, ch),
                "{c:?} must fill the cell"
            );
            assert_eq!(img.xmin(), 0, "{c:?} anchors at the cell's left edge");
            // blit row anchor: cell_y + baseline - height - ymin == cell_y.
            assert_eq!(
                baseline - img.height() as i32 - img.ymin(),
                0,
                "{c:?} anchors at the cell top"
            );
            assert!(
                img.bytes().iter().all(|&b| b == 0 || b == 255),
                "{c:?} must be hard 0/255 coverage"
            );
            assert!(img.bytes().contains(&255), "{c:?} must draw something");
        }
        assert!(
            r.fallback_chain.is_empty(),
            "procedural dispatch must not load the fallback face"
        );
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
        assert!(
            img.width() > 0 && img.height() > 0,
            "colour emoji glyph is empty"
        );
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
            let key = r
                .glyph_key_cluster(s)
                .unwrap_or_else(|| panic!("{name} should shape to a glyph"));
            assert_eq!(
                key.glyph_class,
                GlyphClass::RgbaGid,
                "{name} key is glyph-id-addressed"
            );
            assert_eq!(
                key.source,
                FaceId::ColorEmoji,
                "{name} uses the colour face"
            );
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
        assert!(
            r.glyph_key_cluster("e\u{0301}").is_none(),
            "Latin diacritic must not shape to colour"
        );
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
        // 'M' resolves on the primary face by Unicode glyph id (MonoGid); synthetic
        // bold still applies (the styled set now includes MonoGid).
        assert_eq!(bold.glyph_class, GlyphClass::MonoGid);
        assert!(bold.style.contains(StyleBits::BOLD));
        assert_ne!(reg, bold, "bold key differs from regular");
        let bold_img = r.glyph_image(bold).clone();
        assert!(
            ink(&bold_img) > ink(&reg_img),
            "bold glyph should have more ink than regular"
        );

        let ital = r.glyph_key_styled('M', StyleBits::ITALIC);
        let ital_img = r.glyph_image(ital).clone();
        assert!(
            ital_img.width() >= reg_img.width(),
            "italic shear widens the bitmap"
        );
        assert_ne!(
            reg_img.bytes(),
            ital_img.bytes(),
            "italic glyph differs from regular"
        );

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
        let mut wc = WindowCpu::new();
        let mut term = Terminal::new(3, 12);
        term.process(b"reuse me\x1b[31m!\x1b[0m");
        let input = term.cell_frame(3, 12);

        // Frame 1 via the borrowing cached path: capture the buffer's heap pointer.
        let (ptr1, expected) = {
            let f = r.render_input_cached(&mut wc, &input);
            (f.pixels().as_ptr(), f.pixels().to_vec())
        };
        // Frame 2 (same dims): the buffer must be the SAME allocation — reused in
        // place, not freshly allocated — and the pixels identical.
        let ptr2 = {
            let f = r.render_input_cached(&mut wc, &input);
            assert_eq!(
                f.pixels(),
                expected.as_slice(),
                "reused-buffer frame differs frame-to-frame"
            );
            f.pixels().as_ptr()
        };
        assert_eq!(
            ptr1, ptr2,
            "steady-size frame must REUSE the pixel allocation"
        );

        // And the borrowing path is byte-identical to the allocating render_input.
        let owned = r.render_input(&input);
        assert_eq!(
            owned.pixels, expected,
            "render_input_cached must match render_input pixels"
        );
        assert_eq!(
            (owned.width, owned.height),
            (input.cols * r.cell_w, input.rows * r.cell_h)
        );
    }

    /// S5c (CPU `WindowGpu` analog): TWO `WindowCpu` interleaved through ONE
    /// `Renderer` must NOT cross-contaminate. The damage cache is keyed only on
    /// `(w, h)` + the previous `RenderInput`; holding it per-window keeps each
    /// window's diff isolated. If it instead lived on the shared `Renderer`,
    /// window B's render would diff against window A's cached input → a wrong
    /// dirty set or a FALSE dirty-gate hit handing B window A's pixels. We render
    /// input A into wc_A, a DIFFERENT input B into wc_B, then re-render A into
    /// wc_A and assert wc_A's pixels are byte-identical to a fresh full
    /// `render_input(A)`. This is the S8 correctness oracle for the CPU path.
    #[test]
    fn interleaved_windowcpu_no_cross_contamination() {
        let Some(mut r) = renderer() else {
            eprintln!("SKIP: no system mono font found");
            return;
        };
        // Two distinct frames at the SAME dims (so the cache's `(w, h)` matches
        // and the damage/gate path — not the size-mismatch full-repaint — is the
        // one under test).
        let mut term_a = Terminal::new(3, 12);
        term_a.process(b"window A text\x1b[31m!\x1b[0m");
        let input_a = term_a.cell_frame(3, 12);
        let mut term_b = Terminal::new(3, 12);
        term_b.process(b"DIFFERENT b\x1b[42mX\x1b[0m");
        let input_b = term_b.cell_frame(3, 12);

        // The ground truth for window A: a fresh full render of input A (its own
        // virgin `WindowCpu`, so this is unambiguously the correct pixels for A).
        let expected_a = {
            let mut wc_fresh = WindowCpu::new();
            r.render_input_cached(&mut wc_fresh, &input_a)
                .pixels()
                .to_vec()
        };

        let mut wc_a = WindowCpu::new();
        let mut wc_b = WindowCpu::new();
        // Window A renders input A (populates wc_A's cache with A).
        {
            let va = r.render_input_cached(&mut wc_a, &input_a);
            assert_eq!(
                va.pixels(),
                expected_a.as_slice(),
                "wc_A first render must equal A"
            );
        }
        // Window B renders a DIFFERENT input B through the SAME renderer. If the
        // cache were shared on `Renderer`, this would overwrite A's cached input.
        {
            let _vb = r.render_input_cached(&mut wc_b, &input_b);
        }
        // Window A re-renders input A. wc_A still holds A as its previous frame, so
        // this hits the dirty-gate (nothing changed) and returns A's pixels — it
        // must NOT have been perturbed by B's interleaved render.
        let va2 = r.render_input_cached(&mut wc_a, &input_a);
        assert_eq!(
            va2.pixels(),
            expected_a.as_slice(),
            "wc_A re-render after an interleaved wc_B frame must still be byte-identical to a fresh render_input(A) — no cross-window contamination / false gate hit"
        );
    }

    /// Interior padding insets the grid by `pad` on every edge: the framebuffer
    /// grows by `2·pad` per axis, the rendered grid is the unpadded render shifted
    /// to `(pad, pad)`, and the freed border is theme background. This is the
    /// property the on-screen present AND the `image`/snapshot introspection both
    /// rely on (they share this one renderer, so both get the identical padded
    /// pixels — the WYSIWYG parity constraint).
    #[test]
    fn padding_insets_grid_and_grows_framebuffer() {
        let Some(mut r) = renderer() else {
            eprintln!("SKIP: no system mono font found");
            return;
        };
        let (cw, ch) = r.cell_size();
        let (rows, cols) = (3usize, 12usize);
        let mut term = Terminal::new(rows as u16, cols as u16);
        term.process(b"pad me\x1b[31m!\x1b[0m");
        let input = term.cell_frame(rows, cols);

        // Baseline: no padding (the historical dims + pixels).
        assert_eq!(
            r.pad(),
            0,
            "default pad is 0 (byte-identical historical path)"
        );
        let base = r.render_input(&input);
        assert_eq!((base.width, base.height), (cols * cw, rows * ch));

        // Now pad by P on every edge and re-render.
        const P: usize = 8;
        r.set_pad(P);
        assert_eq!(r.pad(), P);
        let padded = r.render_input(&input);
        // Framebuffer grew by 2·P on each axis.
        assert_eq!(
            (padded.width, padded.height),
            (cols * cw + 2 * P, rows * ch + 2 * P),
            "padded framebuffer is 2·pad larger per axis"
        );
        // The grid content is the SAME pixels, just shifted to (P, P): every base
        // pixel (x, y) equals the padded pixel (x + P, y + P).
        let bg = Theme::default().bg;
        for y in 0..base.height {
            for x in 0..base.width {
                let b = base.pixels[y * base.width + x];
                let p = padded.pixels[(y + P) * padded.width + (x + P)];
                assert_eq!(b, p, "grid pixel at ({x},{y}) must survive the pad shift");
            }
        }
        // The border is theme background: the top P rows and the left P columns.
        for x in 0..padded.width {
            assert_eq!(padded.pixels[x], bg, "top padding row is bg");
        }
        for y in 0..padded.height {
            assert_eq!(
                padded.pixels[y * padded.width],
                bg,
                "left padding column is bg"
            );
        }

        // Back to 0 reproduces the baseline exactly (idempotent round-trip).
        r.set_pad(0);
        let back = r.render_input(&input);
        assert_eq!(
            back.pixels, base.pixels,
            "pad 0 restores the byte-identical render"
        );
    }

    /// C-1: `cell_frame_into` refilling a reused `RenderInput` yields the SAME
    /// snapshot (and therefore the same pixels) as a fresh `cell_frame`. The
    /// snapshot is now built by the ENGINE (A-3); the renderer consumes the value.
    #[test]
    fn cell_frame_into_matches_fresh_cell_frame() {
        let Some(mut r) = renderer() else {
            eprintln!("SKIP: no system mono font found");
            return;
        };
        let mut term = Terminal::new(2, 8);
        term.process(b"AB\x1b[42mC\x1b[0m");

        let fresh = term.cell_frame(2, 8);
        let mut reused = RenderInput::empty();
        // Prime the reused buffer at a DIFFERENT size first, to prove refill
        // (truncate/extend) lands on the right shape regardless of prior state.
        term.cell_frame_into(&mut reused, 1, 4);
        term.cell_frame_into(&mut reused, 2, 8);

        assert_eq!(
            r.render_input(&fresh).pixels,
            r.render_input(&reused).pixels,
            "cell_frame_into must produce pixels identical to a fresh cell_frame"
        );
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
        assert_eq!(
            emoji_key.source,
            FaceId::ColorEmoji,
            "VS16 heart must use the colour face"
        );
        assert_eq!(
            emoji_key.glyph_class,
            GlyphClass::Rgba,
            "VS16 heart must be an Rgba glyph"
        );
        assert_ne!(
            text_key, emoji_key,
            "text and emoji presentations must differ"
        );
        // The text presentation is a mono COVERAGE glyph (the black heart from a
        // text font), never the colour face — whether addressed by char (a fallback
        // face) or by Unicode glyph id (the primary, MonoGid).
        assert!(
            matches!(text_key.glyph_class, GlyphClass::Mono | GlyphClass::MonoGid),
            "bare ❤ should stay a mono coverage glyph, got {:?}",
            text_key.glyph_class
        );
        assert_ne!(
            text_key.source,
            FaceId::ColorEmoji,
            "bare ❤ must not use the colour face"
        );
        // And the colour glyph actually rasterizes to a non-empty colour bitmap.
        let img = r.glyph_image(emoji_key).clone();
        assert!(
            matches!(img, GlyphImage::Rgba { .. }) && img.width() > 0 && img.height() > 0,
            "VS16 heart colour glyph is empty"
        );
    }

    /// `normalize_family` folds case and strips spaces / `-` / `_`, so the common
    /// spellings of a family name collapse to one comparison key.
    #[test]
    fn normalize_family_collapses_spellings() {
        assert_eq!(normalize_family("JetBrains Mono"), "jetbrainsmono");
        assert_eq!(normalize_family("JetBrainsMono"), "jetbrainsmono");
        assert_eq!(normalize_family("jetbrains-mono"), "jetbrainsmono");
        assert_eq!(normalize_family("Jet_Brains Mono"), "jetbrainsmono");
        assert_eq!(normalize_family("   "), "");
    }

    /// An empty / whitespace family resolves to nothing (the caller then falls
    /// back to `$ATERM_FONT` then the built-in candidates), so the default path is
    /// byte-identical to `from_system`.
    #[test]
    fn resolve_family_empty_is_none() {
        assert_eq!(resolve_font_family(""), None);
        assert_eq!(resolve_font_family("   "), None);
    }

    /// An ABSOLUTE-path family value that names an existing file short-circuits the
    /// directory scan — the user can point the family straight at a font file,
    /// like `$ATERM_FONT`. (Use the first built-in candidate that exists.)
    #[test]
    fn resolve_family_explicit_path_passthrough() {
        let Some(existing) = FONT_CANDIDATES
            .iter()
            .find(|p| std::path::Path::new(p).is_file())
        else {
            return; // no system font on this host (e.g. headless CI) — skip
        };
        assert_eq!(resolve_font_family(existing).as_deref(), Some(*existing));
    }

    /// A configured family that DOES resolve to a file is the FIRST candidate
    /// `from_system_with_family` loads. We feed it the path of an existing
    /// built-in candidate as the family (the path passthrough), and confirm the
    /// renderer builds — proving the family path is honored, not ignored. A
    /// `None` family reduces to `from_system`, so the unset path is unchanged.
    #[test]
    fn from_system_with_family_honors_resolved_path() {
        let Some(existing) = FONT_CANDIDATES
            .iter()
            .find(|p| std::path::Path::new(p).is_file())
        else {
            return; // no system font — skip
        };
        let with = Renderer::from_system_with_family(Some(existing), 16.0, Theme::default());
        assert!(with.is_some(), "a resolvable family must build a renderer");
        // None is identical to from_system (which also builds on this host).
        let plain = Renderer::from_system_with_family(None, 16.0, Theme::default());
        assert!(plain.is_some());
    }

    /// A family that matches NOTHING resolves to `None`, so the loader transparently
    /// falls through to `$ATERM_FONT` / the built-in candidates — an unknown family
    /// never makes the renderer fail to build.
    #[test]
    fn resolve_family_unknown_is_none_but_builds() {
        assert_eq!(resolve_font_family("NoSuchFamilyXYZ123"), None);
        // The renderer still builds (the family miss falls through), provided the
        // host has any system font at all.
        if renderer().is_some() {
            assert!(
                Renderer::from_system_with_family(
                    Some("NoSuchFamilyXYZ123"),
                    16.0,
                    Theme::default()
                )
                .is_some()
            );
        }
    }

    // ---- M3 FONT-DISCOVERY: runtime per-codepoint font fallback ----

    /// A renderer whose configured faces (primary / broad / symbol / colour) are
    /// all forced ABSENT, so any non-Latin code point MUST go through the runtime
    /// resolver to find a glyph — isolating the M3 fallback path from the bundled
    /// faces. Built from a real primary mono font (for sane metrics) but with the
    /// candidate fallback lists emptied.
    fn renderer_no_configured_fallbacks() -> Option<Renderer> {
        let bytes = system_font_bytes()?;
        let mut r = Renderer::from_bytes(&bytes, 16.0, Theme::default()).ok()?;
        // Defeat the bundled fallbacks: no broad/symbol/colour candidates, and the
        // (lazy) faces stay None — the ONLY remaining cover for a missed code point
        // is the runtime resolver.
        r.fallback_paths.clear();
        r.symbol_fallback_paths.clear();
        r.color_font_paths.clear();
        Some(r)
    }

    /// On macOS a common CJK code point (中 U+4E2D) resolves to SOME runtime
    /// fallback font with a real glyph, even when every configured fallback face is
    /// removed — this is the CoreText (`CTFontCreateForString`) path doing its job.
    /// CJK fonts ship on every Mac, so this is deterministic there; on a host with
    /// no covering font at all it SKIPs rather than fails.
    #[test]
    #[cfg(target_os = "macos")]
    fn runtime_fallback_resolves_cjk() {
        let Some(mut r) = renderer_no_configured_fallbacks() else {
            eprintln!("SKIP: no system mono font found");
            return;
        };
        let key = r.glyph_key('中'); // U+4E2D
        eprintln!("中 U+4E2D resolved to {:?}", key.source);
        if key.source == FaceId::Primary {
            // The primary mono font happened to cover it, or no system font does.
            eprintln!("SKIP: 中 covered by primary or uncovered on this host");
            return;
        }
        assert_eq!(
            key.source,
            FaceId::RuntimeFallback,
            "中 with no configured fallback must come from the runtime resolver"
        );
        let img = r.glyph_image(key);
        assert!(img.width() > 0 && img.height() > 0, "中 rasterized empty");
        assert!(
            img.bytes().iter().any(|&c| c > 0),
            "中 has no coverage from the runtime fallback face"
        );
    }

    /// On macOS an emoji (😀 U+1F600) resolves to SOME font with a usable glyph —
    /// here the configured colour-emoji face. NOTE on the runtime resolver's scope:
    /// it is a MONOCHROME (fontdue outline) fallback, so it deliberately declines
    /// bitmap-only emoji fonts (Apple Color Emoji has no outlines — `face_can_render`
    /// returns false), since there is no mono outline emoji font on macOS. Emoji are
    /// instead served by the dedicated colour path (`FaceId::ColorEmoji`). This test
    /// pins both facts: a normal renderer routes 😀 to the colour face, AND the
    /// runtime mono resolver correctly returns None for it (no mono cover exists).
    #[test]
    #[cfg(target_os = "macos")]
    fn emoji_resolves_to_colour_face_runtime_mono_declines() {
        let Some(mut r) = renderer() else {
            eprintln!("SKIP: no system mono font found");
            return;
        };
        let key = r.glyph_key('\u{1F600}'); // 😀, Emoji_Presentation=Yes
        eprintln!("😀 U+1F600 resolved to {:?}", key.source);
        // With the configured colour face present, 😀 renders in colour — a usable
        // glyph from a fallback font, exactly the M3 goal for emoji.
        assert_eq!(
            key.source,
            FaceId::ColorEmoji,
            "😀 must render via the colour-emoji fallback face"
        );
        // The runtime MONO resolver has no mono outline emoji font to offer, so it
        // declines (Apple Color Emoji is bitmap-only). This documents the boundary:
        // the runtime mono fallback covers missing OUTLINE scripts, not emoji.
        let mut iso = renderer_no_configured_fallbacks().expect("renderer");
        assert!(
            !iso.runtime_fallback_resolves('\u{1F600}'),
            "the runtime MONO resolver has no outline emoji font, so it declines 😀"
        );
    }

    /// A code point that no real font claims must resolve to `None` from the
    /// runtime resolver (graceful give-up), and dispatch to the primary face for
    /// `.notdef` — never panic, never a bogus face. We use a permanent Unicode
    /// NONCHARACTER (U+FDD0): no real font has a glyph for it, and the universal
    /// LastResort tofu font (which "covers" everything) is explicitly filtered out
    /// by the resolver, so the give-up is deterministic on every host. (An arbitrary
    /// PUA code point is unreliable here — e.g. macOS's U+F8FF is the Apple logo.)
    #[test]
    fn runtime_fallback_uncovered_resolves_to_none() {
        let Some(mut r) = renderer_no_configured_fallbacks() else {
            eprintln!("SKIP: no system mono font found");
            return;
        };
        let uncovered = '\u{FDD0}'; // a permanent Unicode noncharacter.
        assert!(
            !r.runtime_fallback_resolves(uncovered),
            "a noncharacter must not resolve to any runtime fallback font"
        );
        let key = r.glyph_key(uncovered);
        assert_eq!(
            key.source,
            FaceId::Primary,
            "an uncovered code point falls back to the primary face's .notdef"
        );
    }

    /// The decision cache is consulted on repeat: the resolver returns the SAME
    /// decision (and grows the decision map by exactly one entry) for a repeated
    /// lookup of the same code point. Uses a noncharacter so the decision is a
    /// stable `None` on every host, making the cache assertion deterministic.
    #[test]
    fn runtime_fallback_cache_is_stable_on_repeat() {
        let Some(mut r) = renderer_no_configured_fallbacks() else {
            eprintln!("SKIP: no system mono font found");
            return;
        };
        let ch = '\u{FDD1}'; // a noncharacter — stable `None` decision everywhere.
        let first = r.runtime_fallback_resolves(ch);
        let before = r.runtime_fallback.decisions.len();
        let second = r.runtime_fallback_resolves(ch);
        let after = r.runtime_fallback.decisions.len();
        assert_eq!(
            first, second,
            "repeated lookup must return the same decision"
        );
        assert_eq!(
            before, after,
            "a repeated lookup must hit the cache, not insert a second entry"
        );
        assert!(
            r.runtime_fallback.decisions.contains_key(&ch),
            "the decision must be memoized after the first lookup"
        );
    }

    /// The runtime resolver is NEVER consulted for a code point the primary face
    /// covers (the common hot path): an ASCII lookup leaves the decision cache
    /// completely empty, so ordinary text pays zero runtime-fallback cost.
    #[test]
    fn runtime_fallback_untouched_on_primary_hit() {
        let Some(mut r) = renderer() else {
            eprintln!("SKIP: no system mono font found");
            return;
        };
        let key = r.glyph_key('A');
        assert_eq!(key.source, FaceId::Primary);
        assert!(
            r.runtime_fallback.decisions.is_empty(),
            "a primary-covered char must not touch the runtime resolver"
        );
        assert!(
            r.runtime_fallback.faces.is_empty(),
            "a primary-covered char must not load any runtime fallback face"
        );
    }

    /// The decision cache is bounded: once it would exceed `MAX_DECISIONS` it is
    /// cleared wholesale, so an adversarial stream of distinct (uncovered)
    /// noncharacter code points cannot grow it without bound. Drives only a few
    /// past the cap (the bound logic is the same at any size) by temporarily
    /// pre-filling the map.
    #[test]
    fn runtime_fallback_decision_cache_is_bounded() {
        let mut rf = RuntimeFallback::default();
        // Pre-fill to exactly the cap with cheap dummy entries, then prove the next
        // distinct lookup clears + re-seeds rather than growing past the cap.
        for i in 0..RuntimeFallback::MAX_DECISIONS as u32 {
            if let Some(c) = char::from_u32(0xE000 + i) {
                rf.decisions.insert(c, None);
            }
        }
        assert!(rf.decisions.len() >= RuntimeFallback::MAX_DECISIONS);
        // A fresh noncharacter lookup trips the bound: the map is cleared and only
        // this new decision remains.
        let _ = rf.resolve('\u{FDD2}');
        assert!(
            rf.decisions.len() <= RuntimeFallback::MAX_DECISIONS,
            "decision cache must stay within MAX_DECISIONS"
        );
        assert!(
            rf.decisions.contains_key(&'\u{FDD2}'),
            "the triggering lookup must remain cached after the bound resets the map"
        );
    }

    /// Encode a solid-colour `w`×`h` RGBA8 PNG (every pixel `rgb`, opaque).
    fn solid_rgba_png(w: u32, h: u32, rgb: [u8; 3]) -> Vec<u8> {
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for _ in 0..(w * h) {
            rgba.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
        }
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, w, h);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            let mut writer = enc.write_header().expect("png header");
            writer.write_image_data(&rgba).expect("png data");
        }
        out
    }

    /// Craft a tiny but well-formed PNG whose IHDR *declares* `w`×`h` (with a
    /// valid IHDR CRC so `read_info` parses it), without carrying that many
    /// pixels. This is the inline-image allocation bomb: a small payload that, if
    /// honored, forces a `w*h*bpp` output buffer (30000×30000×4 ≈ 3.4 GiB).
    fn png_with_declared_dims(w: u32, h: u32) -> Vec<u8> {
        // Start from a real 1×1 PNG, then rewrite IHDR's width/height + CRC.
        let mut bytes = solid_rgba_png(1, 1, [10, 20, 30]);
        // Layout: 8-byte signature, then IHDR = [len:4][type:4]["IHDR"][w:4][h:4]…
        // The IHDR data starts at offset 16 (8 sig + 4 len + 4 "IHDR").
        let ihdr_data = 16usize;
        bytes[ihdr_data..ihdr_data + 4].copy_from_slice(&w.to_be_bytes());
        bytes[ihdr_data + 4..ihdr_data + 8].copy_from_slice(&h.to_be_bytes());
        // IHDR chunk = type(4) + data(13); CRC covers type+data. Recompute it so
        // `png::read_info` accepts the header and reaches our dimension guard.
        let crc_input = &bytes[ihdr_data - 4..ihdr_data + 13];
        let crc = crc32_ieee(crc_input);
        bytes[ihdr_data + 13..ihdr_data + 17].copy_from_slice(&crc.to_be_bytes());
        bytes
    }

    /// Minimal CRC-32 (IEEE 802.3, the PNG variant) — table-free; tests only.
    fn crc32_ieee(data: &[u8]) -> u32 {
        let mut crc: u32 = 0xFFFF_FFFF;
        for &b in data {
            crc ^= u32::from(b);
            for _ in 0..8 {
                let mask = (crc & 1).wrapping_neg();
                crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
            }
        }
        !crc
    }

    /// DoS guard: a tiny PNG declaring 30000×30000 in its IHDR must be REJECTED
    /// before allocating the multi-GB output buffer — no panic, no huge alloc.
    #[test]
    fn oversized_png_is_rejected_without_giant_alloc() {
        let bomb = png_with_declared_dims(30_000, 30_000);
        assert!(
            bomb.len() < 4096,
            "the bomb payload is tiny ({} bytes) — the threat is the declared size",
            bomb.len()
        );
        // Direct decode helper rejects it.
        assert!(
            decode_png_rgba8(&bomb).is_none(),
            "30000×30000 IHDR must be rejected by the decode guard"
        );
        // And so does the public inline-image footprint path the SSH peer reaches.
        assert!(
            decode_image_to_footprint(&bomb, aterm_core::grid::extra::ImageFormat::Png, 8, 8)
                .is_none(),
            "oversized inline-image PNG must decode to nothing"
        );
    }

    /// Exactly at the cap on one axis (and absurd on the other) is still rejected,
    /// confirming the guard fires on either dimension, not just both.
    #[test]
    fn png_over_cap_on_a_single_axis_is_rejected() {
        let tall = png_with_declared_dims(1, IMAGE_MAX_DIMENSION + 1);
        let wide = png_with_declared_dims(IMAGE_MAX_DIMENSION + 1, 1);
        assert!(
            decode_png_rgba8(&tall).is_none(),
            "height past cap rejected"
        );
        assert!(decode_png_rgba8(&wide).is_none(), "width past cap rejected");
    }

    /// A normal small PNG still decodes fine — the guard rejects bombs, not images.
    #[test]
    fn normal_small_png_still_decodes() {
        let png = solid_rgba_png(8, 4, [200, 30, 30]);
        let (rgba, w, h) = decode_png_rgba8(&png).expect("a real 8×4 PNG decodes");
        assert_eq!((w, h), (8, 4));
        assert_eq!(rgba.len(), 8 * 4 * 4, "packed RGBA8");
        assert_eq!(&rgba[0..4], &[200, 30, 30, 255], "first pixel is the fill");

        // And it flows through the footprint path to a non-empty 16×16 raster.
        let fp = decode_image_to_footprint(&png, aterm_core::grid::extra::ImageFormat::Png, 16, 16)
            .expect("small PNG resamples to its footprint");
        assert_eq!(fp.len(), 16 * 16 * 4);
    }

    /// Selection-fg flooring: a low-contrast fg vs the selection bg is nudged to a
    /// legible contrast (>= 4.5:1), while an already-legible fg is returned as-is.
    #[test]
    fn floor_selection_fg_enforces_legible_contrast() {
        // Dark-blue fg on a dark selection bg is illegible → must be floored.
        let sel_bg = 0x0020_2a40; // a typical dark selection
        let low = 0x0022_2c44; // nearly the same dark blue
        let floored = floor_selection_fg(low, sel_bg);
        assert_ne!(floored, low, "a low-contrast fg must be adjusted");
        assert!(
            contrast_ratio(floored, sel_bg) >= 4.5,
            "floored fg must clear the 4.5:1 legibility floor (got {})",
            contrast_ratio(floored, sel_bg)
        );
        // An already-legible fg (white on dark selection) is unchanged.
        let high = 0x00ff_ffff;
        assert_eq!(
            floor_selection_fg(high, sel_bg),
            high,
            "an already-legible fg must be left untouched"
        );
    }

    /// INACTIVE-SELECTION theming: the renderer reports the ACTIVE selection bg when
    /// focused and the (derived or explicit) inactive bg when unfocused — the single
    /// source of truth both the CPU fill and the GPU encode read.
    #[test]
    fn effective_selection_bg_switches_on_focus() {
        let mut r = renderer().unwrap_or_else(|| {
            // No system font? Build from the embedded face so this stays deterministic.
            #[cfg(feature = "embedded-font")]
            {
                Renderer::from_bytes(embedded_font(), 16.0, Theme::default()).unwrap()
            }
            #[cfg(not(feature = "embedded-font"))]
            panic!("no font available");
        });
        let theme = Theme::default();
        // Focused (default): the ACTIVE selection colour.
        assert!(!r.selection_inactive());
        assert_eq!(r.effective_selection_bg(), theme.selection);
        // Unfocused, no explicit bg: the DERIVED dim (active blended toward bg).
        r.set_selection_inactive(true);
        let derived = derive_inactive_selection_bg(theme.selection, theme.bg);
        assert_eq!(r.effective_selection_bg(), derived);
        // The derived dim must sit strictly between the active selection and the bg
        // (a real recede, not a no-op or a flip): each channel between the two.
        for shift in [16u32, 8, 0] {
            let act = (theme.selection >> shift) & 0xff;
            let bg = (theme.bg >> shift) & 0xff;
            let dim = (derived >> shift) & 0xff;
            let (lo, hi) = (act.min(bg), act.max(bg));
            assert!(
                lo <= dim && dim <= hi,
                "derived channel out of [bg,active] range"
            );
        }
        // Unfocused WITH an explicit override: that exact colour wins.
        let custom = 0x0012_3456;
        r.set_selection_inactive_bg(Some(custom));
        assert_eq!(r.effective_selection_bg(), custom);
        // Back to focused: the active colour again, ignoring the inactive override.
        r.set_selection_inactive(false);
        assert_eq!(r.effective_selection_bg(), theme.selection);
    }

    /// The color-emoji font bytes are interned: injecting the SAME blob twice
    /// shares ONE Arc (the per-pane ~180MB-copy dedup), while distinct blobs stay
    /// separate. Content-keyed, so independent of font parsing.
    #[test]
    fn color_font_bytes_are_interned() {
        let a1 = intern_font_bytes(vec![1, 2, 3, 4]);
        let a2 = intern_font_bytes(vec![1, 2, 3, 4]);
        assert!(
            std::sync::Arc::ptr_eq(&a1, &a2),
            "identical emoji-font bytes must share one Arc across renderers"
        );
        let b = intern_font_bytes(vec![9, 9, 9]);
        assert!(
            !std::sync::Arc::ptr_eq(&a1, &b),
            "different font bytes must not be aliased"
        );
    }
}
