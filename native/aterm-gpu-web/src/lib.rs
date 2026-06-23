// SPDX-License-Identifier: MIT
//
// `aterm-gpu-web` — the GPU rendering substrate for the Electron renderer.
//
// Sibling of `native/aterm-wasm`: that crate parses PTY bytes with the aterm
// engine (`aterm-core`) and rasterizes the grid on the CPU (`aterm-render`),
// then JS `putImageData`s the RGBA frame onto a `<canvas>`. THIS crate keeps the
// same engine front-end but renders on the GPU via `aterm-gpu` (wgpu's WebGL2
// backend — orca's deliberate terminal-acceleration target; production refuses
// unsafe-WebGPU), drawing straight into a `<canvas>` WebGL2 surface — no CPU
// readback, no `putImageData`, on the primary present path.
//
// The init path is ASYNC: a browser cannot block the main thread, so adapter +
// device acquisition is `await`ed (`wasm_bindgen_futures`), NOT `pollster::
// block_on` (the native aterm-gpu path). The surface is created from the
// `HtmlCanvasElement` via wgpu's `SurfaceTarget::Canvas`. The async core
// (`GpuContext::from_instance`) and the canvas surface path are backend-agnostic,
// so the WebGL backend reuses them unchanged.
//
// SCOPE (this file): a COMPILING wasm32 GPU pipeline + a real WebGL2-from-canvas
// init that configures the swapchain, plus a `render` that draws the ACTUAL
// terminal grid — aterm-gpu's instanced-cell-quad encode (glyph atlas + bg/glyph/
// cursor quads rendered offscreen, then blitted into the canvas swapchain) via
// `present_input`. A secondary offscreen render+readback path (`render_offscreen`
// + `rgba`/`width`/`height`) returns the framebuffer bytes so an e2e harness can
// pixel-compare GPU vs CPU even where reading the live canvas is awkward.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

use aterm_core::terminal::Terminal;
use aterm_render::{Renderer, Theme};

// GpuContext is used only by the wasm async init path (`init`); on the native
// target (a compile-verification surface only) it would be unused.
#[cfg(target_arch = "wasm32")]
use aterm_gpu::GpuContext;
use aterm_gpu::{GpuRenderer, GpuSurface, WindowGpu};

/// The terminal engine + GPU presentation state for one `<canvas>`.
///
/// Construction is split in two, matching the browser lifecycle:
///   1. [`AtermGpuTerminal::new`] — synchronous: build the engine grid + a CPU
///      face from injected font bytes (for cell metrics / the glyph atlas). No
///      GPU touched yet, so it can run before WebGL is confirmed.
///   2. [`AtermGpuTerminal::init`] — async: acquire the GPU and create +
///      configure the canvas surface. Separated so the host can fall back to the
///      CPU path (`native/aterm-wasm`) if WebGL is unavailable WITHOUT having
///      paid for the engine teardown.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct AtermGpuTerminal {
    term: Terminal,
    // CPU face: owns the glyph rasterizer + cell metrics. Reused for cols/rows
    // sizing here, and handed to the GPU renderer to build the glyph atlas.
    cpu: Renderer,
    rows: usize,
    cols: usize,
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    theme: Theme,
    // Read only by the wasm GPU paths (`init` rebuilds the face from these). On the
    // native verification target they are stored-but-unread.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    font_bytes: Vec<u8>,
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    px: f32,
    // GPU side: None until `init` succeeds. Once set, `render` presents on the GPU;
    // the host wires `render` into a requestAnimationFrame loop.
    gpu: Option<GpuState>,
    // Offscreen readback cache: the last `render_offscreen` frame, expanded to
    // RGBA8 (width*height*4 bytes), so an e2e harness can pixel-compare GPU vs CPU
    // without reading the live canvas. Mirrors `native/aterm-wasm`'s `rgba` buffer.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    rgba: Vec<u8>,
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    fb_width: usize,
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    fb_height: usize,
}

/// The GPU half of the terminal, populated by [`AtermGpuTerminal::init`].
struct GpuState {
    renderer: GpuRenderer,
    surface: GpuSurface,
    // Per-window present state (prior-frame snapshot for the scissored dirty-row
    // present path). One per surface, per aterm-gpu's design. Drives the
    // `present_input` (canvas) and `render_input` (offscreen readback) paths.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    win: WindowGpu,
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl AtermGpuTerminal {
    /// Build a `rows`x`cols` terminal. `font_bytes` (a TTF/OTF) is injected by the
    /// host (fetched in JS) — the engine does no filesystem font discovery on
    /// wasm. `px` is the cell font-size; `fg`/`bg`/`cursor`/`selection` are
    /// 0x00RRGGBB and seed the DEFAULT theme (per-cell SGR colors still flow
    /// through the grid independently).
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(constructor))]
    pub fn new(
        rows: u16,
        cols: u16,
        font_bytes: &[u8],
        px: f32,
        fg: u32,
        bg: u32,
        cursor: u32,
        selection: u32,
    ) -> Result<AtermGpuTerminal, String> {
        #[cfg(target_arch = "wasm32")]
        console_error_panic_hook::set_once();
        let theme = Theme {
            fg,
            bg,
            cursor,
            selection,
        };
        // Build the CPU face now (cheap, GPU-independent) so cell metrics are
        // available before WebGPU init and the host can size the canvas.
        let cpu = Renderer::from_bytes(font_bytes, px, theme)?;
        Ok(Self {
            term: Terminal::new(rows, cols),
            cpu,
            rows: rows as usize,
            cols: cols as usize,
            theme,
            font_bytes: font_bytes.to_vec(),
            px,
            gpu: None,
            rgba: Vec::new(),
            fb_width: 0,
            fb_height: 0,
        })
    }

    /// Feed raw PTY output bytes into the engine.
    pub fn process(&mut self, bytes: &[u8]) {
        self.term.process(bytes);
    }

    /// Resize the grid AND, if the GPU is live, the swapchain to match the new
    /// pixel extent (host recomputes cols/rows for the canvas first).
    pub fn resize(&mut self, rows: u16, cols: u16) {
        self.term.resize(rows, cols);
        self.rows = rows as usize;
        self.cols = cols as usize;
        if let Some(gpu) = self.gpu.as_mut() {
            let (w, h) = gpu.renderer.frame_size(self.rows, self.cols);
            gpu.renderer.resize_surface(&mut gpu.surface, w as u32, h as u32);
        }
    }

    /// Cell width in device pixels — the host computes cols = floor(canvasW / cellWidth).
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn cell_width(&self) -> usize {
        self.cpu.cell_size().0
    }

    /// Cell height in device pixels — the host computes rows = floor(canvasH / cellHeight).
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn cell_height(&self) -> usize {
        self.cpu.cell_size().1
    }

    /// True once [`AtermGpuTerminal::init`] has acquired a GPU + surface.
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn gpu_ready(&self) -> bool {
        self.gpu.is_some()
    }

    /// The acquired GPU adapter name + backend, once initialized (else empty).
    /// Lets the host log which GPU/backend WebGL handed us.
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn adapter_info(&self) -> String {
        match self.gpu.as_ref() {
            Some(gpu) => {
                let (name, backend) = gpu.renderer.adapter();
                format!("{name} ({backend})")
            }
            None => String::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// ASYNC WebGL init + present — wasm32 only (the WebGL backend + the
// HtmlCanvasElement / wasm_bindgen_futures glue exist only on the browser
// target). On native this whole block is absent; native callers drive
// aterm-gpu directly via its synchronous `GpuRenderer::new` + window surface.
// ---------------------------------------------------------------------------
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl AtermGpuTerminal {
    /// ASYNC: acquire the GPU and create + configure a WebGL2 surface on `canvas`.
    ///
    /// This is the browser equivalent of aterm-gpu's native `GpuRenderer::new` +
    /// `create_window_surface`, but every blocking step is `await`ed AND the
    /// surface is created BEFORE the adapter (the WebGL backend enumerates its
    /// adapter against the canvas surface — the GL context lives on the canvas):
    ///   - `wgpu::Instance` with the WebGL (GL) backend,
    ///   - `instance.create_surface(SurfaceTarget::Canvas(canvas))`,
    ///   - `GpuContext::from_instance_with_surface(instance, Some(&surface)).await`
    ///     — adapter + device, NO `pollster::block_on`,
    ///   - `GpuRenderer::from_parts(ctx, cpu_face, ..)` — the portable, thread-
    ///     free, font-discovery-free renderer assembly (all wgpu pipelines built),
    ///   - `configure_window_surface(surface, w, h)` — same format selection as
    ///     native's `create_window_surface`.
    ///
    /// Returns `Err` (a JS string) if WebGL is unavailable or any step fails, so
    /// the host can fall back to the CPU `aterm-wasm` path.
    pub async fn init(
        &mut self,
        canvas: web_sys::HtmlCanvasElement,
    ) -> Result<(), String> {
        // The browser WebGL2 backend. GL is the only backend compiled into the
        // wasm closure (default-features = false + features=["webgl"]); wgpu maps
        // `Backends::GL` to the canvas WebGL2 context on wasm32.
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::GL,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });

        // The WebGL backend (unlike WebGPU) can only acquire an adapter from a
        // surface — the GL context lives ON the <canvas>. So create the surface
        // FIRST, then request the compatible adapter via the shared async core.
        // `create_surface` is on the instance directly; the rest of init mirrors
        // native.
        let surface_raw = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
            .map_err(|e| format!("create canvas surface failed: {e}"))?;

        // Adapter + device, AWAITED (browsers forbid blocking the main thread).
        // Reuses aterm-gpu's shared async core, but passes the canvas surface as
        // the compatibility target so the GL backend can produce an adapter.
        let ctx = GpuContext::from_instance_with_surface(instance, Some(&surface_raw))
            .await
            .map_err(|e| format!("WebGL adapter/device init failed: {e}"))?;

        // Build the CPU face from the injected font bytes (no system font
        // discovery on wasm) and assemble the portable GPU renderer on the
        // acquired context — this builds every wgpu pipeline.
        let cpu = Renderer::from_bytes(&self.font_bytes, self.px, self.theme)?;
        let renderer = GpuRenderer::from_parts(ctx, cpu, None, self.theme)?;

        // Configure the already-created canvas swapchain (NON-sRGB format, sized
        // to the grid) on the renderer's adapter/device. Reuses aterm-gpu's
        // `configure_window_surface` (same format selection as native).
        let (w, h) = renderer.frame_size(self.rows, self.cols);
        let surface = renderer
            .configure_window_surface(surface_raw, w as u32, h as u32)
            .map_err(|e| format!("configure canvas surface failed: {e}"))?;

        self.gpu = Some(GpuState {
            renderer,
            surface,
            win: WindowGpu::new(),
        });
        Ok(())
    }

    /// Present one frame on the GPU canvas. Errors (returned as JS strings) if
    /// WebGL was not initialized.
    ///
    /// Draws the ACTUAL terminal grid: snapshot the engine state
    /// (`term.cell_frame`), then aterm-gpu's `present_input` renders it offscreen
    /// (glyph atlas upload + instanced bg/glyph/cursor quads) and blits that
    /// texture into the WebGL2 canvas swapchain — the same encode the native
    /// CPU==GPU parity tests gate, now on the WebGL backend.
    pub fn render(&mut self) -> Result<(), String> {
        let input = self.term.cell_frame(self.rows, self.cols);
        let gpu = self
            .gpu
            .as_mut()
            .ok_or("render() before init()")?;
        // `invert == false`: straight present (the visual-bell flash is host-driven).
        gpu.renderer
            .present_input(&mut gpu.win, &mut gpu.surface, &input, false);
        Ok(())
    }

    /// SECONDARY (e2e) path: render the current grid OFFSCREEN and read the pixels
    /// back into the internal RGBA8 framebuffer, so a host harness can pixel-compare
    /// GPU vs CPU output without reading the live canvas (a WebGL swapchain is not
    /// CPU-readable). Mirrors `native/aterm-wasm`'s `render()`+`rgba()` contract:
    /// the same `cell_frame` snapshot, the same `Frame` (0x00RRGGBB) expanded to
    /// RGBA8 with an opaque alpha. Errors if WebGL was not initialized.
    pub fn render_offscreen(&mut self) -> Result<(), String> {
        let input = self.term.cell_frame(self.rows, self.cols);
        let gpu = self
            .gpu
            .as_mut()
            .ok_or("render_offscreen() before init()")?;
        let frame = gpu.renderer.render_input(&mut gpu.win, &input);
        self.fb_width = frame.width;
        self.fb_height = frame.height;
        // aterm Frame pixels are packed 0x00RRGGBB; expand to RGBA8 for ImageData.
        self.rgba.clear();
        self.rgba.reserve(frame.pixels.len() * 4);
        for &p in &frame.pixels {
            self.rgba.push((p >> 16) as u8);
            self.rgba.push((p >> 8) as u8);
            self.rgba.push(p as u8);
            self.rgba.push(0xff);
        }
        Ok(())
    }

    /// Width in pixels of the last [`render_offscreen`](Self::render_offscreen)
    /// framebuffer.
    #[wasm_bindgen(getter)]
    pub fn width(&self) -> usize {
        self.fb_width
    }

    /// Height in pixels of the last [`render_offscreen`](Self::render_offscreen)
    /// framebuffer.
    #[wasm_bindgen(getter)]
    pub fn height(&self) -> usize {
        self.fb_height
    }

    /// Copy of the last [`render_offscreen`](Self::render_offscreen) RGBA8
    /// framebuffer (`width*height*4` bytes), ready for
    /// `ctx.putImageData(new ImageData(rgba, width, height), 0, 0)` or a pixel diff.
    pub fn rgba(&self) -> Vec<u8> {
        self.rgba.clone()
    }
}
