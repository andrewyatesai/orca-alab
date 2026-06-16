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

use aterm_core::terminal::{CursorStyle, Terminal, UnderlineStyle};
use aterm_render::{
    compute_dirty_rows, is_unchanged_frame, DirtyDecision, Frame, GlyphImage, GlyphKey,
    RenderInput, RenderView, Rasterizer, Renderer, Theme,
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
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct BgInstance {
    /// x, y, w, h in pixels (top-left origin, y down).
    rect: [f32; 4],
    /// r, g, b, a normalized (a == 1.0).
    color: [f32; 4],
}

/// One glyph quad: a pixel-space dest rect, an atlas UV rect, and a fg colour.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct GlyphInstance {
    /// dest x, y, w, h in pixels.
    rect: [f32; 4],
    /// atlas u0, v0, du, dv in [0, 1].
    uv: [f32; 4],
    /// fg r, g, b, a normalized (a unused; coverage supplies alpha).
    color: [f32; 4],
}

const BG_ATTRS: [wgpu::VertexAttribute; 2] =
    wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4];
const GLYPH_ATTRS: [wgpu::VertexAttribute; 3] =
    wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4];

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
         @location(0) rect: vec4<f32>,
         @location(1) color: vec4<f32>) -> BgVsOut {
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
                    GlyphSlot { ax: 0, ay: 0, gw: 0, gh: 0, xmin: img.xmin(), ymin: img.ymin() },
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
        self.map
            .insert(key, GlyphSlot { ax, ay, gw, gh, xmin: img.xmin(), ymin: img.ymin() });
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
        for j in 0..slot.gh {
            for i in 0..slot.gw {
                let src = ((j * slot.gw + i) * bpp) as usize;
                let dst = (((slot.ay + j) * self.width + (slot.ax + i)) * bpp) as usize;
                self.data[dst..dst + bpp as usize]
                    .copy_from_slice(&bytes[src..src + bpp as usize]);
            }
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
fn build_atlas(cpu: &mut Renderer, keys: &BTreeSet<GlyphKey>) -> Atlas {
    build_kind(cpu, keys, AtlasKind::Mono)
}

/// Pack every colour-emoji (`GlyphImage::Rgba`) glyph into one fresh RGBA8 atlas
/// (shelf packer, 1px padding), pulling the EXACT cached pixels from the CPU
/// renderer. The CPU already scaled each emoji to its final on-cell size, so the
/// GPU blits these 1:1 with NEAREST sampling — exact bytes, like the mono path.
/// Mono and empty glyphs are skipped here (they live in the R8 atlas).
fn build_color_atlas(cpu: &mut Renderer, keys: &BTreeSet<GlyphKey>) -> Atlas {
    build_kind(cpu, keys, AtlasKind::Color)
}

/// Shared full-pack for either [`AtlasKind`]: place every key, then blit its
/// bytes. `data` is sized to the occupied height once packing is known, so it
/// holds exactly the packed shelves (no slack) — byte-identical to the old
/// per-kind packers.
fn build_kind(cpu: &mut Renderer, keys: &BTreeSet<GlyphKey>, kind: AtlasKind) -> Atlas {
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
        if atlas.place(key, img).is_some() {
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

/// GPU terminal renderer. Holds its own GPU device (via [`GpuContext`]) and a CPU
/// [`Renderer`] used purely for font metrics + glyph coverage, so geometry and
/// rasterization match the CPU renderer exactly.
pub struct GpuRenderer {
    ctx: GpuContext,
    cpu: Renderer,
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
    /// Blit pipelines keyed by swapchain format (built lazily in `present_input` —
    /// the surface's chosen format isn't known until a window exists).
    blit_pipelines: HashMap<wgpu::TextureFormat, wgpu::RenderPipeline>,
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
    // The resident offscreen render target + its blit-source bind group. `None`
    // until the first frame; reused at the same `(w, h)`, recreated only on a
    // dimension change. See `Offscreen`.
    offscreen: Option<Offscreen>,
    // Last `(w, h)` written into the screen uniform. `Uniforms.screen` is a pure
    // function of the frame size, so it only needs (re)writing on the first frame
    // and on a resize — NOT every frame. `None` forces the first write.
    uniform_dims: Option<(u32, u32)>,
    // Last invert flag written into the blit uniform. The blit uniform is a pure
    // function of `invert`, so it is rewritten only when the flag changes. `None`
    // forces the first write.
    blit_invert: Option<bool>,
    // DIRTY-GATE cache for the per-frame PRESENTATION hot path
    // (`render_input_cached`). Holds the previous frame's input + cursor state +
    // the pixels that were read back for it. When the next frame is PIXEL-
    // IDENTICAL (per `is_unchanged_frame`), we re-present these cached pixels and
    // do ZERO GPU work — no encode, no submit, no `device.poll`, no readback.
    gate_cache: Option<GpuGateCache>,
    // TEST/DIAGNOSTIC counters so a test can prove the gate is actually taken
    // (otherwise a "gate" that never fires would still pass a byte-identity
    // test). Counts gate-hits and gate-misses through `render_input_cached`.
    gate_hits: u64,
    gate_misses: u64,
    // SCISSORED DIRTY-ROW REPAINT (the window present path). Holds the PREVIOUS
    // presented frame's input + the renderer cursor state it was drawn with, so
    // `present_input` can consult `compute_dirty_rows` against it and re-encode
    // only the dirty rows (LoadOp::Load + a scissor over the dirty band) into the
    // persistent offscreen — which still holds that prior frame. `None` until the
    // first present (forces a full repaint), and reset on any geometry change
    // (the offscreen is recreated, so its prior contents are gone). See
    // `encode_frame` / `RepaintScope`.
    present_prev: Option<PresentPrev>,
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
}

/// The previous presented frame's state, for the scissored dirty-row repaint.
/// The persistent offscreen still holds this frame's pixels, so the next present
/// can update only the rows that differ from it.
struct PresentPrev {
    /// The previous presented frame's input snapshot (cloned), for
    /// `compute_dirty_rows`.
    input: RenderInput,
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
enum RepaintScope {
    Full,
    Dirty(Vec<bool>),
}

/// The GPU dirty-gate cache: the previous frame's `render_input_cached` inputs
/// and the pixels that were rendered + read back for them. Because the GPU has
/// no persistent CPU-side framebuffer to borrow (it renders on-device and reads
/// back), the gate must remember the prior frame's pixels itself so it can re-
/// present them on an unchanged frame.
struct GpuGateCache {
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
struct Offscreen {
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
        Self { buf: Self::alloc(device, label, 0), capacity: 0, label }
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
        if bytes.is_empty() {
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

/// Round `n` up to the next multiple of `align` (a power of two).
fn align_up(n: u64, align: u64) -> u64 {
    (n + align - 1) & !(align - 1)
}

impl GpuRenderer {
    /// Acquire a GPU and a CPU font face. `px`/`theme` must match the CPU
    /// renderer you want to reproduce.
    pub fn new(px: f32, theme: Theme) -> Result<Self, String> {
        let ctx = GpuContext::new()?;
        let cpu = Renderer::from_system(px, theme).ok_or("no system monospace font")?;
        let device = &ctx.device;

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("aterm-gpu shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });

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

        let bg_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("aterm-gpu bg layout"),
            bind_group_layouts: &[Some(&uniform_bgl)],
            immediate_size: 0,
        });
        let glyph_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("aterm-gpu glyph layout"),
            bind_group_layouts: &[Some(&uniform_bgl), Some(&atlas_bgl)],
            immediate_size: 0,
        });

        let target = wgpu::TextureFormat::Rgba8Unorm;

        let bg_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("aterm-gpu bg pipeline"),
            layout: Some(&bg_layout),
            vertex: wgpu::VertexState {
                module: &shader,
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
                module: &shader,
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
                module: &shader,
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
                module: &shader,
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
                module: &shader,
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
                module: &shader,
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

        // On-glass blit infrastructure (format-independent parts). The pipeline
        // itself depends on the swapchain format, so it is built lazily per
        // surface format in `present_input` and cached in `blit_pipelines`.
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

        let vbufs = VertexBuffers::new(device);

        Ok(Self {
            ctx,
            cpu,
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
            mono_res: None,
            color_res: None,
            resident_keys: BTreeSet::new(),
            atlas_tex_creations: 0,
            vbufs,
            inst: Instances::default(),
            offscreen: None,
            uniform_dims: None,
            blit_invert: None,
            gate_cache: None,
            gate_hits: 0,
            gate_misses: 0,
            present_prev: None,
            scissor_taken: 0,
            full_repaints: 0,
            last_instances: 0,
        })
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
        Some(((m.tex.width(), m.tex.height()), (c.tex.width(), c.tex.height())))
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
    fn cell_key(&mut self, cluster: Option<&str>, cell: &aterm_core::terminal::RenderCell) -> GlyphKey {
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
            AtlasKind::Mono => (wgpu::TextureFormat::R8Unorm, "aterm-gpu atlas", "aterm-gpu atlas bg"),
            AtlasKind::Color => {
                (wgpu::TextureFormat::Rgba8Unorm, "aterm-gpu colour atlas", "aterm-gpu colour atlas bg")
            }
        };
        // Allocate the texture TALLER than the packed data (headroom) so later
        // glyphs append via sub-region upload instead of recreating the texture.
        let tex_h = atlas.height + ATLAS_GROW_HEADROOM;
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width: atlas.width, height: tex_h, depth_or_array_layers: 1 },
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
            wgpu::Extent3d { width: atlas.width, height: atlas.height, depth_or_array_layers: 1 },
        );
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(bg_label),
            layout: &self.atlas_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        });
        ResidentAtlas { atlas, tex, bind, tex_h }
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
            wgpu::Extent3d { width: res.atlas.width, height: y1 - y0, depth_or_array_layers: 1 },
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
        let new_keys: Vec<GlyphKey> =
            keys.iter().copied().filter(|k| !self.resident_keys.contains(k)).collect();

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
        let mono = build_atlas(&mut self.cpu, keys);
        self.mono_res = Some(self.create_atlas_texture(mono));
        let color = build_color_atlas(&mut self.cpu, keys);
        self.color_res = Some(self.create_atlas_texture(color));
        self.resident_keys = keys.clone();
    }

    /// Snapshot everything the GPU renderer reads from `term` for a frame — the
    /// same [`RenderInput`] the CPU path uses, so the windowed frontend can hold
    /// the `Terminal` lock only for the extract and then encode/read back without
    /// it. Re-exported from the CPU [`Renderer`] so both paths capture identically.
    pub fn extract(term: &Terminal, rows: usize, cols: usize) -> RenderInput {
        Renderer::extract(term, rows, cols)
    }

    /// Refill an existing `scratch` [`RenderInput`] in place (C-1), reusing its
    /// row Vecs across frames. Re-exported from the CPU [`Renderer`] so the GPU
    /// present path captures identically and the frontend keeps one persistent
    /// snapshot. Argument order mirrors [`Renderer::extract_into`].
    pub fn extract_into(scratch: &mut RenderInput, term: &Terminal, rows: usize, cols: usize) {
        Renderer::extract_into(scratch, term, rows, cols);
    }

    /// Render the terminal's `rows`x`cols` grid on the GPU and read the pixels
    /// back into a [`Frame`] — same dimensions and (within rounding) same pixels
    /// as `aterm_render::Renderer::render`. (Offscreen + synchronous readback.)
    pub fn render(&mut self, term: &Terminal, rows: usize, cols: usize) -> Frame {
        let input = Self::extract(term, rows, cols);
        self.render_input(&input)
    }

    /// Render a previously [`extract`](Self::extract)ed [`RenderInput`] on the
    /// GPU and read it back — identical pixels to [`render`](Self::render) but
    /// with no `&Terminal` borrow, so the frontend renders after dropping the
    /// lock.
    pub fn render_input(&mut self, input: &RenderInput) -> Frame {
        // FULL repaint (Clear + all rows) — the snapshot / readback / oracle path.
        // It overwrites the offscreen with this (possibly unrelated) input, so it
        // invalidates the scissored present sequence's prior-frame tracking: a
        // subsequent `present_input` must NOT diff against a frame it never drew.
        self.present_prev = None;
        let (w, h) = self.encode_frame(input, &RepaintScope::Full);
        // The freshly rendered target is resident on `self.offscreen`.
        let tex = self.offscreen.as_ref().expect("encode_frame sets offscreen").tex.clone();
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
        &self,
        target: impl Into<wgpu::SurfaceTarget<'static>>,
        width: u32,
        height: u32,
    ) -> Result<GpuSurface, String> {
        let surface = self
            .ctx
            .instance
            .create_surface(target)
            .map_err(|e| format!("create_surface failed: {e}"))?;
        let caps = surface.get_capabilities(&self.ctx.adapter);
        let format = Self::pick_surface_format(&caps)?;
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: width.max(1),
            height: height.max(1),
            // AutoVsync == Fifo: tear-free, present-on-vblank, always supported.
            present_mode: wgpu::PresentMode::AutoVsync,
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
    fn pick_surface_format(caps: &wgpu::SurfaceCapabilities) -> Result<wgpu::TextureFormat, String> {
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

    /// Render the frame offscreen (the single source of truth) and PRESENT it on
    /// the GPU by blitting that texture into `surf`'s swapchain — no CPU readback,
    /// no softbuffer copy. `invert` flips RGB for the visual-bell flash.
    pub fn present_input(&mut self, surf: &mut GpuSurface, input: &RenderInput, invert: bool) {
        // 1. Offscreen render (submits). SCISSORED DIRTY-ROW REPAINT: when the
        //    persistent offscreen still holds the prior presented frame and only
        //    some rows differ, re-encode ONLY those rows (LoadOp::Load + a scissor
        //    over the dirty band) — proportional to the change, not the screen.
        //    Otherwise a full Clear+all-rows repaint (the always-correct path).
        //    The rendered target + its blit-source bind group are resident on
        //    `self.offscreen` (built once, reused across presents at the same
        //    dimensions; rebuilt only on a resize), so this present allocates no
        //    per-frame texture / view / blit bind group.
        self.encode_present_frame(input);

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
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());

        // 3. Ensure a blit pipeline for this swapchain format exists.
        let format = surf.config.format;
        self.ensure_blit_pipeline(format);
        let pipeline = &self.blit_pipelines[&format];

        // 4. Write the invert flag ONLY when it changes (the blit uniform is a
        //    pure function of `invert`; the steady-state present otherwise
        //    re-uploaded an unchanged 4-byte buffer each frame). The offscreen
        //    texture is already bound as the blit source by the resident
        //    `blit_bind` (built in `encode_frame` when the target was created).
        if self.blit_invert != Some(invert) {
            self.ctx.queue.write_buffer(
                &self.blit_uniform_buf,
                0,
                bytemuck::bytes_of(&BlitUniform { flag: invert as u32, _pad: [0; 3] }),
            );
            self.blit_invert = Some(invert);
        }
        let bind = &self.offscreen.as_ref().expect("encode_frame sets offscreen").blit_bind;

        // 5. One blit pass: fullscreen triangle covers every pixel, so Clear is
        //    just the (overwritten) initial value.
        let mut enc = self
            .ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("aterm-gpu blit") });
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
    fn encode_present_frame(&mut self, input: &RenderInput) -> (u32, u32) {
        let cur_blink = self.cpu.cursor_blink_phase();
        let cur_override = self.cpu.cursor_style_override();
        let (cw, ch) = self.cpu.cell_size();
        let (w, h) = ((input.cols * cw) as u32, (input.rows * ch) as u32);

        // The offscreen must already hold the previous frame at THESE dims for a
        // scissored Load to be safe. A dimension change recreates the texture
        // (its prior contents are gone), so any dims mismatch forces Full.
        let offscreen_holds_prev = matches!(&self.offscreen, Some(o) if o.w == w && o.h == h);

        let scope = match (&self.present_prev, offscreen_holds_prev) {
            (Some(prev), true) => match compute_dirty_rows(
                &prev.input,
                input,
                prev.blink_phase,
                prev.cursor_style_override,
                cur_blink,
                cur_override,
            ) {
                // Reusable: scissor the dirty band. The zero-dirty-row case (a
                // gate-class idle frame) is handled correctly downstream — Load
                // preserves the prior frame and the empty dirty set draws nothing,
                // the cheapest possible encode.
                DirtyDecision::Rows(d) => RepaintScope::Dirty(d.dirty),
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
        let dims = self.encode_frame(input, &scope);

        // This frame is now resident on the offscreen; remember it (+ the state it
        // was drawn with) so the NEXT present can diff against it.
        self.present_prev = Some(PresentPrev {
            input: input.clone(),
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
    pub fn present_input_readback(&mut self, input: &RenderInput) -> Frame {
        let (w, h) = self.encode_present_frame(input);
        let tex = self.offscreen.as_ref().expect("encode_frame sets offscreen").tex.clone();
        self.ctx.read_back(&tex, w, h)
    }

    /// TEST/BENCH HELPER: run the SCISSORED present-path encode for `input` and
    /// BLOCK until the GPU finishes, but do NOT read the pixels back. Isolates the
    /// changed-frame ENCODE + instance-build + GPU fill cost (the readback, which
    /// is identical for any scope, would otherwise swamp the scissor's saving).
    #[doc(hidden)]
    pub fn present_encode_poll(&mut self, input: &RenderInput) {
        let _ = self.encode_present_frame(input);
        self.ctx.device.poll(wgpu::PollType::wait_indefinitely()).expect("GPU poll failed");
    }

    /// Build + cache the blit render pipeline for a swapchain `format` if absent.
    fn ensure_blit_pipeline(&mut self, format: wgpu::TextureFormat) {
        if self.blit_pipelines.contains_key(&format) {
            return;
        }
        let pipeline =
            self.ctx.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
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
    pub fn render_input_cached(&mut self, input: &RenderInput) -> RenderView<'_> {
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
        let hit = match &self.gate_cache {
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
            let frame = &self.gate_cache.as_ref().expect("hit implies Some").frame;
            return RenderView::Borrowed {
                width: frame.width,
                height: frame.height,
                pixels: &frame.pixels,
            };
        }

        // MISS: full GPU render + readback, then refresh the gate cache to THIS
        // frame's pixels + state so the next unchanged frame can take the gate.
        self.gate_misses += 1;
        let frame = self.render_input(input);
        self.gate_cache = Some(GpuGateCache {
            input: input.clone(),
            blink_phase: cur_blink,
            cursor_style_override: cur_override,
            frame,
        });
        let frame = &self.gate_cache.as_ref().expect("just stored").frame;
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
    pub fn render_no_readback(&mut self, term: &Terminal, rows: usize, cols: usize) {
        let input = Self::extract(term, rows, cols);
        self.present_prev = None;
        let _ = self.encode_frame(&input, &RepaintScope::Full);
        self.ctx.device.poll(wgpu::PollType::wait_indefinitely()).expect("GPU poll failed");
    }

    /// Build the atlas + instances, encode the single render pass onto the
    /// RESIDENT offscreen target (`self.offscreen`, reused at the same `(w, h)`
    /// and rebuilt only on a resize), and submit. Returns the frame's `(w, h)`;
    /// the rendered texture (+ its blit-source bind group) live on
    /// `self.offscreen` for the caller to read back or present.
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
    fn encode_frame(&mut self, input: &RenderInput, scope: &RepaintScope) -> (u32, u32) {
        let (rows, cols) = (input.rows, input.cols);
        let (cw, ch) = self.cpu.cell_size();
        let baseline = self.cpu.baseline();
        let w = (cols * cw) as u32;
        let h = (rows * ch) as u32;

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
        let style = self.cpu.cursor_style_override().unwrap_or(input.cursor_style);
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
        let mut keys = std::mem::take(&mut self.inst.keys);
        for (r, cells) in rendered.iter().enumerate() {
            for (c, cell) in cells.iter().take(cols).enumerate() {
                if Self::drawable(cell) {
                    let key = self.cell_key(input.cluster_at(r, c), cell);
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
        let mono_res = self.mono_res.as_ref().expect("ensure_atlases sets mono_res");
        let color_res = self.color_res.as_ref().expect("ensure_atlases sets color_res");
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
        for (r, cells) in rendered.iter().enumerate() {
            if !row_active(r) {
                continue;
            }
            let y0 = (r * ch) as f32;
            let sel_row = r as i32 - display_offset;
            // DEC line size (DECDWL/DECDHL): the cell advance, glyph NEAREST
            // enlargement and dest-row clip come from the SAME helpers the CPU
            // blit uses, so the quads reproduce it exactly.
            let line_size = input.line_sizes[r];
            let rcw = aterm_render::row_cell_w(line_size, cw);
            let (scale, anchor_y) = aterm_render::row_scale(line_size, r * ch, ch);
            // SCISSORED PATH ONLY: a FULL-ROW-WIDTH theme-bg quad FIRST, so the
            // band is fully re-established from background even if the per-cell
            // fills below leave any sliver (degenerate cols). bg is REPLACE and the
            // per-cell quads fully tile [0, w) (single: cols·cw == w; double-width:
            // cols·2cw ⊇ w), so this quad is entirely overwritten — byte-identical
            // to the FULL path's `LoadOp::Clear(theme.bg)` for this band, with no
            // seam and no stale contamination. (FULL path keeps the pass Clear, so
            // it does NOT emit this — its whole-target clear already covers it.)
            if matches!(scope, RepaintScope::Dirty(_)) {
                bg_inst.push(BgInstance { rect: [0.0, y0, w as f32, ch as f32], color: theme_bg });
            }
            for (c, cell) in cells.iter().take(cols).enumerate() {
                let x0 = (c * rcw) as f32;
                // A lead cell is wide iff the NEXT cell is its continuation.
                let is_wide_lead = cells.get(c + 1).is_some_and(|n| n.wide);
                let color = if selection.contains_cell(sel_row, c as u16, is_wide_lead, cell.wide) {
                    rgb4_u32(self.theme.selection)
                } else {
                    rgb4(cell.bg)
                };
                bg_inst.push(BgInstance { rect: [x0, y0, rcw as f32, ch as f32], color });
            }
            for (c, cell) in cells.iter().take(cols).enumerate() {
                if !Self::drawable(cell) {
                    continue;
                }
                // Cached resolve (the atlas pass above already keyed this char).
                // Direct `self.cpu` field access (not the `cell_key` method) so
                // the `mono_res`/`color_res` borrows above stay disjoint.
                let key = self.cpu.resolve_cell_key(input.cluster_at(r, c), cell);
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
                        (c * rcw) as f32, anchor_y, baseline, scale,
                        slot.ax, slot.ay, slot.gw, slot.gh, slot.xmin, slot.ymin, caw, cah,
                    ) else {
                        continue;
                    };
                    let inst = GlyphInstance { rect, uv, color: [0.0, 0.0, 0.0, 0.0] };
                    if is_cursor { cursor_color_inst.push(inst) } else { color_inst.push(inst) };
                    continue;
                }
                let Some(slot) = atlas.map.get(&key) else { continue };
                if slot.gw == 0 || slot.gh == 0 {
                    continue;
                }
                // Under the block cursor the glyph is "cut out" in the cell bg
                // colour and drawn AFTER the cursor fill; otherwise normal fg.
                let color = if is_cursor { cell.bg } else { cell.fg };
                let Some((rect, uv)) = aterm_render::glyph_quad(
                    (c * rcw) as f32, anchor_y, baseline, scale,
                    slot.ax, slot.ay, slot.gw, slot.gh, slot.xmin, slot.ymin, aw, ah,
                ) else {
                    continue;
                };
                let inst = GlyphInstance { rect, uv, color: rgb4(color) };
                if is_cursor { cursor_glyph_inst.push(inst) } else { glyph_inst.push(inst) };
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
            let (scale, anchor_y) = aterm_render::row_scale(line_size, r * ch, ch);
            for (c, cell) in cells.iter().take(cols).enumerate() {
                if cell.wide || input.cluster_at(r, c).is_some() {
                    continue;
                }
                let Some(marks) = input.combining_at(r, c) else { continue };
                for &m in marks {
                    let key = self.cpu.glyph_key(m);
                    let Some(slot) = atlas.map.get(&key) else { continue };
                    if slot.gw == 0 || slot.gh == 0 {
                        continue;
                    }
                    // Centre the mark's ink in the cell (see CPU `mark_cell_x`):
                    // identical integer arithmetic → identical pixel position.
                    let cx = aterm_render::mark_cell_x(c, rcw, slot.gw as usize, slot.xmin, scale);
                    let Some((rect, uv)) = aterm_render::glyph_quad(
                        cx as f32, anchor_y, baseline, scale,
                        slot.ax, slot.ay, slot.gw, slot.gh, slot.xmin, slot.ymin, aw, ah,
                    ) else {
                        continue;
                    };
                    glyph_inst.push(GlyphInstance { rect, uv, color: rgb4(cell.fg) });
                }
            }
        }
        // Cursor-row cell width (doubled on any DEC double-size line).
        let cur_cw = if cr < rows { aterm_render::row_cell_w(input.line_sizes[cr], cw) } else { cw };
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
                rect: [(cc * cur_cw) as f32, (cr * ch) as f32, cur_cw as f32, ch as f32],
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
            let y0 = r * ch;
            let rcw = aterm_render::row_cell_w(input.line_sizes[r], cw);
            for (c, cell) in cells.iter().take(cols).enumerate() {
                if cell.wide
                    || (matches!(cell.underline, UnderlineStyle::None)
                        && !cell.strikethrough
                        && !cell.overline)
                {
                    continue;
                }
                let x = c * rcw;
                let dw = if cells.get(c + 1).is_some_and(|n| n.wide) { 2 * rcw } else { rcw };
                let ucolor = rgb4(cell.underline_color.unwrap_or(cell.fg));
                for [rx, ry, rw, rh] in
                    aterm_render::underline_rects(cell.underline, x, y0, dw, ch, baseline)
                {
                    deco_inst.push(BgInstance {
                        rect: [rx as f32, ry as f32, rw as f32, rh as f32],
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
                        rect: [rx as f32, ry as f32, rw as f32, rh as f32],
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
                aterm_render::cursor_rects(style, cc * cur_cw, cr * ch, cur_cw, ch).into_iter().map(
                    |[x, y, rw, rh]| BgInstance {
                        rect: [x as f32, y as f32, rw as f32, rh as f32],
                        color: cursor_color,
                    },
                ),
            );
        }

        // Upload uniforms. `Uniforms.screen` is a pure function of the frame
        // size, so it is rewritten ONLY on the first frame and on a resize — not
        // every frame (the steady-state present otherwise re-uploaded an unchanged
        // 16-byte buffer each frame). The bytes are byte-identical to the old
        // every-frame write for the same `(w, h)`.
        if self.uniform_dims != Some((w, h)) {
            self.ctx.queue.write_buffer(
                &self.uniform_buf,
                0,
                bytemuck::bytes_of(&Uniforms { screen: [w as f32, h as f32], _pad: [0.0, 0.0] }),
            );
            self.uniform_dims = Some((w, h));
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
        let bg_buf = self.vbufs.bg.upload(device, queue, bytemuck::cast_slice(&self.inst.bg));
        let glyph_buf = self.vbufs.glyph.upload(device, queue, bytemuck::cast_slice(&self.inst.glyph));
        let color_buf = self.vbufs.color.upload(device, queue, bytemuck::cast_slice(&self.inst.color));
        let cursor_buf = self.vbufs.cursor.upload(device, queue, bytemuck::cast_slice(&self.inst.cursor));
        let deco_buf = self.vbufs.deco.upload(device, queue, bytemuck::cast_slice(&self.inst.deco));
        let cursor_block_buf =
            self.vbufs.cursor_block.upload(device, queue, bytemuck::cast_slice(&self.inst.cursor_block));
        let cursor_glyph_buf =
            self.vbufs.cursor_glyph.upload(device, queue, bytemuck::cast_slice(&self.inst.cursor_glyph));
        let cursor_color_buf =
            self.vbufs.cursor_color.upload(device, queue, bytemuck::cast_slice(&self.inst.cursor_color));

        // Resident offscreen target: reuse the texture + view when `(w, h)` is
        // unchanged; (re)create them only on the first frame or a resize. On a
        // (re)create the previous target (and the blit-source bind group built
        // from it in `present_input`) is replaced — including the per-format blit
        // PIPELINES staying valid (they key on swapchain format, not this view),
        // so no stale resource survives a dimension change. Usage is unchanged
        // (`RENDER_ATTACHMENT | COPY_SRC | TEXTURE_BINDING`).
        let recreate = match &self.offscreen {
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
            let blit_bind = self.ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
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
            self.offscreen = Some(Offscreen { tex, view, blit_bind, w, h });
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
                        let y0 = (f * ch) as u32;
                        let y1 = (((last + 1) * ch) as u32).min(h);
                        (wgpu::LoadOp::Load, Some((0u32, y0, w, y1.saturating_sub(y0))))
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

        let view = &self.offscreen.as_ref().expect("offscreen set above").view;
        let mut enc = self
            .ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("aterm-gpu frame") });

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
            draw_stream!(bg_buf, self.inst.bg, Pipe::Bg, &self.bg_pipeline, None::<(Atlas, &wgpu::BindGroup)>);
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
            // deco: line decorations (underline/strike/overline), opaque, over
            // the glyphs and before the cursor.
            draw_stream!(deco_buf, self.inst.deco, Pipe::Bg, &self.bg_pipeline, None::<(Atlas, &wgpu::BindGroup)>);
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
            + self.inst.glyph.len()
            + self.inst.color.len()
            + self.inst.cursor.len()
            + self.inst.deco.len()
            + self.inst.cursor_block.len()
            + self.inst.cursor_glyph.len()
            + self.inst.cursor_color.len();
        // The rendered target lives on `self.offscreen` (resident across frames);
        // callers read it from there (`render_input` for readback, `present_input`
        // for the blit source).
        (w, h)
    }
}

/// `[r, g, b]` (0..=255) -> normalized opaque RGBA.
fn rgb4([r, g, b]: [u8; 3]) -> [f32; 4] {
    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0]
}

/// `0x00RRGGBB` -> normalized opaque RGBA.
fn rgb4_u32(c: u32) -> [f32; 4] {
    rgb4([(c >> 16) as u8, (c >> 8) as u8, c as u8])
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
    use aterm_render::Theme;

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
        let input = Renderer::extract(&term, rows, cols);
        let mut keys: BTreeSet<GlyphKey> = BTreeSet::new();
        for cells in &input.cells {
            for cell in cells.iter().take(cols) {
                if GpuRenderer::drawable(cell) {
                    keys.insert(cpu.glyph_key(cell.ch));
                }
            }
        }
        assert!(keys.len() >= 5, "demo frame should contribute several glyphs");

        let atlas = build_atlas(&mut cpu, &keys);
        for &key in &keys {
            let slot = atlas.map.get(&key).expect("every requested glyph gets a slot");
            let img = cpu.glyph_image(key).clone();
            let GlyphImage::Mono { width, height, bytes, .. } = &img else {
                panic!("char keys rasterize as Mono");
            };
            if *width == 0 || *height == 0 {
                assert_eq!((slot.gw, slot.gh), (0, 0), "empty glyph must pack as an empty slot");
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
                    let atlas_byte = atlas.data[((slot.ay + j) * atlas.width + slot.ax + i) as usize];
                    let cpu_byte = bytes[(j * slot.gw + i) as usize];
                    assert_eq!(
                        atlas_byte, cpu_byte,
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
        let input = GpuRenderer::extract(&term, rows, cols);

        // Frame 1: cold cache -> the atlases are built (both textures created).
        let _ = gpu.render_input(&input);
        let after_first = gpu.atlas_tex_creations();
        let dims_first = gpu.atlas_tex_dims().expect("atlases resident after first frame");
        assert!(after_first >= 1, "first frame should have created at least one atlas texture");

        // Frame 2: identical glyph set -> reuse, NO new texture creation.
        let _ = gpu.render_input(&input);
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
        let _ = gpu.render_input(&input);
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

        let render = |gpu: &mut GpuRenderer, bytes: &[u8]| {
            let mut term = Terminal::new(rows as u16, cols as u16);
            term.process(bytes);
            let input = GpuRenderer::extract(&term, rows, cols);
            gpu.render_input(&input);
        };

        // Cold frame with a few glyphs.
        render(&mut gpu, b"\x1b[?25labc");
        let creations_after_cold = gpu.atlas_tex_creations();
        let dims_after_cold = gpu.atlas_tex_dims().expect("atlases resident");

        // A frame adding NEW glyphs (xyz) on top of the resident set: the mono
        // atlas grows in place. The R8 atlas is 1024 wide with vast vertical
        // headroom, so three more small glyphs append without overflow — no new
        // texture, same dims.
        render(&mut gpu, b"\x1b[?25labcxyz");
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
}

// The GPU renderer as the injected `Rasterizer` (ATERM_DESIGN WS-F): the same
// trait `aterm_render::Renderer` implements, so a frontend can hold
// `Box<dyn Rasterizer>` and choose CPU vs GPU at runtime. Forwards to the inherent
// methods via UFCS to avoid the trait/inherent name clash.
impl Rasterizer for GpuRenderer {
    fn cell_size(&self) -> (usize, usize) {
        GpuRenderer::cell_size(self)
    }
    fn render(&mut self, term: &Terminal, rows: usize, cols: usize) -> Frame {
        GpuRenderer::render(self, term, rows, cols)
    }
    fn render_input(&mut self, input: &RenderInput) -> Frame {
        GpuRenderer::render_input(self, input)
    }
    fn render_input_cached(&mut self, input: &RenderInput) -> RenderView<'_> {
        GpuRenderer::render_input_cached(self, input)
    }
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
