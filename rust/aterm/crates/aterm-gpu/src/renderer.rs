// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors
//
// GPU terminal renderer: glyph atlas + instanced cell quads, drawn offscreen and
// read back into an `aterm_render::Frame`. The output is built to MATCH the CPU
// `aterm_render::Renderer` exactly: same cell geometry (from the CPU renderer's
// metrics), same per-cell background fill, same glyph placement, and the same
// coverage blend (`out = fg*cov + bg*(1-cov)`) — implemented here as alpha
// blending (`SrcAlpha`/`OneMinusSrcAlpha`) over an `Rgba8Unorm` (linear) target,
// which blends in the raw 8-bit space exactly like the CPU `blend`.
//
// On-glass present: the offscreen `Rgba8Unorm` texture is the SINGLE SOURCE OF
// TRUTH (parity-tested, and the exact buffer the AI snapshot/`image` introspection
// reads). The window path does NOT re-render into the swapchain; it BLITS that
// same texture into a swapchain texture with a fullscreen-triangle pass and
// presents on the GPU, so on-screen pixels are byte-identical to introspection
// (a hard invariant). The blit fragment can invert RGB for the visual bell.
//
// Up to three passes over the same target:
//   1. BG pass   — clear to the theme bg, then draw one opaque quad per cell.
//   2. GLYPH pass — draw one alpha-blended quad per glyph, sampling the atlas
//                   coverage texture with NEAREST filtering (exact texel = cov).
//   3. CURSOR pass — the non-block cursor shapes (underline/bar/hollow), drawn
//                   as opaque quads OVER the glyphs, exactly where the CPU
//                   renderer fills them after its glyph blits.
// Cursor shapes follow DECSCUSR via the SAME geometry helper the CPU uses
// (`aterm_render::cursor_rects`). A block cursor is reproduced exactly as the
// CPU does it: the cursor cell's bg is overwritten with the cursor colour
// (last bg quad) and its glyph is drawn in the cell's own bg colour ("cut
// out"); the other shapes paint over the normally coloured glyph in pass 3.

use std::collections::{BTreeSet, HashMap};

// A-3: the GPU renderer no longer borrows `&Terminal` — it consumes only the
// engine-built `RenderInput`. `Terminal` is imported solely in the test modules
// (which build terminals + call `Terminal::cell_frame` to feed the renderer).
use aterm_core::terminal::{CursorStyle, UnderlineStyle};
use aterm_render::{
    DirtyDecision, Frame, GlyphImage, GlyphKey, Rasterizer, RenderInput, RenderView, Renderer,
    Theme, compute_dirty_rows, is_unchanged_frame,
};

use crate::GpuContext;

/// Atlas texture width in texels. A multiple of 256 so the R8 `bytes_per_row`
/// (== width) needs no extra padding on upload.
const ATLAS_WIDTH: u32 = 1024;

/// Extra texel rows the resident atlas TEXTURE carries beyond its currently
/// packed (occupied) height. New glyphs append into this headroom via a cheap
/// sub-region upload — no texture recreation — until it is exhausted, at which
/// point a glyph that would exceed it is genuine overflow (full repack into a
/// fresh, taller texture). Sized for several more shelves of typical glyphs so
/// the steady state grows in place. (Untouched headroom rows are never sampled:
/// no glyph slot points into them until an append writes them.)
const ATLAS_GROW_HEADROOM: u32 = 256;

/// Screen size uniform (vec2 + pad to 16 bytes for std140 alignment).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    screen: [f32; 2],
    _pad: [f32; 2],
}

/// One background quad: a pixel-space rect filled with an opaque colour.
///
/// PACKED LAYOUT (12 B, was 32 B): every bg rect is a NON-NEGATIVE INTEGER pixel
/// coord (x, y, w, h) — produced by `usize`/`u32` arithmetic and the `usize`-rect
/// helpers (`cursor_rects`/`underline_rects`/`strike_overline_rects`) — so it fits
/// `u16` exactly and decodes via `Uint16x4`→`vec4<u32>`→`vec4<f32>` with NO
/// precision loss at these magnitudes (integer→f32 is exact). The colour is an
/// opaque RGBA byte quad (a == 255) decoded by the fixed-function `Unorm8x4` path
/// as exactly `value/255.0` — the IDENTICAL IEEE-754 result `rgb4` used to compute,
/// so the rendered pixels stay byte-identical. `[u16;4]` (8 B) then `[u8;4]` (4 B)
/// pack with no padding (`#[repr(C)]`, 2-byte align): `size_of` == 12.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct BgInstance {
    /// x, y, w, h in pixels (top-left origin, y down) — non-negative integers.
    rect: [u16; 4],
    /// r, g, b, a bytes (a == 255). Unorm8x4-decoded to the same `rgb4` floats.
    color: [u8; 4],
}

/// One glyph quad: a pixel-space dest rect, an atlas UV rect, and a fg colour.
///
/// PACKED LAYOUT (36 B, was 48 B): ONLY the colour packs (`Unorm8x4`, exact
/// `value/255.0`). The rect and UV STAY `Float32x4` and MUST NOT pack: the glyph
/// rect's `gx0`/`gy0` can be NEGATIVE (font bearings, DEC double-size scaling) so
/// `Uint16` would corrupt them, and `Unorm16x4` UV quantization (`k/65535`) can
/// cross a texel-sample boundary at glyph edges — neither is byte-identical.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct GlyphInstance {
    /// dest x, y, w, h in pixels (may be negative — bearings / DEC scaling).
    rect: [f32; 4],
    /// atlas u0, v0, du, dv in [0, 1].
    uv: [f32; 4],
    /// fg r, g, b, a bytes (a unused; coverage supplies alpha). Unorm8x4-decoded.
    color: [u8; 4],
}

// Lock in the packed strides (no `#[repr(C)]` padding surprises): the GPU
// `array_stride` is `size_of` of these, so a regression here would silently
// change the per-instance bandwidth. BgInstance: u16x4 (8) + u8x4 (4) = 12.
// GlyphInstance: f32x4 (16) + f32x4 (16) + u8x4 (4) = 36.
const _: () = {
    assert!(std::mem::size_of::<BgInstance>() == 12);
    assert!(std::mem::size_of::<GlyphInstance>() == 36);
};

const BG_ATTRS: [wgpu::VertexAttribute; 2] = wgpu::vertex_attr_array![0 => Uint16x4, 1 => Unorm8x4];
const GLYPH_ATTRS: [wgpu::VertexAttribute; 3] =
    wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Unorm8x4];

const SHADER: &str = r#"
struct Uniforms { screen: vec2<f32>, pad: vec2<f32> };
@group(0) @binding(0) var<uniform> u: Uniforms;

// Unit quad corner for vertex index 0..6 (two CCW triangles).
fn corner(vi: u32) -> vec2<f32> {
    var c = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0), vec2<f32>(0.0, 1.0)
    );
    return c[vi];
}

// Pixel coords (top-left origin, y down) -> clip space (y up, row 0 at top).
fn to_ndc(px: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(2.0 * px.x / u.screen.x - 1.0, 1.0 - 2.0 * px.y / u.screen.y);
}

struct BgVsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_bg(@builtin(vertex_index) vi: u32,
         @location(0) rect_u: vec4<u32>,
         @location(1) color: vec4<f32>) -> BgVsOut {
    // Uint16x4 arrives as vec4<u32>; integer pixel coords -> exact f32 (no loss).
    let rect = vec4<f32>(rect_u);
    let k = corner(vi);
    let px = rect.xy + k * rect.zw;
    var o: BgVsOut;
    o.pos = vec4<f32>(to_ndc(px), 0.0, 1.0);
    o.color = color;
    return o;
}

@fragment
fn fs_bg(in: BgVsOut) -> @location(0) vec4<f32> {
    return in.color;
}

@group(1) @binding(0) var atlas_tex: texture_2d<f32>;
@group(1) @binding(1) var atlas_samp: sampler;

struct GlyphVsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_glyph(@builtin(vertex_index) vi: u32,
            @location(0) rect: vec4<f32>,
            @location(1) uv: vec4<f32>,
            @location(2) color: vec4<f32>) -> GlyphVsOut {
    let k = corner(vi);
    let px = rect.xy + k * rect.zw;
    var o: GlyphVsOut;
    o.pos = vec4<f32>(to_ndc(px), 0.0, 1.0);
    o.uv = uv.xy + k * uv.zw;
    o.color = color;
    return o;
}

@fragment
fn fs_glyph(in: GlyphVsOut) -> @location(0) vec4<f32> {
    let cov = textureSample(atlas_tex, atlas_samp, in.uv).r;
    return vec4<f32>(in.color.rgb, cov);
}

// Colour-emoji glyphs: the atlas (an RGBA8 texture bound in the SAME group-1
// slot) already holds the CPU renderer's final, cell-sized RGBA pixels, so we
// blit them straight through. ALPHA_BLENDING then does `rgb*a + dst*(1-a)` —
// byte-for-byte the CPU `blend(dst, rgb, a)` for the Rgba blit. The vertex
// `color` is ignored (the emoji carries its own colour).
@fragment
fn fs_glyph_color(in: GlyphVsOut) -> @location(0) vec4<f32> {
    return textureSample(atlas_tex, atlas_samp, in.uv);
}
"#;

/// On-glass blit: a fullscreen triangle generated from `@builtin(vertex_index)`
/// (3 verts, no vertex buffer) samples the offscreen frame with NEAREST (1:1,
/// no smear) and writes it straight to the swapchain. When `invert.flag != 0`
/// the RGB is inverted (`1.0 - rgb`) for the visual-bell flash — the GPU twin of
/// the CPU softbuffer `px ^ 0x00ffffff`.
const BLIT_SHADER: &str = r#"
struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// Oversized triangle covering the whole clip rect; UVs map the framebuffer 1:1.
@vertex
fn vs_blit(@builtin(vertex_index) vi: u32) -> VsOut {
    var uv = array<vec2<f32>, 3>(
        vec2<f32>(0.0, 0.0), vec2<f32>(2.0, 0.0), vec2<f32>(0.0, 2.0)
    );
    var o: VsOut;
    o.uv = uv[vi];
    // uv (0..2, y down) -> clip (x: -1..3, y: 1..-3).
    o.pos = vec4<f32>(o.uv.x * 2.0 - 1.0, 1.0 - o.uv.y * 2.0, 0.0, 1.0);
    return o;
}

@group(0) @binding(0) var src_tex: texture_2d<f32>;
@group(0) @binding(1) var src_samp: sampler;
struct Invert { flag: u32, pad0: u32, pad1: u32, pad2: u32 };
@group(0) @binding(2) var<uniform> inv: Invert;

@fragment
fn fs_blit(in: VsOut) -> @location(0) vec4<f32> {
    let c = textureSample(src_tex, src_samp, in.uv);
    if (inv.flag != 0u) {
        return vec4<f32>(1.0 - c.rgb, 1.0);
    }
    return vec4<f32>(c.rgb, 1.0);
}
"#;

/// Blit invert flag, padded to 16 bytes (std140 uniform alignment).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct BlitUniform {
    /// Non-zero inverts RGB (visual-bell flash); zero blits straight through.
    flag: u32,
    _pad: [u32; 3],
}

/// An on-screen presentation target: a configured wgpu swapchain surface that the
/// offscreen frame is blitted into. Opaque (fields private) — the frontend holds
/// it and passes it back to [`GpuRenderer::present_input`] / `resize_surface`.
pub struct GpuSurface {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
}

/// Where a glyph lives in the atlas, plus its placement offsets.
#[derive(Clone, Copy)]
struct GlyphSlot {
    ax: u32,
    ay: u32,
    gw: u32,
    gh: u32,
    xmin: i32,
    ymin: i32,
}

/// Which kind of glyph an [`Atlas`] holds — selects the bytes-per-texel and the
/// glyph-image variant a key is packed from. The mono (R8) and colour (RGBA8)
/// atlases share all packing logic; only this differs.
#[derive(Clone, Copy, PartialEq, Eq)]
enum AtlasKind {
    /// 8-bit coverage (R8), 1 byte/texel. Every requested key gets an entry —
    /// non-mono / empty glyphs pack as zero-sized slots (the mono atlas is the
    /// canonical place to look a key up).
    Mono,
    /// 32-bit colour (RGBA8), 4 bytes/texel. Only non-empty `Rgba` glyphs are
    /// packed; the rest live in the mono atlas.
    Color,
}

impl AtlasKind {
    /// Bytes per texel for this atlas's pixel format.
    fn bpp(self) -> u32 {
        match self {
            AtlasKind::Mono => 1,
            AtlasKind::Color => 4,
        }
    }
}

/// A packed coverage atlas (R8) or colour atlas (RGBA8) + per-glyph placement,
/// keyed by the CPU renderer's [`GlyphKey`] (face + class + char + style + size
/// — the full rasterization identity, so e.g. a styled variant of the same char
/// gets its own slot).
///
/// PERSISTENT across frames: the `data`/`map` and the shelf-packer cursor
/// (`px`,`py`,`shelf_h`) survive so new glyphs can be APPENDED into free space
/// without repacking. `height` is the height the bytes occupy; a glyph appends
/// only while it fits under `cap_h` (the resident GPU texture height) — past
/// that is genuine overflow and the caller repacks fresh.
struct Atlas {
    kind: AtlasKind,
    width: u32,
    height: u32,
    data: Vec<u8>,
    map: HashMap<GlyphKey, GlyphSlot>,
    // Shelf-packer cursor (next free position on the current shelf).
    px: u32,
    py: u32,
    shelf_h: u32,
}

impl Atlas {
    /// Whether this kind packs `img` as a real (non-zero) slot.
    fn packs(kind: AtlasKind, img: &GlyphImage) -> bool {
        let real = img.width() > 0 && img.height() > 0;
        match kind {
            // Mono atlas: only Mono coverage occupies real space (Rgba/empty
            // pack as zero slots, recorded for lookup).
            AtlasKind::Mono => real && matches!(img, GlyphImage::Mono { .. }),
            // Colour atlas: only non-empty Rgba glyphs are packed at all.
            AtlasKind::Color => real && matches!(img, GlyphImage::Rgba { .. }),
        }
    }

    /// Pack `key`/`img` into the current shelves, advancing the cursor and
    /// recording the slot. Returns `Some((ay, gh))` (the y band the glyph
    /// occupies) for a real slot, else `None`. Pure CPU bookkeeping — it does
    /// NOT touch `data`; the caller blits bytes for the returned band.
    fn place(&mut self, key: GlyphKey, img: &GlyphImage) -> Option<(u32, u32)> {
        let pad = 1u32;
        if !Self::packs(self.kind, img) {
            // Mono records every key (zero slot for non-mono/empty) so a lookup
            // there always resolves; Color simply skips.
            if self.kind == AtlasKind::Mono {
                self.map.insert(
                    key,
                    GlyphSlot {
                        ax: 0,
                        ay: 0,
                        gw: 0,
                        gh: 0,
                        xmin: img.xmin(),
                        ymin: img.ymin(),
                    },
                );
            }
            return None;
        }
        let (gw, gh) = (img.width() as u32, img.height() as u32);
        if self.px + gw + pad > self.width {
            self.px = 0;
            self.py += self.shelf_h + pad;
            self.shelf_h = 0;
        }
        let (ax, ay) = (self.px, self.py);
        self.map.insert(
            key,
            GlyphSlot {
                ax,
                ay,
                gw,
                gh,
                xmin: img.xmin(),
                ymin: img.ymin(),
            },
        );
        self.px += gw + pad;
        self.shelf_h = self.shelf_h.max(gh);
        Some((ay, gh))
    }

    /// Blit one glyph's bytes into `data` at its slot (no-op for a zero slot).
    fn blit(&mut self, img: &GlyphImage, slot: &GlyphSlot) {
        if slot.gw == 0 || slot.gh == 0 {
            return;
        }
        let bpp = self.kind.bpp();
        let bytes = img.bytes();
        // A glyph row is `gw` contiguous texels in both source and destination
        // (rows are stored linearly, texels packed at `bpp` each), so each row
        // copies in one memcpy — byte-identical to the old per-texel loop, just
        // without the per-texel bounds-check / call overhead.
        let row_bytes = (slot.gw * bpp) as usize;
        for j in 0..slot.gh {
            let src = ((j * slot.gw) * bpp) as usize;
            let dst = (((slot.ay + j) * self.width + slot.ax) * bpp) as usize;
            self.data[dst..dst + row_bytes].copy_from_slice(&bytes[src..src + row_bytes]);
        }
    }

    /// The height (in texels) the shelves would occupy after packing everything
    /// placed so far, including 1px bottom padding.
    fn occupied_height(&self) -> u32 {
        (self.py + self.shelf_h + 1).max(1)
    }
}

/// Pack every requested glyph's image into one fresh R8 coverage atlas (shelf
/// packer, 1px padding), pulling the EXACT cached bytes from the CPU renderer
/// via [`Renderer::glyph_image`]. A full (re)pack — used on the first frame and
/// on genuine overflow; the steady state APPENDS instead (see `grow_atlas`).
///
/// A free function (not a `GpuRenderer` method) so the atlas-byte-identity
/// unit test can exercise it with no GPU device.
fn build_atlas(cpu: &mut Renderer, keys: &BTreeSet<GlyphKey>, cap_h: u32) -> Atlas {
    build_kind(cpu, keys, AtlasKind::Mono, cap_h)
}

/// Pack every colour-emoji (`GlyphImage::Rgba`) glyph into one fresh RGBA8 atlas
/// (shelf packer, 1px padding), pulling the EXACT cached pixels from the CPU
/// renderer. The CPU already scaled each emoji to its final on-cell size, so the
/// GPU blits these 1:1 with NEAREST sampling — exact bytes, like the mono path.
/// Mono and empty glyphs are skipped here (they live in the R8 atlas).
fn build_color_atlas(cpu: &mut Renderer, keys: &BTreeSet<GlyphKey>, cap_h: u32) -> Atlas {
    build_kind(cpu, keys, AtlasKind::Color, cap_h)
}

/// Shared full-pack for either [`AtlasKind`]: place every key, then blit its
/// bytes. `data` is sized to the occupied height once packing is known, so it
/// holds exactly the packed shelves (no slack) — byte-identical to the old
/// per-kind packers.
fn build_kind(cpu: &mut Renderer, keys: &BTreeSet<GlyphKey>, kind: AtlasKind, cap_h: u32) -> Atlas {
    let mut atlas = Atlas {
        kind,
        width: ATLAS_WIDTH,
        height: 1,
        data: Vec::new(),
        map: HashMap::new(),
        px: 0,
        py: 0,
        shelf_h: 0,
    };
    // Borrow each cached image just long enough to record placement; defer the
    // byte blit until `data` is allocated. Collect the (key, slot) pairs whose
    // bytes we still need so we can re-borrow the cache per glyph (no clone).
    let mut placed: Vec<GlyphKey> = Vec::new();
    for &key in keys {
        let img = cpu.glyph_image(key);
        // Snapshot the shelf cursor so a glyph that would push the packed height
        // past `cap_h` (the device's max 2D texture dimension) can be ROLLED BACK
        // and packing stopped — creating a texture taller than the GPU allows would
        // abort the device. The skipped glyphs find no slot (render nothing) only in
        // the pathological overflow case (thousands of distinct glyphs); for every
        // real workload `cap_h` is far above the packed height and nothing is
        // dropped, so this is byte-identical to the unbounded pack.
        let (sx, sy, sh) = (atlas.px, atlas.py, atlas.shelf_h);
        if atlas.place(key, img).is_some() {
            if atlas.occupied_height() > cap_h {
                atlas.map.remove(&key);
                atlas.px = sx;
                atlas.py = sy;
                atlas.shelf_h = sh;
                break;
            }
            placed.push(key);
        }
    }
    atlas.height = atlas.occupied_height();
    atlas.data = vec![0u8; (atlas.width * atlas.height * kind.bpp()) as usize];
    for key in placed {
        let slot = atlas.map[&key];
        let img = cpu.glyph_image(key);
        atlas.blit(img, &slot);
    }
    atlas
}

/// Append `new_keys` (each NOT already resident) into `atlas`'s free space,
/// keeping every existing slot put. `cap_h` is the resident GPU texture height
/// the bytes must still fit under. Returns `Some((dirty_y0, dirty_y1))` — the
/// half-open row band that changed and must be re-uploaded — on success, or
/// `None` on genuine overflow (the caller must full-repack into a taller
/// texture). On `None` the atlas is left UNMODIFIED.
fn grow_atlas(
    cpu: &mut Renderer,
    atlas: &mut Atlas,
    new_keys: &[GlyphKey],
    cap_h: u32,
) -> Option<(u32, u32)> {
    // Dry-run placement on a scratch copy of the cursor so a mid-list overflow
    // does not leave the atlas half-grown.
    let (sx, sy, sh) = (atlas.px, atlas.py, atlas.shelf_h);
    let mut probe = Atlas {
        kind: atlas.kind,
        width: atlas.width,
        height: atlas.height,
        data: Vec::new(),
        map: HashMap::new(),
        px: sx,
        py: sy,
        shelf_h: sh,
    };
    let mut dirty_lo = u32::MAX;
    let mut dirty_hi = 0u32;
    for &key in new_keys {
        let img = cpu.glyph_image(key);
        if let Some((ay, gh)) = probe.place(key, img) {
            dirty_lo = dirty_lo.min(ay);
            dirty_hi = dirty_hi.max(ay + gh);
        }
    }
    let need_h = probe.occupied_height();
    if need_h > cap_h {
        return None; // genuine overflow: caller repacks fresh into a new texture
    }
    if dirty_hi == 0 {
        // All new keys packed as zero slots (mono only) — record them, no upload.
        for &key in new_keys {
            let img = cpu.glyph_image(key);
            atlas.place(key, img);
        }
        return Some((0, 0));
    }

    // Commit: replay the placement on the real atlas (cursor advances exactly as
    // the probe did) and blit each new glyph's bytes. `data` is grown to the
    // occupied height first so the new band has backing storage.
    let new_total = (atlas.width * need_h * atlas.kind.bpp()) as usize;
    if atlas.data.len() < new_total {
        atlas.data.resize(new_total, 0);
    }
    atlas.height = need_h.max(atlas.height);
    for &key in new_keys {
        let img = cpu.glyph_image(key);
        // `place` records every key (zero slot for non-mono/empty too); only a
        // real slot needs its bytes blitted into the dirty band.
        if atlas.place(key, img).is_some() {
            let slot = atlas.map[&key];
            atlas.blit(img, &slot);
        }
    }
    Some((dirty_lo, dirty_hi))
}

/// One persisted, on-GPU glyph atlas: the CPU-side packed [`Atlas`] (so we can
/// append new glyphs into free space) alongside its resident texture + bind
/// group and the texture's dims. Lives across frames — the whole point of the
/// atlas is that idle frames reuse it untouched.
struct ResidentAtlas {
    atlas: Atlas,
    tex: wgpu::Texture,
    bind: wgpu::BindGroup,
    /// The resident texture's height (texels). New glyphs may append while the
    /// packed shelves stay under this; beyond it is overflow → a new texture.
    tex_h: u32,
}

/// Per-window GPU state: the offscreen render target the window draws into and
/// blits from, its dirty-gate cache, and the last-written uniform/ blit-invert
/// memo. One per logical window. The device, glyph atlas, and pipelines live on
/// the shared `GpuRenderer`; only these are per-window, so N windows cost ~1
/// device + 1 atlas + N small offscreens.
#[derive(Default)]
pub struct WindowGpu {
    // The resident offscreen render target + its blit-source bind group. `None`
    // until the first frame; reused at the same `(w, h)`, recreated only on a
    // dimension change. See `Offscreen`.
    pub(crate) offscreen: Option<Offscreen>,
    // DIRTY-GATE cache for the per-frame PRESENTATION hot path
    // (`render_input_cached`). Holds the previous frame's input + cursor state +
    // the pixels that were read back for it. When the next frame is PIXEL-
    // IDENTICAL (per `is_unchanged_frame`), we re-present these cached pixels and
    // do ZERO GPU work — no encode, no submit, no `device.poll`, no readback.
    pub(crate) gate_cache: Option<GpuGateCache>,
    // Last `(w, h)` written into the screen uniform. `Uniforms.screen` is a pure
    // function of the frame size, so it only needs (re)writing on the first frame
    // and on a resize — NOT every frame. `None` forces the first write.
    pub(crate) uniform_dims: Option<(u32, u32)>,
    // Last invert flag written into the blit uniform. The blit uniform is a pure
    // function of `invert`, so it is rewritten only when the flag changes. `None`
    // forces the first write.
    pub(crate) blit_invert: Option<bool>,
    // Decoded-image LRU for the inline-image (iTerm2 OSC 1337) pixel pass. Keyed
    // by `(arc_ptr, fp_w, fp_h)` like the CPU `ImageCache`, so each distinct
    // placement is decoded+scaled at most once. PER-WINDOW so window B's images
    // never leak into window A. Empty in the common image-free case.
    pub(crate) image_cache: GpuImageCache,
    // The per-frame inline-image texture (every distinct visible image stacked)
    // and its bind group. `None` until the first image frame; rebuilt when the
    // set of visible images changes, reused otherwise. PER-WINDOW. Cleared to
    // `None` on an image-free frame so nothing is bound or drawn.
    pub(crate) image_plane: Option<ImagePlane>,
    // SCISSORED DIRTY-ROW REPAINT (the window present path). Holds the PREVIOUS
    // presented frame's input + the renderer cursor state it was drawn with, so
    // `encode_present_frame` can consult `compute_dirty_rows` against it and
    // re-encode only the dirty rows (LoadOp::Load + a scissor over the dirty
    // band) into this window's persistent offscreen — which still holds that
    // prior frame. `None` until the first present (forces a full repaint), and
    // reset on any geometry change (the offscreen is recreated, so its prior
    // contents are gone). PER-WINDOW: window B's prior frame must never be
    // diffed against window A's input on the SHARED `GpuRenderer`. See
    // `encode_frame` / `RepaintScope`.
    pub(crate) present_prev: Option<PresentPrev>,
    // The PREVIOUS presented frame's input snapshot, kept RESIDENT across frames
    // (never reallocated in steady state): `encode_present_frame` updates it via
    // `clone_from` (Vec::clone_from reuses the destination's grid allocation when
    // dims are stable — a changed frame at the same size does ZERO grid alloc,
    // just a memcpy into the retained per-row buffers; only an actual
    // cluster/combining cell allocates its Box<str>/Box<[char]>, never on the
    // ASCII path). It holds a VALID prior frame iff `present_prev` is `Some`; the
    // reset-on-`render_input` / `render_no_readback` path clears `present_prev`
    // (invalidating it) but keeps the buffer's capacity for the next present's
    // `clone_from`. PER-WINDOW alongside `present_prev`.
    pub(crate) prev_input: RenderInput,
}

impl WindowGpu {
    pub fn new() -> Self {
        Self::default()
    }

    /// Drop this window's prior-frame validity so the NEXT `present_input` is a
    /// FULL repaint (Clear + all rows) rather than a scissored dirty-row diff.
    /// Needed after a theme change: the steady selection band, idle cursor, and
    /// padding border are theme-derived but NOT content, so the dirty-row diff
    /// would leave them painted in the OLD theme until cell content changes.
    pub fn invalidate_present(&mut self) {
        self.present_prev = None;
    }
}

/// One decoded + footprint-scaled inline image, ready to upload as an RGBA8
/// texture region. Mirrors the CPU renderer's `DecodedImage`: the same
/// `decode_image_to_footprint` bytes, so the GPU samples (NEAREST, per cell) the
/// exact pixels the CPU `blit_image_cell` copies — the CPU/GPU parity gate.
struct GpuDecodedImage {
    /// Footprint pixel width (`cols * cell_w`).
    w: u32,
    /// Footprint pixel height (`rows * cell_h`).
    h: u32,
    /// `w * h * 4` straight-alpha RGBA bytes, or empty if the decode failed
    /// (a cached negative result: the image draws nothing but is not re-decoded).
    rgba: Vec<u8>,
}

/// Decoded-image LRU for the GPU renderer's inline-image pass. Keyed identically
/// to the CPU `ImageCache` — `(arc_ptr, fp_w, fp_h)` — so each distinct placement
/// is decoded+scaled at most once and reused across frames (idle image frames do
/// no decode work). Empty (zero footprint) in the common image-free case.
#[derive(Default)]
pub(crate) struct GpuImageCache {
    /// `(arc_ptr, fp_w, fp_h) -> decoded`, MRU at the back.
    entries: Vec<((usize, usize, usize), GpuDecodedImage)>,
}

impl GpuImageCache {
    /// Maximum distinct decoded images retained — matches the CPU cap.
    const MAX: usize = 8;

    /// Look up a decoded image by key, promoting it to MRU on a hit.
    fn get(&mut self, key: (usize, usize, usize)) -> Option<&GpuDecodedImage> {
        let idx = self.entries.iter().position(|(k, _)| *k == key)?;
        let entry = self.entries.remove(idx);
        self.entries.push(entry);
        self.entries.last().map(|(_, v)| v)
    }

    /// Immutable lookup that does NOT promote to MRU — for batch reads (the
    /// image-plane pack) that hold several entries borrowed at once, where a
    /// `&mut self` `get` per item would conflict. Recency is unaffected: the
    /// pack runs right after the placements loop already promoted every placed
    /// key in `order` sequence, so re-promoting here would be a no-op anyway.
    fn peek(&self, key: (usize, usize, usize)) -> Option<&GpuDecodedImage> {
        self.entries.iter().find(|(k, _)| *k == key).map(|(_, v)| v)
    }

    /// Insert a freshly decoded image, evicting the LRU entry past the cap.
    fn put(&mut self, key: (usize, usize, usize), value: GpuDecodedImage) {
        if self.entries.len() >= Self::MAX {
            self.entries.remove(0);
        }
        self.entries.push((key, value));
    }
}

/// The per-frame inline-image texture: every DISTINCT image placement visible
/// this frame, stacked vertically into one RGBA8 texture, plus a map from
/// `(arc_ptr, fp_w, fp_h)` to the y-row at which that image's footprint begins.
/// A covered cell's quad samples its tile at `(cell_col*cw, image_y0 + cell_row*ch)`.
/// Rebuilt only on frames that actually carry images; `None`/cleared otherwise, so
/// image-free frames bind nothing and stay byte-identical to the pre-image path.
pub(crate) struct ImagePlane {
    /// The bind group samples the per-frame image texture; it owns a
    /// `TextureView` of that texture (which keeps the texture itself alive), so no
    /// separate `tex` handle is retained here.
    bind: wgpu::BindGroup,
    /// Texture dims (texels) — the divisor for the sampled UVs.
    w: u32,
    h: u32,
    /// `(arc_ptr, fp_w, fp_h) -> (y0_in_texture, fp_w, fp_h)` for each placement.
    placements: HashMap<(usize, usize, usize), (u32, u32, u32)>,
}

/// GPU terminal renderer. Holds its own GPU device (via [`GpuContext`]) and a CPU
/// [`Renderer`] used purely for font metrics + glyph coverage, so geometry and
/// rasterization match the CPU renderer exactly.
pub struct GpuRenderer {
    ctx: GpuContext,
    cpu: Renderer,
    /// The configured font family (`font_family` config / `$ATERM_FONT`), kept so an
    /// IN-PLACE font/theme rebuild (`set_font_theme`: zoom, config hot-reload, Retina
    /// auto-scale) re-resolves the SAME family instead of silently falling back to the
    /// system monospace. Without this, a Retina rebuild on the first frame dropped a
    /// configured family out of the box on the GPU backend.
    // Read only by the native font-discovery rebuild (`set_font_theme`), which is
    // cfg'd out on wasm (no system fonts in the browser) — hence unused there.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    font_family: Option<String>,
    theme: Theme,
    uniform_buf: wgpu::Buffer,
    uniform_bg: wgpu::BindGroup,
    atlas_bgl: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    bg_pipeline: wgpu::RenderPipeline,
    glyph_pipeline: wgpu::RenderPipeline,
    color_glyph_pipeline: wgpu::RenderPipeline,
    // On-glass blit (the offscreen frame -> swapchain), built once and reused.
    blit_shader: wgpu::ShaderModule,
    blit_bgl: wgpu::BindGroupLayout,
    blit_layout: wgpu::PipelineLayout,
    blit_sampler: wgpu::Sampler,
    blit_uniform_buf: wgpu::Buffer,
    /// Blit pipelines keyed by swapchain format. Built EAGERLY in
    /// `create_window_surface` (the format is known there) so the compile is off
    /// the first-FRAME path; `present_input` then only looks one up.
    /// `ensure_blit_pipeline` remains the idempotent lazy fallback (and the
    /// test/readback path's builder) for any format not seen at surface-create.
    blit_pipelines: HashMap<wgpu::TextureFormat, wgpu::RenderPipeline>,
    /// Cached swapchain-format choice: the NON-sRGB format `pick_surface_format`
    /// picks is adapter/platform-stable (Bgra8Unorm on macOS/Metal, offered by
    /// every on-screen surface on this single adapter), so the blocking
    /// `surface.get_capabilities` round-trip is done ONCE and reused for later
    /// window-attaches. Clear this to restore per-attach querying.
    cached_surface_format: Option<wgpu::TextureFormat>,
    /// Persisted mono (R8) + colour (RGBA8) atlases — `None` until the first
    /// frame builds them. Reused untouched when a frame's glyph set is a subset
    /// of what's resident; grown incrementally on a miss.
    mono_res: Option<ResidentAtlas>,
    color_res: Option<ResidentAtlas>,
    /// The full glyph-key set currently resident across both atlases. A frame
    /// whose keys are a SUBSET of this skips all atlas work.
    resident_keys: BTreeSet<GlyphKey>,
    /// Count of atlas textures created (full (re)builds, not reuses). The
    /// persistence test asserts this does NOT advance across an unchanged frame.
    atlas_tex_creations: u64,
    // Persistent per-frame vertex streams. Previously these were allocated from
    // scratch every frame via `create_buffer_init` (one driver allocation per
    // stream per frame). Now each stream owns a buffer that is reused across
    // frames: we `write_buffer` into it when the frame's bytes fit the current
    // capacity, and only recreate (grow) the buffer when they don't. Capacities
    // are tracked alongside so we know when a grow is needed.
    vbufs: VertexBuffers,
    // Persistent per-frame instance streams + glyph-key set. Cleared (capacity
    // retained) at the top of each `encode_frame` instead of re-allocated. See
    // `Instances`.
    inst: Instances,
    // TEST/DIAGNOSTIC counters so a test can prove the gate is actually taken
    // (otherwise a "gate" that never fires would still pass a byte-identity
    // test). Counts gate-hits and gate-misses through `render_input_cached`.
    gate_hits: u64,
    gate_misses: u64,
    // NOTE: present_prev (Option<PresentPrev>) and prev_input (RenderInput) moved
    // to per-window `WindowGpu` so the scissored present path diffs each window's
    // input against ITS OWN prior frame — the shared GpuRenderer must hold no
    // per-window prior-frame state, or window B's present would diff against
    // window A's last frame and corrupt the scissor decision.
    // Persistent dirty-row scratch for the scissored present path. The
    // `&mut Vec<bool>` handed to `compute_dirty_rows` each present, taken out of
    // `self` across the encode and restored after. Resident across frames so a
    // stable-dimension changed frame allocates no per-call dirty Vec. The flags —
    // and thus the scissor decision — are byte-identical to the old per-call Vec.
    dirty_scratch: Vec<bool>,
    // TEST/DIAGNOSTIC counters: how many `present_input` frames took the SCISSOR
    // (dirty-row) path vs a FULL repaint. The byte-identity test asserts the
    // scissor path is actually exercised on typing/cursor frames and that
    // DECDHL/DECDWL... no: DECDWL is safe; DECDHL/selection/scroll frames fall
    // back to full.
    scissor_taken: u64,
    full_repaints: u64,
    // TEST/DIAGNOSTIC: total instances built in the LAST `encode_frame` (sum of
    // all eight streams). A scissored 1-row frame builds ~`1/rows` of a full
    // frame's instances — the proportional-to-dirty-rows win the benchmark
    // reports.
    last_instances: usize,
    // NOTE: image_cache (GpuImageCache) and image_plane (Option<ImagePlane>) moved
    // to per-window `WindowGpu` so window B's inline images never leak into window
    // A — the shared GpuRenderer must hold no per-window image state.
}

/// The previous presented frame's overlay state, for the scissored dirty-row
/// repaint. The persistent offscreen still holds this frame's pixels, so the next
/// present can update only the rows that differ from it.
///
/// The prior input SNAPSHOT itself lives in the always-resident per-window
/// `WindowGpu::prev_input` buffer (updated via `clone_from`, reusing its
/// allocation) rather than here — so a stable-dimension changed frame stores the
/// new prior frame with ZERO grid allocation. This struct, when `Some`, is the
/// validity flag for that buffer: it is `Some` exactly when `prev_input` holds a
/// VALID prior presented frame at the current offscreen dims.
pub(crate) struct PresentPrev {
    /// The blink phase that frame was drawn with.
    blink_phase: bool,
    /// The cursor-style override that frame was drawn with.
    cursor_style_override: Option<CursorStyle>,
}

/// What portion of the offscreen `encode_frame` must repaint:
///   * `Full` — clear the whole target and draw every row (the always-correct
///     path; byte-identical to the original encode).
///   * `Dirty(dirty)` — the persistent offscreen already holds the prior frame;
///     preserve it (`LoadOp::Load`), scissor to the dirty rows' bounding band, and
///     draw ONLY the dirty rows (`dirty[r]`). A re-shaded dirty row gets the
///     IDENTICAL instances the full path would build for it, so its pixels are
///     bit-identical; untouched rows are preserved by Load.
///
/// `Dirty` BORROWS the per-row flags from the caller's persistent dirty scratch
/// (`GpuRenderer::dirty_scratch`, taken out across the encode) rather than owning
/// a fresh `Vec`, so a changed frame allocates no dirty Vec. The flags are
/// byte-identical to the old owned form — only the allocation lifetime changed.
enum RepaintScope<'a> {
    Full,
    Dirty(&'a [bool]),
}

/// The GPU dirty-gate cache: the previous frame's `render_input_cached` inputs
/// and the pixels that were rendered + read back for them. Because the GPU has
/// no persistent CPU-side framebuffer to borrow (it renders on-device and reads
/// back), the gate must remember the prior frame's pixels itself so it can re-
/// present them on an unchanged frame.
pub(crate) struct GpuGateCache {
    /// The previous frame's input snapshot (cloned), for the gate comparison.
    input: RenderInput,
    /// The blink phase the previous frame was drawn with.
    blink_phase: bool,
    /// The cursor-style override the previous frame was drawn with.
    cursor_style_override: Option<CursorStyle>,
    /// The pixels read back for the previous frame (the cached framebuffer the
    /// gate re-presents verbatim on a hit). Byte-identical to what the GPU would
    /// re-render for an unchanged input.
    frame: Frame,
}

/// The persistent offscreen render target (the frame the GPU draws into and the
/// blit samples from). Previously a fresh `Rgba8Unorm` texture + view was created
/// EVERY presented frame (~6.4 MB at 1080p), and the blit-source view + blit bind
/// group were rebuilt EVERY present. Now they are resident: a frame at the same
/// `(w, h)` reuses `tex`/`view`/`blit_bind` untouched; only a `None` field or a
/// dimension change (resize) recreates them. Lifetime-only change — the same draws
/// land in the same texture, so the swapchain blit stays byte-identical.
pub(crate) struct Offscreen {
    tex: wgpu::Texture,
    view: wgpu::TextureView,
    /// The blit-source bind group (samples `tex` into the swapchain). Built ONCE
    /// when the offscreen is (re)created and reused every present.
    blit_bind: wgpu::BindGroup,
    w: u32,
    h: u32,
}

/// The persistent per-frame instance streams + glyph-key set. Previously fresh
/// `Vec`s and a `BTreeSet` were allocated EVERY `encode_frame`; now they are
/// hoisted and `.clear()`ed (capacity retained) at the start of each frame, so the
/// steady state does zero heap allocation for them. Identical contents built in
/// identical order → byte-identical. Field order mirrors `VertexBuffers`.
#[derive(Default)]
struct Instances {
    keys: BTreeSet<GlyphKey>,
    bg: Vec<BgInstance>,
    /// Inline-image (iTerm2 OSC 1337) cell tiles: one quad per image-covered cell
    /// sampling its tile of the per-frame image texture. Drawn AFTER the colour
    /// glyphs (the image owns the cell, so no glyph competes) and BEFORE the
    /// decorations/cursor — over the cell bg, alpha-blended, exactly like the CPU
    /// `blit_image_cell` straight-alpha-over composite. Empty (and so a no-op) for
    /// every image-free frame, keeping the text path byte-identical.
    image: Vec<GlyphInstance>,
    glyph: Vec<GlyphInstance>,
    color: Vec<GlyphInstance>,
    cursor: Vec<BgInstance>,
    deco: Vec<BgInstance>,
    cursor_block: Vec<BgInstance>,
    cursor_glyph: Vec<GlyphInstance>,
    cursor_color: Vec<GlyphInstance>,
}

impl Instances {
    /// Empty all streams (retaining capacity) for a fresh frame.
    fn clear(&mut self) {
        self.keys.clear();
        self.bg.clear();
        self.image.clear();
        self.glyph.clear();
        self.color.clear();
        self.cursor.clear();
        self.deco.clear();
        self.cursor_block.clear();
        self.cursor_glyph.clear();
        self.cursor_color.clear();
    }
}

/// The eight persistent per-frame vertex streams (one `VertexBuffer` each).
/// Field order/labels mirror the instance vecs built in `encode_frame`.
struct VertexBuffers {
    bg: VertexBuffer,
    image: VertexBuffer,
    glyph: VertexBuffer,
    color: VertexBuffer,
    cursor: VertexBuffer,
    deco: VertexBuffer,
    cursor_block: VertexBuffer,
    cursor_glyph: VertexBuffer,
    cursor_color: VertexBuffer,
}

impl VertexBuffers {
    fn new(device: &wgpu::Device) -> Self {
        Self {
            bg: VertexBuffer::new(device, "aterm-gpu bg instances"),
            image: VertexBuffer::new(device, "aterm-gpu image instances"),
            glyph: VertexBuffer::new(device, "aterm-gpu glyph instances"),
            color: VertexBuffer::new(device, "aterm-gpu colour glyph instances"),
            cursor: VertexBuffer::new(device, "aterm-gpu cursor instances"),
            deco: VertexBuffer::new(device, "aterm-gpu decoration instances"),
            cursor_block: VertexBuffer::new(device, "aterm-gpu cursor block fill"),
            cursor_glyph: VertexBuffer::new(device, "aterm-gpu cursor cut-out glyph"),
            cursor_color: VertexBuffer::new(device, "aterm-gpu cursor colour glyph"),
        }
    }
}

/// A reusable `VERTEX | COPY_DST` buffer plus its byte capacity. Grows (recreates
/// the underlying buffer) only when a frame's contents exceed `capacity`.
struct VertexBuffer {
    buf: wgpu::Buffer,
    capacity: u64,
    label: &'static str,
}

impl VertexBuffer {
    /// Start at zero capacity; the first non-empty upload grows it. No GPU
    /// allocation happens for streams that are never used (e.g. colour-emoji
    /// buffers on a frame with no emoji).
    fn new(device: &wgpu::Device, label: &'static str) -> Self {
        Self {
            buf: Self::alloc(device, label, 0),
            capacity: 0,
            label,
        }
    }

    fn alloc(device: &wgpu::Device, label: &'static str, size: u64) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    /// Upload `bytes` into the buffer, growing it first if they don't fit.
    /// Returns a slice of exactly `bytes.len()` bytes ready to bind, or `None`
    /// when there is nothing to draw (empty stream) — the caller skips that pass,
    /// exactly as the old `Option<Buffer>` gating did. Identical contents and
    /// draw counts to the per-frame-allocated path; only the buffer's lifetime
    /// changes.
    fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bytes: &[u8],
    ) -> Option<wgpu::BufferSlice<'_>> {
        // The slice-precondition decision (the GpuEncode.tla `NeverSliceEmpty` rule):
        // a buffer is bound/sliced ONLY when it holds at least one instance. An empty
        // stream returns `None` so the caller skips the draw (wgpu panics on an empty
        // `buf.slice(..)` — the exact 4ab4eb9 bug). Factored through `should_slice`
        // so the real precondition is testable headlessly without a GPU.
        if !should_slice(bytes.len()) {
            return None;
        }
        // `write_buffer` requires the write size to be a multiple of
        // COPY_BUFFER_ALIGNMENT (4). Our instance structs are 16-byte aligned so
        // `bytes.len()` is already a multiple of 4, but round up defensively and
        // ensure capacity covers the padded length.
        let needed = align_up(bytes.len() as u64, wgpu::COPY_BUFFER_ALIGNMENT);
        if needed > self.capacity {
            // Grow geometrically (next power of two, padded) to amortise the cost
            // of bursts that keep enlarging the stream, then recreate the buffer.
            let new_cap = align_up(needed.next_power_of_two(), wgpu::COPY_BUFFER_ALIGNMENT);
            self.buf = Self::alloc(device, self.label, new_cap);
            self.capacity = new_cap;
        }
        queue.write_buffer(&self.buf, 0, bytes);
        Some(self.buf.slice(..bytes.len() as u64))
    }
}

/// The slice-precondition decision shared by every per-frame vertex stream — the
/// real implementation of `GpuEncode.tla`'s `NeverSliceEmpty` / `SliceImpliesFill`
/// rule: a stream is sliced/bound ONLY when it holds at least one instance
/// (`byte_len > 0`). The bg-instance path (the `Encode` action) calls
/// `bg_buf.slice(..)` exactly when this is `true`; an empty (zero-cell) frame
/// returns `None` from [`InstanceBuf::upload`] and draws nothing — the exact fix for
/// the wgpu "buffer slices can not be empty" panic (4ab4eb9). Pure + GPU-free so the
/// precondition is conformance-checked headlessly (`tests/conformance_gpuencode.rs`).
#[must_use]
pub fn should_slice(byte_len: usize) -> bool {
    byte_len != 0
}

/// Round `n` up to the next multiple of `align` (a power of two).
fn align_up(n: u64, align: u64) -> u64 {
    (n + align - 1) & !(align - 1)
}

/// Build the per-frame uniform buffer, its (vertex-visible) bind-group layout,
/// and the bind group that wires the buffer to binding 0. Extracted from
/// [`GpuRenderer::new_with_family`] verbatim.
fn build_uniform_resources(
    device: &wgpu::Device,
) -> (wgpu::Buffer, wgpu::BindGroupLayout, wgpu::BindGroup) {
    let uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("aterm-gpu uniforms"),
        size: std::mem::size_of::<Uniforms>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let uniform_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("aterm-gpu uniform bgl"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    });
    let uniform_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("aterm-gpu uniform bg"),
        layout: &uniform_bgl,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: uniform_buf.as_entire_binding(),
        }],
    });

    (uniform_buf, uniform_bgl, uniform_bg)
}

/// Build the glyph-atlas bind-group layout (a fragment-visible texture +
/// sampler) and the NEAREST sampler used to read it. Extracted from
/// [`GpuRenderer::new_with_family`] verbatim.
fn build_atlas_resources(device: &wgpu::Device) -> (wgpu::BindGroupLayout, wgpu::Sampler) {
    let atlas_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("aterm-gpu atlas bgl"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });

    // NEAREST: the atlas holds the exact CPU coverage bytes; nearest sampling
    // at texel centres reproduces them with no interpolation smear.
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("aterm-gpu nearest"),
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        mipmap_filter: wgpu::MipmapFilterMode::Nearest,
        ..Default::default()
    });

    (atlas_bgl, sampler)
}

/// Build the three offscreen render pipelines that draw into the `Rgba8Unorm`
/// framebuffer: the per-cell background fill, the coverage-blended mono glyph
/// pass, and the straight-RGBA colour-emoji pass. Extracted from
/// [`GpuRenderer::new_with_family`] verbatim. The bg pipeline binds only the
/// uniforms; both glyph pipelines additionally bind the atlas.
fn build_cell_pipelines(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    uniform_bgl: &wgpu::BindGroupLayout,
    atlas_bgl: &wgpu::BindGroupLayout,
    target: wgpu::TextureFormat,
) -> (
    wgpu::RenderPipeline,
    wgpu::RenderPipeline,
    wgpu::RenderPipeline,
) {
    let bg_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("aterm-gpu bg layout"),
        bind_group_layouts: &[Some(uniform_bgl)],
        immediate_size: 0,
    });
    let glyph_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("aterm-gpu glyph layout"),
        bind_group_layouts: &[Some(uniform_bgl), Some(atlas_bgl)],
        immediate_size: 0,
    });

    let bg_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("aterm-gpu bg pipeline"),
        layout: Some(&bg_layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_bg"),
            compilation_options: Default::default(),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<BgInstance>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &BG_ATTRS,
            }],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_bg"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: target,
                blend: Some(wgpu::BlendState::REPLACE),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    });

    let glyph_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("aterm-gpu glyph pipeline"),
        layout: Some(&glyph_layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_glyph"),
            compilation_options: Default::default(),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<GlyphInstance>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &GLYPH_ATTRS,
            }],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_glyph"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: target,
                // out = fg*cov + dst*(1-cov): exactly the CPU `blend`.
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    });

    // Colour-emoji pipeline: same layout/vertex/blend as the mono glyph
    // pipeline, but the `fs_glyph_color` fragment samples an RGBA8 atlas
    // straight (no coverage tint). Reuses `atlas_bgl` — RGBA8Unorm is a
    // filterable float texture, so the layout is identical.
    let color_glyph_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("aterm-gpu colour-glyph pipeline"),
        layout: Some(&glyph_layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_glyph"),
            compilation_options: Default::default(),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<GlyphInstance>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &GLYPH_ATTRS,
            }],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_glyph_color"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: target,
                // out = rgb*a + dst*(1-a): exactly the CPU `blend` for Rgba.
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    });

    (bg_pipeline, glyph_pipeline, color_glyph_pipeline)
}

/// Build the format-independent on-glass blit infrastructure: the blit shader,
/// its bind-group layout (texture + sampler + invert uniform), the pipeline
/// layout, the NEAREST blit sampler, and the invert uniform buffer. The blit
/// pipeline itself depends on the swapchain format, so it is built lazily per
/// surface format in `present_input` and cached in `blit_pipelines`. Extracted
/// from [`GpuRenderer::new_with_family`] verbatim.
fn build_blit_resources(
    device: &wgpu::Device,
) -> (
    wgpu::ShaderModule,
    wgpu::BindGroupLayout,
    wgpu::PipelineLayout,
    wgpu::Sampler,
    wgpu::Buffer,
) {
    let blit_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("aterm-gpu blit shader"),
        source: wgpu::ShaderSource::Wgsl(BLIT_SHADER.into()),
    });
    let blit_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("aterm-gpu blit bgl"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    });
    let blit_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("aterm-gpu blit layout"),
        bind_group_layouts: &[Some(&blit_bgl)],
        immediate_size: 0,
    });
    // NEAREST: a 1:1 framebuffer->swapchain blit, no interpolation smear.
    let blit_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("aterm-gpu blit nearest"),
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        mipmap_filter: wgpu::MipmapFilterMode::Nearest,
        ..Default::default()
    });
    let blit_uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("aterm-gpu blit invert uniform"),
        size: std::mem::size_of::<BlitUniform>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    (
        blit_shader,
        blit_bgl,
        blit_layout,
        blit_sampler,
        blit_uniform_buf,
    )
}

impl GpuRenderer {
    /// Acquire a GPU and a CPU font face. `px`/`theme` must match the CPU
    /// renderer you want to reproduce.
    ///
    /// NATIVE ONLY: uses `pollster::block_on` (GPU init) + `std::thread::spawn`
    /// (font load) + system font discovery, none of which exist on the browser
    /// wasm target. The wasm path builds a [`GpuContext`] asynchronously and a CPU
    /// [`Renderer`] from injected font bytes, then assembles the renderer via
    /// [`GpuRenderer::from_parts`].
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new(px: f32, theme: Theme) -> Result<Self, String> {
        Self::new_with_family(None, px, theme)
    }

    /// Like [`GpuRenderer::new`], but resolves a configured font FAMILY first
    /// (then `$ATERM_FONT`, then the built-in candidates), mirroring the CPU
    /// renderer's [`Renderer::from_system_with_family`]. `None` is identical to
    /// [`GpuRenderer::new`]. NATIVE ONLY (see [`GpuRenderer::new`]).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new_with_family(family: Option<&str>, px: f32, theme: Theme) -> Result<Self, String> {
        // Cold-launch overlap: font resolution + rasterization (CPU-bound, GPU-
        // independent) and GPU adapter/device init (blocking driver round-trips)
        // are the two dominant serial costs of building the renderer. Run the font
        // load on a background thread while the GPU device initializes on this
        // thread, then join. They share no state (the font path touches no GPU
        // object), so this is pure scheduling — no work is eliminated, the two
        // legs just overlap, saving ~min(gpu_init, font_load) off cold start.
        let family_owned = family.map(String::from);
        let font_handle = std::thread::spawn(move || {
            let mut cpu = Renderer::from_system_with_family(family_owned.as_deref(), px, theme)?;
            // Warm the printable-ASCII glyph cache here (still off the critical path,
            // overlapping GPU init) so the first frame's atlas build doesn't
            // rasterize them on the hot path. Byte-identical output (cache fill only).
            cpu.prewarm_ascii();
            Some(cpu)
        });
        let ctx = GpuContext::new()?;
        let cpu = font_handle
            .join()
            .map_err(|_| "font-load thread panicked".to_string())?
            .ok_or("no system monospace font")?;
        Self::from_parts(ctx, cpu, family.map(String::from), theme)
    }

    /// Assemble a `GpuRenderer` from an already-acquired [`GpuContext`] and a
    /// pre-built CPU [`Renderer`] (font face). This is the PORTABLE core that does
    /// no GPU acquisition, no threads, and no font discovery — every wgpu pipeline
    /// is built here. The native constructors call it after their blocking init;
    /// the wasm WebGPU path calls it after awaiting the device + building the CPU
    /// face from injected font bytes (`Renderer::from_bytes`).
    pub fn from_parts(
        ctx: GpuContext,
        cpu: Renderer,
        font_family: Option<String>,
        theme: Theme,
    ) -> Result<Self, String> {
        let device = &ctx.device;

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("aterm-gpu shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });

        let (uniform_buf, uniform_bgl, uniform_bg) = build_uniform_resources(device);
        let (atlas_bgl, sampler) = build_atlas_resources(device);

        let target = wgpu::TextureFormat::Rgba8Unorm;
        let (bg_pipeline, glyph_pipeline, color_glyph_pipeline) =
            build_cell_pipelines(device, &shader, &uniform_bgl, &atlas_bgl, target);

        let (blit_shader, blit_bgl, blit_layout, blit_sampler, blit_uniform_buf) =
            build_blit_resources(device);

        let vbufs = VertexBuffers::new(device);

        Ok(Self {
            ctx,
            cpu,
            font_family,
            theme,
            uniform_buf,
            uniform_bg,
            atlas_bgl,
            sampler,
            bg_pipeline,
            glyph_pipeline,
            color_glyph_pipeline,
            blit_shader,
            blit_bgl,
            blit_layout,
            blit_sampler,
            blit_uniform_buf,
            blit_pipelines: HashMap::new(),
            cached_surface_format: None,
            mono_res: None,
            color_res: None,
            resident_keys: BTreeSet::new(),
            atlas_tex_creations: 0,
            vbufs,
            inst: Instances::default(),
            gate_hits: 0,
            gate_misses: 0,
            dirty_scratch: Vec::new(),
            scissor_taken: 0,
            full_repaints: 0,
            last_instances: 0,
        })
    }

    /// Rebuild the font/theme IN PLACE without recreating the wgpu device — the
    /// device, all pipelines, and every window's swapchain stay valid (dropping the
    /// device would orphan every other window's surface). Only the CPU face and the
    /// glyph atlas are font-dependent: rebuild the face at the new px/theme and
    /// invalidate the atlas so the next frame re-rasterizes on the SAME device.
    ///
    /// NATIVE ONLY: re-resolves the face via system font discovery. The wasm path
    /// rebuilds the face from injected font bytes via [`GpuRenderer::set_face`].
    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_font_theme(&mut self, px: f32, theme: Theme) -> Result<(), String> {
        // Re-resolve the CONFIGURED family (not system default) so a zoom / config
        // reload / Retina auto-scale rebuild keeps the user's font.
        let cpu = Renderer::from_system_with_family(self.font_family.as_deref(), px, theme)
            .ok_or("no system monospace font")?;
        self.set_face(cpu, theme);
        Ok(())
    }

    /// Swap in an already-built CPU face + theme IN PLACE (no device rebuild, no
    /// font discovery). The portable core of [`set_font_theme`]; the wasm path
    /// calls it directly with a face built from injected font bytes.
    pub fn set_face(&mut self, cpu: Renderer, theme: Theme) {
        self.cpu = cpu;
        self.theme = theme;
        self.resident_keys.clear();
        self.mono_res = None;
        self.color_res = None;
    }

    /// Replace just the fg/bg/cursor/selection theme live (host theme change) on both
    /// the GPU presentation state and the CPU face, so a pane re-themes without a
    /// device/face rebuild. Glyphs are coverage masks coloured at draw time, so no
    /// atlas invalidation is needed.
    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
        self.cpu.set_theme(theme);
    }

    /// Explicit selected-text foreground (theme `selectionForeground`), or `None`
    /// for the contrast-floor default. Routed through the wrapped CPU face so the
    /// GPU and CPU selection-glyph colours stay byte-identical.
    pub fn set_selection_fg(&mut self, fg: Option<u32>) {
        self.cpu.set_selection_fg(fg);
    }

    /// Re-rasterize at a new pixel size (host DPI / devicePixelRatio change): update
    /// the wrapped CPU face's metrics + glyph caches and drop the GPU atlas so the
    /// next frame re-uploads glyphs at the new size. The host then resizes the grid.
    pub fn set_px(&mut self, px: f32) {
        self.cpu.set_px(px);
        self.invalidate_atlas();
    }

    /// Inject a broad-coverage (CJK + symbols) fallback face into the GPU's CPU
    /// face from font bytes and invalidate the atlas so the next frame
    /// re-rasterizes the new coverage. The browser GPU path has no system-font
    /// discovery, so the host pushes OS font bytes in.
    pub fn set_fallback_font_bytes(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.cpu.set_fallback_bytes(bytes)?;
        self.invalidate_atlas();
        Ok(())
    }

    /// Inject a colour-emoji (sbix) face into the GPU's CPU face from font bytes
    /// and invalidate the atlas so the colour atlas re-rasterizes with the new
    /// emoji coverage. Mirrors [`set_fallback_font_bytes`].
    pub fn set_emoji_font_bytes(&mut self, bytes: Vec<u8>) -> Result<(), String> {
        self.cpu.set_color_font_bytes(bytes)?;
        self.invalidate_atlas();
        Ok(())
    }

    /// Drop the resident atlases + key set so the next present rebuilds them with
    /// the current CPU face's coverage (mirrors [`set_face`]'s invalidation).
    fn invalidate_atlas(&mut self) {
        self.resident_keys.clear();
        self.mono_res = None;
        self.color_res = None;
    }

    /// TEST/DIAGNOSTIC: number of `render_input_cached` calls that took the
    /// dirty-gate (re-presented cached pixels with ZERO GPU work).
    #[doc(hidden)]
    #[must_use]
    pub fn gate_hits(&self) -> u64 {
        self.gate_hits
    }

    /// TEST/DIAGNOSTIC: number of `render_input_cached` calls that MISSED the
    /// dirty-gate (ran a full encode + readback).
    #[doc(hidden)]
    #[must_use]
    pub fn gate_misses(&self) -> u64 {
        self.gate_misses
    }

    /// TEST/DIAGNOSTIC: number of `present_input` frames that took the SCISSORED
    /// dirty-row repaint (LoadOp::Load + scissor over the dirty band, only dirty
    /// rows re-encoded) instead of a full Clear+all-rows repaint.
    #[doc(hidden)]
    #[must_use]
    pub fn scissor_taken(&self) -> u64 {
        self.scissor_taken
    }

    /// TEST/DIAGNOSTIC: number of `present_input` frames that did a FULL repaint
    /// (Clear + all rows) — the first frame, a geometry/scrollback/selection
    /// change, a double-HEIGHT row, etc. (the conservative always-correct path).
    #[doc(hidden)]
    #[must_use]
    pub fn full_repaints(&self) -> u64 {
        self.full_repaints
    }

    /// TEST/DIAGNOSTIC: total instances built in the LAST encoded frame (sum of
    /// the eight per-frame streams). The scissored path builds ~`dirty_rows/rows`
    /// of a full frame's instances.
    #[doc(hidden)]
    #[must_use]
    pub fn last_instances(&self) -> usize {
        self.last_instances
    }

    /// Cell size (pixels), straight from the CPU renderer so geometry matches.
    pub fn cell_size(&self) -> (usize, usize) {
        self.cpu.cell_size()
    }

    /// Interior padding (px per edge), delegated to the inner CPU renderer — the
    /// single source of `pad`, so the GPU encode and the CPU encode always agree.
    pub fn pad(&self) -> usize {
        self.cpu.pad()
    }

    /// Set the interior padding on the inner CPU renderer. The GPU `encode_frame`
    /// reads `self.cpu.pad()` each frame, so this takes effect on the next present.
    pub fn set_pad(&mut self, pad: usize) {
        self.cpu.set_pad(pad);
    }

    /// Padded pixel size of a `rows`×`cols` grid (`cols·cell_w + 2·pad`, etc.) —
    /// the size to configure the swapchain / window to. Mirrors the CPU renderer.
    pub fn frame_size(&self, rows: usize, cols: usize) -> (usize, usize) {
        self.cpu.frame_size(rows, cols)
    }

    /// Number of glyph-atlas TEXTURES created so far (full (re)packs only — a
    /// reuse or incremental sub-region append creates none). The persistence
    /// test asserts an unchanged-glyph frame does not advance this.
    #[cfg(test)]
    fn atlas_tex_creations(&self) -> u64 {
        self.atlas_tex_creations
    }

    /// The `(width, height)` of the resident mono + colour atlas textures (for
    /// the persistence test: identity check that the SAME textures are reused).
    #[cfg(test)]
    fn atlas_tex_dims(&self) -> Option<((u32, u32), (u32, u32))> {
        let m = self.mono_res.as_ref()?;
        let c = self.color_res.as_ref()?;
        Some((
            (m.tex.width(), m.tex.height()),
            (c.tex.width(), c.tex.height()),
        ))
    }

    /// The adapter/backend the GPU device is running on (for diagnostics).
    pub fn adapter(&self) -> (&str, &str) {
        (&self.ctx.adapter_name, &self.ctx.backend)
    }

    /// Mirror of [`Renderer::set_cursor_blink_phase`]: `false` skips drawing
    /// the cursor for the frame, but ONLY for the `Blinking*` DECSCUSR styles.
    /// Defaults to `true`. State lives on the inner CPU renderer so both paths
    /// always agree.
    pub fn set_cursor_blink_phase(&mut self, on: bool) {
        self.cpu.set_cursor_blink_phase(on);
    }

    /// Mirror of [`Renderer::set_cursor_style_override`]: when set, the cursor
    /// is drawn in THIS style instead of the terminal's DECSCUSR style (the
    /// windowed frontend forces `HollowBlock` while unfocused).
    pub fn set_cursor_style_override(&mut self, style: Option<CursorStyle>) {
        self.cpu.set_cursor_style_override(style);
    }

    /// Whether a drawable glyph lives in this cell (not wide-continuation, not a
    /// space, not a control char) — mirrors the CPU renderer's `blit` guard.
    fn drawable(cell: &aterm_core::terminal::RenderCell) -> bool {
        !cell.wide && cell.ch != ' ' && !cell.ch.is_control()
    }

    /// Resolve a cell to its glyph key via the SAME dispatch the CPU `blit`
    /// uses: a shaped emoji cluster (ZWJ / skin-tone / keycap) first, then a
    /// VS16 emoji-presentation base, then ordinary text. Keeps the GPU atlas key
    /// set and the per-cell instance lookup in lockstep with the CPU, so `❤️`,
    /// `👨‍👩‍👧`, `👍🏽`, `1️⃣` key to the colour atlas on both paths.
    fn cell_key(
        &mut self,
        cluster: Option<&str>,
        cell: &aterm_core::terminal::RenderCell,
    ) -> GlyphKey {
        self.cpu.resolve_cell_key(cluster, cell)
    }

    /// Take ownership of a freshly packed `atlas`, create its GPU texture (sized
    /// to it) + bind group, upload `atlas.data` in full, and return the resident
    /// bundle. Counts as ONE texture creation — used on a full (re)pack, NOT on a
    /// reuse or incremental append. The format follows the atlas kind (R8Unorm /
    /// RGBA8Unorm); both bind through `atlas_bgl` with the NEAREST sampler.
    fn create_atlas_texture(&mut self, atlas: Atlas) -> ResidentAtlas {
        let device = &self.ctx.device;
        let bpp = atlas.kind.bpp();
        let (format, label, bg_label) = match atlas.kind {
            AtlasKind::Mono => (
                wgpu::TextureFormat::R8Unorm,
                "aterm-gpu atlas",
                "aterm-gpu atlas bg",
            ),
            AtlasKind::Color => (
                wgpu::TextureFormat::Rgba8Unorm,
                "aterm-gpu colour atlas",
                "aterm-gpu colour atlas bg",
            ),
        };
        // Allocate the texture TALLER than the packed data (headroom) so later
        // glyphs append via sub-region upload instead of recreating the texture.
        // Clamp to the device's max 2D texture dimension: the packer already bounds
        // `atlas.height` to this same limit (so the upload, whose Extent3d height ==
        // atlas.height, always fits), and this stops the +headroom from ever pushing
        // the texture past the limit — which would abort the device.
        let max_tex_dim = device.limits().max_texture_dimension_2d;
        let tex_h = (atlas.height + ATLAS_GROW_HEADROOM).min(max_tex_dim);
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: atlas.width,
                height: tex_h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.atlas_tex_creations += 1;
        // Upload only the OCCUPIED rows (`atlas.height`); the headroom stays
        // unwritten (never sampled until an append fills it). bytes_per_row ==
        // width * bpp: width is 1024, so 1024 (R8) and 4096 (RGBA8) are both
        // multiples of 256 — no row padding needed.
        self.ctx.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &atlas.data[..(atlas.width * atlas.height * bpp) as usize],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(atlas.width * bpp),
                rows_per_image: Some(atlas.height),
            },
            wgpu::Extent3d {
                width: atlas.width,
                height: atlas.height,
                depth_or_array_layers: 1,
            },
        );
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(bg_label),
            layout: &self.atlas_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        ResidentAtlas {
            atlas,
            tex,
            bind,
            tex_h,
        }
    }

    /// Re-upload only the changed row band `[y0, y1)` of a resident atlas — the
    /// incremental-append fast path. Writes FULL atlas rows (origin x=0, full
    /// width) so `bytes_per_row` stays a multiple of 256, and creates no new
    /// texture (the whole point of the optimisation). No-op for an empty band.
    fn upload_atlas_rows(&self, res: &ResidentAtlas, y0: u32, y1: u32) {
        if y1 <= y0 {
            return;
        }
        let bpp = res.atlas.kind.bpp();
        let row = (res.atlas.width * bpp) as usize;
        let start = y0 as usize * row;
        let end = y1 as usize * row;
        self.ctx.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &res.tex,
                mip_level: 0,
                origin: wgpu::Origin3d { x: 0, y: y0, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            &res.atlas.data[start..end],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(res.atlas.width * bpp),
                rows_per_image: Some(y1 - y0),
            },
            wgpu::Extent3d {
                width: res.atlas.width,
                height: y1 - y0,
                depth_or_array_layers: 1,
            },
        );
    }

    /// Ensure both glyph atlases hold every key in `keys`, reusing the resident
    /// textures untouched when `keys` is already a subset, appending new glyphs
    /// into free space on a small miss, and full-repacking into a fresh texture
    /// only on genuine overflow (or the first frame). Pure GPU/CPU bookkeeping
    /// — the render passes below just bind `mono_res`/`color_res`.
    fn ensure_atlases(&mut self, keys: &BTreeSet<GlyphKey>) {
        // Fast path: every requested key already resident in BOTH atlases → the
        // resident textures + bind groups are exactly what this frame needs.
        // (`resident_keys` is the union packed across both atlases.)
        if self.mono_res.is_some()
            && self.color_res.is_some()
            && keys.iter().all(|k| self.resident_keys.contains(k))
        {
            return;
        }

        // Which keys are genuinely new this frame.
        let new_keys: Vec<GlyphKey> = keys
            .iter()
            .copied()
            .filter(|k| !self.resident_keys.contains(k))
            .collect();

        // First frame (or a prior overflow cleared the cache): full pack both.
        if self.mono_res.is_none() || self.color_res.is_none() {
            self.rebuild_atlases(keys);
            return;
        }

        // Try to grow both resident atlases in place. A capacity miss in either
        // forces a full repack of BOTH from the complete key set (simplest
        // correct fallback; overflow is rare).
        let cap_mono = self.mono_res.as_ref().unwrap().tex_h;
        let cap_color = self.color_res.as_ref().unwrap().tex_h;
        let mono_band = {
            let res = self.mono_res.as_mut().unwrap();
            grow_atlas(&mut self.cpu, &mut res.atlas, &new_keys, cap_mono)
        };
        let color_band = {
            let res = self.color_res.as_mut().unwrap();
            grow_atlas(&mut self.cpu, &mut res.atlas, &new_keys, cap_color)
        };
        match (mono_band, color_band) {
            (Some(mb), Some(cb)) => {
                // Both grew (or stayed) within capacity: upload only the dirty
                // bands; no new texture is created.
                let mono_res = self.mono_res.as_ref().unwrap();
                self.upload_atlas_rows(mono_res, mb.0, mb.1);
                let color_res = self.color_res.as_ref().unwrap();
                self.upload_atlas_rows(color_res, cb.0, cb.1);
                self.resident_keys.extend(new_keys);
            }
            _ => self.rebuild_atlases(keys),
        }
    }

    /// Full (re)pack of both atlases from the complete key set, replacing the
    /// resident textures. Used on the first frame and on genuine overflow.
    fn rebuild_atlases(&mut self, keys: &BTreeSet<GlyphKey>) {
        // Cap the packed atlas height at the device's max 2D texture dimension so a
        // very large distinct-glyph set can never ask wgpu to create a texture
        // taller than the GPU allows (which aborts the device). Far above any real
        // workload; only the pathological case is bounded.
        let cap_h = self.ctx.device.limits().max_texture_dimension_2d;
        let mono = build_atlas(&mut self.cpu, keys, cap_h);
        self.mono_res = Some(self.create_atlas_texture(mono));
        let color = build_color_atlas(&mut self.cpu, keys, cap_h);
        self.color_res = Some(self.create_atlas_texture(color));
        self.resident_keys = keys.clone();
    }

    /// Pack each placed footprint's RGBA rows into a stacked `tw`×`th` plane
    /// buffer. `items` is `(y0, dw, dh, rgba)` per footprint. When a footprint
    /// spans the full plane width (`dw == tw`) its rows are contiguous in both
    /// source and destination, so the whole footprint copies in ONE memcpy;
    /// otherwise rows are copied individually (right padding stays transparent).
    /// Byte-identical either way — factored out of [`build_image_plane`] so the
    /// copy can be unit-tested and benchmarked in isolation from GPU upload.
    ///
    /// `#[doc(hidden)] pub` ONLY so the `image_plane` bench can call it directly;
    /// not part of the public API.
    #[doc(hidden)]
    #[must_use]
    pub fn pack_image_plane(items: &[(u32, u32, u32, &[u8])], tw: u32, th: u32) -> Vec<u8> {
        let mut data = vec![0u8; (tw * th * 4) as usize];
        for &(y0, dw, dh, rgba) in items {
            if dw == tw {
                // Footprint spans the full plane width: rows are contiguous in both
                // source and destination, so collapse the `dh` per-row copies into
                // one memcpy (the common cell-sized / single-image case).
                let dst = (y0 * tw) as usize * 4;
                let len = (dw * dh * 4) as usize;
                data[dst..dst + len].copy_from_slice(&rgba[..len]);
            } else {
                // Narrower than the plane: each row has right padding — copy row by row.
                for y in 0..dh {
                    let src = (y * dw * 4) as usize;
                    let dst = ((y0 + y) * tw) as usize * 4;
                    data[dst..dst + (dw * 4) as usize]
                        .copy_from_slice(&rgba[src..src + (dw * 4) as usize]);
                }
            }
        }
        data
    }

    /// Build the per-frame inline-image texture (iTerm2 OSC 1337) from every
    /// DISTINCT image visible in `input`'s rows (whole grid — every image cell is
    /// repainted; the scissor path falls back to FULL whenever images differ, see
    /// `compute_dirty_rows`). Each distinct `(arc_ptr, fp_w, fp_h)` footprint is
    /// decoded+scaled via `aterm_render::decode_image_to_footprint` (the SAME
    /// bytes the CPU `blit_image_cell` copies, cached by the same key) and stacked
    /// vertically into ONE RGBA8 texture; a covered cell then samples its tile
    /// NEAREST, so the GPU pixels match the CPU per-cell copy (the parity gate).
    ///
    /// Sets `self.image_plane` to the resident texture + placement map, or to
    /// `None` when no decodable image is visible (so an image-free frame binds and
    /// draws nothing — the text path stays byte-identical).
    fn build_image_plane(&mut self, win: &mut WindowGpu, input: &RenderInput) {
        let (cw, ch) = self.cpu.cell_size();
        // Distinct placements: keyed like the CPU cache. Insertion order is
        // deterministic (row-major over the grid), so the packed texture layout
        // is stable frame to frame for the same image set.
        let mut order: Vec<(usize, usize, usize)> = Vec::new();
        let mut seen: HashMap<(usize, usize, usize), ()> = HashMap::new();
        for row in &input.images {
            for (_c, image) in row {
                let fp_w = image.image.cols as usize * cw;
                let fp_h = image.image.rows as usize * ch;
                if fp_w == 0 || fp_h == 0 {
                    continue;
                }
                let key = (std::sync::Arc::as_ptr(&image.image) as usize, fp_w, fp_h);
                if seen.insert(key, ()).is_none() {
                    order.push(key);
                }
            }
        }
        if order.is_empty() {
            // No images this frame: drop any prior plane so nothing is bound.
            win.image_plane = None;
            return;
        }

        // Decode each distinct image to its footprint RGBA (cached), and lay them
        // out top-to-bottom in one texture. Failed decodes (empty rgba) are kept
        // in the cache as a negative result but contribute no texture rows; a cell
        // referencing one simply emits no quad (its bg shows through, == CPU).
        // Bound the stacked plane to the device's max 2D texture dimension on BOTH
        // axes: stacking enough (or wide enough) inline images to exceed the limit
        // would otherwise ask wgpu for an oversized texture and abort the device. An
        // image that doesn't fit emits no quad (its bg shows through) — the same
        // graceful fallback a failed decode already uses.
        let max_tex_dim = self.ctx.device.limits().max_texture_dimension_2d;
        let mut placements: HashMap<(usize, usize, usize), (u32, u32, u32)> = HashMap::new();
        let mut total_h: u32 = 0;
        let mut max_w: u32 = 0;
        for &key in &order {
            if win.image_cache.get(key).is_none() {
                // `key` is `(arc_ptr, fp_w, fp_h)` — re-find the ImageRef to decode.
                let decoded = decode_for_key(input, key, cw, ch);
                win.image_cache.put(key, decoded);
            }
            let decoded = win.image_cache.get(key).expect("just inserted");
            if decoded.rgba.is_empty() || decoded.w == 0 || decoded.h == 0 {
                continue;
            }
            // Skip a single footprint that exceeds the limit on its own, and stop
            // once the next image would push the stacked height past the limit.
            if decoded.w > max_tex_dim || decoded.h > max_tex_dim {
                continue;
            }
            if total_h + decoded.h > max_tex_dim {
                break;
            }
            placements.insert(key, (total_h, decoded.w, decoded.h));
            total_h += decoded.h;
            max_w = max_w.max(decoded.w);
        }
        if placements.is_empty() || max_w == 0 || total_h == 0 {
            win.image_plane = None;
            return;
        }

        // One straight-RGBA buffer holding every footprint, stacked. Unused right
        // padding (when footprints differ in width) stays zero/transparent and is
        // never sampled — a cell only reads its own `(cell_col*cw, y0+cell_row*ch)`
        // tile, fully inside its footprint.
        let (tw, th) = (max_w, total_h);
        // Gather each placed footprint's source rows, then pack them in one pass
        // (extracted to `pack_image_plane` so the copy is unit-tested + benched in
        // isolation from GPU upload).
        let mut items: Vec<(u32, u32, u32, &[u8])> = Vec::with_capacity(order.len());
        for &key in &order {
            let Some(&(y0, dw, dh)) = placements.get(&key) else {
                continue;
            };
            let decoded = win.image_cache.peek(key).expect("placed image is cached");
            items.push((y0, dw, dh, decoded.rgba.as_slice()));
        }
        let data = Self::pack_image_plane(&items, tw, th);

        let device = &self.ctx.device;
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("aterm-gpu image plane"),
            size: wgpu::Extent3d {
                width: tw,
                height: th,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.ctx.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(tw * 4),
                rows_per_image: Some(th),
            },
            wgpu::Extent3d {
                width: tw,
                height: th,
                depth_or_array_layers: 1,
            },
        );
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("aterm-gpu image plane bg"),
            layout: &self.atlas_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        win.image_plane = Some(ImagePlane {
            bind,
            w: tw,
            h: th,
            placements,
        });
    }

    /// Render a [`RenderInput`] snapshot (built by the engine via
    /// [`aterm_core::terminal::Terminal::cell_frame_into`]) on the GPU and read it
    /// back — no `&Terminal` borrow, so the frontend renders after dropping the
    /// lock. As of REARCH A-3 the GPU renderer is a PURE consumer of the snapshot;
    /// the engine emits it and both CPU/GPU paths consume the identical value.
    pub fn render_input(&mut self, win: &mut WindowGpu, input: &RenderInput) -> Frame {
        // FULL repaint (Clear + all rows) — the snapshot / readback / oracle path.
        // It overwrites the offscreen with this (possibly unrelated) input, so it
        // invalidates the scissored present sequence's prior-frame tracking: a
        // subsequent `present_input` must NOT diff against a frame it never drew.
        // Per-window: reset THIS window's prior-frame validity.
        win.present_prev = None;
        let (w, h) = self.encode_frame(win, input, &RepaintScope::Full);
        // The freshly rendered target is resident on `win.offscreen`.
        let tex = win
            .offscreen
            .as_ref()
            .expect("encode_frame sets offscreen")
            .tex
            .clone();
        self.ctx.read_back(&tex, w, h)
    }

    /// Create an on-screen presentation surface for `target` (e.g. an
    /// `Arc<winit::window::Window>`) at the given pixel size, on the SAME
    /// instance/adapter as the offscreen renderer.
    ///
    /// The swapchain format is chosen to be NON-sRGB (preferring `Bgra8Unorm`) so
    /// the raw 8-bit colours blitted from the `Rgba8Unorm` offscreen frame land on
    /// screen byte-identical to the readback the AI introspection sees. An `*Srgb`
    /// surface would re-encode every channel and break that invariant.
    pub fn create_window_surface(
        &mut self,
        target: impl Into<wgpu::SurfaceTarget<'static>>,
        width: u32,
        height: u32,
    ) -> Result<GpuSurface, String> {
        let surface = self
            .ctx
            .instance
            .create_surface(target)
            .map_err(|e| format!("create_surface failed: {e}"))?;
        // Query the surface capabilities only on the FIRST attach; the NON-sRGB
        // format choice is adapter/platform-stable, so reuse it (saving the
        // blocking driver round-trip) for later windows.
        let format = match self.cached_surface_format {
            Some(f) => {
                // The cache assumes the NON-sRGB format choice is stable across every
                // surface on this single adapter (true on macOS/Metal). Guard that
                // assumption in debug builds at zero release cost: if a surface ever
                // did NOT support the cached format (e.g. a future multi-adapter/
                // -surface backend), this fires loudly so the cache can be re-keyed
                // (clear `cached_surface_format`) instead of silently mis-configuring.
                #[cfg(debug_assertions)]
                {
                    let caps = surface.get_capabilities(&self.ctx.adapter);
                    debug_assert!(
                        caps.formats.contains(&f),
                        "cached surface format {f:?} is not supported by this surface \
                         (supported: {:?}); the per-surface-stable assumption broke — \
                         clear `cached_surface_format` to re-query per attach",
                        caps.formats
                    );
                }
                f
            }
            None => {
                let caps = surface.get_capabilities(&self.ctx.adapter);
                let f = Self::pick_surface_format(&caps)?;
                self.cached_surface_format = Some(f);
                f
            }
        };
        // Compile the blit pipeline for this format NOW (off the first-FRAME path);
        // `present_input` then only looks it up. Idempotent if already built.
        self.ensure_blit_pipeline(format);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: width.max(1),
            height: height.max(1),
            // AutoNoVsync: lowest input→display latency (no vblank lock). On Metal
            // this picks Mailbox where supported (newest frame wins, still tear-free)
            // and falls back to Immediate otherwise. A terminal redraws discretely and
            // rarely scrolls full-screen, so tearing is near-imperceptible — the latency
            // win (≈ one refresh interval, ~8–16 ms) is the better default for typing.
            present_mode: wgpu::PresentMode::AutoNoVsync,
            alpha_mode: wgpu::CompositeAlphaMode::Opaque,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&self.ctx.device, &config);
        Ok(GpuSurface { surface, config })
    }

    /// Configure an ALREADY-CREATED `wgpu::Surface` for presentation at the given
    /// size, on this renderer's adapter/device. Same NON-sRGB format selection as
    /// [`create_window_surface`](Self::create_window_surface) — split out because
    /// the WebGL backend must create the canvas surface BEFORE the adapter exists
    /// (the adapter is enumerated against that surface), so the surface and the
    /// renderer are assembled in the opposite order from native. Takes `&self` (no
    /// first-attach format cache / eager blit-pipeline build — the wasm path makes
    /// exactly one surface and `present_input` lazily ensures the blit pipeline).
    pub fn configure_window_surface(
        &self,
        surface: wgpu::Surface<'static>,
        width: u32,
        height: u32,
    ) -> Result<GpuSurface, String> {
        let caps = surface.get_capabilities(&self.ctx.adapter);
        let format = Self::pick_surface_format(&caps)?;
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: width.max(1),
            height: height.max(1),
            present_mode: wgpu::PresentMode::AutoNoVsync,
            alpha_mode: wgpu::CompositeAlphaMode::Opaque,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&self.ctx.device, &config);
        Ok(GpuSurface { surface, config })
    }

    /// Pick a NON-sRGB swapchain format: `Bgra8Unorm` if offered (the macOS/Metal
    /// native), else `Rgba8Unorm`, else the first non-`*Srgb` format the surface
    /// supports. Errs only if the surface offers *exclusively* sRGB formats (it
    /// won't on Metal), since presenting through one would gamma-shift the colours.
    fn pick_surface_format(
        caps: &wgpu::SurfaceCapabilities,
    ) -> Result<wgpu::TextureFormat, String> {
        use wgpu::TextureFormat::{Bgra8Unorm, Rgba8Unorm};
        if caps.formats.contains(&Bgra8Unorm) {
            return Ok(Bgra8Unorm);
        }
        if caps.formats.contains(&Rgba8Unorm) {
            return Ok(Rgba8Unorm);
        }
        caps.formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .ok_or_else(|| "surface offers no non-sRGB format".to_string())
    }

    /// Resize an on-screen surface to a new pixel size and reconfigure it (clamped
    /// to a minimum of 1×1, which wgpu requires).
    pub fn resize_surface(&self, surf: &mut GpuSurface, width: u32, height: u32) {
        surf.config.width = width.max(1);
        surf.config.height = height.max(1);
        surf.surface.configure(&self.ctx.device, &surf.config);
    }

    /// Clear `surf`'s swapchain to a solid `rgb` (0x00RRGGBB) and present it.
    ///
    /// The minimal "GPU is live" present: no offscreen, no atlas, no instances —
    /// just acquire → clear → present. Used by the WebGPU-from-canvas proving slice
    /// (`the aterm-gpu-web crate`) to confirm the whole instance→adapter→device→
    /// surface→present chain works under the browser WebGPU backend before the
    /// instanced-cell-quad encode (`present_input`) is wired in.
    pub fn clear_surface(&self, surf: &mut GpuSurface, rgb: u32) {
        use wgpu::CurrentSurfaceTexture as C;
        let frame = match surf.surface.get_current_texture() {
            C::Success(f) | C::Suboptimal(f) => f,
            C::Outdated | C::Lost => {
                surf.surface.configure(&self.ctx.device, &surf.config);
                return;
            }
            C::Timeout | C::Occluded | C::Validation => return,
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut enc = self
            .ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("aterm-gpu clear-surface"),
            });
        {
            let _pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("aterm-gpu clear-surface pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: ((rgb >> 16) & 0xff) as f64 / 255.0,
                            g: ((rgb >> 8) & 0xff) as f64 / 255.0,
                            b: (rgb & 0xff) as f64 / 255.0,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }
        self.ctx.queue.submit([enc.finish()]);
        frame.present();
    }

    /// Render the frame offscreen (the single source of truth) and PRESENT it on
    /// the GPU by blitting that texture into `surf`'s swapchain — no CPU readback,
    /// no softbuffer copy. `invert` flips RGB for the visual-bell flash.
    pub fn present_input(
        &mut self,
        win: &mut WindowGpu,
        surf: &mut GpuSurface,
        input: &RenderInput,
        invert: bool,
    ) {
        // 1. Offscreen render (submits). SCISSORED DIRTY-ROW REPAINT: when the
        //    persistent offscreen still holds the prior presented frame and only
        //    some rows differ, re-encode ONLY those rows (LoadOp::Load + a scissor
        //    over the dirty band) — proportional to the change, not the screen.
        //    Otherwise a full Clear+all-rows repaint (the always-correct path).
        //    The rendered target + its blit-source bind group are resident on
        //    `win.offscreen` (built once, reused across presents at the same
        //    dimensions; rebuilt only on a resize), so this present allocates no
        //    per-frame texture / view / blit bind group.
        self.encode_present_frame(win, input);

        // 2. Acquire the next swapchain texture. On Outdated/Lost the surface
        //    config no longer matches; reconfigure and skip this frame (the next
        //    redraw presents). Timeout/Occluded/Validation: skip this frame.
        use wgpu::CurrentSurfaceTexture as C;
        let frame = match surf.surface.get_current_texture() {
            C::Success(f) | C::Suboptimal(f) => f,
            C::Outdated | C::Lost => {
                surf.surface.configure(&self.ctx.device, &surf.config);
                return;
            }
            C::Timeout | C::Occluded | C::Validation => return,
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        // 3. Ensure a blit pipeline for this swapchain format exists.
        let format = surf.config.format;
        self.ensure_blit_pipeline(format);
        let pipeline = &self.blit_pipelines[&format];

        // 4. Write the invert flag ONLY when it changes (the blit uniform is a
        //    pure function of `invert`; the steady-state present otherwise
        //    re-uploaded an unchanged 4-byte buffer each frame). The offscreen
        //    texture is already bound as the blit source by the resident
        //    `blit_bind` (built in `encode_frame` when the target was created).
        if win.blit_invert != Some(invert) {
            self.ctx.queue.write_buffer(
                &self.blit_uniform_buf,
                0,
                bytemuck::bytes_of(&BlitUniform {
                    flag: invert as u32,
                    _pad: [0; 3],
                }),
            );
            win.blit_invert = Some(invert);
        }
        let bind = &win
            .offscreen
            .as_ref()
            .expect("encode_frame sets offscreen")
            .blit_bind;

        // 5. One blit pass: fullscreen triangle covers every pixel, so Clear is
        //    just the (overwritten) initial value.
        let mut enc = self
            .ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("aterm-gpu blit"),
            });
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("aterm-gpu blit pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, bind, &[]);
            pass.draw(0..3, 0..1);
        }
        self.ctx.queue.submit([enc.finish()]);
        frame.present();
    }

    /// Compute the SCISSORED dirty-row scope for THIS presented frame against the
    /// previous one (the persistent offscreen still holds it), encode it, then
    /// record this frame as the new `present_prev`. Shared by `present_input` (the
    /// window blit path) and the readback test helper, so the scissor decision is
    /// computed in exactly one place.
    ///
    /// The cardinal rule: when in ANY doubt, FULL REPAINT. The scissor activates
    /// ONLY when a prior presented frame exists, the offscreen still holds it at
    /// the right dims, and `compute_dirty_rows` says the frame is reusable with a
    /// non-empty dirty set. Everything else (first frame, resize, scrollback /
    /// selection / double-height change) falls back to the full Clear+all-rows
    /// path — byte-identical to the original encode.
    fn encode_present_frame(&mut self, win: &mut WindowGpu, input: &RenderInput) -> (u32, u32) {
        let cur_blink = self.cpu.cursor_blink_phase();
        let cur_override = self.cpu.cursor_style_override();
        let (cw, ch) = self.cpu.cell_size();
        // Match `encode_frame`'s PADDED offscreen dims (`cols*cw + 2*pad`): the
        // offscreen is created/stored at the inset size, so the gate below must
        // compare against the same padded extent. Reading the unpadded size here
        // made `offscreen_holds_prev` ALWAYS false whenever `pad > 0` (the windowed
        // GUI runs at `pad = pad_for_scale(..) > 0`), silently forcing every present
        // to a Full repaint and disabling the scissored dirty-row path entirely. At
        // `pad == 0` the `+ 2*pad` terms drop out, so this is unchanged there.
        let pad = self.cpu.pad();
        let (w, h) = (
            (input.cols * cw + 2 * pad) as u32,
            (input.rows * ch + 2 * pad) as u32,
        );

        // The offscreen must already hold the previous frame at THESE dims for a
        // scissored Load to be safe. A dimension change recreates the texture
        // (its prior contents are gone), so any dims mismatch forces Full.
        let offscreen_holds_prev = matches!(&win.offscreen, Some(o) if o.w == w && o.h == h);

        // Take the persistent dirty scratch out of `self` (swapping in an empty Vec
        // — no allocation) so `compute_dirty_rows` can write into it while the
        // per-window `prev_input`/`present_prev` are read, and `encode_frame` can
        // borrow `win` mutably without aliasing it. The `Dirty` scope BORROWS this
        // local across the encode; it is restored to `self.dirty_scratch` after.
        // Take the per-window prior-frame state OUT of `win` too (swapping in
        // empties) so the scope can hold a `&` into them while `encode_frame`
        // mutably borrows `win`; both are written back below.
        let mut dirty = std::mem::take(&mut self.dirty_scratch);
        let prev_present = win.present_prev.take();
        let prev_input = std::mem::take(&mut win.prev_input);
        let scope = match (&prev_present, offscreen_holds_prev) {
            (Some(prev), true) => match compute_dirty_rows(
                // The prior presented frame's snapshot lives in this window's
                // resident `prev_input` buffer; `present_prev` being `Some` is its
                // validity.
                &prev_input,
                input,
                prev.blink_phase,
                prev.cursor_style_override,
                cur_blink,
                cur_override,
                &mut dirty,
            ) {
                // Reusable: scissor the dirty band. The zero-dirty-row case (a
                // gate-class idle frame) is handled correctly downstream — Load
                // preserves the prior frame and the empty dirty set draws nothing,
                // the cheapest possible encode.
                DirtyDecision::Rows(_) => RepaintScope::Dirty(&dirty),
                // Not reusable (geometry / scrollback / selection / double-height):
                // the conservative full repaint.
                DirtyDecision::FullRepaint => RepaintScope::Full,
            },
            // No prior frame, or the offscreen no longer holds it: full repaint.
            _ => RepaintScope::Full,
        };

        match &scope {
            RepaintScope::Dirty(_) => self.scissor_taken += 1,
            RepaintScope::Full => self.full_repaints += 1,
        }
        let dims = self.encode_frame(win, input, &scope);
        // Done with the borrowed scope; restore the scratch (capacity retained).
        self.dirty_scratch = dirty;

        // This frame is now resident on THIS window's offscreen; remember it (+ the
        // state it was drawn with) ON THE WINDOW so the NEXT present of THIS window
        // can diff against it. The snapshot goes into the window's RESIDENT
        // `prev_input` buffer (taken out above) via `clone_from`, which reuses its
        // grid allocation when the dims are stable — a stable-dims changed frame
        // does ZERO grid allocation here, just a memcpy into the retained buffers.
        let mut prev_input = prev_input;
        prev_input.clone_from(input);
        win.prev_input = prev_input;
        win.present_prev = Some(PresentPrev {
            blink_phase: cur_blink,
            cursor_style_override: cur_override,
        });
        dims
    }

    /// TEST HELPER (byte-identity gate): run the SCISSORED present-path encode for
    /// `input` exactly as [`present_input`](Self::present_input) does — same
    /// `compute_dirty_rows` decision, same `present_prev` tracking, same persistent
    /// offscreen — then read the offscreen back into a [`Frame`]. This is the path
    /// the `scissor_repaint` test asserts is byte-identical to a fresh full render.
    #[doc(hidden)]
    pub fn present_input_readback(&mut self, win: &mut WindowGpu, input: &RenderInput) -> Frame {
        let (w, h) = self.encode_present_frame(win, input);
        let tex = win
            .offscreen
            .as_ref()
            .expect("encode_frame sets offscreen")
            .tex
            .clone();
        self.ctx.read_back(&tex, w, h)
    }

    /// TEST/BENCH HELPER: run the SCISSORED present-path encode for `input` and
    /// BLOCK until the GPU finishes, but do NOT read the pixels back. Isolates the
    /// changed-frame ENCODE + instance-build + GPU fill cost (the readback, which
    /// is identical for any scope, would otherwise swamp the scissor's saving).
    #[doc(hidden)]
    pub fn present_encode_poll(&mut self, win: &mut WindowGpu, input: &RenderInput) {
        let _ = self.encode_present_frame(win, input);
        self.ctx
            .device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("GPU poll failed");
    }

    /// TEST HELPER (on-glass blit coverage): run the REAL on-glass blit — the
    /// SAME `vs_blit`/`fs_blit` pipeline, the SAME `blit_sampler` (NEAREST), and
    /// the SAME `BlitUniform` that [`present_input`](Self::present_input) uses —
    /// against a fresh READABLE `Rgba8Unorm` target (the window swapchain isn't
    /// readable headless) and read the result back into a [`Frame`].
    ///
    /// The blit SOURCE is the renderer's CURRENT resident offscreen (whatever the
    /// last `render_input`/`present_input` drew), so the caller renders a frame,
    /// captures the offscreen pixels with the existing readback, then calls this
    /// to obtain the blitted pixels and compare. `invert == false` is the
    /// straight-through (byte-exact) present; `invert == true` is the visual-bell
    /// `1.0 - rgb` flash.
    ///
    /// Production-inert: only test/bench code reaches it, it builds its own target
    /// + bind group, and it leaves `win.offscreen` / `win.blit_invert` /
    /// `present_prev` untouched (it restores `blit_invert` to whatever the real
    /// present path last set, so the next `present_input` still skips the uniform
    /// write iff the flag is unchanged). The `Rgba8Unorm` target format matches one
    /// of the two formats `pick_surface_format` chooses for a real swapchain, so
    /// the blit pipeline it exercises is exactly a real present pipeline.
    #[doc(hidden)]
    pub fn blit_to_offscreen_for_test(&mut self, win: &mut WindowGpu, invert: bool) -> Frame {
        let src = win
            .offscreen
            .as_ref()
            .expect("render a frame before blitting");
        let (w, h) = (src.w, src.h);
        let src_view = src.tex.create_view(&wgpu::TextureViewDescriptor::default());

        // Fresh readable RGBA8 target — same format/usage as the real offscreen,
        // i.e. a stand-in for the `Rgba8Unorm` swapchain branch of present.
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let dst = self.ctx.offscreen_texture(w, h);
        let dst_view = dst.create_view(&wgpu::TextureViewDescriptor::default());

        // Write the invert flag through the REAL blit uniform buffer the present
        // path uses, and reflect it in `blit_invert` so the bookkeeping stays
        // consistent (the next `present_input` skips the write iff unchanged).
        self.ctx.queue.write_buffer(
            &self.blit_uniform_buf,
            0,
            bytemuck::bytes_of(&BlitUniform {
                flag: invert as u32,
                _pad: [0; 3],
            }),
        );
        win.blit_invert = Some(invert);

        // The REAL blit bind group: source view + the REAL `blit_sampler` (NEAREST)
        // + the REAL `blit_uniform_buf`, under the REAL `blit_bgl`.
        let bind = self
            .ctx
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("aterm-gpu test blit bg"),
                layout: &self.blit_bgl,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&src_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.blit_sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: self.blit_uniform_buf.as_entire_binding(),
                    },
                ],
            });

        // The REAL blit pipeline (vs_blit + fs_blit) for this format.
        self.ensure_blit_pipeline(format);
        let pipeline = &self.blit_pipelines[&format];

        let mut enc = self
            .ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("aterm-gpu test blit"),
            });
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("aterm-gpu test blit pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &dst_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.draw(0..3, 0..1);
        }
        self.ctx.queue.submit([enc.finish()]);
        self.ctx.read_back(&dst, w, h)
    }

    /// Build + cache the blit render pipeline for a swapchain `format` if absent.
    fn ensure_blit_pipeline(&mut self, format: wgpu::TextureFormat) {
        if self.blit_pipelines.contains_key(&format) {
            return;
        }
        let pipeline = self
            .ctx
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("aterm-gpu blit pipeline"),
                layout: Some(&self.blit_layout),
                vertex: wgpu::VertexState {
                    module: &self.blit_shader,
                    entry_point: Some("vs_blit"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &self.blit_shader,
                    entry_point: Some("fs_blit"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(wgpu::BlendState::REPLACE),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            });
        self.blit_pipelines.insert(format, pipeline);
    }

    /// THE GPU DIRTY-GATE — the per-frame PRESENTATION hot path.
    ///
    /// On an UNCHANGED frame this does ZERO GPU work: no encode, no submit, no
    /// `device.poll`, no readback. It re-presents the previous frame's already-
    /// read-back pixels (a [`RenderView::Borrowed`] over the gate cache). The
    /// gate-hit decision is the SHARED [`is_unchanged_frame`] predicate — the
    /// SAME one the CPU [`Renderer::render_input_cached`] uses — so the GPU and
    /// CPU gates cannot diverge, and on a hit the cached pixels ARE exactly what
    /// the GPU would re-render for that input (nothing changed since we read them
    /// back).
    ///
    /// On a MISS it runs the full GPU render + readback (exactly
    /// [`render_input`](Self::render_input)), stores the resulting `Frame` plus
    /// this frame's input / blink / style-override in the gate cache, and returns
    /// a borrow of the freshly-cached pixels.
    ///
    /// The owned-`Frame` [`render_input`](Self::render_input) path (snapshot /
    /// image / headless verbs) is UNCHANGED and still does its full readback.
    pub fn render_input_cached<'a>(
        &mut self,
        win: &'a mut WindowGpu,
        input: &RenderInput,
    ) -> RenderView<'a> {
        // Current cursor state lives on the inner CPU renderer (the frontend
        // forwards blink/override there), exactly as the CPU gate reads it.
        let cur_blink = self.cpu.cursor_blink_phase();
        let cur_override = self.cpu.cursor_style_override();

        // Expected pixel dims for this frame, to cross-check the cached frame.
        let (cw, ch) = self.cpu.cell_size();
        let (w, h) = (input.cols * cw, input.rows * ch);

        // GATE-HIT: a prior frame exists, it is pixel-identical to this input,
        // AND its cached pixels are the right size (defensive — `is_unchanged_
        // frame` already requires equal rows/cols, which fixes the dims, but we
        // assert the buffer we are about to hand back genuinely matches).
        let hit = match &win.gate_cache {
            Some(c) => {
                c.frame.width == w
                    && c.frame.height == h
                    && is_unchanged_frame(
                        &c.input,
                        c.blink_phase,
                        c.cursor_style_override,
                        input,
                        cur_blink,
                        cur_override,
                    )
            }
            None => false,
        };

        if hit {
            self.gate_hits += 1;
            let frame = &win.gate_cache.as_ref().expect("hit implies Some").frame;
            return RenderView::Borrowed {
                width: frame.width,
                height: frame.height,
                pixels: &frame.pixels,
            };
        }

        // MISS: full GPU render + readback, then refresh the gate cache to THIS
        // frame's pixels + state so the next unchanged frame can take the gate.
        self.gate_misses += 1;
        let frame = self.render_input(win, input);
        win.gate_cache = Some(GpuGateCache {
            input: input.clone(),
            blink_phase: cur_blink,
            cursor_style_override: cur_override,
            frame,
        });
        let frame = &win.gate_cache.as_ref().expect("just stored").frame;
        RenderView::Borrowed {
            width: frame.width,
            height: frame.height,
            pixels: &frame.pixels,
        }
    }

    /// Encode + submit the GPU work and BLOCK until the GPU finishes, but do NOT
    /// read the pixels back. This is the on-screen render cost (a window presents
    /// the texture instead of copying it to CPU) — i.e. what the readback path
    /// adds on top is pure verification overhead. Returns nothing; time the call.
    ///
    /// Takes a pre-built [`RenderInput`] (A-3: the engine emits the snapshot via
    /// [`aterm_core::terminal::Terminal::cell_frame_into`]); the renderer never
    /// borrows `&Terminal`.
    pub fn render_no_readback(&mut self, win: &mut WindowGpu, input: &RenderInput) {
        win.present_prev = None;
        let _ = self.encode_frame(win, input, &RepaintScope::Full);
        self.ctx
            .device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("GPU poll failed");
    }

    /// Build the atlas + instances, encode the single render pass onto the
    /// RESIDENT offscreen target (`win.offscreen`, reused at the same `(w, h)`
    /// and rebuilt only on a resize), and submit. Returns the frame's `(w, h)`;
    /// the rendered texture (+ its blit-source bind group) live on
    /// `win.offscreen` for the caller to read back or present.
    ///
    /// `scope` selects FULL (Clear + every row — the always-correct path,
    /// byte-identical to the original encode) or SCISSORED (`RepaintScope::Dirty`):
    /// the offscreen already holds the prior frame, so we preserve it with
    /// `LoadOp::Load`, build instances ONLY for the dirty rows, and scissor the
    /// pass to the dirty rows' bounding band. A re-shaded dirty row gets the
    /// IDENTICAL instances (same values, same order) the full path would build for
    /// it, and rows are disjoint vertical bands (double-HEIGHT, the only cross-band
    /// case, forces FULL), so the scissored band is bit-identical to a full render
    /// and the untouched rows are preserved verbatim.
    ///
    /// SPEC: the bg-instance path of this method is the real implementation of the
    /// external `GpuEncode.tla` model (TRUST_NATIVE_TLA Phase 2, GPU FRAME-ENCODE
    /// safety). The CPU cell walk that `bg_inst.push(BgInstance { … })` per
    /// non-default-bg cell is the spec's `Append`; the frame encode that uploads +
    /// slices `bg_buf` is `Encode`, gated by [`should_slice`] (the real
    /// `NeverSliceEmpty` precondition — slice ONLY when `bgInst > 0`, the exact
    /// 4ab4eb9 fix for the empty-buffer wgpu panic). Tier-1 conformance drives the
    /// real [`should_slice`] decision over the bg-instance count
    /// (`tests/conformance_gpuencode.rs`); the full GPU encode needs a live device,
    /// so the slice DECISION is what is bound, which is exactly the modeled property.
    // PROJECTION (TRUST_VACUITY_GATE §2.2 / finding 2): both actions project the real
    // bg-instance buffer onto the spec's `<<bgInst, sliced>>` — `Append` bumps the
    // instance count, `Encode` is the `should_slice`-gated slice decision. The
    // projection `conformance_gpuencode.rs` drives is named `aterm_gpu::renderer::
    // project_bg_encode`; L2 requires the projection NAME be present (Trust does not
    // execute it — the slice DECISION is the aterm-side Tier-1 binding).
    #[cfg_attr(
        any(test, feature = "spec-anchors"),
        aterm_spec::refines(
            machine = "gpu_encode",
            action = "Append",
            project = "aterm_gpu::renderer::project_bg_encode"
        )
    )]
    #[cfg_attr(
        any(test, feature = "spec-anchors"),
        aterm_spec::refines(
            machine = "gpu_encode",
            action = "Encode",
            project = "aterm_gpu::renderer::project_bg_encode"
        )
    )]
    fn encode_frame(
        &mut self,
        win: &mut WindowGpu,
        input: &RenderInput,
        scope: &RepaintScope,
    ) -> (u32, u32) {
        let (rows, cols) = (input.rows, input.cols);
        let (cw, ch) = self.cpu.cell_size();
        let baseline = self.cpu.baseline();
        // Interior padding (px per edge), read from the inner CPU renderer so the
        // GPU grid is inset by the SAME amount as the CPU path: the framebuffer is
        // `2·pad` larger on each axis and every cell origin shifts by `(pad, pad)`.
        // With `pad == 0` this is byte-identical to before (the `+ pad` terms drop
        // out). Keeping the GPU and CPU pad in lockstep preserves CPU/GPU parity
        // AND the image-vs-window parity within the GPU backend (the offscreen
        // `render_input` and the on-glass `present_input` both run this encode).
        let pad = self.cpu.pad();
        let w = (cols * cw + 2 * pad) as u32;
        let h = (rows * ch + 2 * pad) as u32;

        // Which rows to (re)build instances for. FULL: every row. Dirty: only the
        // flagged rows (others are preserved on the offscreen by LoadOp::Load).
        // A closure over the scope so every per-row loop below shares ONE filter.
        let row_active = |r: usize| -> bool {
            match scope {
                RepaintScope::Full => true,
                RepaintScope::Dirty(dirty) => dirty.get(r).copied().unwrap_or(false),
            }
        };

        // Visible rows, already resolved by `extract` under the lock.
        let rendered: &[Vec<aterm_core::terminal::RenderCell>] = &input.cells;

        let (cr, cc) = (input.cursor_row, input.cursor_col);
        let cursor_in = cr < rows && cc < cols;
        // Cursor shape for THIS frame: DECSCUSR or the frontend's override,
        // gated by DECTCEM and the blink phase — read from the inner CPU
        // renderer so the suppression rules are byte-for-byte the CPU's.
        let style = self
            .cpu
            .cursor_style_override()
            .unwrap_or(input.cursor_style);
        let cursor_drawn = cursor_in
            && input.cursor_visible
            && aterm_render::cursor_shown(style, self.cpu.cursor_blink_phase());
        let block_cursor =
            cursor_drawn && matches!(style, CursorStyle::BlinkingBlock | CursorStyle::SteadyBlock);

        // Reset the persistent per-frame instance streams + key set (capacity
        // retained — no per-frame allocation in the steady state). They are
        // rebuilt below with identical contents in identical order.
        self.inst.clear();

        // Atlas for every drawable glyph across the grid: each char resolves
        // to its full glyph identity (face/style/size) via the CPU renderer's
        // cached dispatch, so the set — and the packing — is per-glyph. The set
        // is moved OUT of `self.inst` (swapping in an empty placeholder) so the
        // `self.cell_key`/`self.cpu.glyph_key` calls below can borrow `self`
        // freely; it is moved back before being read.
        // Ligature plan per active row, built via the SHARED CPU planner so the
        // GPU keys + quads the IDENTICAL `mono_gid` glyph at the IDENTICAL column
        // the CPU blits — the CPU==GPU byte-identical invariant. Built BEFORE the
        // atlas/key loops so those can borrow `self.cpu` (the planner needs `&mut`).
        // Empty rows / globally-off ligatures yield all-`PerCell` plans (the
        // pre-ligature key set, byte-identical).
        let mut row_plans: Vec<Vec<aterm_render::ColumnGlyph>> = vec![Vec::new(); rendered.len()];
        for (r, plan) in row_plans.iter_mut().enumerate() {
            if !row_active(r) {
                continue;
            }
            let break_cols = self.cpu.ligature_break_cols_for_row(input, r);
            self.cpu.row_glyph_plan(input, r, &break_cols, plan);
        }

        let mut keys = std::mem::take(&mut self.inst.keys);
        for (r, cells) in rendered.iter().enumerate() {
            // A scissored Dirty repaint only re-encodes its dirty rows (the instance
            // loops below skip the rest via `row_active`), so it only needs THOSE
            // rows' atlas keys; the resident atlases already hold the untouched rows'
            // glyphs. Under Full, `row_active` is true for every row (unchanged).
            if !row_active(r) {
                continue;
            }
            let plan = &row_plans[r];
            for (c, cell) in cells.iter().take(cols).enumerate() {
                // An image-covered cell skips its glyph (image-vs-glyph
                // precedence, mirroring the CPU `image_covers` guard), so it
                // contributes no atlas key — UNLESS the image is z<0 (behind text),
                // where the glyph still draws (shared `image_hides_glyph_at`).
                if Self::drawable(cell) && !input.image_hides_glyph_at(r, c) {
                    // A ligature-owned column contributes the shaped `mono_gid`
                    // key (matching the CPU plan); other columns the per-cell key.
                    let key = match plan
                        .get(c)
                        .copied()
                        .unwrap_or(aterm_render::ColumnGlyph::PerCell)
                    {
                        aterm_render::ColumnGlyph::Ligated(gid) => {
                            self.cpu.ligature_key(gid, aterm_render::cell_style(cell))
                        }
                        aterm_render::ColumnGlyph::PerCell => {
                            self.cell_key(input.cluster_at(r, c), cell)
                        }
                    };
                    keys.insert(key);
                    // Combining-mark glyphs share the mono atlas.
                    if input.cluster_at(r, c).is_none()
                        && let Some(marks) = input.combining_at(r, c)
                    {
                        for &m in marks {
                            keys.insert(self.cpu.glyph_key(m));
                        }
                    }
                }
            }
        }
        // Persist the atlases across frames: a subset frame (the steady state —
        // including idle cursor-blink ticks) reuses the resident textures + bind
        // groups untouched; a miss grows them incrementally; only genuine
        // overflow recreates a texture. This is the G-1 fix — no per-frame
        // rebuild/re-upload. After this, `mono_res`/`color_res` are Some and hold
        // every key in `keys`. (`keys` is still the moved-out local here, so
        // `ensure_atlases`' `&mut self` doesn't alias it; it returns to
        // `self.inst.keys` right after.)
        self.ensure_atlases(&keys);
        self.inst.keys = keys;
        // Build the per-frame inline-image texture (iTerm2 OSC 1337). `&mut self`,
        // so it runs BEFORE the resident-atlas borrows below. A no-op (drops any
        // prior plane) for image-free frames — the common path is untouched.
        self.build_image_plane(win, input);
        let mono_res = self
            .mono_res
            .as_ref()
            .expect("ensure_atlases sets mono_res");
        let color_res = self
            .color_res
            .as_ref()
            .expect("ensure_atlases sets color_res");
        let atlas = &mono_res.atlas;
        let color_atlas = &color_res.atlas;
        // UVs normalise against the RESIDENT TEXTURE height (`tex_h`), which is
        // what the GPU samples — after an incremental append the CPU `atlas.height`
        // may exceed neither but the texture is unchanged, so `tex_h` is the only
        // correct divisor. (Slot `ay` are absolute texture rows.)
        let (aw, ah) = (atlas.width as f32, mono_res.tex_h as f32);
        let (caw, cah) = (color_atlas.width as f32, color_res.tex_h as f32);
        let atlas_bind = &mono_res.bind;
        let color_bind = &color_res.bind;

        // Selection highlight, exactly the CPU rule: selection rows are
        // live-screen coords, viewport row r shows live row (r - display_offset),
        // and a selected cell's bg fill becomes the theme selection colour.
        let selection = &input.selection;
        let display_offset = input.display_offset;

        // Instances. BG: one opaque quad per cell. GLYPH: one alpha quad per
        // drawable glyph. A BLOCK cursor cell's glyph is drawn in the cell's bg
        // colour (cut out), and its bg fill (cursor colour) is appended LAST so
        // it overwrites the normal fill in the opaque bg pass — exactly the CPU.
        // These now push into the persistent (cleared) streams on `self.inst`;
        // disjoint-field borrows keep them split from `self.cpu`/`self.theme`/
        // `self.mono_res`/`self.color_res` used in the same loop. The block-
        // cursor cell's OWN glyph is held out of the main passes and drawn AFTER
        // the cursor fill (so the fill covers any neighbour glyph overflow first,
        // exactly like the CPU paints the block cursor last).
        let bg_inst = &mut self.inst.bg;
        let glyph_inst = &mut self.inst.glyph;
        let color_inst = &mut self.inst.color;
        let cursor_glyph_inst = &mut self.inst.cursor_glyph;
        let cursor_color_inst = &mut self.inst.cursor_color;
        let theme_bg = rgb4_u32(self.theme.bg);
        // Captured once so the per-cell selection-fg floor below doesn't borrow self
        // inside the loop (where self.cpu is borrowed for glyph-key resolution).
        let theme_selection = self.theme.selection;
        // Explicit selectionForeground override (read off the CPU face — the single
        // source of truth — so GPU/CPU selected-glyph colour stays identical).
        let selection_fg = self.cpu.selection_fg();
        for (r, cells) in rendered.iter().enumerate() {
            if !row_active(r) {
                continue;
            }
            // Integer pixel row top for the packed-u16 bg rects (inset by `pad`).
            let y0u = (pad + r * ch) as u16;
            let sel_row = r as i32 - display_offset;
            // DEC line size (DECDWL/DECDHL): the cell advance, glyph NEAREST
            // enlargement and dest-row clip come from the SAME helpers the CPU
            // blit uses, so the quads reproduce it exactly.
            let line_size = input.line_sizes[r];
            let rcw = aterm_render::row_cell_w(line_size, cw);
            let (scale, anchor_y) = aterm_render::row_scale(line_size, pad + r * ch, ch);
            // SCISSORED PATH ONLY: a FULL-ROW-WIDTH theme-bg quad FIRST, so the
            // band is fully re-established from background even if the per-cell
            // fills below leave any sliver (degenerate cols). bg is REPLACE and the
            // per-cell quads fully tile [0, w) (single: cols·cw == w; double-width:
            // cols·2cw ⊇ w), so this quad is entirely overwritten — byte-identical
            // to the FULL path's `LoadOp::Clear(theme.bg)` for this band, with no
            // seam and no stale contamination. (FULL path keeps the pass Clear, so
            // it does NOT emit this — its whole-target clear already covers it.)
            if matches!(scope, RepaintScope::Dirty(_)) {
                bg_inst.push(BgInstance {
                    rect: [0, y0u, w as u16, ch as u16],
                    color: theme_bg,
                });
            }
            for (c, cell) in cells.iter().take(cols).enumerate() {
                let x0u = (pad + c * rcw) as u16;
                // A lead cell is wide iff the NEXT cell is its continuation.
                let is_wide_lead = cells.get(c + 1).is_some_and(|n| n.wide);
                let color = if selection.contains_cell(sel_row, c as u16, is_wide_lead, cell.wide) {
                    rgb4_u32(self.theme.selection)
                } else {
                    rgb4(cell.bg)
                };
                bg_inst.push(BgInstance {
                    rect: [x0u, y0u, rcw as u16, ch as u16],
                    color,
                });
            }
            for (c, cell) in cells.iter().take(cols).enumerate() {
                // Image-covered cells skip their glyph (image-vs-glyph
                // precedence) — the CPU `image_covers` guard, mirrored. A z<0 image
                // (behind text) does NOT hide the glyph (shared `image_hides_glyph_at`).
                if !Self::drawable(cell) || input.image_hides_glyph_at(r, c) {
                    continue;
                }
                // Cached resolve (the atlas pass above already keyed this char).
                // Direct `self.cpu` field access (not the `cell_key` method) so
                // the `mono_res`/`color_res` borrows above stay disjoint. A
                // ligature-owned column draws the shaped `mono_gid` glyph at the
                // column origin, IDENTICAL to the CPU plan (parity).
                let key = match row_plans[r]
                    .get(c)
                    .copied()
                    .unwrap_or(aterm_render::ColumnGlyph::PerCell)
                {
                    aterm_render::ColumnGlyph::Ligated(gid) => {
                        self.cpu.ligature_key(gid, aterm_render::cell_style(cell))
                    }
                    aterm_render::ColumnGlyph::PerCell => {
                        self.cpu.resolve_cell_key(input.cluster_at(r, c), cell)
                    }
                };
                let is_cursor = block_cursor && r == cr && c == cc;
                // Colour emoji: blit straight RGBA from the colour atlas. The
                // emoji carries its own colour, so the instance `color` is unused
                // — exactly like the CPU's Rgba blit. Under the block cursor it is
                // held out and drawn over the cursor fill (own colours), as the
                // CPU does (its Rgba blit ignores the cut-out colour).
                if let Some(slot) = color_atlas.map.get(&key) {
                    if slot.gw == 0 || slot.gh == 0 {
                        continue;
                    }
                    let Some((rect, uv)) = aterm_render::glyph_quad(
                        (pad + c * rcw) as f32,
                        anchor_y,
                        baseline,
                        scale,
                        slot.ax,
                        slot.ay,
                        slot.gw,
                        slot.gh,
                        slot.xmin,
                        slot.ymin,
                        caw,
                        cah,
                    ) else {
                        continue;
                    };
                    let inst = GlyphInstance {
                        rect,
                        uv,
                        color: [0, 0, 0, 0],
                    };
                    if is_cursor {
                        cursor_color_inst.push(inst)
                    } else {
                        color_inst.push(inst)
                    };
                    continue;
                }
                let Some(slot) = atlas.map.get(&key) else {
                    continue;
                };
                if slot.gw == 0 || slot.gh == 0 {
                    continue;
                }
                // Under the block cursor the glyph is "cut out" in the cell bg
                // colour and drawn AFTER the cursor fill; otherwise normal fg —
                // floored against the selection bg for selected cells (matches CPU).
                let glyph_color = if is_cursor {
                    rgb4(cell.bg)
                } else if selection.contains_cell(
                    sel_row,
                    c as u16,
                    cells.get(c + 1).is_some_and(|n| n.wide),
                    cell.wide,
                ) {
                    rgb4_u32(selection_fg.unwrap_or_else(|| {
                        aterm_render::floor_selection_fg(
                            aterm_render::rgb_to_u32(cell.fg),
                            theme_selection,
                        )
                    }))
                } else {
                    rgb4(cell.fg)
                };
                let Some((rect, uv)) = aterm_render::glyph_quad(
                    (pad + c * rcw) as f32,
                    anchor_y,
                    baseline,
                    scale,
                    slot.ax,
                    slot.ay,
                    slot.gw,
                    slot.gh,
                    slot.xmin,
                    slot.ymin,
                    aw,
                    ah,
                ) else {
                    continue;
                };
                let inst = GlyphInstance {
                    rect,
                    uv,
                    color: glyph_color,
                };
                if is_cursor {
                    cursor_glyph_inst.push(inst)
                } else {
                    glyph_inst.push(inst)
                };
            }
        }
        // Combining diacritics: overlay each mark's glyph on its base cell, in
        // the cell foreground — appended AFTER the bases so they draw on top,
        // matching the CPU's mark-after-base blit order.
        for (r, cells) in rendered.iter().enumerate() {
            if !row_active(r) || input.combining[r].is_empty() {
                continue;
            }
            let line_size = input.line_sizes[r];
            let rcw = aterm_render::row_cell_w(line_size, cw);
            // Pad the row origin (`pad + r * ch`) to MATCH the base-glyph loop
            // (line ~2328) and the CPU path. Without the `pad` the GPU rendered
            // decomposed combining marks `pad` px too high — a CPU/GPU divergence
            // for NFD sequences (e.g. base + U+0301). The mark x is already padded
            // below, so this aligns the y onto the identical pixel.
            let (scale, anchor_y) = aterm_render::row_scale(line_size, pad + r * ch, ch);
            for (c, cell) in cells.iter().take(cols).enumerate() {
                // Image-covered cells skip their combining overlay too (the CPU
                // overlays marks only inside the non-image glyph path).
                if cell.wide || input.cluster_at(r, c).is_some() || input.image_at(r, c).is_some() {
                    continue;
                }
                let Some(marks) = input.combining_at(r, c) else {
                    continue;
                };
                for &m in marks {
                    let key = self.cpu.glyph_key(m);
                    let Some(slot) = atlas.map.get(&key) else {
                        continue;
                    };
                    if slot.gw == 0 || slot.gh == 0 {
                        continue;
                    }
                    // Centre the mark's ink in the cell (see CPU `mark_cell_x`):
                    // identical integer arithmetic → identical pixel position.
                    let cx = aterm_render::mark_cell_x(c, rcw, slot.gw as usize, slot.xmin, scale)
                        + pad as i32;
                    let Some((rect, uv)) = aterm_render::glyph_quad(
                        cx as f32, anchor_y, baseline, scale, slot.ax, slot.ay, slot.gw, slot.gh,
                        slot.xmin, slot.ymin, aw, ah,
                    ) else {
                        continue;
                    };
                    glyph_inst.push(GlyphInstance {
                        rect,
                        uv,
                        color: rgb4(cell.fg),
                    });
                }
            }
        }
        // Inline images (iTerm2 OSC 1337 `File=`): one quad per image-covered
        // cell, sampling that cell's tile of the per-frame image texture. Built
        // ONLY when `image_plane` was populated this frame (an image-free frame
        // leaves the stream empty → zero image draws, byte-identical text path).
        // Geometry MIRRORS the CPU `render_one_row` Pass 1b EXACTLY: the tile's
        // dest box is `cell_w × cell_h` (the image is NEVER DEC-scaled — the CPU
        // `blit_image_cell` blits a natural-size tile), but it is positioned at the
        // ROW cell advance `c * rcw` (so on a DECDWL/DECDHL row, tiles are spaced
        // by `2*cell_w` with bg gaps between them, just like the CPU). The UV is
        // the cell's `(cell_col, cell_row)` tile inside the image's footprint
        // region of the stacked texture — NEAREST-sampled, so each texel maps to
        // the CPU's 1:1 per-cell copy (the parity gate).
        if let Some(plane) = win.image_plane.as_ref() {
            let image_inst = &mut self.inst.image;
            let (pw, ph) = (plane.w as f32, plane.h as f32);
            for (r, row_images) in input.images.iter().enumerate() {
                if !row_active(r) || row_images.is_empty() {
                    continue;
                }
                // Row cell advance, doubled on a DEC double-size line — exactly the
                // `cw` the CPU Pass 1b passes as the per-cell x stride.
                let rcw = aterm_render::row_cell_w(input.line_sizes[r], cw);
                // Inset the image tile's dest origin by `pad`, exactly like the CPU
                // `blit_image_cell` call site (which passes `pad_x + c*cw`, `y0 =
                // pad + r*cell_h`). Without this the inline image would draw at the
                // unpadded grid origin while glyphs/bg shifted — breaking both the
                // CPU/GPU parity and the image-vs-window parity. `pad == 0` keeps the
                // historical origin (byte-identical).
                let y0 = (pad + r * ch) as f32;
                for (c, image) in row_images {
                    if *c >= cols {
                        continue;
                    }
                    let fp_w = image.image.cols as usize * cw;
                    let fp_h = image.image.rows as usize * ch;
                    let key = (std::sync::Arc::as_ptr(&image.image) as usize, fp_w, fp_h);
                    let Some(&(img_y0, dw, dh)) = plane.placements.get(&key) else {
                        // Failed decode (no texture rows): nothing to draw, the
                        // cell bg shows through — exactly the CPU negative-cache.
                        continue;
                    };
                    // Source tile origin within the footprint (CPU `blit_image_cell`
                    // uses `cell_col*cw`/`cell_row*ch`), offset by the image's row in
                    // the stacked texture. Clamp the tile to the footprint so a cell
                    // at the image edge never samples a neighbour's region.
                    let sx0 = (image.cell_col as usize * cw) as u32;
                    let sy0 = (image.cell_row as usize * ch) as u32;
                    if sx0 >= dw || sy0 >= dh {
                        continue;
                    }
                    let tile_w = (cw as u32).min(dw - sx0);
                    let tile_h = (ch as u32).min(dh - sy0);
                    let x0 = (pad + *c * rcw) as f32;
                    let rect = [x0, y0, tile_w as f32, tile_h as f32];
                    let uv = [
                        sx0 as f32 / pw,
                        (img_y0 + sy0) as f32 / ph,
                        tile_w as f32 / pw,
                        tile_h as f32 / ph,
                    ];
                    image_inst.push(GlyphInstance {
                        rect,
                        uv,
                        color: [0, 0, 0, 0],
                    });
                }
            }
        }
        // Cursor-row cell width (doubled on any DEC double-size line).
        let cur_cw = if cr < rows {
            aterm_render::row_cell_w(input.line_sizes[cr], cw)
        } else {
            cw
        };
        // Block-cursor fill — drawn AFTER the glyph/decoration passes (not in the
        // bg pass) so it covers any neighbour glyph overflow into the cursor
        // cell, exactly as the CPU paints the block cursor last. The cell's own
        // glyph is then re-drawn over it (cut-out) from cursor_glyph/color_inst.
        // (Pushed into the cleared persistent `cursor_block` stream: an empty
        // stream == the old `Vec::new()`, a single push == the old one-elem vec.)
        // (`row_active(cr)` is always true here in the scissored path — the cursor
        // row is in the dirty set whenever the cursor is shown — but guard it
        // explicitly so no cursor instance can ever leak outside the dirty band.)
        if block_cursor && row_active(cr) {
            self.inst.cursor_block.push(BgInstance {
                rect: [
                    (pad + cc * cur_cw) as u16,
                    (pad + cr * ch) as u16,
                    cur_cw as u16,
                    ch as u16,
                ],
                color: rgb4_u32(self.theme.cursor),
            });
        }

        // Line decorations (underline / strikethrough / overline) OVER the
        // glyphs — same rects as the CPU Pass 3 (`aterm_render::underline_rects`
        // / `strike_overline_rects`), drawn as opaque quads in a pass after the
        // glyphs so CPU and GPU produce identical pixels.
        let deco_inst = &mut self.inst.deco;
        for (r, cells) in rendered.iter().enumerate() {
            if !row_active(r) {
                continue;
            }
            let y0 = pad + r * ch;
            let rcw = aterm_render::row_cell_w(input.line_sizes[r], cw);
            for (c, cell) in cells.iter().take(cols).enumerate() {
                if cell.wide
                    || (matches!(cell.underline, UnderlineStyle::None)
                        && !cell.strikethrough
                        && !cell.overline)
                {
                    continue;
                }
                let x = pad + c * rcw;
                let dw = if cells.get(c + 1).is_some_and(|n| n.wide) {
                    2 * rcw
                } else {
                    rcw
                };
                let ucolor = rgb4(cell.underline_color.unwrap_or(cell.fg));
                for [rx, ry, rw, rh] in
                    aterm_render::underline_rects(cell.underline, x, y0, dw, ch, baseline)
                {
                    deco_inst.push(BgInstance {
                        rect: [rx as u16, ry as u16, rw as u16, rh as u16],
                        color: ucolor,
                    });
                }
                let fgc = rgb4(cell.fg);
                for [rx, ry, rw, rh] in aterm_render::strike_overline_rects(
                    cell.strikethrough,
                    cell.overline,
                    x,
                    y0,
                    dw,
                    ch,
                    baseline,
                ) {
                    deco_inst.push(BgInstance {
                        rect: [rx as u16, ry as u16, rw as u16, rh as u16],
                        color: fgc,
                    });
                }
            }
        }
        // Underline/bar/hollow cursors paint OVER the glyph (the CPU fills
        // them after its glyph blits), so their quads form a third pass that
        // runs after the glyph pass. Same rects as the CPU: `cursor_rects`.
        // (Extends the cleared persistent `cursor` stream — identical contents
        // in identical order to the old `.collect()`.)
        if cursor_drawn && !block_cursor && row_active(cr) {
            let cursor_color = rgb4_u32(self.theme.cursor);
            self.inst.cursor.extend(
                aterm_render::cursor_rects(style, pad + cc * cur_cw, pad + cr * ch, cur_cw, ch)
                    .into_iter()
                    .map(|[x, y, rw, rh]| BgInstance {
                        rect: [x as u16, y as u16, rw as u16, rh as u16],
                        color: cursor_color,
                    }),
            );
        }

        // Upload uniforms. `Uniforms.screen` is a pure function of the frame
        // size, so it is rewritten ONLY on the first frame and on a resize — not
        // every frame (the steady-state present otherwise re-uploaded an unchanged
        // 16-byte buffer each frame). The bytes are byte-identical to the old
        // every-frame write for the same `(w, h)`.
        if win.uniform_dims != Some((w, h)) {
            self.ctx.queue.write_buffer(
                &self.uniform_buf,
                0,
                bytemuck::bytes_of(&Uniforms {
                    screen: [w as f32, h as f32],
                    _pad: [0.0, 0.0],
                }),
            );
            win.uniform_dims = Some((w, h));
        }
        // Per-frame vertex streams now reuse persistent buffers (grow-only), so
        // there is no per-frame allocation in the common case — only a
        // `write_buffer` copy. `upload` returns `None` for an empty stream,
        // exactly like the old `Option<Buffer>` gating: an EMPTY `bg` stream (e.g.
        // a degenerate/zero-cell frame) draws nothing, and the bg pass still
        // CLEARS the target (LoadOp::Clear) — matching the CPU's all-background
        // frame. We also slice each buffer to EXACTLY this frame's byte length so
        // stale tail bytes from a larger previous frame are never bound or drawn.
        let (device, queue) = (&self.ctx.device, &self.ctx.queue);
        let bg_buf = self
            .vbufs
            .bg
            .upload(device, queue, bytemuck::cast_slice(&self.inst.bg));
        let image_buf =
            self.vbufs
                .image
                .upload(device, queue, bytemuck::cast_slice(&self.inst.image));
        let glyph_buf =
            self.vbufs
                .glyph
                .upload(device, queue, bytemuck::cast_slice(&self.inst.glyph));
        let color_buf =
            self.vbufs
                .color
                .upload(device, queue, bytemuck::cast_slice(&self.inst.color));
        let cursor_buf =
            self.vbufs
                .cursor
                .upload(device, queue, bytemuck::cast_slice(&self.inst.cursor));
        let deco_buf = self
            .vbufs
            .deco
            .upload(device, queue, bytemuck::cast_slice(&self.inst.deco));
        let cursor_block_buf = self.vbufs.cursor_block.upload(
            device,
            queue,
            bytemuck::cast_slice(&self.inst.cursor_block),
        );
        let cursor_glyph_buf = self.vbufs.cursor_glyph.upload(
            device,
            queue,
            bytemuck::cast_slice(&self.inst.cursor_glyph),
        );
        let cursor_color_buf = self.vbufs.cursor_color.upload(
            device,
            queue,
            bytemuck::cast_slice(&self.inst.cursor_color),
        );

        // Resident offscreen target: reuse the texture + view when `(w, h)` is
        // unchanged; (re)create them only on the first frame or a resize. On a
        // (re)create the previous target (and the blit-source bind group built
        // from it in `present_input`) is replaced — including the per-format blit
        // PIPELINES staying valid (they key on swapchain format, not this view),
        // so no stale resource survives a dimension change. Usage is unchanged
        // (`RENDER_ATTACHMENT | COPY_SRC | TEXTURE_BINDING`).
        let recreate = match &win.offscreen {
            Some(o) => o.w != w || o.h != h,
            None => true,
        };
        if recreate {
            let tex = self.ctx.offscreen_texture(w, h);
            let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
            // The blit-source bind group samples this exact `tex` into the
            // swapchain. Built ONCE here (and reused every present) instead of
            // per-present. `present_input` only writes the per-frame invert flag.
            let src_view = tex.create_view(&wgpu::TextureViewDescriptor::default());
            let blit_bind = self
                .ctx
                .device
                .create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("aterm-gpu blit bg"),
                    layout: &self.blit_bgl,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&src_view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&self.blit_sampler),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: self.blit_uniform_buf.as_entire_binding(),
                        },
                    ],
                });
            win.offscreen = Some(Offscreen {
                tex,
                view,
                blit_bind,
                w,
                h,
            });
        }

        // SCISSORED PATH: the load op + the scissor band. FULL clears the whole
        // target (== CPU `vec![theme.bg]`); SCISSORED loads the prior frame
        // (preserving every untouched row) and clips the pass to the dirty rows'
        // bounding band so only those pixels are written. The dirty-row instances
        // we built above are the ONLY draws, and rows are disjoint vertical bands,
        // so the band is bit-identical to a full render and the rest is verbatim.
        //
        // Load on a JUST-(re)created texture would read undefined tiles — but the
        // scissor path is only chosen by `encode_present_frame` when the offscreen
        // already held the prior frame at these dims (so `recreate` is false).
        // Assert that invariant, and fall back to Clear if it is ever violated.
        let (load_op, scissor) = match &scope {
            RepaintScope::Dirty(dirty) if !recreate => {
                // Bounding band [y0, y1) over all dirty rows, clamped to the frame.
                let mut first = None;
                let mut last = 0usize;
                for (r, &d) in dirty.iter().enumerate() {
                    if d {
                        first.get_or_insert(r);
                        last = r;
                    }
                }
                match first {
                    Some(f) => {
                        // Inset the dirty band by `pad` (the grid origin), exactly
                        // like the per-row instances above. The top/bottom pad
                        // bands are bg from the first full render and preserved by
                        // `LoadOp::Load`, so they never need rewriting.
                        let y0 = (pad + f * ch) as u32;
                        let y1 = ((pad + (last + 1) * ch) as u32).min(h);
                        (
                            wgpu::LoadOp::Load,
                            Some((0u32, y0, w, y1.saturating_sub(y0))),
                        )
                    }
                    // Reusable but zero dirty rows: nothing to draw. Load preserves
                    // the prior frame; a degenerate 0-height scissor draws nothing.
                    None => (wgpu::LoadOp::Load, Some((0, 0, 0, 0))),
                }
            }
            // FULL (or the can't-happen Dirty-after-recreate): clear everything.
            _ => {
                debug_assert!(
                    matches!(scope, RepaintScope::Full),
                    "scissored Load requires the prior frame resident (no recreate)"
                );
                (wgpu::LoadOp::Clear(theme_color(self.theme.bg)), None)
            }
        };

        let view = &win.offscreen.as_ref().expect("offscreen set above").view;
        let mut enc = self
            .ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("aterm-gpu frame"),
            });

        // SINGLE render pass: the eight former passes (bg → glyph → color →
        // deco → cursor_block → cursor_glyph → cursor_color → cursor_strip) all
        // targeted the SAME offscreen view with LoadOp::Clear (first) then
        // LoadOp::Load (the rest). Collapsing them into one `begin_render_pass`
        // removes seven gratuitous pass setups and — on Apple-Silicon/TBDR —
        // seven full-target store+reload tile round-trips, while issuing the
        // EXACT same draws in the EXACT same order with the same pipelines and
        // blend states, so the output stays BYTE-IDENTICAL.
        //
        // Safe to fuse because (confirmed above):
        //   * the render target is RENDER_ATTACHMENT|COPY_SRC only (no
        //     TEXTURE_BINDING), and shaders sample only the atlas — no
        //     read-after-write hazard between streams;
        //   * no MSAA (sample_count 1), no depth/stencil, no resolve target;
        //   * no scissor/viewport/blend-constant/stencil state to re-establish.
        //
        // LoadOp::Clear stays the pass's single load op (clearing to the theme
        // bg, == CPU `vec![theme.bg]`); we do NOT switch to Load (which on
        // Metal/TBDR would force a tile load-from-undefined). Bind group 0
        // (`uniform_bg`) is identical for all three pipelines, so it is set ONCE.
        // `set_pipeline`/`set_bind_group(1, ..)` are emitted only when the
        // pipeline / atlas actually changes between consecutive *drawn* streams
        // — the gating (skip-when-empty) is preserved, so an empty stream sets
        // no state and draws nothing, exactly as before.
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("aterm-gpu frame pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: load_op,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            // SCISSORED PATH: clip the pass to the dirty rows' bounding band so
            // only those pixels are written; the LoadOp::Load above preserves the
            // rest. A `(_, _, 0, 0)` rect (zero dirty rows) draws nothing — the
            // prior frame is re-presented verbatim. FULL leaves the default
            // full-target scissor.
            if let Some((sx, sy, sw, sh)) = scissor {
                pass.set_scissor_rect(sx, sy, sw, sh);
            }
            // Bind group 0 is shared (same `uniform_bg`, same layout slot) by
            // every pipeline — set once for the whole pass.
            pass.set_bind_group(0, &self.uniform_bg, &[]);

            // Track which pipeline / bind-group-1 (atlas) are currently set so
            // we emit `set_pipeline` / `set_bind_group(1, ..)` only on a real
            // change between consecutive drawn streams.
            #[derive(PartialEq, Clone, Copy)]
            enum Pipe {
                Bg,
                Glyph,
                Color,
            }
            #[derive(PartialEq, Clone, Copy)]
            enum Atlas {
                Mono,
                Color,
                /// The per-frame inline-image texture (bound through the SAME
                /// group-1 layout as the colour atlas, sampled by the SAME
                /// `color_glyph_pipeline`); distinct so the bind-group tracker
                /// rebinds when switching between emoji and image streams.
                Image,
            }
            let mut cur_pipe: Option<Pipe> = None;
            let mut cur_atlas: Option<Atlas> = None;

            // Helper: bind the pipeline (+ atlas group for glyph streams) only
            // when it changes, then draw the stream's quads. `atlas` is `None`
            // for the bg pipeline (its layout has no group 1).
            macro_rules! draw_stream {
                ($buf:expr, $insts:expr, $pipe:expr, $pipeline:expr, $atlas:expr) => {
                    if let Some(buf) = $buf.as_ref() {
                        if cur_pipe != Some($pipe) {
                            pass.set_pipeline($pipeline);
                            cur_pipe = Some($pipe);
                        }
                        if let Some((atlas_kind, atlas_bg)) = $atlas {
                            if cur_atlas != Some(atlas_kind) {
                                pass.set_bind_group(1, atlas_bg, &[]);
                                cur_atlas = Some(atlas_kind);
                            }
                        }
                        pass.set_vertex_buffer(0, *buf);
                        pass.draw(0..6, 0..$insts.len() as u32);
                    }
                };
            }

            // bg: cell background fills (REPLACE). The pass already cleared the
            // target, so an empty frame stays the theme bg.
            draw_stream!(
                bg_buf,
                self.inst.bg,
                Pipe::Bg,
                &self.bg_pipeline,
                None::<(Atlas, &wgpu::BindGroup)>
            );
            // glyph: mono glyphs, alpha-blended over the bg.
            draw_stream!(
                glyph_buf,
                self.inst.glyph,
                Pipe::Glyph,
                &self.glyph_pipeline,
                Some((Atlas::Mono, atlas_bind))
            );
            // color: colour-emoji glyphs (straight RGBA, alpha-blended).
            draw_stream!(
                color_buf,
                self.inst.color,
                Pipe::Color,
                &self.color_glyph_pipeline,
                Some((Atlas::Color, color_bind))
            );
            // image: inline-image cell tiles (straight RGBA over the bg, NEAREST,
            // alpha-blended) — drawn AFTER the colour glyphs and BEFORE the
            // decorations/cursor, reusing the colour-glyph pipeline but sampling
            // the per-frame image texture. `image_buf` is `None` (empty stream) for
            // image-free frames, so this is a no-op there.
            if let Some(plane) = win.image_plane.as_ref() {
                draw_stream!(
                    image_buf,
                    self.inst.image,
                    Pipe::Color,
                    &self.color_glyph_pipeline,
                    Some((Atlas::Image, &plane.bind))
                );
            }
            // deco: line decorations (underline/strike/overline), opaque, over
            // the glyphs and before the cursor.
            draw_stream!(
                deco_buf,
                self.inst.deco,
                Pipe::Bg,
                &self.bg_pipeline,
                None::<(Atlas, &wgpu::BindGroup)>
            );
            // cursor_block: block-cursor fill, opaque over the glyphs/deco.
            draw_stream!(
                cursor_block_buf,
                self.inst.cursor_block,
                Pipe::Bg,
                &self.bg_pipeline,
                None::<(Atlas, &wgpu::BindGroup)>
            );
            // cursor_glyph: the cursor cell's own mono glyph, cut out in the
            // cell bg colour over the fill.
            draw_stream!(
                cursor_glyph_buf,
                self.inst.cursor_glyph,
                Pipe::Glyph,
                &self.glyph_pipeline,
                Some((Atlas::Mono, atlas_bind))
            );
            // cursor_color: a colour-emoji cursor cell glyph over the fill.
            draw_stream!(
                cursor_color_buf,
                self.inst.cursor_color,
                Pipe::Color,
                &self.color_glyph_pipeline,
                Some((Atlas::Color, color_bind))
            );
            // cursor_strip: non-block cursor quads (underline/bar/hollow),
            // opaque over the glyphs — painted last, like the CPU.
            draw_stream!(
                cursor_buf,
                self.inst.cursor,
                Pipe::Bg,
                &self.bg_pipeline,
                None::<(Atlas, &wgpu::BindGroup)>
            );
            // The last `draw_stream!` may store into the trackers without a
            // subsequent read; acknowledge that so the tracking stays explicit.
            let _ = (cur_pipe, cur_atlas);
        }

        self.ctx.queue.submit([enc.finish()]);
        // Record the total instances built this frame (diagnostic). In the
        // scissored path this is ~proportional to the dirty-row count, not the
        // screen — the headline win.
        self.last_instances = self.inst.bg.len()
            + self.inst.image.len()
            + self.inst.glyph.len()
            + self.inst.color.len()
            + self.inst.cursor.len()
            + self.inst.deco.len()
            + self.inst.cursor_block.len()
            + self.inst.cursor_glyph.len()
            + self.inst.cursor_color.len();
        // The rendered target lives on `win.offscreen` (resident across frames);
        // callers read it from there (`render_input` for readback, `present_input`
        // for the blit source).
        (w, h)
    }
}

/// `[r, g, b]` (0..=255) -> opaque RGBA bytes (a == 255). The `Unorm8x4` vertex
/// attribute decodes these as exactly `value/255.0` — the identical IEEE-754
/// floats the old `[f32;4]` form computed, so packing stays byte-identical.
fn rgb4([r, g, b]: [u8; 3]) -> [u8; 4] {
    [r, g, b, 255]
}

/// `0x00RRGGBB` -> opaque RGBA bytes (a == 255).
fn rgb4_u32(c: u32) -> [u8; 4] {
    rgb4([(c >> 16) as u8, (c >> 8) as u8, c as u8])
}

/// Decode the inline image identified by `key` (`(arc_ptr, fp_w, fp_h)`) to its
/// footprint RGBA, finding the matching `ImageRef` in `input` by `Arc` identity.
/// Uses the SAME `aterm_render::decode_image_to_footprint` the CPU path caches, so
/// the bytes the GPU samples are byte-identical to the CPU `blit_image_cell` copy.
/// Returns a negative result (empty `rgba`) when the image is absent or fails to
/// decode — cached so a bad image draws nothing without re-decoding every frame.
fn decode_for_key(
    input: &RenderInput,
    key: (usize, usize, usize),
    cw: usize,
    ch: usize,
) -> GpuDecodedImage {
    let (arc_ptr, fp_w, fp_h) = key;
    for row in &input.images {
        for (_c, image) in row {
            if std::sync::Arc::as_ptr(&image.image) as usize == arc_ptr
                && image.image.cols as usize * cw == fp_w
                && image.image.rows as usize * ch == fp_h
            {
                let rgba = aterm_render::decode_image_to_footprint(
                    &image.image.bytes,
                    image.image.format,
                    fp_w,
                    fp_h,
                )
                .unwrap_or_default();
                return GpuDecodedImage {
                    w: fp_w as u32,
                    h: fp_h as u32,
                    rgba,
                };
            }
        }
    }
    GpuDecodedImage {
        w: fp_w as u32,
        h: fp_h as u32,
        rgba: Vec::new(),
    }
}

/// `0x00RRGGBB` -> a `wgpu::Color` clear value (linear; matches CPU's raw bytes).
fn theme_color(c: u32) -> wgpu::Color {
    wgpu::Color {
        r: ((c >> 16) & 0xff) as f64 / 255.0,
        g: ((c >> 8) & 0xff) as f64 / 255.0,
        b: (c & 0xff) as f64 / 255.0,
        a: 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aterm_core::terminal::Terminal;
    use aterm_render::Theme;

    /// `pack_image_plane`'s `dw == tw` single-memcpy fast path must produce
    /// BYTE-IDENTICAL output to a naive row-by-row pack — the invariant the GPU
    /// build path relies on (also guarded end-to-end by `inline_image_parity`).
    #[test]
    fn pack_image_plane_fast_path_matches_per_row() {
        // Reference: always copy row by row (the pre-optimization path).
        fn per_row(items: &[(u32, u32, u32, &[u8])], tw: u32, th: u32) -> Vec<u8> {
            let mut data = vec![0u8; (tw * th * 4) as usize];
            for &(y0, dw, dh, rgba) in items {
                for y in 0..dh {
                    let src = (y * dw * 4) as usize;
                    let dst = ((y0 + y) * tw) as usize * 4;
                    data[dst..dst + (dw * 4) as usize]
                        .copy_from_slice(&rgba[src..src + (dw * 4) as usize]);
                }
            }
            data
        }
        // Deterministic raster: distinct bytes per pixel so a mis-copy is visible.
        let raster = |w: u32, h: u32, seed: u8| -> Vec<u8> {
            (0..(w * h * 4))
                .map(|i| (i as u8).wrapping_add(seed))
                .collect()
        };
        let a = raster(7, 5, 1);
        // Case 1: single full-width image (dw == tw -> fast path).
        let items = [(0u32, 7u32, 5u32, a.as_slice())];
        assert_eq!(
            GpuRenderer::pack_image_plane(&items, 7, 5),
            per_row(&items, 7, 5)
        );
        // Case 2: two stacked same-width images (both hit the fast path).
        let b = raster(7, 3, 9);
        let items = [(0u32, 7, 5, a.as_slice()), (5u32, 7, 3, b.as_slice())];
        assert_eq!(
            GpuRenderer::pack_image_plane(&items, 7, 8),
            per_row(&items, 7, 8)
        );
        // Case 3: a narrow image under a wider plane (dw < tw -> else path).
        let narrow = raster(3, 4, 4);
        let wide = raster(7, 2, 7);
        let items = [
            (0u32, 7, 2, wide.as_slice()),
            (2u32, 3, 4, narrow.as_slice()),
        ];
        assert_eq!(
            GpuRenderer::pack_image_plane(&items, 7, 6),
            per_row(&items, 7, 6)
        );
    }

    /// SACRED CONSTRAINT (rendering architecture): the GPU consumes the CPU
    /// renderer's EXACT glyph bytes. For every glyph of a representative frame
    /// (the parity-suite demo grid: red RR, blue bg, CJK 日本 via fallback,
    /// inverse XX, plain ab, cursor), every texel the atlas holds for that
    /// glyph must equal the CPU cache byte — exact, not within tolerance.
    /// Pure CPU: `build_atlas` needs no GPU device, so this runs headless.
    #[test]
    fn atlas_texel_bytes_match_cpu_glyph_bytes_exactly() {
        let Some(mut cpu) = Renderer::from_system(18.0, Theme::default()) else {
            eprintln!("SKIP: no system monospace font");
            return;
        };

        let (rows, cols) = (6usize, 12usize);
        let mut term = Terminal::new(rows as u16, cols as u16);
        term.process(
            b"\x1b[31mRR\x1b[0m\r\n\
\x1b[44m  \x1b[0m\r\n\
\xe6\x97\xa5\xe6\x9c\xac\r\n\
\x1b[7mXX\x1b[0m\r\n\
ab\r\n",
        );

        // The same key set encode_frame builds for this frame.
        let input = term.cell_frame(rows, cols);
        let mut keys: BTreeSet<GlyphKey> = BTreeSet::new();
        for cells in &input.cells {
            for cell in cells.iter().take(cols) {
                if GpuRenderer::drawable(cell) {
                    keys.insert(cpu.glyph_key(cell.ch));
                }
            }
        }
        assert!(
            keys.len() >= 5,
            "demo frame should contribute several glyphs"
        );

        let atlas = build_atlas(&mut cpu, &keys, u32::MAX);
        for &key in &keys {
            let slot = atlas
                .map
                .get(&key)
                .expect("every requested glyph gets a slot");
            let img = cpu.glyph_image(key).clone();
            let GlyphImage::Mono {
                width,
                height,
                bytes,
                ..
            } = &img
            else {
                panic!("char keys rasterize as Mono");
            };
            if *width == 0 || *height == 0 {
                assert_eq!(
                    (slot.gw, slot.gh),
                    (0, 0),
                    "empty glyph must pack as an empty slot"
                );
                continue;
            }
            assert_eq!(
                (slot.gw as usize, slot.gh as usize),
                (*width, *height),
                "slot size differs from CPU bitmap for {:?}",
                key.chr()
            );
            for j in 0..slot.gh {
                for i in 0..slot.gw {
                    let atlas_byte =
                        atlas.data[((slot.ay + j) * atlas.width + slot.ax + i) as usize];
                    let cpu_byte = bytes[(j * slot.gw + i) as usize];
                    assert_eq!(
                        atlas_byte,
                        cpu_byte,
                        "atlas texel ({i},{j}) of {:?} differs from the CPU cache byte",
                        key.chr()
                    );
                }
            }
        }
    }

    /// G-1 fix gate: the glyph atlas is PERSISTED across frames. Two consecutive
    /// `render_input` calls with an UNCHANGED glyph set must NOT create a new
    /// atlas texture (the steady state — incl. idle cursor-blink ticks — reuses
    /// the resident textures + bind groups untouched). Asserted two ways: the
    /// texture-creation counter does not advance, and the resident texture dims
    /// are byte-identical between the frames (same textures, not recreated ones).
    /// Gated: no GPU/font -> skip cleanly.
    #[test]
    fn set_font_theme_keeps_configured_family() {
        // Regression (the multi-window/splits merge LOST this): the in-place GPU
        // rebuild (zoom / config hot-reload / Retina auto-scale) must re-resolve the
        // CONFIGURED font family, not the system monospace. Before the fix
        // `set_font_theme` called the family-LESS `from_system`, so a configured
        // family was silently dropped on the first Retina-forced rebuild. Construct
        // WITH a family, rebuild via set_font_theme, and confirm the family is wired
        // onto GpuRenderer and the rebuild leaves a valid renderer. (A face-name
        // assertion would need a Renderer resolved-family accessor — a follow-up.)
        let theme = Theme::default();
        let mut gpu = match GpuRenderer::new_with_family(Some("Menlo"), 16.0, theme) {
            Ok(g) => g,
            Err(e) => {
                eprintln!("SKIP: no GPU/font available: {e}");
                return;
            }
        };
        assert_eq!(
            gpu.font_family.as_deref(),
            Some("Menlo"),
            "family wired at construction"
        );
        gpu.set_font_theme(24.0, theme)
            .expect("in-place rebuild succeeds with a configured family");
        assert_eq!(
            gpu.font_family.as_deref(),
            Some("Menlo"),
            "family retained across rebuild"
        );
        let (cw, ch) = gpu.cell_size();
        assert!(
            cw > 0 && ch > 0,
            "renderer valid after the family-aware rebuild"
        );
    }

    #[test]
    fn atlas_persists_across_unchanged_frames() {
        let theme = Theme::default();
        let px = 18.0;
        let mut gpu = match GpuRenderer::new(px, theme) {
            Ok(g) => g,
            Err(e) => {
                eprintln!("SKIP: no GPU/font available: {e}");
                return;
            }
        };

        // A small static grid (cursor hidden so a blink phase can't change the
        // glyph set), then the EXACT same input rendered again.
        let (rows, cols) = (3usize, 8usize);
        let mut term = Terminal::new(rows as u16, cols as u16);
        term.process(b"\x1b[?25lhello\r\nworld\r\nabcd");
        let input = term.cell_frame(rows, cols);
        let mut win = WindowGpu::new();

        // Frame 1: cold cache -> the atlases are built (both textures created).
        let _ = gpu.render_input(&mut win, &input);
        let after_first = gpu.atlas_tex_creations();
        let dims_first = gpu
            .atlas_tex_dims()
            .expect("atlases resident after first frame");
        assert!(
            after_first >= 1,
            "first frame should have created at least one atlas texture"
        );

        // Frame 2: identical glyph set -> reuse, NO new texture creation.
        let _ = gpu.render_input(&mut win, &input);
        let after_second = gpu.atlas_tex_creations();
        let dims_second = gpu.atlas_tex_dims().expect("atlases still resident");

        assert_eq!(
            after_first, after_second,
            "an unchanged-glyph frame must NOT create a new atlas texture \
             (creations {after_first} -> {after_second}) — the atlas is not persisting"
        );
        assert_eq!(
            dims_first, dims_second,
            "resident atlas texture dims changed across an unchanged frame — textures were recreated"
        );

        // And a THIRD identical frame is still a no-op (steady state holds).
        let _ = gpu.render_input(&mut win, &input);
        assert_eq!(
            after_second,
            gpu.atlas_tex_creations(),
            "repeated unchanged frames must keep reusing the resident atlas"
        );
    }

    /// Companion to the persistence test: introducing a NEW glyph must NOT force
    /// a full repack into a new texture in the common case — it APPENDS into the
    /// resident atlas (incremental growth) via a sub-region upload, so the
    /// texture identity (dims) is unchanged and no new texture is created. Only
    /// genuine overflow recreates. Gated: no GPU/font -> skip.
    #[test]
    fn new_glyph_grows_atlas_in_place_without_recreating_texture() {
        let theme = Theme::default();
        let px = 18.0;
        let mut gpu = match GpuRenderer::new(px, theme) {
            Ok(g) => g,
            Err(e) => {
                eprintln!("SKIP: no GPU/font available: {e}");
                return;
            }
        };
        let (rows, cols) = (2usize, 8usize);

        let mut win = WindowGpu::new();
        let render = |gpu: &mut GpuRenderer, win: &mut WindowGpu, bytes: &[u8]| {
            let mut term = Terminal::new(rows as u16, cols as u16);
            term.process(bytes);
            let input = term.cell_frame(rows, cols);
            gpu.render_input(win, &input);
        };

        // Cold frame with a few glyphs.
        render(&mut gpu, &mut win, b"\x1b[?25labc");
        let creations_after_cold = gpu.atlas_tex_creations();
        let dims_after_cold = gpu.atlas_tex_dims().expect("atlases resident");

        // A frame adding NEW glyphs (xyz) on top of the resident set: the mono
        // atlas grows in place. The R8 atlas is 1024 wide with vast vertical
        // headroom, so three more small glyphs append without overflow — no new
        // texture, same dims.
        render(&mut gpu, &mut win, b"\x1b[?25labcxyz");
        assert_eq!(
            creations_after_cold,
            gpu.atlas_tex_creations(),
            "appending a few new glyphs must grow the atlas in place, not recreate the texture"
        );
        assert_eq!(
            dims_after_cold,
            gpu.atlas_tex_dims().expect("atlases still resident"),
            "incremental growth must keep the SAME atlas texture (dims unchanged)"
        );
    }

    /// FIX 3 gate: `present_prev`/`prev_input` are PER-WINDOW. Two `WindowGpu`
    /// driven INTERLEAVED through the scissored present-readback path against ONE
    /// shared `GpuRenderer` (at equal dims) must each read back byte-identical to a
    /// FRESH FULL render of THAT window's own input. If the prior-frame state were
    /// shared on the renderer, window B's present would diff against window A's last
    /// frame: the scissor would Load A's pixels and repaint only the rows that
    /// differ between A and B, leaking A's content into B's readback. Per-window
    /// state means each window diffs only against its OWN prior frame. Gated: no
    /// GPU/font -> skip cleanly.
    #[test]
    fn present_prev_is_per_window_no_cross_window_leak() {
        let theme = Theme::default();
        let px = 18.0;
        let mut gpu = match GpuRenderer::new(px, theme) {
            Ok(g) => g,
            Err(e) => {
                eprintln!("SKIP: no GPU/font available: {e}");
                return;
            }
        };

        let (rows, cols) = (4usize, 10usize);
        // Two DISTINCT frame sequences per window. A prev-frame leak between the two
        // windows would only manifest when the two windows hold DIFFERENT content,
        // so make every interleaved frame differ from the other window's frame and
        // from this window's own prior frame (so the scissor path is exercised).
        let frame = |bytes: &[u8]| {
            let mut term = Terminal::new(rows as u16, cols as u16);
            term.process(b"\x1b[?25l");
            term.process(bytes);
            term.cell_frame(rows, cols)
        };
        let a_frames = [
            frame(b"AAAA"),
            frame(b"AAAA\r\nbbbb"),
            frame(b"AAAA\r\ncccc"),
        ];
        let b_frames = [
            frame(b"ZZZZ"),
            frame(b"ZZZZ\r\nyyyy"),
            frame(b"ZZZZ\r\nxxxx"),
        ];

        let mut win_a = WindowGpu::new();
        let mut win_b = WindowGpu::new();

        // Drive the two windows interleaved through the SCISSORED present path on
        // the ONE shared renderer: A0, B0, A1, B1, A2, B2.
        let mut last_a = None;
        let mut last_b = None;
        for i in 0..a_frames.len() {
            last_a = Some(gpu.present_input_readback(&mut win_a, &a_frames[i]));
            last_b = Some(gpu.present_input_readback(&mut win_b, &b_frames[i]));
        }
        let got_a = last_a.expect("at least one frame driven");
        let got_b = last_b.expect("at least one frame driven");

        // The SCISSOR path must actually have fired (else this proves nothing about
        // cross-window diffing): the second+ frame of each window is a stable-dims
        // change, which takes the scissor.
        assert!(
            gpu.scissor_taken() > 0,
            "scissor path must be exercised by the changed frames"
        );

        // Ground truth: a FRESH full render of each window's FINAL input on its own
        // clean window. `render_input` always clears + draws every row, so it is the
        // leak-free reference. Use fresh windows so no prior-frame state is consulted.
        let mut ref_win_a = WindowGpu::new();
        let mut ref_win_b = WindowGpu::new();
        let want_a = gpu.render_input(&mut ref_win_a, &a_frames[a_frames.len() - 1]);
        let want_b = gpu.render_input(&mut ref_win_b, &b_frames[b_frames.len() - 1]);

        assert_eq!(
            (got_a.width, got_a.height),
            (want_a.width, want_a.height),
            "window A readback dims must match the reference"
        );
        assert_eq!(
            got_a.pixels, want_a.pixels,
            "window A's interleaved scissored readback must be byte-identical to a fresh full \
             render — a mismatch means window B's prior frame leaked into window A"
        );
        assert_eq!(
            (got_b.width, got_b.height),
            (want_b.width, want_b.height),
            "window B readback dims must match the reference"
        );
        assert_eq!(
            got_b.pixels, want_b.pixels,
            "window B's interleaved scissored readback must be byte-identical to a fresh full \
             render — a mismatch means window A's prior frame leaked into window B"
        );
    }

    /// REGRESSION: the scissored dirty-row present path must fire at the GUI's REAL
    /// interior padding (`pad > 0`), not only at `pad == 0`. The scissor gate sizes
    /// `(w, h)` to compare against the offscreen, which `encode_frame` creates at
    /// PADDED dims (`cols*cw + 2*pad`); if the gate used the unpadded size the
    /// comparison would NEVER match when `pad > 0` and every present would silently
    /// fall back to a Full repaint, defeating the optimization in the windowed GUI.
    /// This drives the production present path at `pad = 14` and asserts (a) the
    /// scissor actually fires and (b) the scissored readback is byte-identical to a
    /// fresh full render — so the dead-scissor regression cannot return unnoticed.
    #[test]
    fn scissored_present_fires_and_is_correct_at_nonzero_pad() {
        let theme = Theme::default();
        let px = 18.0;
        let mut gpu = match GpuRenderer::new(px, theme) {
            Ok(g) => g,
            Err(e) => {
                eprintln!("SKIP: no GPU/font available: {e}");
                return;
            }
        };
        // The GUI runs at pad = pad_for_scale(scale) > 0; exercise that regime.
        gpu.set_pad(14);
        assert!(gpu.pad() > 0, "precondition: this test exercises pad > 0");

        let (rows, cols) = (4usize, 10usize);
        let frame = |bytes: &[u8]| {
            let mut term = Terminal::new(rows as u16, cols as u16);
            term.process(b"\x1b[?25l");
            term.process(bytes);
            term.cell_frame(rows, cols)
        };
        // Same dims, changing content: frame 2+ is a stable-dims change → scissor.
        let frames = [
            frame(b"AAAA"),
            frame(b"AAAA\r\nbbbb"),
            frame(b"AAAA\r\ncccc"),
        ];

        let mut win = WindowGpu::new();
        let mut last = None;
        for f in &frames {
            last = Some(gpu.present_input_readback(&mut win, f));
        }
        let got = last.expect("at least one frame driven");

        // The whole point: with padded gate dims the scissor MUST fire at pad>0.
        // Before the fix this was 0 (offscreen_holds_prev always false) and the
        // present silently full-repainted every frame.
        assert!(
            gpu.scissor_taken() > 0,
            "scissor must fire at pad>0 (got scissor_taken={}, full_repaints={})",
            gpu.scissor_taken(),
            gpu.full_repaints(),
        );

        // Correctness: the scissored band must be byte-identical to a fresh full
        // render of the same final input on a clean window (the leak-free reference).
        let mut ref_win = WindowGpu::new();
        let want = gpu.render_input(&mut ref_win, &frames[frames.len() - 1]);
        assert_eq!(
            (got.width, got.height),
            (want.width, want.height),
            "padded scissored readback dims must match the full-render reference"
        );
        assert_eq!(
            got.pixels, want.pixels,
            "padded scissored readback must be byte-identical to a fresh full render"
        );
    }
}

// The GPU renderer as the injected `Rasterizer` (ATERM_DESIGN WS-F): the same
// trait `aterm_render::Renderer` implements, so a frontend can hold
// `Box<dyn Rasterizer>` and choose CPU vs GPU at runtime. Forwards to the inherent
// methods via UFCS to avoid the trait/inherent name clash. The trait is
// `&Terminal`-free (A-3): the renderer consumes only the engine-built `RenderInput`.
impl Rasterizer for GpuRenderer {
    fn cell_size(&self) -> (usize, usize) {
        GpuRenderer::cell_size(self)
    }
    fn render_input(&mut self, input: &RenderInput) -> Frame {
        // The inherent path threads per-window GPU state (offscreen / caches) on a
        // `WindowGpu`; the trait is `&Terminal`-/window-free, so the DI seam owns a
        // throwaway one for this call. A full-repaint readback (`render_input`
        // always clears + draws every row), so a fresh `WindowGpu` only forgoes
        // offscreen REUSE — the pixels are byte-identical. Frontends use the
        // inherent `GpuRenderer::render_input(win, ..)` (a persistent `WindowGpu`)
        // for the hot path; this object-safe forward exists for the `dyn Rasterizer`
        // seam only.
        let mut win = WindowGpu::new();
        GpuRenderer::render_input(self, &mut win, input)
    }
    // `render_input_cached` is intentionally NOT overridden: the inherent version
    // returns a `RenderView` borrowing the per-window `gate_cache`, which can't
    // outlive a local `WindowGpu`. The trait's default (`RenderView::Owned(self.
    // render_input(input))`) is byte-identical and object-safe — the GPU hot path
    // calls the inherent `render_input_cached(win, ..)` directly, not via the trait.
    fn set_cursor_blink_phase(&mut self, on: bool) {
        GpuRenderer::set_cursor_blink_phase(self, on)
    }
    fn set_cursor_style_override(&mut self, style: Option<CursorStyle>) {
        GpuRenderer::set_cursor_style_override(self, style)
    }
}

#[cfg(test)]
mod rasterizer_di_tests {
    use super::*;

    // Locks the WS-F injected-rasterizer abstraction: both the CPU and GPU
    // renderers must satisfy `Rasterizer`, so a frontend can hold either behind
    // one trait. Compile-time only — no GPU/font needed.
    #[test]
    fn both_renderers_implement_rasterizer() {
        fn assert_rasterizer<R: Rasterizer>() {}
        assert_rasterizer::<Renderer>();
        assert_rasterizer::<GpuRenderer>();
        // And the trait is object-safe (dyn dispatch = the DI the design wants).
        fn _takes_dyn(_: &mut dyn Rasterizer) {}
    }
}
