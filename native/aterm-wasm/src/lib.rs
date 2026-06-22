//! `aterm-wasm` — the in-page rendering substrate that replaces `@xterm/xterm`'s
//! rendering in the Electron renderer.
//!
//! Architecture (see docs/rust-migration): the daemon keeps the PTY and streams
//! bytes to the renderer; here, in the renderer process, the aterm engine
//! (`aterm-core`) parses those bytes into its grid and the pure-Rust CPU
//! rasterizer (`aterm-render`) turns the grid into an RGBA framebuffer that JS
//! blits to a `<canvas>`. No GPU/winit/DOM dependency — everything compiles to
//! `wasm32-unknown-unknown`. Fonts are injected as bytes (fetched in JS) so there
//! is no `std::fs` font discovery.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

use aterm_core::selection::{SelectionSide, SelectionType};
use aterm_core::terminal::Terminal;
use aterm_render::{Renderer, Theme};

/// A terminal + CPU renderer pair. Feed PTY bytes with [`AtermTerminal::process`],
/// then [`AtermTerminal::render`] to refresh the RGBA framebuffer, then read it
/// back via [`AtermTerminal::rgba`] (+ `width`/`height`) to draw onto a canvas.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct AtermTerminal {
    term: Terminal,
    renderer: Renderer,
    rows: usize,
    cols: usize,
    rgba: Vec<u8>,
    width: usize,
    height: usize,
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl AtermTerminal {
    /// Build a `rows`x`cols` terminal rendered with `font_bytes` (a TTF/OTF) at
    /// `px` cell font-size. `font_bytes` is injected by the host (fetched in JS),
    /// keeping the engine free of filesystem font discovery. `fg`/`bg`/`cursor`/
    /// `selection` are 0x00RRGGBB and seed the renderer's DEFAULT theme colors;
    /// per-cell SGR colors still flow through the grid independently.
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
    ) -> Result<AtermTerminal, String> {
        #[cfg(target_arch = "wasm32")]
        console_error_panic_hook::set_once();
        let theme = Theme {
            fg,
            bg,
            cursor,
            selection,
        };
        let renderer = Renderer::from_bytes(font_bytes, px, theme)?;
        Ok(Self {
            term: Terminal::new(rows, cols),
            renderer,
            rows: rows as usize,
            cols: cols as usize,
            rgba: Vec::new(),
            width: 0,
            height: 0,
        })
    }

    /// Feed raw PTY output bytes into the engine.
    pub fn process(&mut self, bytes: &[u8]) {
        self.term.process(bytes);
    }

    /// Resize the grid (after the host recomputes cols/rows for the canvas).
    pub fn resize(&mut self, rows: u16, cols: u16) {
        self.term.resize(rows, cols);
        self.rows = rows as usize;
        self.cols = cols as usize;
    }

    /// Rasterize the current grid into the internal RGBA8 framebuffer.
    pub fn render(&mut self) {
        let input = self.term.cell_frame(self.rows, self.cols);
        let frame = self.renderer.render_input(&input);
        self.width = frame.width;
        self.height = frame.height;
        // aterm Frame pixels are packed 0x00RRGGBB; expand to RGBA8 for ImageData.
        self.rgba.clear();
        self.rgba.reserve(frame.pixels.len() * 4);
        for &p in &frame.pixels {
            self.rgba.push((p >> 16) as u8);
            self.rgba.push((p >> 8) as u8);
            self.rgba.push(p as u8);
            self.rgba.push(0xff);
        }
    }

    /// Last-rendered framebuffer width in pixels.
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn width(&self) -> usize {
        self.width
    }

    /// Last-rendered framebuffer height in pixels.
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn height(&self) -> usize {
        self.height
    }

    /// Cell width in device pixels — the host computes cols = floor(canvasW / cellWidth).
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn cell_width(&self) -> usize {
        self.renderer.cell_size().0
    }

    /// Cell height in device pixels — the host computes rows = floor(canvasH / cellHeight).
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn cell_height(&self) -> usize {
        self.renderer.cell_size().1
    }

    /// Copy of the last-rendered RGBA8 framebuffer (`width*height*4` bytes),
    /// ready for `ctx.putImageData(new ImageData(rgba, width, height), 0, 0)`.
    pub fn rgba(&self) -> Vec<u8> {
        self.rgba.clone()
    }

    /// Scroll the viewport through scrollback: positive `delta` reveals older
    /// lines, negative reveals newer. `render` already honors the display offset,
    /// so the host only needs to redraw afterwards.
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

    /// Begin a character selection at display `row`/`col` (clears any prior one).
    pub fn selection_start(&mut self, row: i32, col: u16) {
        self.term
            .text_selection_mut()
            .start_selection(row, col, SelectionSide::Left, SelectionType::Simple);
    }

    /// Move the selection endpoint to `row`/`col` (during a drag).
    pub fn selection_extend(&mut self, row: i32, col: u16) {
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
}

// Native-only constructor for headless tests/benches: discovers a system font so
// the render pipeline can be exercised without injecting font bytes. The wasm
// build always uses `new` with injected fonts.
#[cfg(not(target_arch = "wasm32"))]
impl AtermTerminal {
    pub fn new_from_system(rows: u16, cols: u16, px: f32) -> Option<AtermTerminal> {
        let renderer = Renderer::from_system(px, Theme::default())?;
        Some(Self {
            term: Terminal::new(rows, cols),
            renderer,
            rows: rows as usize,
            cols: cols as usize,
            rgba: Vec::new(),
            width: 0,
            height: 0,
        })
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn renders_text_to_a_nonempty_rgba_framebuffer() {
        let Some(mut t) = AtermTerminal::new_from_system(24, 80, 16.0) else {
            // No system font available in this environment; skip rather than fail.
            eprintln!("no system font; skipping render test");
            return;
        };
        t.process(b"\x1b[1;32mhello\x1b[0m world\r\nsecond line");
        t.render();
        assert!(t.width() > 0 && t.height() > 0, "frame has dimensions");
        let rgba = t.rgba();
        assert_eq!(rgba.len(), t.width() * t.height() * 4, "RGBA8 buffer size");
        // Some pixel must differ from the top-left (background) pixel — i.e. glyphs
        // were actually rasterized, not a blank frame.
        let bg = &rgba[0..4];
        assert!(
            rgba.chunks_exact(4).any(|px| px != bg),
            "rendered glyphs should produce non-background pixels"
        );
    }

    #[test]
    fn scrolls_into_scrollback_and_extracts_a_selection() {
        let Some(mut t) = AtermTerminal::new_from_system(24, 80, 16.0) else {
            eprintln!("no system font; skipping scroll/select test");
            return;
        };
        for i in 0..200 {
            t.process(format!("line {i}\r\n").as_bytes());
        }
        assert_eq!(t.display_offset(), 0, "live output stays at the bottom");
        t.scroll_lines(40);
        assert_eq!(t.display_offset(), 40, "scrolling up reveals older history");
        t.scroll_to_bottom();
        assert_eq!(t.display_offset(), 0, "scroll_to_bottom snaps back to live");
        t.selection_start(0, 0);
        t.selection_extend(1, 4);
        t.selection_finish();
        let selected = t.selection_text().expect("a drag selects text");
        assert!(!selected.is_empty(), "selection should not be empty");
    }
}
