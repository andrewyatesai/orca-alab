// SPDX-License-Identifier: MIT
//
// `aterm-gpu-web` — the GPU rendering substrate for the Electron renderer.
//
// Sibling of `native/aterm-wasm`: that crate parses PTY bytes with the aterm
// engine (`aterm-core`) and rasterizes the grid on the CPU (`aterm-render`),
// then JS `putImageData`s the RGBA frame onto a `<canvas>`. THIS crate keeps the
// same engine front-end but renders on the GPU via `aterm-gpu` (wgpu's WebGPU
// backend), drawing straight into a `<canvas>` WebGPU surface — no CPU readback,
// no `putImageData`.
//
// The init path is ASYNC: a browser cannot block the main thread, so adapter +
// device acquisition is `await`ed (`wasm_bindgen_futures`), NOT `pollster::
// block_on` (the native aterm-gpu path). The surface is created from the
// `HtmlCanvasElement` via wgpu's `SurfaceTarget::Canvas`.
//
// INCREMENT 1 SCOPE (this file): a COMPILING wasm32 GPU pipeline + a real
// WebGPU-from-canvas init that configures the swapchain and CLEARS it to the
// theme background — the proving vertical slice. The instanced-cell-quad encode
// (aterm-gpu's `present_input`) is wired behind a single documented TODO in
// `render`; everything it needs (the `Terminal`, a CPU face for cell metrics,
// the `GpuContext`, the configured `GpuSurface`) is already assembled here.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

use aterm_core::terminal::Terminal;
use aterm_render::{Renderer, Theme};

// GpuContext is used only by the wasm async init path (`init_webgpu`); on the
// native target (a compile-verification surface only) it would be unused.
#[cfg(target_arch = "wasm32")]
use aterm_gpu::GpuContext;
use aterm_gpu::{GpuRenderer, GpuSurface, WindowGpu};

/// The terminal engine + GPU presentation state for one `<canvas>`.
///
/// Construction is split in two, matching the browser lifecycle:
///   1. [`AtermGpuTerminal::new`] — synchronous: build the engine grid + a CPU
///      face from injected font bytes (for cell metrics / the future glyph
///      atlas). No GPU touched yet, so it can run before WebGPU is confirmed.
///   2. [`AtermGpuTerminal::init_webgpu`] — async: acquire the GPU and create +
///      configure the canvas surface. Separated so the host can fall back to the
///      CPU path (`native/aterm-wasm`) if WebGPU is unavailable WITHOUT having
///      paid for the engine teardown.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct AtermGpuTerminal {
    term: Terminal,
    // CPU face: owns the glyph rasterizer + cell metrics. Reused for cols/rows
    // sizing now, and (TODO) handed to the GPU renderer to build the atlas.
    cpu: Renderer,
    rows: usize,
    cols: usize,
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    theme: Theme,
    // Read only by the wasm GPU paths (`init_webgpu` rebuilds the face from these,
    // `render` clears to the theme bg). On the native verification target they are
    // stored-but-unread.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    font_bytes: Vec<u8>,
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    px: f32,
    // GPU side: None until `init_webgpu` succeeds. Once set, `render` presents on
    // the GPU; the host wires `render` into a requestAnimationFrame loop.
    gpu: Option<GpuState>,
}

/// The GPU half of the terminal, populated by [`AtermGpuTerminal::init_webgpu`].
struct GpuState {
    renderer: GpuRenderer,
    surface: GpuSurface,
    // Per-window present state (prior-frame snapshot for the scissored dirty-row
    // present path). One per surface, per aterm-gpu's design. Consumed by the
    // increment-2 `present_input` wiring; on native it is constructed-but-unread.
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

    /// True once [`AtermGpuTerminal::init_webgpu`] has acquired a GPU + surface.
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn gpu_ready(&self) -> bool {
        self.gpu.is_some()
    }

    /// The acquired GPU adapter name + backend, once initialized (else empty).
    /// Lets the host log which GPU/backend WebGPU handed us.
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
// ASYNC WebGPU init + present — wasm32 only (the WebGPU backend + the
// HtmlCanvasElement / wasm_bindgen_futures glue exist only on the browser
// target). On native this whole block is absent; native callers drive
// aterm-gpu directly via its synchronous `GpuRenderer::new` + window surface.
// ---------------------------------------------------------------------------
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl AtermGpuTerminal {
    /// ASYNC: acquire the GPU and create + configure a WebGPU surface on `canvas`.
    ///
    /// This is the browser equivalent of aterm-gpu's native `GpuRenderer::new` +
    /// `create_window_surface`, but every blocking step is `await`ed:
    ///   - `wgpu::Instance` with the WebGPU backend,
    ///   - `GpuContext::from_instance(..).await` — adapter + device, NO
    ///     `pollster::block_on`,
    ///   - `GpuRenderer::from_parts(ctx, cpu_face, ..)` — the portable, thread-
    ///     free, font-discovery-free renderer assembly (all wgpu pipelines built),
    ///   - `create_window_surface(SurfaceTarget::Canvas(canvas), w, h)`.
    ///
    /// Returns `Err` (a JS string) if WebGPU is unavailable or any step fails, so
    /// the host can fall back to the CPU `aterm-wasm` path.
    pub async fn init_webgpu(
        &mut self,
        canvas: web_sys::HtmlCanvasElement,
    ) -> Result<(), String> {
        // The browser WebGPU backend. BROWSER_WEBGPU is the only backend compiled
        // into the wasm closure (default-features = false + features=["webgpu"]).
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::BROWSER_WEBGPU,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });

        // Adapter + device, AWAITED (browsers forbid blocking the main thread).
        // Reuses aterm-gpu's shared async core so wasm + native hit identical
        // adapter/device descriptors. The instance is kept on the ctx so the
        // canvas surface is created from it next.
        let ctx = GpuContext::from_instance(instance)
            .await
            .map_err(|e| format!("WebGPU adapter/device init failed: {e}"))?;

        // Build the CPU face from the injected font bytes (no system font
        // discovery on wasm) and assemble the portable GPU renderer on the
        // acquired context — this builds every wgpu pipeline.
        let cpu = Renderer::from_bytes(&self.font_bytes, self.px, self.theme)?;
        let renderer = GpuRenderer::from_parts(ctx, cpu, None, self.theme)?;

        // Create + configure the canvas swapchain (NON-sRGB format, sized to the
        // grid). `SurfaceTarget::Canvas` is wgpu 29's web entry point; this reuses
        // aterm-gpu's `create_window_surface` (same format selection as native).
        let (w, h) = renderer.frame_size(self.rows, self.cols);
        let surface = renderer
            .create_window_surface(wgpu::SurfaceTarget::Canvas(canvas), w as u32, h as u32)
            .map_err(|e| format!("create canvas surface failed: {e}"))?;

        self.gpu = Some(GpuState {
            renderer,
            surface,
            win: WindowGpu::new(),
        });
        Ok(())
    }

    /// Present one frame on the GPU. Errors (returned as JS strings) if WebGPU was
    /// not initialized.
    ///
    /// INCREMENT 1: this CLEARS the canvas swapchain to the theme background — the
    /// proving slice that the whole WebGPU-from-canvas path (instance → adapter →
    /// device → surface → present) is live end to end.
    ///
    /// TODO(increment 2): replace the clear with aterm-gpu's instanced-cell-quad
    /// present. The wiring is one call — everything it needs is already here:
    ///
    /// ```ignore
    /// let input = self.term.cell_frame(self.rows, self.cols);
    /// let gpu = self.gpu.as_mut().unwrap();
    /// gpu.renderer.present_input(&mut gpu.win, &mut gpu.surface, &input, false);
    /// ```
    ///
    /// (present_input renders the grid offscreen — glyph atlas + bg/glyph
    /// instanced quads — then blits that texture into the swapchain. The remaining
    /// work is verifying the glyph-atlas TEXTURE UPLOAD path runs under the WebGPU
    /// backend, since the parity tests exercise it only on native Metal/Vulkan.)
    pub fn render(&mut self) -> Result<(), String> {
        let gpu = self
            .gpu
            .as_mut()
            .ok_or("render() before init_webgpu()")?;
        // Suppress the unused-field warning until increment 2 wires present_input.
        let _ = &gpu.win;
        gpu.renderer
            .clear_surface(&mut gpu.surface, self.theme.bg);
        Ok(())
    }
}
