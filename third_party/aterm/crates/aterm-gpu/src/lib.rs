// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors
//
// aterm GPU renderer (wgpu → Metal on macOS).
//
// The terminal grid is drawn on the GPU: a glyph atlas texture + one instanced
// quad per cell (background fill + glyph blit). The SAME renderer can target an
// on-screen window surface OR an offscreen texture; the offscreen path reads the
// pixels back into an `aterm_render::Frame` so GPU output is verifiable headless
// (PNG round-trip) — exactly like the CPU `read_image` oracle, but on the GPU.
//
// This file currently lands the device + offscreen readback foundation; glyph
// rendering builds on top.

use aterm_render::Frame;

mod renderer;
pub use renderer::{GpuRenderer, GpuSurface};

/// wgpu device + queue, plus what we learned about the adapter.
///
/// The `instance` and `adapter` are KEPT (not dropped after device creation) so a
/// window surface can be created on the SAME instance/adapter later — the GPU
/// on-glass present path (`GpuRenderer::create_window_surface`) blits the
/// offscreen frame straight into a swapchain instead of reading it back to CPU.
pub struct GpuContext {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub adapter_name: String,
    pub backend: String,
    /// Kept alive so window surfaces can be created from this instance, and so
    /// `surface.get_capabilities(&adapter)` can be queried at surface setup.
    pub(crate) instance: wgpu::Instance,
    pub(crate) adapter: wgpu::Adapter,
}

/// Row alignment required by `copy_texture_to_buffer`.
const ALIGN: usize = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize;

fn padded_bytes_per_row(width: usize) -> usize {
    let unpadded = width * 4;
    unpadded.div_ceil(ALIGN) * ALIGN
}

impl GpuContext {
    /// Acquire a GPU. Works headless (no window/surface needed) — picks the
    /// default high-performance adapter (Metal on macOS).
    pub fn new() -> Result<Self, String> {
        // This instance must OUTLIVE device creation: it is kept on `GpuContext`
        // so a window surface can be created from it for the on-glass present
        // path. `new_without_display_handle()` (no `OwnedDisplayHandle`) is still
        // surface-capable on Metal — the platform doesn't use the display handle
        // (it's only required for GLES/Wayland presentation), so the headless
        // adapter request below can keep `compatible_surface: None`.
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
        }))
        .map_err(|e| format!("no GPU adapter available: {e}"))?;
        let info = adapter.get_info();
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("aterm-gpu device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            }))
            .map_err(|e| e.to_string())?;
        Ok(Self {
            device,
            queue,
            adapter_name: info.name,
            backend: format!("{:?}", info.backend),
            instance,
            adapter,
        })
    }

    /// Create an offscreen colour target (Rgba8Unorm; render + copy-src +
    /// texture-binding). `TEXTURE_BINDING` is additive — it lets the on-glass
    /// blit SAMPLE this exact texture into the swapchain, so the pixels on screen
    /// are byte-identical to the readback the AI introspection sees. The parity
    /// tests (which build the atlas on the CPU, not this texture) are unaffected.
    pub fn offscreen_texture(&self, width: u32, height: u32) -> wgpu::Texture {
        self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("aterm-gpu offscreen"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
    }

    /// Read an Rgba8Unorm texture back into an `aterm_render::Frame`
    /// (0x00RRGGBB, opaque), stripping GPU row padding.
    pub fn read_back(&self, texture: &wgpu::Texture, width: u32, height: u32) -> Frame {
        let (w, h) = (width as usize, height as usize);
        let padded = padded_bytes_per_row(w);
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("aterm-gpu readback"),
            size: (padded * h) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("readback") });
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded as u32),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        );
        self.queue.submit([enc.finish()]);

        let slice = buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        self.device.poll(wgpu::PollType::wait_indefinitely()).expect("GPU poll failed");
        let data = slice.get_mapped_range();

        let mut pixels = Vec::with_capacity(w * h);
        for row in 0..h {
            let base = row * padded;
            for col in 0..w {
                let p = base + col * 4;
                let (r, g, b) = (data[p] as u32, data[p + 1] as u32, data[p + 2] as u32);
                pixels.push((r << 16) | (g << 8) | b);
            }
        }
        drop(data);
        buffer.unmap();
        Frame { width: w, height: h, pixels }
    }

    /// Phase-1 proof of life: clear an offscreen target to a colour and read it
    /// back. Confirms the GPU pipeline + readback work on this machine.
    pub fn clear_to_frame(&self, width: u32, height: u32, rgb: u32) -> Frame {
        let tex = self.offscreen_texture(width, height);
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("clear") });
        {
            let _pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("clear pass"),
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
        self.queue.submit([enc.finish()]);
        self.read_back(&tex, width, height)
    }
}
