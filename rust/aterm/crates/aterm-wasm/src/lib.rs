// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

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

use aterm_core::selection::SmartSelection;
use aterm_core::selection::{SelectionSide, SelectionType};
use aterm_core::terminal::{CursorStyle, MouseMode, Rgb, Terminal};
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
    // Built-in smart-selection rules (url/file_path/email/...) for scroll-correct
    // link detection via smart_word_at; reused across link_at calls.
    smart: SmartSelection,
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl AtermTerminal {
    /// Build a `rows`x`cols` terminal rendered with `font_bytes` (a TTF/OTF) at
    /// `px` cell font-size. `font_bytes` is injected by the host (fetched in JS),
    /// keeping the engine free of filesystem font discovery. `fg`/`bg`/`cursor`/
    /// `selection` are 0x00RRGGBB and seed the renderer's DEFAULT theme colors;
    /// per-cell SGR colors still flow through the grid independently.
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
    ) -> Result<AtermTerminal, String> {
        #[cfg(target_arch = "wasm32")]
        console_error_panic_hook::set_once();
        let theme = Theme {
            fg,
            bg,
            cursor,
            selection,
        };
        let mut renderer = Renderer::from_bytes(font_bytes, px, theme)?;
        // Programming ligatures ON for the in-page renderer (the bundled
        // JetBrains Mono carries =>, !=, === …). Explicit, though Enabled is the
        // default, so the intent survives any future default change.
        renderer.set_text_shaping(aterm_render::TextShapingConfig {
            ligature_mode: aterm_render::LigatureMode::Enabled,
            ..Default::default()
        });
        Ok(Self {
            term: Terminal::new(rows, cols),
            renderer,
            rows: rows as usize,
            cols: cols as usize,
            rgba: Vec::new(),
            width: 0,
            height: 0,
            smart: SmartSelection::with_builtin_rules(),
        })
    }

    /// Feed raw PTY output bytes into the engine.
    pub fn process(&mut self, bytes: &[u8]) {
        self.term.process(bytes);
    }

    /// Inject a broad-coverage (CJK + symbols) fallback face from font bytes, so
    /// glyphs the primary face lacks render real shapes instead of `.notdef` tofu.
    /// The canvas renderer can't read the host filesystem, so the host pushes the
    /// OS font bytes in. No-throw: a bad blob leaves the existing face untouched.
    pub fn set_fallback_font(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.renderer.set_fallback_bytes(bytes)
    }

    /// Inject a colour-emoji (sbix) face from font bytes, driving the existing
    /// ColorEmoji colour path. Same rationale as [`set_fallback_font`]: the host
    /// supplies the OS emoji font. No-throw (the `String` Err surfaces as a
    /// catchable JS exception); a bad blob leaves the slot untouched.
    pub fn set_emoji_font(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.renderer.set_color_font_bytes(bytes.to_vec())
    }

    /// Set an ANSI/indexed palette colour (index 0–255; 0–15 are the 16 ANSI
    /// colours) to RGB components, so the renderer resolves SGR-indexed cell colours
    /// through the host's theme palette instead of the engine's built-in VGA
    /// defaults. Per-cell truecolor SGR still flows independently.
    pub fn set_palette_color(&mut self, index: u8, r: u8, g: u8, b: u8) {
        self.term.set_palette_color_components(index, r, g, b);
    }

    /// Set the cursor blink phase: `true` draws the cursor this frame, `false`
    /// hides it. The host drives a ~530ms blink timer; independent of DECSCUSR.
    pub fn set_cursor_blink_phase(&mut self, on: bool) {
        self.renderer.set_cursor_blink_phase(on);
    }

    /// Force a hollow (unfocused) cursor when `true`, or restore the terminal's
    /// DECSCUSR style when `false` — the standard focused/unfocused affordance.
    pub fn set_cursor_hollow(&mut self, hollow: bool) {
        self.renderer
            .set_cursor_style_override(if hollow { Some(CursorStyle::HollowBlock) } else { None });
    }

    /// Drain the engine's pending query replies (DA1/DA2/DSR/CPR/DECRQM/OSC color/
    /// window-size, …) — the host forwards these to the PTY so the RENDERER (not the
    /// daemon, which stays silent) is the authoritative responder. Call after each
    /// `process`; returns `None` when nothing is pending.
    pub fn take_response(&mut self) -> Option<Vec<u8>> {
        self.term.take_response()
    }

    /// Seed the engine's DEFAULT foreground/background so its OSC 10/11 colour-query
    /// replies report the host theme (the engine otherwise reports its built-in
    /// defaults). RGB components, 0–255.
    pub fn set_default_foreground(&mut self, r: u8, g: u8, b: u8) {
        self.term.set_default_foreground(Rgb { r, g, b });
    }

    pub fn set_default_background(&mut self, r: u8, g: u8, b: u8) {
        self.term.set_default_background(Rgb { r, g, b });
    }

    /// Tell the engine the real device-pixel cell size so its CSI 14t/16t
    /// window/cell-pixel reports are accurate (the engine has no canvas otherwise).
    pub fn set_cell_pixel_size(&mut self, width: u16, height: u16) {
        self.term.set_cell_pixel_size(width, height);
    }

    /// Replace the default fg/bg/cursor/selection theme live (0x00RRGGBB), so a host
    /// theme change re-themes the pane without rebuilding it. Per-cell SGR colours
    /// flow independently; pair with set_palette_color for the ANSI palette.
    pub fn set_theme(&mut self, fg: u32, bg: u32, cursor: u32, selection: u32) {
        self.renderer.set_theme(Theme {
            fg,
            bg,
            cursor,
            selection,
        });
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

    /// True when DECCKM (application cursor keys) is set: the host must encode
    /// arrows/Home/End as SS3 (ESC O A) instead of CSI (ESC [ A) so full-screen
    /// apps (vi, less, readline) receive the sequences they expect.
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn is_app_cursor_mode(&self) -> bool {
        self.term.modes().application_cursor_keys()
    }

    /// True when a TUI has enabled mouse tracking (any of DECSET 9/1000/1002/1003).
    /// The host then ENCODES canvas mouse events to the PTY instead of running
    /// selection/scroll/link for them (unless Shift is held = user override).
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn is_mouse_tracking(&self) -> bool {
        self.term.mouse_tracking_enabled()
    }

    /// True when the active mouse mode reports MOTION (ButtonEvent 1002 = drag
    /// while a button is down, AnyEvent 1003 = all motion), so the host only
    /// forwards `mousemove` when an app actually wants it (no spam in 1000).
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn mouse_wants_motion(&self) -> bool {
        matches!(
            self.term.mouse_mode(),
            MouseMode::ButtonEvent | MouseMode::AnyEvent
        )
    }

    /// True for AnyEvent (1003): report motion even with NO button pressed.
    /// 1002 only reports motion while a button is held; the host uses this to
    /// decide whether a button-less `mousemove` should be forwarded.
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn mouse_wants_any_motion(&self) -> bool {
        matches!(self.term.mouse_mode(), MouseMode::AnyEvent)
    }

    /// True when DECSET 1004 (focus reporting) is active: the host sends CSI I on
    /// focus-in and CSI O on focus-out so apps (vim, tmux) track terminal focus.
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn is_focus_event_mode(&self) -> bool {
        self.term.focus_reporting_enabled()
    }

    /// Active DECSCUSR cursor style as the discriminant of `aterm_core`'s
    /// `CursorStyle` (1=BlinkingBlock, 2=SteadyBlock, 3=BlinkingUnderline,
    /// 4=SteadyUnderline, 5=BlinkingBar, 6=SteadyBar, 7=Hidden, 8=HollowBlock).
    /// The CPU renderer ALREADY paints this shape from the grid (cell_frame copies
    /// it into the render input, draw_cursor honors it), so this getter exists for
    /// host introspection/tests — no JS overlay is needed to draw the shape.
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn cursor_style(&self) -> u8 {
        self.term.cursor_style() as u8
    }

    /// Encode a mouse-button PRESS at 0-based on-screen cell `col`/`row` for the
    /// app's active mouse mode+encoding (returns `None`/`undefined` when tracking
    /// is off). `button` is the raw X10 button code (0=left,1=middle,2=right) and
    /// `mods` is the OR of Shift(4)/Alt(8)/Ctrl(16) masks — the engine combines
    /// them. Bytes are sent verbatim to the PTY.
    pub fn encode_mouse_press(&self, col: u16, row: u16, button: u8, mods: u8) -> Option<Vec<u8>> {
        self.term.encode_mouse_press(button, col, row, mods)
    }

    /// Encode a mouse-button RELEASE (see [`AtermTerminal::encode_mouse_press`]);
    /// `None` in X10 press-only mode.
    pub fn encode_mouse_release(
        &self,
        col: u16,
        row: u16,
        button: u8,
        mods: u8,
    ) -> Option<Vec<u8>> {
        self.term.encode_mouse_release(button, col, row, mods)
    }

    /// Encode mouse MOTION at `col`/`row`; `button` is the held button (3 = none).
    /// `None` unless the mode reports motion (1002 while a button is down, 1003
    /// always) — see [`AtermTerminal::mouse_wants_motion`].
    pub fn encode_mouse_motion(&self, col: u16, row: u16, button: u8, mods: u8) -> Option<Vec<u8>> {
        self.term.encode_mouse_motion(button, col, row, mods)
    }

    /// Encode a mouse WHEEL tick at `col`/`row` (`up` = wheel-up); the host sends
    /// these instead of scrolling scrollback while tracking is on. `None` in X10.
    pub fn encode_mouse_wheel(&self, col: u16, row: u16, up: bool, mods: u8) -> Option<Vec<u8>> {
        self.term.encode_mouse_wheel(up, col, row, mods)
    }

    /// Begin a character selection at display `row`/`col` (clears any prior one).
    pub fn selection_start(&mut self, row: i32, col: u16) {
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
        let (start, last) = match self
            .term
            .smart_word_at(row as usize, col as usize, &self.smart)
        {
            Some((s, e)) => (s as u16, e.saturating_sub(1).max(s) as u16),
            None => (col, col),
        };
        let sel = self.term.text_selection_mut();
        sel.start_selection(row, col, SelectionSide::Left, SelectionType::Semantic);
        sel.expand_semantic(start, last);
        sel.complete_selection();
        self.term.selection_to_string()
    }

    /// Select the whole line at display `row` (triple-click) and return its text.
    /// Mirrors aterm-gui's select_line: a Lines selection expanded to the full row
    /// width. `col` is accepted for a uniform host API but unused (whole row).
    pub fn selection_line(&mut self, row: i32, col: u16) -> Option<String> {
        let _ = col;
        let max_col = (self.cols as u16).saturating_sub(1);
        let sel = self.term.text_selection_mut();
        sel.start_selection(row, 0, SelectionSide::Left, SelectionType::Lines);
        sel.expand_lines(max_col);
        sel.complete_selection();
        self.term.selection_to_string()
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

    /// Detect a link under display `row`/`col`. Prefers an OSC-8 hyperlink, then
    /// falls back to smart-selection rules (url/file_path). Returns `None` for
    /// plain words. `kind`: 0=osc8, 1=url, 2=file_path, 3=other.
    pub fn link_at(&self, row: u16, col: u16) -> Option<LinkHit> {
        // OSC-8 lookups are NOT display_offset-aware (only valid at the live
        // bottom), so only consult hyperlink_at when the viewport isn't scrolled.
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

        // Smart-selection fallback is scroll-correct (display_row_text is
        // display_offset-aware) and works on any scrollback row.
        let (sc, ec) = self
            .term
            .smart_word_at(row as usize, col as usize, &self.smart)?;
        let text = self.term.display_row_text(row as usize)?;
        let matched = slice_by_columns(&text, sc, ec);
        let kind = classify(&matched);
        if kind == 3 {
            // A plain word is not a link.
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

    /// Search the full retained buffer (scrollback + visible) for `query`,
    /// returning matches as a flat `[abs_line, start_col, len]` triplet array so
    /// the JS host can highlight + scroll without re-scanning text. Lines are
    /// ABSOLUTE rows (the index's native coordinate); the host maps them to
    /// display rows via [`AtermTerminal::search_display_origin`] /
    /// [`AtermTerminal::scroll_search_line_into_view`], which stay correct as the
    /// viewport scrolls. Empty `query` (or a regex error) yields an empty array.
    pub fn search(&mut self, query: &str, case_sensitive: bool) -> Vec<u32> {
        if query.is_empty() {
            return Vec::new();
        }
        // Reuse the cached full-content index (O(1) on unchanged content); plain
        // substring search (is_regex=false) matches the xterm search default.
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

    /// Absolute row of display row 0 at the live bottom (`display_offset == 0`):
    /// `oldest_absolute_row + scrollback_lines`. A match at absolute `line` is at
    /// display row `line - origin + display_offset`, so the host computes the
    /// on-screen cell of any [`AtermTerminal::search`] match without a round-trip.
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(getter))]
    pub fn search_display_origin(&self) -> u32 {
        let grid = self.term.grid();
        let origin = grid
            .oldest_absolute_row()
            .saturating_add(grid.scrollback_lines() as u64);
        u32::try_from(origin).unwrap_or(u32::MAX)
    }

    /// Scroll the viewport so the match at absolute `line` is visible, placing it
    /// at (or near) the top row. Clamps the target display_offset to the retained
    /// scrollback so a live-region match snaps to the bottom. Host redraws after.
    pub fn scroll_search_line_into_view(&mut self, line: u32) {
        let grid = self.term.grid();
        let origin = grid
            .oldest_absolute_row()
            .saturating_add(grid.scrollback_lines() as u64);
        let scrollback = grid.scrollback_lines();
        let current = grid.display_offset();
        // Target offset that lands `line` on display row 0; clamp to [0, scrollback].
        let want = origin.saturating_sub(u64::from(line));
        let want = (want as usize).min(scrollback);
        // scroll_display takes a delta (positive = older); convert from current.
        let delta = want as i64 - current as i64;
        if let Ok(delta) = i32::try_from(delta) {
            self.term.scroll_display(delta);
        }
    }
}

impl AtermTerminal {
    /// Expand an OSC-8 hyperlink to the span of contiguous cells sharing its
    /// link. Cells group by `id=` when present (OSC 8 grouping), else by URL.
    /// Returns `[start_col, end_col_exclusive)`. Only valid at display_offset 0.
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

/// Slice `text` to the half-open display-column range `[start_col, end_col)`.
/// No `unicode-width` dep here, so we approximate display width as 1 per char —
/// correct for the ASCII URLs/paths that dominate link detection.
fn slice_by_columns(text: &str, start_col: usize, end_col: usize) -> String {
    text.chars()
        .skip(start_col)
        .take(end_col.saturating_sub(start_col))
        .collect()
}

/// Classify a matched span: 1=url (scheme or www. host), 2=file_path (absolute,
/// relative, home, or contains `/` with no scheme), else 3=other (plain word).
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
            smart: SmartSelection::with_builtin_rules(),
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

    #[test]
    fn double_click_selects_whole_word_triple_click_whole_line() {
        let Some(mut t) = AtermTerminal::new_from_system(24, 80, 16.0) else {
            eprintln!("no system font; skipping word/line select test");
            return;
        };
        t.process(b"hello world");
        // Double-click anywhere in "hello" (cols 0..5) selects the WHOLE word,
        // not just the clicked cell — the expand_semantic fix.
        let word = t.selection_word(0, 2).expect("word selection");
        assert_eq!(word, "hello", "double-click selects the full word");
        // Triple-click selects the whole line (trailing blanks trimmed).
        let line = t.selection_line(0, 2).expect("line selection");
        assert_eq!(line, "hello world", "triple-click selects the full line");
    }

    #[test]
    fn search_finds_a_token_in_scrollback_and_scrolls_it_into_view() {
        let Some(mut t) = AtermTerminal::new_from_system(24, 80, 16.0) else {
            eprintln!("no system font; skipping search test");
            return;
        };
        // Push a unique token far into scrollback, then bury it under filler.
        t.process(b"UNIQUE_SEARCH_TOKEN here\r\n");
        for i in 0..200 {
            t.process(format!("filler line {i}\r\n").as_bytes());
        }
        let hits = t.search("UNIQUE_SEARCH_TOKEN", true);
        assert_eq!(
            hits.len(),
            3,
            "exactly one match → one [line,col,len] triple"
        );
        let (line, col, len) = (hits[0], hits[1], hits[2]);
        assert_eq!(col, 0, "token starts at column 0");
        assert_eq!(len, "UNIQUE_SEARCH_TOKEN".len() as u32, "match length");
        // The match is in scrollback, so it is not visible at the live bottom.
        let origin = t.search_display_origin();
        assert!(
            line < origin,
            "token line is above the live viewport origin"
        );
        // Scrolling it into view must move the viewport off the bottom and land
        // the match within the visible rows.
        assert_eq!(t.display_offset(), 0, "starts at the live bottom");
        t.scroll_search_line_into_view(line);
        assert!(t.display_offset() > 0, "viewport scrolled up to the match");
        let display_row = (line as i64) - (origin as i64) + (t.display_offset() as i64);
        assert!(
            (0..24).contains(&display_row),
            "match landed on a visible row, got {display_row}"
        );
        // A case-sensitive miss and an empty query both yield nothing.
        assert!(t.search("unique_search_token", true).is_empty());
        assert!(t.search("", false).is_empty());
    }

    #[test]
    fn tracks_application_cursor_mode_via_decckm() {
        let Some(mut t) = AtermTerminal::new_from_system(24, 80, 16.0) else {
            eprintln!("no system font; skipping app-cursor-mode test");
            return;
        };
        assert!(
            !t.is_app_cursor_mode(),
            "DECCKM defaults off (cursor → CSI)"
        );
        // CSI ? 1 h sets DECCKM (application cursor keys); CSI ? 1 l resets it.
        t.process(b"\x1b[?1h");
        assert!(
            t.is_app_cursor_mode(),
            "DECCKM set → application cursor keys"
        );
        t.process(b"\x1b[?1l");
        assert!(!t.is_app_cursor_mode(), "DECCKM reset → normal cursor keys");
    }

    #[test]
    fn reports_mouse_tracking_and_encodes_sgr_reports() {
        let Some(mut t) = AtermTerminal::new_from_system(24, 80, 16.0) else {
            eprintln!("no system font; skipping mouse-tracking test");
            return;
        };
        // No tracking by default → encoders return None, motion not wanted.
        assert!(!t.is_mouse_tracking(), "mouse tracking defaults off");
        assert!(t.encode_mouse_press(0, 0, 0, 0).is_none(), "no report off");
        assert!(!t.mouse_wants_motion(), "no motion wanted off");
        // DECSET 1000 (normal tracking) + 1006 (SGR encoding).
        t.process(b"\x1b[?1000h\x1b[?1006h");
        assert!(t.is_mouse_tracking(), "1000 enables tracking");
        assert!(!t.mouse_wants_motion(), "1000 does not report motion");
        // Left press at col 4 / row 2 → SGR \e[<0;5;3M (encoders +1 to coords).
        let press = t.encode_mouse_press(4, 2, 0, 0).expect("press encoded");
        assert_eq!(press, b"\x1b[<0;5;3M", "SGR press report");
        let release = t.encode_mouse_release(4, 2, 0, 0).expect("release encoded");
        assert_eq!(release, b"\x1b[<0;5;3m", "SGR release uses lowercase m");
        // Normal mode (1000) reports no motion.
        assert!(
            t.encode_mouse_motion(0, 0, 0, 0).is_none(),
            "1000 no motion"
        );
        // Switch to 1002 (button-event) → motion while a button is held.
        t.process(b"\x1b[?1002h");
        assert!(t.mouse_wants_motion(), "1002 reports drag motion");
        assert!(!t.mouse_wants_any_motion(), "1002 is not any-motion");
        // 1003 (any-event) reports motion with no button held.
        t.process(b"\x1b[?1003h");
        assert!(t.mouse_wants_any_motion(), "1003 reports any motion");
        // Wheel-up → button 64 → SGR \e[<64;...M.
        let wheel = t.encode_mouse_wheel(4, 2, true, 0).expect("wheel encoded");
        assert_eq!(wheel, b"\x1b[<64;5;3M", "SGR wheel-up report");
        // DECRST 1003/1002/1000 clears tracking entirely.
        t.process(b"\x1b[?1003l\x1b[?1002l\x1b[?1000l");
        assert!(
            !t.is_mouse_tracking(),
            "clearing all modes disables tracking"
        );
    }

    #[test]
    fn reports_focus_event_mode_via_decset_1004() {
        let Some(mut t) = AtermTerminal::new_from_system(24, 80, 16.0) else {
            eprintln!("no system font; skipping focus-mode test");
            return;
        };
        assert!(!t.is_focus_event_mode(), "focus reporting defaults off");
        t.process(b"\x1b[?1004h");
        assert!(t.is_focus_event_mode(), "1004 enables focus reporting");
        t.process(b"\x1b[?1004l");
        assert!(
            !t.is_focus_event_mode(),
            "1004 reset disables focus reporting"
        );
    }

    #[test]
    fn tracks_cursor_style_via_decscusr() {
        let Some(mut t) = AtermTerminal::new_from_system(24, 80, 16.0) else {
            eprintln!("no system font; skipping cursor-style test");
            return;
        };
        // DECSCUSR is CSI Ps SP q; Ps=5 → BlinkingBar (discriminant 5), Ps=2 →
        // SteadyBlock (2). The engine paints the shape; we just read it back.
        t.process(b"\x1b[5 q");
        assert_eq!(t.cursor_style(), 5, "DECSCUSR 5 → BlinkingBar");
        t.process(b"\x1b[2 q");
        assert_eq!(t.cursor_style(), 2, "DECSCUSR 2 → SteadyBlock");
    }

    #[test]
    fn detects_a_url_link_under_the_cursor() {
        let Some(mut t) = AtermTerminal::new_from_system(24, 80, 16.0) else {
            eprintln!("no system font; skipping link detection test");
            return;
        };
        t.process(b"https://example.com/foo bar");
        // Column 5 is inside "https://example.com/foo".
        let hit = t.link_at(0, 5).expect("a URL under the cursor is a link");
        assert!(
            hit.kind() == 0 || hit.kind() == 1,
            "expected osc8 or url kind, got {}",
            hit.kind()
        );
        assert!(
            hit.url().contains("example.com"),
            "url should contain the host, got {:?}",
            hit.url()
        );
    }
}
