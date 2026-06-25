// SPDX-License-Identifier: MIT
// Copyright 2026 The aterm Authors
//
// `aterm-gpu-web` — the GPU rendering substrate for the Electron renderer.
//
// Sibling of `the aterm-wasm crate`: that crate parses PTY bytes with the aterm
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

use aterm_core::selection::SmartSelection;
use aterm_core::selection::{SelectionSide, SelectionType};
use aterm_core::terminal::{CursorStyle, MouseMode, Rgb, Terminal};
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
///      CPU path (`the aterm-wasm crate`) if WebGL is unavailable WITHOUT having
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
    // without reading the live canvas. Mirrors `the aterm-wasm crate`'s `rgba` buffer.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    rgba: Vec<u8>,
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    fb_width: usize,
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    fb_height: usize,
    // Built-in smart-selection rules (url/file_path/email/...) for scroll-correct
    // link detection via smart_word_at; reused across link_at calls. Mirrors
    // the aterm-wasm crate so the ONE engine per pane serves both draw + state.
    smart: SmartSelection,
    // Host-injected OS fallback faces (CJK/symbols + colour emoji). Kept so `init`
    // can RE-APPLY them to the fresh GPU CPU face it builds from `font_bytes`
    // (which lacks the fallbacks); fonts injected before init would otherwise be
    // lost. Empty until the host calls `set_fallback_font` / `set_emoji_font`.
    // INTERNED Arc (shared across panes via aterm_render::intern_font_bytes) so this
    // reinit-retention copy isn't a per-pane ~180MB (emoji) / ~100MB (CJK) duplicate.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    fallback_font: Option<std::sync::Arc<Vec<u8>>>,
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    emoji_font: Option<std::sync::Arc<Vec<u8>>>,
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
    #[allow(clippy::too_many_arguments)]
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
            smart: SmartSelection::with_builtin_rules(),
            fallback_font: None,
            emoji_font: None,
        })
    }

    /// Feed raw PTY output bytes into the engine.
    pub fn process(&mut self, bytes: &[u8]) {
        self.term.process(bytes);
    }

    /// Inject a broad-coverage (CJK + symbols) fallback face from font bytes, so
    /// glyphs the primary face lacks render real shapes instead of `.notdef` tofu.
    /// Applies to the CPU face (metrics) and the live GPU face if `init` already
    /// ran; the bytes are also remembered so `init` re-applies them to the fresh
    /// GPU face it builds. No-throw: a bad blob leaves the existing faces untouched.
    pub fn set_fallback_font(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.cpu.set_fallback_bytes(bytes)?;
        if let Some(gpu) = self.gpu.as_mut() {
            gpu.renderer.set_fallback_font_bytes(bytes)?;
        }
        self.fallback_font = Some(aterm_render::intern_font_bytes(bytes.to_vec()));
        Ok(())
    }

    /// Inject a colour-emoji (sbix) face from font bytes, driving the existing
    /// ColorEmoji colour path. Same wiring as [`set_fallback_font`]. No-throw
    /// (the `String` Err surfaces as a catchable JS exception).
    pub fn set_emoji_font(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.cpu.set_color_font_bytes(bytes.to_vec())?;
        if let Some(gpu) = self.gpu.as_mut() {
            gpu.renderer.set_emoji_font_bytes(bytes.to_vec())?;
        }
        self.emoji_font = Some(aterm_render::intern_font_bytes(bytes.to_vec()));
        Ok(())
    }

    /// Set an ANSI/indexed palette colour (index 0–255; 0–15 are the 16 ANSI
    /// colours) to RGB components, so SGR-indexed cell colours resolve through the
    /// host's theme palette instead of the engine's built-in VGA defaults. The
    /// palette lives on the shared grid (`self.term`), so this applies to both the
    /// GPU and CPU-fallback draw paths. Per-cell truecolor SGR flows independently.
    pub fn set_palette_color(&mut self, index: u8, r: u8, g: u8, b: u8) {
        self.term.set_palette_color_components(index, r, g, b);
    }

    /// Set the cursor blink phase (see aterm-wasm). Applies to the live GPU renderer
    /// AND the CPU face so the GPU present + offscreen readback paths agree.
    pub fn set_cursor_blink_phase(&mut self, on: bool) {
        self.cpu.set_cursor_blink_phase(on);
        if let Some(gpu) = self.gpu.as_mut() {
            gpu.renderer.set_cursor_blink_phase(on);
        }
    }

    /// Force a hollow (unfocused) cursor when `true`, or restore the terminal's
    /// DECSCUSR style when `false`. Applies to both GPU and CPU faces.
    pub fn set_cursor_hollow(&mut self, hollow: bool) {
        let style = if hollow { Some(CursorStyle::HollowBlock) } else { None };
        self.cpu.set_cursor_style_override(style);
        if let Some(gpu) = self.gpu.as_mut() {
            gpu.renderer.set_cursor_style_override(style);
        }
    }

    /// Drain the engine's pending query replies (DA1/DA2/DSR/CPR/DECRQM/OSC color/
    /// window-size, …) so the host can forward them to the PTY — the renderer is the
    /// authoritative responder. Call after each `process`.
    pub fn take_response(&mut self) -> Option<Vec<u8>> {
        self.term.take_response()
    }

    /// Drain pending OSC app-events as a JSON array of `[code, payload]` pairs
    /// (`[[7,"/home"],[52,"copied"]]`); `None` when empty. REAL decoded payloads
    /// (OSC 52 clipboard / OSC 7 cwd / OSC 133 mark) — distinct from PTY replies.
    pub fn take_osc_events(&mut self) -> Option<String> {
        if !self.term.has_osc_events() {
            return None;
        }
        let mut pairs = Vec::new();
        while let Some((code, payload)) = self.term.take_osc_event() {
            pairs.push(format!("[{code},{}]", json_string(&payload)));
        }
        Some(format!("[{}]", pairs.join(",")))
    }

    /// Display-relative cursor column (0-based).
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn cursor_x(&self) -> u16 {
        self.term.grid().cursor().col
    }

    /// Display-relative cursor row (0-based, top of viewport).
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn cursor_y(&self) -> u16 {
        self.term.grid().cursor().row
    }

    /// Absolute row index of the live/last line (xterm `buffer.active.baseY`):
    /// `oldest_absolute_row() + scrollback_lines()`. `usize` → plain JS number.
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn base_y(&self) -> usize {
        self.term.grid().base_y()
    }

    /// Absolute row index of the TOP visible line for the current viewport
    /// (`base_y - display_offset`); the search/link origin.
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn display_origin_absolute(&self) -> usize {
        self.term.grid().display_origin_absolute()
    }

    /// Soft-wrap flag for a visible `row`: `true` if it continues the previous
    /// row (autowrap), `None` when out of range.
    pub fn row_is_wrapped(&self, row: u16) -> Option<bool> {
        self.term.grid().row_is_wrapped(row)
    }

    /// Logical length of a visible `row` (last non-empty cell + 1, 0 if blank);
    /// `None` when out of range.
    pub fn row_len(&self, row: u16) -> Option<u16> {
        self.term.grid().row_len(row)
    }

    /// Grapheme text at visible cell `row`/`col` — base char plus complex
    /// cluster and combining marks. Empty string for a blank cell, a
    /// wide-continuation spacer, or out-of-range coords.
    pub fn cell_text(&self, row: u16, col: u16) -> String {
        self.term
            .cell_grapheme(row as usize, col as usize)
            .unwrap_or_default()
    }

    /// Whether the cell at `row`/`col` is a wide (double-width) character;
    /// `None` when out of range.
    pub fn cell_is_wide(&self, row: u16, col: u16) -> Option<bool> {
        self.term.grid().cell(row, col).map(|c| c.is_wide())
    }

    /// Drain the edge-triggered BEL flag: `true` if a BEL fired since the last
    /// call, then clears it (poll-based flash/ring without the bell callback).
    pub fn drain_bell(&mut self) -> bool {
        self.term.drain_bell()
    }

    /// Seed the engine's DEFAULT foreground/background so OSC 10/11 colour-query
    /// replies report the host theme. RGB components, 0–255.
    pub fn set_default_foreground(&mut self, r: u8, g: u8, b: u8) {
        self.term.set_default_foreground(Rgb { r, g, b });
    }

    pub fn set_default_background(&mut self, r: u8, g: u8, b: u8) {
        self.term.set_default_background(Rgb { r, g, b });
    }

    /// Tell the engine the real device-pixel cell size so CSI 14t/16t reports are
    /// accurate (the engine has no canvas otherwise).
    pub fn set_cell_pixel_size(&mut self, width: u16, height: u16) {
        self.term.set_cell_pixel_size(width, height);
    }

    /// Replace the default fg/bg/cursor/selection theme live (0x00RRGGBB) on both the
    /// GPU renderer and the CPU face, so a host theme change re-themes the pane
    /// without a device/face rebuild.
    pub fn set_theme(&mut self, fg: u32, bg: u32, cursor: u32, selection: u32) {
        let theme = Theme {
            fg,
            bg,
            cursor,
            selection,
        };
        self.cpu.set_theme(theme);
        if let Some(gpu) = self.gpu.as_mut() {
            gpu.renderer.set_theme(theme);
            // Force the next present to repaint everything: the selection band,
            // idle cursor, and padding border are theme-derived but not content,
            // so the dirty-row diff alone would leave them in the OLD theme.
            gpu.win.invalidate_present();
        }
    }

    /// Explicit selected-text foreground (theme `selectionForeground`), 0x00RRGGBB,
    /// or `undefined` for the WCAG contrast-floor default. Set on both the CPU
    /// fallback face and the live GPU renderer; forces a full present (appearance).
    pub fn set_selection_fg(&mut self, fg: Option<u32>) {
        self.cpu.set_selection_fg(fg);
        if let Some(gpu) = self.gpu.as_mut() {
            gpu.renderer.set_selection_fg(fg);
            gpu.win.invalidate_present();
        }
    }

    /// Re-rasterize at a new cell font px (host DPI / devicePixelRatio change) on
    /// both the CPU fallback face and the live GPU renderer (which also drops its
    /// atlas). The host re-reads cell_width/cell_height + resizes the grid after.
    pub fn set_px(&mut self, px: f32) {
        self.cpu.set_px(px);
        if let Some(gpu) = self.gpu.as_mut() {
            gpu.renderer.set_px(px);
            gpu.win.invalidate_present();
        }
    }

    /// Resize the grid AND, if the GPU is live, the swapchain to match the new
    /// pixel extent (host recomputes cols/rows for the canvas first).
    pub fn resize(&mut self, rows: u16, cols: u16) {
        self.term.resize(rows, cols);
        self.rows = rows as usize;
        self.cols = cols as usize;
        if let Some(gpu) = self.gpu.as_mut() {
            let (w, h) = gpu.renderer.frame_size(self.rows, self.cols);
            gpu.renderer
                .resize_surface(&mut gpu.surface, w as u32, h as u32);
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

    // ---------------------------------------------------------------------
    // Engine-state surface — passthroughs mirroring `the aterm-wasm crate`'s
    // `AtermTerminal`. Why: ONE engine per pane. The host's input handlers
    // (scroll/selection/search/mouse/link/cursor/focus) call these `term.*`
    // methods; exposing the SAME surface here lets the GPU drawer reuse the
    // single engine for both drawing AND state, so bytes are parsed once.
    // ---------------------------------------------------------------------

    /// Scroll the viewport through scrollback: positive `delta` reveals older
    /// lines, negative reveals newer. The host redraws afterwards.
    pub fn scroll_lines(&mut self, delta: i32) {
        self.term.scroll_display(delta);
    }

    /// Snap the viewport to the live bottom (latest output).
    pub fn scroll_to_bottom(&mut self) {
        self.term.scroll_to_bottom();
    }

    /// Snap the viewport to the oldest retained scrollback line.
    pub fn scroll_to_top(&mut self) {
        self.term.scroll_to_top();
    }

    /// Lines the viewport is scrolled up from the live bottom (0 = at bottom).
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn display_offset(&self) -> usize {
        self.term.grid().display_offset()
    }

    /// True when the alternate screen is active (TUIs own their own scrolling),
    /// so the host should let wheel events pass through to the app.
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn is_alt_screen(&self) -> bool {
        self.term.is_alternate_screen()
    }

    /// Serialize the terminal to a REPLAYABLE ANSI string (mirrors the CPU
    /// `AtermTerminal::serialize`) — the aterm-native replacement for xterm's
    /// SerializeAddon. `scrollback_rows`: None = all history, Some(n) = last n,
    /// Some(0) = viewport only. Operates on the shared engine grid.
    pub fn serialize(&self, scrollback_rows: Option<u32>) -> String {
        let grid = self.term.grid();
        let cap = scrollback_rows.map(|n| n as usize);
        let active_history = grid.scrollback_lines();
        let take = cap.map_or(active_history, |n| n.min(active_history));
        let mut out = String::from("\x1b[0m");
        for i in (active_history - take)..active_history {
            let line = grid
                .get_history_line(i)
                .and_then(|l| l.as_str().map(|s| s.trim_end().to_string()))
                .unwrap_or_default();
            out.push_str(&line);
            out.push_str("\r\n");
        }
        out.push_str("\x1b[H");
        for r in 0..self.rows as u16 {
            out.push_str(&format!("\x1b[{};1H\x1b[K", r + 1));
            if let Some(row_ansi) = grid.row_ansi_text(r) {
                out.push_str(&row_ansi);
            }
            out.push_str("\x1b[0m");
        }
        let c = self.term.cursor();
        out.push_str(&format!("\x1b[{};{}H", c.row as usize + 1, c.col as usize + 1));
        out
    }

    /// Scrollback HISTORY only (main buffer) — mirrors the CPU
    /// `AtermTerminal::serialize_scrollback`.
    pub fn serialize_scrollback(&self, max_rows: Option<u32>) -> String {
        let grid = self.term.main_grid();
        let history = grid.scrollback_lines();
        if history == 0 {
            return String::new();
        }
        let take = max_rows.map_or(history, |n| (n as usize).min(history));
        let mut out = String::new();
        for i in (history - take)..history {
            let line = grid
                .get_history_line(i)
                .and_then(|l| l.as_str().map(|s| s.trim_end().to_string()))
                .unwrap_or_default();
            out.push_str(&line);
            out.push_str("\r\n");
        }
        out
    }

    /// The window title (OSC 0/2), or `None` when unset (mirrors the CPU binding).
    pub fn title(&self) -> Option<String> {
        let title = self.term.title();
        if title.is_empty() {
            None
        } else {
            Some(title.to_string())
        }
    }

    /// Whether bracketed-paste mode (DECSET 2004) is active (mirrors the CPU binding).
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn bracketed_paste_mode(&self) -> bool {
        self.term.modes().bracketed_paste()
    }

    /// True when DECCKM (application cursor keys) is set: the host encodes
    /// arrows/Home/End as SS3 instead of CSI for full-screen apps.
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn is_app_cursor_mode(&self) -> bool {
        self.term.modes().application_cursor_keys()
    }

    /// True when a TUI has enabled mouse tracking (DECSET 9/1000/1002/1003).
    /// The host then ENCODES canvas mouse events to the PTY instead of running
    /// selection/scroll/link for them (unless Shift = user override).
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn is_mouse_tracking(&self) -> bool {
        self.term.mouse_tracking_enabled()
    }

    /// True when the active mouse mode reports MOTION (1002 drag, 1003 any).
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn mouse_wants_motion(&self) -> bool {
        matches!(
            self.term.mouse_mode(),
            MouseMode::ButtonEvent | MouseMode::AnyEvent
        )
    }

    /// True for AnyEvent (1003): report motion even with NO button pressed.
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn mouse_wants_any_motion(&self) -> bool {
        matches!(self.term.mouse_mode(), MouseMode::AnyEvent)
    }

    /// True when DECSET 1004 (focus reporting) is active: the host sends CSI I
    /// on focus-in and CSI O on focus-out so apps track terminal focus.
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn is_focus_event_mode(&self) -> bool {
        self.term.focus_reporting_enabled()
    }

    /// True when DEC mode 2031 (color-scheme update notifications) is set: the
    /// app wants `CSI ? 997 ; n` on OS light/dark theme changes.
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn is_color_scheme_updates_mode(&self) -> bool {
        self.term.report_color_scheme_enabled()
    }

    /// Active DECSCUSR cursor style as the discriminant of `aterm_core`'s
    /// `CursorStyle`. The GPU renderer paints the shape from the grid; this
    /// getter exists for host introspection/tests, mirroring aterm-wasm.
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn cursor_style(&self) -> u8 {
        self.term.cursor_style() as u8
    }

    /// Encode a mouse-button PRESS at 0-based cell `col`/`row` for the active
    /// mouse mode+encoding (`None` when tracking is off). See aterm-wasm.
    pub fn encode_mouse_press(&self, col: u16, row: u16, button: u8, mods: u8) -> Option<Vec<u8>> {
        self.term.encode_mouse_press(button, col, row, mods)
    }

    /// Encode a mouse-button RELEASE; `None` in X10 press-only mode.
    pub fn encode_mouse_release(
        &self,
        col: u16,
        row: u16,
        button: u8,
        mods: u8,
    ) -> Option<Vec<u8>> {
        self.term.encode_mouse_release(button, col, row, mods)
    }

    /// Encode mouse MOTION at `col`/`row`; `button` is the held button (3=none).
    pub fn encode_mouse_motion(&self, col: u16, row: u16, button: u8, mods: u8) -> Option<Vec<u8>> {
        self.term.encode_mouse_motion(button, col, row, mods)
    }

    /// Encode a mouse WHEEL tick at `col`/`row` (`up` = wheel-up); `None` in X10.
    pub fn encode_mouse_wheel(&self, col: u16, row: u16, up: bool, mods: u8) -> Option<Vec<u8>> {
        self.term.encode_mouse_wheel(up, col, row, mods)
    }

    /// Convert a display-relative row (0 = top of viewport) to the
    /// TERMINAL-relative row the selection model stores: `display_row -
    /// display_offset`, negative for scrollback. The renderer and
    /// `selection_to_string` both read terminal-relative rows, so converting
    /// here keeps the highlight and copied text correct while scrolled (#sel-fix).
    fn display_row_to_terminal(&self, display_row: i32) -> i32 {
        display_row - self.term.grid().display_offset() as i32
    }

    /// Begin a character selection at display `row`/`col` (clears any prior one).
    pub fn selection_start(&mut self, row: i32, col: u16) {
        let row = self.display_row_to_terminal(row);
        self.term.text_selection_mut().start_selection(
            row,
            col,
            SelectionSide::Left,
            SelectionType::Simple,
        );
    }

    /// Select the whole word/URL at display `row`/`col` (double-click) and return
    /// its text. Mirrors aterm-gui's select_word: a Semantic selection EXPANDED to
    /// the word's inclusive cell span (smart_word_at's end col is exclusive); on
    /// whitespace it falls back to the clicked cell. The selection stays active so
    /// the highlight paints.
    pub fn selection_word(&mut self, row: i32, col: u16) -> Option<String> {
        // smart_word_at is display-offset-aware (takes the DISPLAY row); the
        // selection anchor must be terminal-relative.
        let (start, last) = match self
            .term
            .smart_word_at(row as usize, col as usize, &self.smart)
        {
            Some((s, e)) => (s as u16, e.saturating_sub(1).max(s) as u16),
            None => (col, col),
        };
        let term_row = self.display_row_to_terminal(row);
        let sel = self.term.text_selection_mut();
        sel.start_selection(term_row, col, SelectionSide::Left, SelectionType::Semantic);
        sel.expand_semantic(start, last);
        sel.complete_selection();
        self.term.selection_to_string()
    }

    /// Select the whole line at display `row` (triple-click) and return its text.
    /// Mirrors aterm-gui's select_line: a Lines selection expanded to the full row
    /// width. `col` is accepted for a uniform host API but unused (whole row).
    pub fn selection_line(&mut self, row: i32, col: u16) -> Option<String> {
        let _ = col;
        let row = self.display_row_to_terminal(row);
        let max_col = (self.cols as u16).saturating_sub(1);
        let sel = self.term.text_selection_mut();
        sel.start_selection(row, 0, SelectionSide::Left, SelectionType::Lines);
        sel.expand_lines(max_col);
        sel.complete_selection();
        self.term.selection_to_string()
    }

    /// Move the selection endpoint to `row`/`col` (during a drag).
    pub fn selection_extend(&mut self, row: i32, col: u16) {
        let row = self.display_row_to_terminal(row);
        self.term
            .text_selection_mut()
            .update_selection(row, col, SelectionSide::Right);
    }

    /// Finalize the selection (mouse released).
    pub fn selection_finish(&mut self) {
        self.term.text_selection_mut().complete_selection();
    }

    /// Drop the current selection so the highlight clears on the next render.
    pub fn selection_clear(&mut self) {
        self.term.text_selection_mut().clear();
    }

    /// The selected text, if any (`None` when the selection is empty).
    pub fn selection_text(&self) -> Option<String> {
        self.term.selection_to_string()
    }

    /// Current selection bounds in DISPLAY viewport cell coords (0 = top visible
    /// row), side-adjusted to match `selection_text` and the painted highlight.
    /// `None` when there is no selection OR it lies fully outside the viewport.
    pub fn selection_range(&self) -> Option<SelectionRange> {
        selection_range_for(&self.term, self.rows, self.cols)
    }

    /// Detect a link under display `row`/`col`. Prefers an OSC-8 hyperlink, then
    /// falls back to smart-selection rules (url/file_path). `None` for plain
    /// words. `kind`: 0=osc8, 1=url, 2=file_path, 3=other. See aterm-wasm.
    pub fn link_at(&self, row: u16, col: u16) -> Option<LinkHit> {
        // OSC-8 lookups are NOT display_offset-aware, so only consult
        // hyperlink_at when the viewport isn't scrolled.
        if self.term.grid().display_offset() == 0 {
            if let Some(url) = self.term.hyperlink_at(row, col) {
                let url = url.to_string();
                let (s, e) = self.osc8_span(row, col);
                return Some(LinkHit {
                    url,
                    start_col: s,
                    end_col: e,
                    kind: 0,
                });
            }
        }

        // Smart-selection fallback is scroll-correct on any scrollback row.
        let (sc, ec) = self
            .term
            .smart_word_at(row as usize, col as usize, &self.smart)?;
        let text = self.term.display_row_text(row as usize)?;
        let matched = slice_by_columns(&text, sc, ec);
        let kind = classify(&matched);
        if kind == 3 {
            return None;
        }
        Some(LinkHit {
            url: matched,
            start_col: sc as u16,
            end_col: ec as u16,
            kind,
        })
    }

    /// Scroll-correct text of a display `row` (display_offset-aware), for a TS
    /// fallback that re-runs link matching in JS.
    pub fn row_text(&self, row: u16) -> Option<String> {
        self.term.display_row_text(row as usize)
    }

    /// Search the full retained buffer for `query`, returning matches as a flat
    /// `[abs_line, start_col, len]` triplet array. Empty query / regex error →
    /// empty array. See aterm-wasm for the coordinate contract.
    pub fn search(&mut self, query: &str, case_sensitive: bool) -> Vec<u32> {
        if query.is_empty() {
            return Vec::new();
        }
        let Ok(results) =
            self.term
                .indexed_search()
                .search_results_opts(query, case_sensitive, false)
        else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(results.matches.len() * 3);
        for m in &results.matches {
            out.push(u32::try_from(m.line).unwrap_or(u32::MAX));
            out.push(u32::try_from(m.start_col).unwrap_or(u32::MAX));
            out.push(u32::try_from(m.len()).unwrap_or(u32::MAX));
        }
        out
    }

    /// Absolute row of display row 0 at the live bottom. A match at absolute
    /// `line` is at display row `line - origin + display_offset`.
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn search_display_origin(&self) -> u32 {
        let grid = self.term.grid();
        let origin = grid
            .oldest_absolute_row()
            .saturating_add(grid.scrollback_lines() as u64);
        u32::try_from(origin).unwrap_or(u32::MAX)
    }

    /// Scroll the viewport so the match at absolute `line` is visible (top row),
    /// clamped to the retained scrollback. Host redraws after.
    pub fn scroll_search_line_into_view(&mut self, line: u32) {
        let grid = self.term.grid();
        let origin = grid
            .oldest_absolute_row()
            .saturating_add(grid.scrollback_lines() as u64);
        let scrollback = grid.scrollback_lines();
        let current = grid.display_offset();
        let want = origin.saturating_sub(u64::from(line));
        let want = (want as usize).min(scrollback);
        let delta = want as i64 - current as i64;
        if let Ok(delta) = i32::try_from(delta) {
            self.term.scroll_display(delta);
        }
    }
}

impl AtermGpuTerminal {
    /// Expand an OSC-8 hyperlink to the span of contiguous cells sharing its
    /// link. Cells group by `id=` when present, else by URL. Returns
    /// `[start_col, end_col_exclusive)`. Only valid at display_offset 0.
    fn osc8_span(&self, row: u16, col: u16) -> (u16, u16) {
        let same = |c: u16| -> bool {
            let id_here = self.term.hyperlink_id_at(row, col);
            let id_there = self.term.hyperlink_id_at(row, c);
            if id_here.is_some() && id_there.is_some() {
                id_here == id_there
            } else {
                self.term.hyperlink_at(row, c) == self.term.hyperlink_at(row, col)
            }
        };

        let mut start = col;
        while start > 0 && same(start - 1) {
            start -= 1;
        }

        let cols = self.cols as u16;
        let mut end = col + 1;
        while end < cols && same(end) {
            end += 1;
        }

        (start, end)
    }
}

/// A detected link under a cell: its text/URL, the half-open display-column span
/// it covers, and a `kind` discriminant (0=osc8, 1=url, 2=file_path, 3=other).
/// Mirrors `the aterm-wasm crate`'s `LinkHit` so the host link input is unchanged.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct LinkHit {
    url: String,
    start_col: u16,
    end_col: u16,
    kind: u8,
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl LinkHit {
    /// The link's URL/target text.
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn url(&self) -> String {
        self.url.clone()
    }

    /// Inclusive start display column of the link span.
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn start_col(&self) -> u16 {
        self.start_col
    }

    /// Exclusive end display column of the link span.
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn end_col(&self) -> u16 {
        self.end_col
    }

    /// Link kind: 0=osc8, 1=url, 2=file_path, 3=other.
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn kind(&self) -> u8 {
        self.kind
    }
}

/// Selection bounds in DISPLAY viewport cell coords (0 = top visible row),
/// inclusive of `start`, with `end` already side-adjusted to match
/// `selection_text` and the painted highlight. Mirrors the aterm-wasm crate.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct SelectionRange {
    start_x: u16,
    start_y: u16,
    end_x: u16,
    end_y: u16,
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl SelectionRange {
    /// Start column (display-relative).
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn start_x(&self) -> u16 {
        self.start_x
    }

    /// Start row (display-relative, 0 = top visible row).
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn start_y(&self) -> u16 {
        self.start_y
    }

    /// End column (display-relative, side-adjusted/inclusive).
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn end_x(&self) -> u16 {
        self.end_x
    }

    /// End row (display-relative).
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn end_y(&self) -> u16 {
        self.end_y
    }
}

/// Project the engine's selection (terminal-relative rows) into DISPLAY viewport
/// coords, clamping partially-scrolled selections to `[0, rows)`. Uses the SAME
/// `project_range` + side-adjustment the renderer and `selection_text` use, so
/// the three always agree. `None` when there is no selection or it is fully
/// outside the viewport. Mirrors the aterm-wasm crate.
fn selection_range_for(term: &Terminal, rows: usize, cols: usize) -> Option<SelectionRange> {
    let last_col = (cols as u16).saturating_sub(1);
    let proj = term.text_selection().project_range(last_col)?;
    let offset = term.grid().display_offset() as i32;
    let rows_i = rows as i32;

    // terminal_row -> display_row = terminal_row + display_offset.
    let start_disp = proj.start_row + offset;
    let end_disp = proj.end_row + offset;

    // Fully outside the viewport (both ends above the top or below the bottom).
    if end_disp < 0 || start_disp >= rows_i {
        return None;
    }

    // Clamp to the viewport: a row scrolled off the top enters at col 0 of the
    // top row; one past the bottom exits at the last col of the bottom row.
    let (start_y, start_x) = if start_disp < 0 {
        (0u16, 0u16)
    } else {
        (start_disp as u16, proj.start_col)
    };
    let (end_y, end_x) = if end_disp >= rows_i {
        ((rows_i - 1) as u16, last_col)
    } else {
        (end_disp as u16, proj.end_col)
    };

    Some(SelectionRange {
        start_x,
        start_y,
        end_x,
        end_y,
    })
}

/// JSON-escape `s` and wrap it in double quotes for `take_osc_events`.
/// Mirrors the aterm-wasm crate.
fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Slice `text` to the half-open display-column range `[start_col, end_col)`.
/// Approximates display width as 1 per char — correct for the ASCII URLs/paths
/// that dominate link detection (mirrors the aterm-wasm crate).
fn slice_by_columns(text: &str, start_col: usize, end_col: usize) -> String {
    text.chars()
        .skip(start_col)
        .take(end_col.saturating_sub(start_col))
        .collect()
}

/// Classify a matched span: 1=url, 2=file_path, else 3=other (plain word).
fn classify(s: &str) -> u8 {
    let lower = s.to_ascii_lowercase();
    if lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("ftp://")
        || lower.starts_with("file://")
        || lower.starts_with("www.")
    {
        return 1;
    }
    if s.starts_with('/')
        || s.starts_with("./")
        || s.starts_with("../")
        || s.starts_with("~/")
        || (s.contains('/') && !s.contains("://"))
    {
        return 2;
    }
    3
}

// ---------------------------------------------------------------------------
// ASYNC WebGL init + present — wasm32 only (the WebGL backend + the
// HtmlCanvasElement / wasm_bindgen_futures glue exist only on the browser
// target). On native this whole block is absent; native callers drive
// aterm-gpu directly via its synchronous `GpuRenderer::new` + window surface.
// ---------------------------------------------------------------------------
/// An empty `RawDisplayHandle::Web` provider. wgpu 29 requires the instance to
/// carry a display handle before `create_surface()`, but the WebGL backend reads
/// the canvas from the WINDOW handle and ignores the display — so this ZST marker
/// only exists to satisfy wgpu-core's display-handle gate on the canvas path.
#[cfg(target_arch = "wasm32")]
#[derive(Debug)]
struct WebDisplay;

#[cfg(target_arch = "wasm32")]
impl wgpu::rwh::HasDisplayHandle for WebDisplay {
    fn display_handle(&self) -> Result<wgpu::rwh::DisplayHandle<'_>, wgpu::rwh::HandleError> {
        let raw = wgpu::rwh::RawDisplayHandle::Web(wgpu::rwh::WebDisplayHandle::new());
        // SAFETY: the Web display handle is an empty marker (no borrowed data),
        // so a 'static borrow is sound — there is nothing for it to outlive.
        Ok(unsafe { wgpu::rwh::DisplayHandle::borrow_raw(raw) })
    }
}

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
    pub async fn init(&mut self, canvas: web_sys::HtmlCanvasElement) -> Result<(), String> {
        // The browser WebGL2 backend. GL is the only backend compiled into the
        // wasm closure (default-features = false + features=["webgl"]); wgpu maps
        // `Backends::GL` to the canvas WebGL2 context on wasm32.
        //
        // wgpu 29 gates `create_surface()` on the instance carrying SOME display
        // handle (wgpu-core returns MissingDisplayHandle for (None, None) — the
        // safe `SurfaceTarget::Canvas` path passes no display handle). The WebGL
        // backend reads the canvas from the WINDOW handle and ignores the display,
        // so we attach an empty `RawDisplayHandle::Web` marker purely to satisfy
        // that gate. Without it, canvas surface creation fails headless.
        let instance = wgpu::Instance::new(
            wgpu::InstanceDescriptor {
                backends: wgpu::Backends::GL,
                ..wgpu::InstanceDescriptor::new_without_display_handle()
            }
            .with_display_handle(Box::new(WebDisplay)),
        );

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
        let mut cpu = Renderer::from_bytes(&self.font_bytes, self.px, self.theme)?;
        // Re-apply any fonts the host injected BEFORE init: the fresh face above is
        // built from `font_bytes` alone, so it lacks them otherwise.
        if let Some(bytes) = self.fallback_font.as_deref() {
            cpu.set_fallback_bytes(bytes)?;
        }
        if let Some(bytes) = self.emoji_font.as_deref() {
            // The transient Vec clone is re-interned to the shared Arc inside
            // set_color_font_bytes, so no persistent per-pane duplicate remains.
            cpu.set_color_font_bytes(bytes.clone())?;
        }
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
        let gpu = self.gpu.as_mut().ok_or("render() before init()")?;
        // `invert == false`: straight present (the visual-bell flash is host-driven).
        gpu.renderer
            .present_input(&mut gpu.win, &mut gpu.surface, &input, false);
        Ok(())
    }

    /// SECONDARY (e2e) path: render the current grid OFFSCREEN and read the pixels
    /// back into the internal RGBA8 framebuffer, so a host harness can pixel-compare
    /// GPU vs CPU output without reading the live canvas (a WebGL swapchain is not
    /// CPU-readable). Mirrors `the aterm-wasm crate`'s `render()`+`rgba()` contract:
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
