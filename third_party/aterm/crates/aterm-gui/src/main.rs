// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! `aterm-gui` — a native windowed aterm terminal.
//!
//! A real window (winit) presenting the `aterm-render` CPU framebuffer over a
//! real `$SHELL` in a PTY. A background thread reads the PTY and feeds the
//! engine; the main thread rasterizes the grid and handles keyboard/resize.
//! Per-cell colours and a GPU path come later; this is the first window you can
//! actually launch and use.

use std::collections::VecDeque;
use std::num::NonZeroU32;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use aterm_containment::ContainmentMode;
use aterm_core::bell::{BellFlash, BellRateLimiter};
use aterm_core::selection::{SelectionAnchor, SelectionSide, SelectionState, SelectionType};
use aterm_core::terminal::{
    ClipboardAccess, ClipboardOperation, ColorPalette, CursorStyle, RenderCell, Rgb, Terminal,
};
use aterm_render::{Frame, RenderInput, Renderer, Theme};
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{
    ElementState, Ime, KeyEvent, MouseButton as WinitMouseButton, MouseScrollDelta, StartCause,
    WindowEvent,
};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{CursorIcon, UserAttentionType, Window, WindowId};

mod control;
mod control_auth;
mod keymap;
mod logging;
mod snapshot_path;

const FONT_PX: f32 = 16.0;

/// Cmd-+/Cmd-- live font-zoom step, in physical px; Cmd-0 resets to the launch size.
const FONT_ZOOM_STEP: f32 = 2.0;
/// Clamp for the live font size (matches the $ATERM_FONT_PX bounds).
const FONT_PX_MIN: f32 = 6.0;
const FONT_PX_MAX: f32 = 200.0;

/// User config file (`$XDG_CONFIG_HOME/aterm/aterm.toml`, else
/// `~/.config/aterm/aterm.toml`). Every field is optional; unknown keys are
/// ignored (forward-compatible). Precedence at startup is env var > config >
/// built-in default, so existing `ATERM_*` usage and `-e`/`-d` flags still win.
/// v1 exposes the settings that were previously env-only; it will grow to mirror
/// the engine's `TerminalConfig` (colours, cursor, scrollback) as themes land.
#[derive(Default, serde::Deserialize)]
#[serde(default)]
struct Config {
    /// Glyph size in physical px (like `$ATERM_FONT_PX`).
    font_px: Option<f32>,
    /// GPU (Metal) rendering (like `$ATERM_GPU`).
    gpu: Option<bool>,
    /// Scrollback history limit, in lines (engine `TerminalConfig.scrollback_limit`;
    /// default 100 000). 0 means unlimited (bounded only by the memory budget).
    scrollback_lines: Option<usize>,
    /// Cursor shape: `"block"` (default), `"underline"`, or `"bar"` (alias `"beam"`).
    cursor_style: Option<String>,
    /// Whether the cursor blinks (default true). Combined with `cursor_style`.
    cursor_blink: Option<bool>,
    /// Theme: default text colour, `#RRGGBB` (engine `default_foreground`).
    foreground: Option<String>,
    /// Theme: default background colour, `#RRGGBB` (engine `default_background`).
    background: Option<String>,
    /// Theme: cursor colour, `#RRGGBB` (engine `cursor_color`).
    cursor_color: Option<String>,
    /// Theme: selection-highlight colour, `#RRGGBB` (renderer `Theme.selection`).
    selection_color: Option<String>,
    /// Theme: indexed palette colours, `#RRGGBB`, by 0-based index (0–15 are the
    /// ANSI/bright set; up to 256). e.g. `palette = ["#1d1f21", "#cc6666", …]`.
    palette: Option<Vec<String>>,
    /// Initial window width in columns (default 80, clamped 20..=500).
    columns: Option<u16>,
    /// Initial window height in rows (default 24, clamped 5..=300).
    lines: Option<u16>,
}

impl Config {
    /// The RENDERER theme (window clear colour, cursor, selection highlight) from
    /// config, falling back to built-in defaults. fg/bg/cursor mirror the engine
    /// theme so the window CLEAR colour (areas past the last cell) matches a
    /// configured `background`, and `selection_color` themes the highlight.
    fn theme(&self) -> Theme {
        let mut t = Theme::default();
        let u = |c: Rgb| (u32::from(c.r) << 16) | (u32::from(c.g) << 8) | u32::from(c.b);
        if let Some(c) = self.foreground.as_deref().and_then(parse_hex_color) {
            t.fg = u(c);
        }
        if let Some(c) = self.background.as_deref().and_then(parse_hex_color) {
            t.bg = u(c);
        }
        if let Some(c) = self.cursor_color.as_deref().and_then(parse_hex_color) {
            t.cursor = u(c);
        }
        if let Some(c) = self.selection_color.as_deref().and_then(parse_hex_color) {
            t.selection = u(c);
        }
        t
    }
}

/// Parse a `#RRGGBB` (or bare `RRGGBB`) hex colour; `None` on malformed input.
fn parse_hex_color(s: &str) -> Option<Rgb> {
    let h = s.trim();
    let h = h.strip_prefix('#').unwrap_or(h);
    if h.len() != 6 || !h.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some(Rgb::new(
        u8::from_str_radix(&h[0..2], 16).ok()?,
        u8::from_str_radix(&h[2..4], 16).ok()?,
        u8::from_str_radix(&h[4..6], 16).ok()?,
    ))
}

impl Config {
    /// Build the engine `TerminalConfig` deltas this config implies, or `None`
    /// when nothing engine-side is set (so the GUI skips `apply_config`).
    fn terminal_config(&self) -> Option<aterm_core::config::TerminalConfig> {
        let mut tc = aterm_core::config::TerminalConfig::default();
        let mut any = false;
        if let Some(n) = self.scrollback_lines {
            // 0 → unlimited (None); N → cap at N lines.
            tc.scrollback_limit = (n != 0).then_some(n);
            any = true;
        }
        if self.cursor_style.is_some() || self.cursor_blink.is_some() {
            let blink = self.cursor_blink.unwrap_or(true);
            tc.cursor_style = match self.cursor_style.as_deref().unwrap_or("block") {
                "block" if blink => CursorStyle::BlinkingBlock,
                "block" => CursorStyle::SteadyBlock,
                "underline" if blink => CursorStyle::BlinkingUnderline,
                "underline" => CursorStyle::SteadyUnderline,
                "bar" | "beam" if blink => CursorStyle::BlinkingBar,
                "bar" | "beam" => CursorStyle::SteadyBar,
                other => {
                    eprintln!(
                        "aterm-gui: config cursor_style: expected block|underline|bar, got {other:?}"
                    );
                    if blink { CursorStyle::BlinkingBlock } else { CursorStyle::SteadyBlock }
                }
            };
            tc.cursor_blink = blink;
            any = true;
        }
        // Theme colours → engine `default_*`/`cursor_color`. The engine resolves
        // these into each `RenderCell.fg/bg`, so this is NOT a renderer change.
        for (key, raw, slot) in [
            ("foreground", &self.foreground, 0u8),
            ("background", &self.background, 1),
            ("cursor_color", &self.cursor_color, 2),
        ] {
            if let Some(s) = raw {
                match parse_hex_color(s) {
                    Some(rgb) => {
                        match slot {
                            0 => tc.default_foreground = rgb,
                            1 => tc.default_background = rgb,
                            _ => tc.cursor_color = Some(rgb),
                        }
                        any = true;
                    }
                    None => eprintln!("aterm-gui: config {key}: expected #RRGGBB, got {s:?}"),
                }
            }
        }
        // Indexed palette (engine `custom_palette`; also resolved into RenderCell).
        if let Some(entries) = &self.palette {
            let mut pal = ColorPalette::new();
            let mut ok = false;
            for (i, hex) in entries.iter().take(256).enumerate() {
                match parse_hex_color(hex) {
                    Some(rgb) => {
                        pal.set(i as u8, rgb);
                        ok = true;
                    }
                    None => eprintln!("aterm-gui: config palette[{i}]: expected #RRGGBB, got {hex:?}"),
                }
            }
            if ok {
                tc.custom_palette = Some(pal);
                any = true;
            }
        }
        any.then_some(tc)
    }
}

/// Resolve the config file path without creating anything.
fn config_path() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    if let Some(x) = std::env::var_os("XDG_CONFIG_HOME").filter(|x| !x.is_empty()) {
        return Some(PathBuf::from(x).join("aterm").join("aterm.toml"));
    }
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config/aterm/aterm.toml"))
}

/// Load the user config. A missing file is fine (defaults); a malformed file is
/// reported and ignored rather than aborting the launch.
fn load_config() -> Config {
    let Some(path) = config_path() else {
        return Config::default();
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Config::default(); // not present / unreadable → defaults
    };
    toml::from_str(&text).unwrap_or_else(|e| {
        eprintln!("aterm-gui: ignoring invalid config {}: {e}", path.display());
        Config::default()
    })
}

/// Security allowlist for opening a hyperlink that arrived in (untrusted) program
/// output via Cmd-click: ONLY `http`/`https`/`mailto`. Rejects `file://`, custom
/// app schemes (`x-apple-…`, `tel:`, …), and anything with control bytes or
/// whitespace — so `open` can never be steered into launching an app or reaching
/// the local filesystem from hostile terminal output. (OSC 8 hyperlinks are also
/// already authorization-gated in the engine; this is the second gate, at the
/// point of action.)
fn is_safe_url(url: &str) -> bool {
    let u = url.trim();
    if u.is_empty() || u.bytes().any(|b| b.is_ascii_control() || b == b' ') {
        return false;
    }
    let l = u.to_ascii_lowercase();
    l.starts_with("http://") || l.starts_with("https://") || l.starts_with("mailto:")
}

/// Find a plain-text `http(s)://` URL covering position `col` in a row of chars.
/// Returns `(url, start_col, end_col)` with INCLUSIVE column bounds. Scans for a
/// scheme start, extends over URL-permitted ASCII, trims trailing sentence
/// punctuation (so `(see http://x.com).` yields `http://x.com`), and checks `col`
/// is inside. Pure over `&[char]` so it is unit-testable; `plain_url_at` adapts a
/// `RenderCell` row to it.
fn find_url_span(chars: &[char], col: usize) -> Option<(String, usize, usize)> {
    let n = chars.len();
    let is_url = |c: char| c.is_ascii_alphanumeric() || "-._~:/?#[]@!$&'()*+,;=%".contains(c);
    let mut i = 0;
    while i < n {
        let s = &chars[i..];
        if s.starts_with(&['h', 't', 't', 'p', ':', '/', '/'])
            || s.starts_with(&['h', 't', 't', 'p', 's', ':', '/', '/'])
        {
            let mut j = i;
            while j < n && is_url(chars[j]) {
                j += 1;
            }
            let mut end = j;
            while end > i
                && matches!(chars[end - 1], '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '\'' | '"')
            {
                end -= 1;
            }
            if (i..end).contains(&col) {
                return Some((chars[i..end].iter().collect(), i, end - 1));
            }
            i = j.max(i + 1);
        } else {
            i += 1;
        }
    }
    None
}

/// Plain-text URL covering cell `col` in a rendered row (one cell = one char;
/// wide-continuation cells read as a space, which breaks a run as it should).
fn plain_url_at(cells: &[RenderCell], col: usize) -> Option<(String, usize, usize)> {
    let chars: Vec<char> = cells.iter().map(|c| if c.wide { ' ' } else { c.ch }).collect();
    find_url_span(&chars, col)
}

/// Case-insensitive, non-overlapping matches of `query` in each `(row, text)`
/// line (rows are selection coords: 0..rows = live screen, negative = scrollback).
/// Returns `(row, start_col, end_col)` (INCLUSIVE columns) per match, in the order
/// the lines are given. Column = char index (v1: one column per char; wide chars
/// not adjusted). Empty query / no match → empty. Pure, so it is unit-testable.
fn find_line_matches(lines: &[(i32, String)], query: &str) -> Vec<(i32, u16, u16)> {
    let q: Vec<char> = query.to_lowercase().chars().collect();
    if q.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (row, text) in lines {
        let hay: Vec<char> = text.to_lowercase().chars().collect();
        if hay.len() < q.len() {
            continue;
        }
        let mut i = 0;
        while i + q.len() <= hay.len() {
            if hay[i..i + q.len()] == q[..] {
                out.push((*row, i as u16, (i + q.len() - 1) as u16));
                i += q.len(); // non-overlapping
            } else {
                i += 1;
            }
        }
    }
    out
}

/// (Re)build the [`Backend`] at `px` for live font-zoom. When `use_gpu`, rebuilds
/// the GPU renderer — it renders offscreen and the window's GPU surface is
/// re-created separately by `on_resize`, so re-creating the renderer mid-session
/// is the same proven call as startup. A GPU failure or a missing system font
/// yields `None`, and the caller keeps the current backend (zoom is a no-op
/// rather than a crash).
fn build_backend(px: f32, use_gpu: bool, theme: Theme) -> Option<Backend> {
    if use_gpu {
        if let Ok(g) = aterm_gpu::GpuRenderer::new(px, theme) {
            return Some(Backend::Gpu(g));
        }
    }
    Renderer::from_system(px, theme).map(Backend::Cpu)
}

/// Open a new window: launch a fresh, independent `aterm-gui` process (the macOS
/// "new window", Cmd-N). It inherits the environment + working directory and runs
/// its own event loop / shell; a spawn failure is logged, never fatal to the
/// current window. Detached — we do not wait on it.
fn open_new_window() {
    match std::env::current_exe() {
        Ok(exe) => {
            if let Err(e) = std::process::Command::new(exe).spawn() {
                eprintln!("aterm-gui: could not open a new window: {e}");
            }
        }
        Err(e) => eprintln!("aterm-gui: could not resolve own path for a new window: {e}"),
    }
}

/// Half-period of the cursor blink: a `Blinking*` DECSCUSR cursor toggles
/// on/off every this long (the classic terminal cadence).
const BLINK_INTERVAL: Duration = Duration::from_millis(530);

/// Multi-click window: a left press within this many milliseconds of the
/// previous press, in the SAME cell, advances the click count (1 -> 2 -> 3,
/// then wraps back to 1).
const MULTI_CLICK_MS: u128 = 500;

/// Audible-bell floor: at most one beep per this interval, however fast BEL
/// floods (the visual flash still re-arms on every bell).
const BELL_BEEP_INTERVAL: Duration = Duration::from_secs(1);

/// Lock the shared [`Terminal`], recovering from a poisoned mutex.
///
/// A panic on the renderer/PTY-reader/control thread poisons this lock; the
/// previous `.unwrap()` then panicked every other thread that touched it and
/// killed the whole session. A possibly mid-update grid beats process death
/// here: the next `process()`/redraw pass repaints a consistent screen while
/// the user's foreground job keeps running. Warns once — not per acquisition;
/// the lock stays poisoned forever — so recovery is visible without flooding.
pub(crate) fn term_lock(term: &Mutex<Terminal>) -> MutexGuard<'_, Terminal> {
    term.lock().unwrap_or_else(|poisoned| {
        static WARNED: AtomicBool = AtomicBool::new(false);
        if !WARNED.swap(true, Ordering::Relaxed) {
            aterm_log::warn!("Terminal mutex poisoned by a panicked thread; recovering");
        }
        poisoned.into_inner()
    })
}

/// Origin unit of a double/triple-click gesture: the word (or line) the
/// gesture started on, kept while the button stays down so a drag extends the
/// selection by whole words/lines with the origin unit always fully selected.
#[derive(Debug, Clone, Copy)]
struct GestureOrigin {
    /// Live-screen selection row of the original word/line.
    row: i32,
    /// Inclusive first column of the original unit (0 for a line).
    start_col: u16,
    /// Inclusive last column of the original unit (`cols-1` for a line).
    end_col: u16,
    /// `Semantic` (double-click, by words) or `Lines` (triple-click, by rows).
    kind: SelectionType,
}

/// A cheap fingerprint of the text selection, for the redraw early-out (D-1).
///
/// The grid's damage tracker does NOT see selection changes (they mutate
/// `Terminal::text_selection`, not the grid), yet the selection IS painted, so
/// a selection change must still force a repaint. Comparing this `Copy`,
/// `PartialEq` fingerprint of the selection's state machine captures exactly
/// that. `SelectionAnchor`/`SelectionState`/`SelectionType` are all `PartialEq`.
#[derive(Clone, Copy, PartialEq)]
struct SelectionFingerprint {
    state: SelectionState,
    kind: SelectionType,
    start: SelectionAnchor,
    end: SelectionAnchor,
}

impl SelectionFingerprint {
    /// Fingerprint a live selection (read from a [`RenderInput`] or a `Terminal`).
    fn of(sel: &aterm_core::selection::TextSelection) -> Self {
        SelectionFingerprint {
            state: sel.state(),
            kind: sel.selection_type(),
            start: sel.start(),
            end: sel.end(),
        }
    }
}

/// Everything that affects the PRESENTED frame which the grid damage tracker
/// does not already cover, plus the grid's own monotonic damage epoch (D-1).
///
/// The redraw early-out compares the key for the current frame against the key
/// recorded at the last real present: if they are equal, nothing the user can
/// see has changed, so the whole extract + rasterize + present is skipped. The
/// `damage_epoch` term covers every grid mutation (writes, scroll, erase,
/// resize); the other terms cover purely-visual state the grid never tracks —
/// the cursor blink phase, the bell-flash invert, the unfocused cursor-style
/// override, and the text selection.
#[derive(Clone, Copy, PartialEq)]
struct RepaintKey {
    /// `Terminal::damage_epoch()` — advances on any net-new grid damage.
    damage_epoch: u64,
    /// Cursor blink phase pushed to the renderer (a flip toggles the cursor).
    blink_phase: bool,
    /// Visual-bell invert active for this frame (toggles the whole frame).
    invert: bool,
    /// Unfocused cursor-style override (hollow block), if any.
    cursor_override: Option<CursorStyle>,
    /// The active text selection fingerprint.
    selection: SelectionFingerprint,
}

/// The redraw early-out decision (D-1), as a PURE function so it is unit
/// testable without a window/event loop.
///
/// Returns `true` (must repaint) iff this is the first frame (`prev` is `None`)
/// or any presented-state term changed since the last present. Returns `false`
/// (skip the extract + rasterize + present) only when the previously presented
/// key is byte-identical to the current one — i.e. a steady screen with the same
/// blink phase, no bell flash, the same selection and cursor override. This is
/// what eliminates the steady-screen and blink-only-wake full-frame redraws.
fn should_repaint(prev: Option<RepaintKey>, cur: RepaintKey) -> bool {
    prev != Some(cur)
}

/// Wake the UI when the PTY produced output, exited, a screen snapshot was
/// requested (SIGUSR1), or the control socket needs the renderer (`Control`) —
/// the latter two are aterm introspecting itself: it renders its CURRENT live
/// screen to a PNG (pixels) + a .txt (text), so an intelligence can "see"
/// exactly what is on the terminal without any OS screen-recording.
#[derive(Debug, Clone, Copy)]
enum Wake {
    Output,
    Exit,
    Snapshot,
    /// The engine saw BEL (0x07): flash the frame, beep (rate-limited), and
    /// request user attention when the window is unfocused.
    Bell,
    /// The control thread queued one or more `ImageReq`s and needs the main
    /// thread (which owns the renderer) to render and reply.
    Control,
    /// RES-1: the control socket's `resize` verb asked for a new geometry. The
    /// main thread is the SOLE geometry owner (`App.rows/cols`, the framebuffer,
    /// the window), so the verb must NOT resize the term/PTY directly — that
    /// leaves `App` stale and sends no repaint, so a follow-up `image`/`dims`
    /// disagrees with the engine. Routed here so the main thread applies the
    /// term + PTY + window resize and requests a redraw, all in one owner.
    Resize { rows: u16, cols: u16 },
}

/// The single resident renderer (ATERM_DESIGN WS-F): EXACTLY ONE is held — the
/// GPU `GpuRenderer` (wgpu/Metal) when `ATERM_GPU` is live, else the CPU
/// `Renderer`. A concrete enum (not `Box<dyn Rasterizer>`) so the GPU variant can
/// expose its on-glass present API (`present_input`/surface management) that the
/// trait can't, while keeping the single-font-cache property — only one renderer,
/// hence one glyph cache, is ever built.
///
/// The inherent methods below mirror the `Rasterizer` calls the frontend uses, so
/// call sites stay branch-free; `is_gpu`/`gpu_mut` reach the GPU-only present path.
enum Backend {
    Cpu(Renderer),
    Gpu(aterm_gpu::GpuRenderer),
}

impl Backend {
    /// Pixel size of one cell, `(width, height)` — from the live renderer.
    fn cell_size(&self) -> (usize, usize) {
        match self {
            Backend::Cpu(r) => r.cell_size(),
            Backend::Gpu(g) => g.cell_size(),
        }
    }

    /// Push the cursor blink phase (`on` = solid) into the renderer's state.
    fn set_cursor_blink_phase(&mut self, on: bool) {
        match self {
            Backend::Cpu(r) => r.set_cursor_blink_phase(on),
            Backend::Gpu(g) => g.set_cursor_blink_phase(on),
        }
    }

    /// Override the rendered cursor style regardless of DECSCUSR; `None` clears.
    fn set_cursor_style_override(&mut self, style: Option<CursorStyle>) {
        match self {
            Backend::Cpu(r) => r.set_cursor_style_override(style),
            Backend::Gpu(g) => g.set_cursor_style_override(style),
        }
    }

    /// Render a frame offscreen and read it back into an owned [`Frame`]. Used by
    /// the snapshot + `image` introspection paths on BOTH backends (not the hot
    /// path). The per-frame CPU window present instead calls `Renderer`'s
    /// `render_input_into` directly (reusing the renderer's buffer, no per-frame
    /// pixel alloc — C-1), and the GPU window present uses `present_input`.
    fn render_input(&mut self, input: &RenderInput) -> Frame {
        match self {
            Backend::Cpu(r) => r.render_input(input),
            Backend::Gpu(g) => g.render_input(input),
        }
    }

    /// Whether the GPU on-glass present path is active.
    fn is_gpu(&self) -> bool {
        matches!(self, Backend::Gpu(_))
    }

    /// The GPU renderer, for the on-glass surface + present calls; `None` on CPU.
    fn gpu_mut(&mut self) -> Option<&mut aterm_gpu::GpuRenderer> {
        match self {
            Backend::Gpu(g) => Some(g),
            Backend::Cpu(_) => None,
        }
    }
}

struct App {
    term: Arc<Mutex<Terminal>>,
    /// The single resident renderer (CPU or GPU) — see [`Backend`]. EXACTLY ONE
    /// is held: the GPU `GpuRenderer` (wgpu/Metal) when `ATERM_GPU` is live (and
    /// initializes), else the CPU `Renderer`. The CPU path presents via softbuffer
    /// and the GPU path blits straight to the swapchain (`present_input`).
    backend: Backend,
    /// Current and launch-default font size (physical px), for live Cmd-+/-/0 zoom.
    font_px: f32,
    default_font_px: f32,
    /// Whether the live backend is the GPU one, so a zoom rebuilds the same kind.
    use_gpu: bool,
    /// The configured renderer theme, re-applied when a font-zoom rebuilds the backend.
    theme: Theme,
    master: i32,
    rows: u16,
    cols: u16,
    mods: ModifiersState,
    /// Last cell (row, col) the cursor moved over, updated on `CursorMoved` and
    /// used to position mouse button/wheel reports (winit delivers the pointer
    /// position on motion, not on click/scroll).
    last_mouse_cell: (u16, u16),
    /// Whether the OS cursor is currently the link "pointer" (Cmd-hovering a link),
    /// so `set_cursor` is only called on a state change, not every mouse move.
    hover_pointer: bool,
    /// True while a left-button drag is building a text selection (only when
    /// no app is tracking the mouse). Cleared on release.
    selecting: bool,
    /// Whether the pointer left the press cell during the current drag: a
    /// press+release within one cell is a plain click, which deselects.
    sel_dragged: bool,
    /// Live-screen selection cell of the initial press (row may be negative
    /// when the press lands in scrollback), for click-vs-drag detection.
    sel_press_cell: (i32, u16),
    /// Instant + live-screen cell of the last left press, for multi-click
    /// (double/triple-click) detection.
    last_press: Option<(Instant, (i32, u16))>,
    /// Click count of the current multi-click streak: 1 = single (simple
    /// drag), 2 = double (word selection), 3 = triple (line selection); a
    /// fourth rapid click wraps back to 1.
    click_count: u8,
    /// Which half of the cell the pointer was last in (updated on
    /// `CursorMoved`): the anchor side for drag updates and shift-click
    /// extension — Right includes the hovered cell, Left stops before it.
    last_mouse_side: SelectionSide,
    /// Origin word/line of an in-flight double/triple-click drag, so motion
    /// extends the selection by whole units. `None` for simple/block drags.
    gesture: Option<GestureOrigin>,
    /// When set ($ATERM_HEADLESS), no window/surface is ever created: the
    /// engine, control socket, and offscreen rendering (`image`/snapshot via
    /// [`Wake::Control`]) all run, but nothing is presented on screen.
    headless: bool,
    /// Whether the window currently has keyboard focus (from
    /// `WindowEvent::Focused`). Unfocused: the cursor draws as a hollow block
    /// and blink scheduling stops (the loop stays in `Wait` — 0% idle).
    focused: bool,
    /// Current cursor blink phase: `true` = the cursor is shown. Only consulted
    /// by the renderers for the `Blinking*` DECSCUSR styles.
    blink_phase: bool,
    /// The next blink toggle deadline. Armed ONLY while blinking is active
    /// (focused window + Blinking* style + cursor visible); `None` keeps the
    /// event loop in pure `Wait` so an idle steady/unfocused session burns 0%.
    next_blink: Option<Instant>,
    /// Visual bell: while active the presented frame is inverted; like blink,
    /// its deadline arms `WaitUntil` only while a flash is pending, so the
    /// loop stays in pure `Wait` (0% idle) between bells.
    bell_flash: BellFlash,
    /// Audible bell gate: one beep per [`BELL_BEEP_INTERVAL`] (the engine
    /// already throttles BEL callbacks to 10/s; this slows the sound further).
    bell_beep: BellRateLimiter,
    window: Option<Arc<Window>>,
    /// CPU-mode presentation: the softbuffer surface the CPU `Frame` is copied
    /// into and presented. `None` in GPU mode (which uses `gpu_surface`).
    surface: Option<softbuffer::Surface<Arc<Window>, Arc<Window>>>,
    _context: Option<softbuffer::Context<Arc<Window>>>,
    /// GPU-mode presentation: the wgpu swapchain the offscreen frame is blitted
    /// into and presented on the GPU (no readback, no softbuffer copy). `None` in
    /// CPU mode and until `resumed()` creates the window surface.
    gpu_surface: Option<aterm_gpu::GpuSurface>,
    /// The title currently shown in the window chrome. Mirrors the engine's
    /// program-set title (OSC 0/2); `redraw()` calls `set_title` only when this
    /// diverges, so a program that updates its title (shell cwd, vim, ssh) is
    /// reflected in the titlebar like any real terminal.
    current_title: String,
    /// Shared queue of control-socket `image` requests, drained on
    /// [`Wake::Control`] (the control thread cannot touch the renderer).
    image_queue: control::ImageQueue,
    /// Latency self-introspection ($ATERM_TRACE_LATENCY). When on, the PTY
    /// reader stamps the leading edge of each output burst into `last_output_ns`
    /// (nanos since `lat_epoch`), and `redraw()` logs output→present latency
    /// after `present()` — the software-controllable slice of input-to-photon
    /// (the rest is fixed panel scan-out, identical across every terminal).
    trace_latency: bool,
    lat_epoch: Instant,
    last_output_ns: Arc<AtomicU64>,
    /// Persistent per-frame snapshot buffer (C-1): `redraw()` refills this in
    /// place via `Renderer::extract_into` under the lock instead of allocating a
    /// fresh `RenderInput` every frame, so a steady-size session does no
    /// per-frame heap allocation for the grid snapshot.
    input_scratch: RenderInput,
    /// The [`RepaintKey`] of the last frame actually presented (D-1), or `None`
    /// before the first present. `redraw()` skips the whole extract + rasterize +
    /// present when the current key equals this (see [`should_repaint`]).
    last_present: Option<RepaintKey>,
    /// IME composition (IME-1): the marked/preedit text currently being
    /// composed (CJK input, dead keys), or empty when no composition is active.
    /// While non-empty, direct key sends are SUPPRESSED so composing keystrokes
    /// don't ALSO emit raw bytes; the committed text arrives via `Ime::Commit`.
    /// A minimal inline indicator is rendered (see `preedit_indicator`).
    preedit: String,
    /// Active Cmd-F find: query + matches on the visible screen + current index.
    /// `None` when not searching. While `Some`, keystrokes edit the query instead
    /// of going to the PTY.
    search: Option<SearchState>,
    /// Set by Cmd-W (close window) in `on_key`; `window_event` exits the loop after
    /// the handler returns (on_key has no `ActiveEventLoop` to call `el.exit()`).
    should_exit: bool,
}

/// In-progress Cmd-F find over the live screen + recent scrollback. Matches are
/// `(row, start_col, end_col)` in SELECTION coordinates (0..rows = live screen,
/// negative = scrollback); the current one is highlighted by setting the text
/// selection (the existing overlay — no renderer change) and scrolled into view.
#[derive(Default)]
struct SearchState {
    query: String,
    matches: Vec<(i32, u16, u16)>,
    current: usize,
}

/// How many scrollback lines back a Cmd-F find scans (plus the live screen). Bounds
/// per-keystroke cost and keeps the scan in the fast hot/warm tiers, not the disk
/// tier. Deep-history search beyond this is a follow-up.
const MAX_SEARCH_HISTORY: i32 = 5000;

impl App {
    /// Whether the cursor should be blinking RIGHT NOW: a real focused window,
    /// a `Blinking*` DECSCUSR style, and a visible cursor. Anything else (steady
    /// style, unfocused, hidden, headless) must leave the loop in pure `Wait`.
    fn blink_active(&self) -> bool {
        if self.headless || self.window.is_none() || !self.focused {
            return false;
        }
        let term = term_lock(&self.term);
        term.cursor_visible()
            && matches!(
                term.cursor_style(),
                CursorStyle::BlinkingBlock
                    | CursorStyle::BlinkingUnderline
                    | CursorStyle::BlinkingBar
            )
    }

    /// The glyph cell size in pixels, from the live rasterizer (GPU's internal
    /// CPU face, or the standalone CPU renderer).
    fn cell_size(&self) -> (usize, usize) {
        self.backend.cell_size()
    }

    /// Push the current blink phase into the rasterizer.
    fn sync_blink_phase(&mut self) {
        self.backend.set_cursor_blink_phase(self.blink_phase);
    }

    /// Force the blink phase ON (cursor solid) and restart the blink period —
    /// the standard "cursor is solid while you type" behavior. Repaints only
    /// if the phase actually changed.
    fn reset_blink(&mut self) {
        if self.next_blink.is_some() {
            self.next_blink = Some(Instant::now() + BLINK_INTERVAL);
        }
        if !self.blink_phase {
            self.blink_phase = true;
            self.sync_blink_phase();
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }
    }

    /// Focus change: an unfocused window draws the cursor as a steady hollow
    /// block regardless of DECSCUSR (standard terminal behavior) and stops
    /// blink scheduling; regaining focus restores the app's style and re-arms
    /// the blink (via `about_to_wait`).
    fn on_focus(&mut self, focused: bool) {
        self.focused = focused;
        // Focus reporting (DEC mode 1004): apps that opted in (vim `autoread`,
        // tmux, neovim) expect ESC[I on focus-in and ESC[O on focus-out. Sent
        // only while reporting is enabled — these bytes match the engine's
        // `encode_focus_state`.
        if term_lock(&self.term).focus_reporting_enabled() {
            self.write_pty(if focused { b"\x1b[I" } else { b"\x1b[O" });
        }
        let over = (!focused).then_some(CursorStyle::HollowBlock);
        self.backend.set_cursor_style_override(over);
        self.blink_phase = true;
        self.sync_blink_phase();
        self.next_blink = None;
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    /// BEL reached the engine: audible beep (rate-limited), visual flash
    /// (repaint now; `about_to_wait` arms the un-flash wake), and — unfocused —
    /// ask the OS to mark the window urgent (Dock bounce / taskbar highlight),
    /// the tmux bell-on-activity flow.
    fn on_bell(&mut self) {
        let now = Instant::now();
        if self.bell_beep.try_fire(now) {
            // The user's configured macOS alert sound. AppKit is already
            // in-process (winit); safe to call from the main thread.
            #[cfg(target_os = "macos")]
            unsafe {
                objc2_app_kit::NSBeep();
            }
        }
        if let Some(w) = &self.window {
            self.bell_flash.ring(now);
            w.request_redraw();
            if !self.focused {
                w.request_user_attention(Some(UserAttentionType::Informational));
            }
        }
    }

    /// Reflect the program-set title (OSC 0/2) in the window chrome, falling back
    /// to "aterm" when nothing has set one. Calls `set_title` only on an actual
    /// change (a cheap String compare), so it is safe to call every frame — even
    /// on the redraw early-out path, where a title-only change still updates the
    /// titlebar without a pixel repaint.
    ///
    /// IME-1: while a composition is in flight, the marked preedit text is shown
    /// as `title [‹preedit›]` — the minimal inline indicator that an
    /// IME/dead-key composition is active and what it currently holds. Because
    /// this runs on the early-out path too, the indicator follows the
    /// composition without forcing a full pixel repaint.
    fn apply_title(&mut self, window: &Window, title: &str) {
        let base = if title.is_empty() { "aterm" } else { title };
        let desired = if self.preedit.is_empty() {
            base.to_string()
        } else {
            format!("{base} [‹{}›]", self.preedit)
        };
        if desired != self.current_title {
            window.set_title(&desired);
            self.current_title.clear();
            self.current_title.push_str(&desired);
        }
    }

    fn redraw(&mut self) {
        let Some(window) = self.window.clone() else {
            return;
        };
        // No present target yet (surface not created): nothing to draw into, and
        // we must NOT consume damage, so bail before touching the lock.
        if self.backend.is_gpu() {
            if self.gpu_surface.is_none() {
                return;
            }
        } else if self.surface.is_none() {
            return;
        }
        let (rows, cols) = (self.rows as usize, self.cols as usize);
        // Visual bell: the presented frame has its RGB inverted while a flash is
        // active. The flash state machine decides "active"; `about_to_wait` wakes
        // the loop at its deadline so the normal frame returns.
        let invert = self.bell_flash.is_active(Instant::now());
        // Unfocused windows force a hollow cursor (mirrors `on_focus`); part of
        // the visual state the grid damage tracker doesn't see.
        let cursor_override = (!self.focused).then_some(CursorStyle::HollowBlock);

        // D-1 early-out. Hold the Terminal mutex only long enough to read the
        // damage epoch + selection + title and, IF we decide to repaint, refill
        // the persistent RenderInput in place and consume the damage — all
        // atomically so no PTY damage is dropped. The early-out compares this
        // frame's RepaintKey to the last presented one: a steady screen with the
        // same blink phase / bell-flash / selection / focus skips the entire
        // extract + rasterize + present (the coarse screen-level skip, on top of
        // the renderer's own row-level damage cache in `render_input_cached`).
        let title = {
            let mut term = term_lock(&self.term);
            let key = RepaintKey {
                damage_epoch: term.damage_epoch(),
                blink_phase: self.blink_phase,
                invert,
                cursor_override,
                selection: SelectionFingerprint::of(term.text_selection()),
            };
            let title = term.title().to_string();
            if !should_repaint(self.last_present, key) {
                // Nothing visible changed since the last present. Drop the lock,
                // refresh only the window chrome (a title-only change needs no
                // pixel repaint), and skip the frame entirely.
                drop(term);
                self.apply_title(&window, &title);
                return;
            }
            // We are committing to present this frame: REFILL the reused snapshot
            // in place (no per-frame container-Vec alloc when dims are stable) and
            // consume the damage under the SAME lock; render after the guard drops.
            aterm_render::Renderer::extract_into(&mut self.input_scratch, &term, rows, cols);
            term.take_damage();
            self.last_present = Some(key);
            title
        };
        // Reflect the program-set title (OSC 0/2) in the window chrome, falling
        // back to "aterm" when nothing has set one. Only calls set_title on an
        // actual change (a cheap String compare on the already-unlocked path).
        self.apply_title(&window, &title);

        if self.backend.is_gpu() {
            // GPU on-glass present: render the offscreen frame (the single source
            // of truth) and BLIT it straight into the swapchain — no Frame, no
            // softbuffer copy, no GPU->CPU readback. The blit shader applies the
            // visual-bell invert. The same offscreen texture is what the
            // snapshot/`image` introspection reads back, so screen == introspection.
            //
            // Disjoint field borrows: the input snapshot (`&self.input_scratch`),
            // the backend (`&mut self.backend` via `gpu_mut`), and the GPU surface
            // (`&mut self.gpu_surface`) are SEPARATE fields, so the borrow checker
            // permits all three at once with no aliasing.
            let input = &self.input_scratch;
            if let (Some(gpu), Some(gpu_surface)) =
                (self.backend.gpu_mut(), self.gpu_surface.as_mut())
            {
                gpu.present_input(gpu_surface, input, invert);
            } else {
                return;
            }
        } else {
            // CPU present: rasterize via the renderer's damage-tracked cache and
            // take a BORROW of the framebuffer (`render_input_cached`) rather than
            // an owned `Frame` — eliding the per-frame cache→Frame clone — then
            // copy it into the softbuffer surface, applying the visual-bell invert
            // per pixel. The only full-framebuffer copy left is cache→surface.
            let Some(surface) = self.surface.as_mut() else {
                return;
            };
            let view = match &mut self.backend {
                Backend::Cpu(r) => r.render_input_cached(&self.input_scratch),
                Backend::Gpu(_) => return,
            };
            let pixels = view.pixels();
            let (w, h) = (view.width().max(1) as u32, view.height().max(1) as u32);
            surface
                .resize(NonZeroU32::new(w).unwrap(), NonZeroU32::new(h).unwrap())
                .ok();
            if let Ok(mut buf) = surface.buffer_mut() {
                let n = buf.len().min(pixels.len());
                if invert {
                    for (dst, &src) in buf[..n].iter_mut().zip(&pixels[..n]) {
                        *dst = src ^ 0x00ff_ffff;
                    }
                } else {
                    buf[..n].copy_from_slice(&pixels[..n]);
                }
                for px in buf.iter_mut().skip(n) {
                    *px = 0;
                }
                let _ = buf.present();
            }
        }
        // Latency self-introspection: the frame is now presented. If an output
        // burst is pending, log how long it waited from "content ready" to
        // "presented" (output->present) — aterm's render-pipeline latency, the
        // slice of input-to-photon software controls. Logged on BOTH paths after
        // present. swap(0) so the next burst's leading edge restarts the clock.
        if self.trace_latency {
            let stamp = self.last_output_ns.swap(0, Ordering::Relaxed);
            if stamp != 0 {
                let now = self.lat_epoch.elapsed().as_nanos() as u64;
                eprintln!(
                    "aterm-latency output->present: {:.2} ms",
                    now.saturating_sub(stamp) as f64 / 1e6
                );
            }
        }
        let _ = window;
    }

    /// Introspect the live screen: render the CURRENT terminal to a PNG (the
    /// exact pixels on screen, via the same renderer the window uses) and write a
    /// parallel .txt of the visible text. Triggered by SIGUSR1. The files are
    /// written 0600 into the per-user 0700 control dir by default;
    /// $ATERM_SNAPSHOT_PATH overrides only into a safe dir (see `snapshot_path`).
    fn snapshot(&mut self) {
        let Some(path) = snapshot_path::resolve() else {
            return; // refusal already logged by resolve()
        };
        let (rows, cols) = (self.rows as usize, self.cols as usize);
        // Lock only to snapshot the grid; render + serialize without the lock.
        {
            let term = term_lock(&self.term);
            // REFILL the reused snapshot in place (no per-frame container-Vec alloc).
            aterm_render::Renderer::extract_into(&mut self.input_scratch, &term, rows, cols);
        }
        // pixels: the same offscreen frame the window blits on screen (GPU path
        // if active) — byte-identical, so the AI sees exactly what is presented.
        // `backend.render_input` returns an owned Frame on both backends (the
        // snapshot/image path keeps the pixels past the next render, unlike the
        // borrowing window hot path).
        let mut frame = self.backend.render_input(&self.input_scratch);
        // I-2: WYSIWYG — the on-screen present inverts the whole frame during a
        // visual-bell flash (CPU `src ^ 0x00ff_ffff`; GPU blit shader). Apply the
        // SAME invert here so a snapshot taken DURING a flash matches the glass
        // instead of showing the un-inverted frame.
        apply_bell_invert(&mut frame, self.bell_flash.is_active(Instant::now()));
        // text: the visible grid, row by row, from the same snapshot
        let mut text = String::with_capacity(rows * (cols + 1));
        for cells in &self.input_scratch.cells {
            for cell in cells.iter().take(cols) {
                text.push(if cell.ch == '\0' || cell.ch.is_control() { ' ' } else { cell.ch });
            }
            while text.ends_with(' ') {
                text.pop();
            }
            text.push('\n');
        }
        let _ = snapshot_path::write_private(std::path::Path::new(&path), &frame.to_png());
        let _ = snapshot_path::write_private(std::path::Path::new(&format!("{path}.txt")), text.as_bytes());
        // a marker the requester can stat() for; stderr is unreliable for GUIs
        let _ = snapshot_path::write_private(
            std::path::Path::new(&format!("{path}.done")),
            format!("{}x{}\n", frame.width, frame.height).as_bytes(),
        );
        eprintln!("aterm-gui: snapshot written to {path} (+ .txt, .done)");
    }

    /// Render the CURRENT terminal to the confined `target` (the same renderer
    /// the window uses, GPU path if active) and return the frame's
    /// `(width, height)`. Serves the control socket's `image` verb; runs on the
    /// main thread per [`Wake::Control`].
    fn render_image(&mut self, target: &control_auth::ConfinedImage) -> (u32, u32) {
        let (rows, cols) = (self.rows as usize, self.cols as usize);
        // Lock only to snapshot the grid; render without the lock.
        {
            let term = term_lock(&self.term);
            // REFILL the reused snapshot in place (no per-frame container-Vec alloc).
            aterm_render::Renderer::extract_into(&mut self.input_scratch, &term, rows, cols);
        }
        let mut frame = self.backend.render_input(&self.input_scratch);
        // I-2: match the on-screen visual-bell invert (see `snapshot`) so the
        // `image` verb is WYSIWYG even during a bell flash.
        apply_bell_invert(&mut frame, self.bell_flash.is_active(Instant::now()));
        // `confine_image_path` (control thread) produced `target` as a canonical
        // `images/` dir + a SINGLE filename, forbidding nested target dirs. We
        // write by opening THAT directory `O_DIRECTORY|O_NOFOLLOW` and
        // `openat`-ing the final component `O_NOFOLLOW|O_CREAT|O_TRUNC` — so the
        // only guarantee we rely on is: the write lands in the canonical images
        // dir and never follows a symlink at the directory OR the final name.
        // (We do NOT claim atomicity vs. a same-uid client deleting+recreating
        // the directory between threads; we DO close the intermediate-dir
        // symlink-swap window by never re-resolving a multi-segment path string.)
        let _ = snapshot_path::write_private_at(&target.dir, &target.file_name, &frame.to_png());
        (frame.width as u32, frame.height as u32)
    }

    /// Paste the macOS system clipboard (`pbpaste`) to the PTY via
    /// [`Terminal::format_paste`], which strips control bytes a hostile
    /// clipboard could use to escape the guards (ESC, C1 controls), converts
    /// line breaks to CR, and wraps the body in the bracketed-paste guards
    /// (ESC[200~ .. ESC[201~) when the app has enabled bracketed paste — so
    /// editors/shells treat it as inert pasted text.
    fn paste_clipboard(&self) {
        let Ok(out) = std::process::Command::new("pbpaste").output() else {
            return;
        };
        if !out.status.success() || out.stdout.is_empty() {
            return;
        }
        let text = String::from_utf8_lossy(&out.stdout);
        let bytes = term_lock(&self.term).format_paste(&text);
        self.write_pty(&bytes);
    }

    /// Cmd-C: copy the selected text to the macOS system clipboard (`pbcopy`).
    /// Returns whether anything was copied; the selection is NOT cleared (so a
    /// highlight survives the copy, and repeated copies work).
    fn copy_selection(&self) -> bool {
        let Some(text) = term_lock(&self.term).selection_to_string() else {
            return false;
        };
        !text.is_empty() && control::pbcopy(&text)
    }

    /// Clear any active selection (the standard "typing deselects" behavior)
    /// and repaint so the highlight disappears. No-op when nothing is selected.
    fn clear_selection(&mut self) {
        let cleared = {
            let mut term = term_lock(&self.term);
            if term.text_selection().has_selection() {
                term.text_selection_mut().clear();
                true
            } else {
                false
            }
        };
        if cleared {
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }
    }

    fn write_pty(&self, bytes: &[u8]) {
        aterm_pty::write_all(self.master, bytes);
    }

    /// Enter (or refresh) Cmd-F find mode.
    fn search_enter(&mut self) {
        if self.search.is_none() {
            self.search = Some(SearchState::default());
        }
        self.search_recompute();
    }

    /// Re-run the find for the current query over the live screen + recent
    /// scrollback, then show the first match. Snaps the viewport to the bottom
    /// first so `get_line_text` rows are stable selection coordinates (0..rows =
    /// live, negative = scrollback); the lines are gathered oldest→newest so match
    /// order reads top-to-bottom.
    fn search_recompute(&mut self) {
        let query = match &self.search {
            Some(s) => s.query.clone(),
            None => return,
        };
        let matches = if query.is_empty() {
            Vec::new()
        } else {
            let rows = i32::from(self.rows);
            let mut term = term_lock(&self.term);
            term.scroll_to_bottom(); // display_offset = 0 → stable coords
            // Scrollback (negative rows) oldest→newest, bounded; then the live screen.
            let mut hist: Vec<(i32, String)> = Vec::new();
            let mut r = -1;
            while r >= -MAX_SEARCH_HISTORY {
                match term.get_line_text(r, None) {
                    Some(t) => hist.push((r, t)),
                    None => break, // past the top of history
                }
                r -= 1;
            }
            hist.reverse();
            for r in 0..rows {
                hist.push((r, term.get_line_text(r, None).unwrap_or_default()));
            }
            drop(term);
            find_line_matches(&hist, &query)
        };
        if let Some(s) = self.search.as_mut() {
            s.matches = matches;
            s.current = 0;
        }
        self.search_apply_current();
    }

    /// Highlight the current match via the text selection (the existing overlay —
    /// no renderer change), scroll it into view, and show the find state in the
    /// window title.
    fn search_apply_current(&mut self) {
        let (query, mat, idx, total) = match &self.search {
            Some(s) => (
                s.query.clone(),
                s.matches.get(s.current).copied(),
                s.current,
                s.matches.len(),
            ),
            None => return,
        };
        {
            let mut term = term_lock(&self.term);
            term.scroll_to_bottom(); // reset to display_offset = 0 (stable coords)
            let sel = term.text_selection_mut();
            sel.clear();
            if let Some((row, c0, c1)) = mat {
                sel.start_selection(row, c0, SelectionSide::Left, SelectionType::Simple);
                sel.update_selection(row, c1, SelectionSide::Right);
                // A scrollback match (row < 0) is scrolled up to the top visible row.
                if row < 0 {
                    term.scroll_display(-row);
                }
            }
        }
        let title = if query.is_empty() {
            "aterm — find:".to_string()
        } else if total == 0 {
            format!("aterm — find: {query} (no matches)")
        } else {
            format!("aterm — find: {query} ({}/{total})", idx + 1)
        };
        if let Some(w) = &self.window {
            w.set_title(&title);
            w.request_redraw();
        }
    }

    /// Cycle to the next (`forward`) / previous match, wrapping.
    fn search_step(&mut self, forward: bool) {
        if let Some(s) = self.search.as_mut() {
            let n = s.matches.len();
            if n == 0 {
                return;
            }
            s.current = if forward {
                (s.current + 1) % n
            } else {
                (s.current + n - 1) % n
            };
        }
        self.search_apply_current();
    }

    /// Leave find mode: clear the highlight + restore the title.
    fn search_exit(&mut self) {
        self.search = None;
        term_lock(&self.term).text_selection_mut().clear();
        if let Some(w) = &self.window {
            w.set_title("aterm");
            w.request_redraw();
        }
    }

    fn on_key(&mut self, ev: KeyEvent) {
        if ev.state != ElementState::Pressed {
            return;
        }
        // Typing makes the cursor solid and restarts the blink period.
        self.reset_blink();
        // Cmd-N opens a new window (a fresh, independent aterm-gui process — the
        // standard macOS "new window", distinct from in-window tabs). Cmd-W closes
        // this window (the keyboard equivalent of the close button).
        if self.mods.super_key() && !self.mods.shift_key() {
            if let Key::Character(s) = &ev.logical_key {
                match s.to_ascii_lowercase().as_str() {
                    "n" => {
                        open_new_window();
                        return;
                    }
                    "w" => {
                        self.should_exit = true;
                        return;
                    }
                    _ => {}
                }
            }
        }
        // Cmd-F enters find mode; while active, keystrokes drive the find (query
        // edit + match navigation) instead of reaching the PTY.
        if self.mods.super_key() {
            if let Key::Character(s) = &ev.logical_key {
                if s.eq_ignore_ascii_case("f") {
                    self.search_enter();
                    return;
                }
            }
        }
        if self.search.is_some() {
            match &ev.logical_key {
                Key::Named(NamedKey::Escape) => self.search_exit(),
                Key::Named(NamedKey::Enter) => self.search_step(!self.mods.shift_key()),
                Key::Named(NamedKey::Backspace) => {
                    if let Some(s) = self.search.as_mut() {
                        s.query.pop();
                    }
                    self.search_recompute();
                }
                _ => {
                    // Plain typing edits the query; modifier combos are swallowed.
                    if !self.mods.super_key() && !self.mods.control_key() {
                        if let Some(text) = &ev.text {
                            if !text.is_empty() {
                                if let Some(s) = self.search.as_mut() {
                                    s.query.push_str(text);
                                }
                                self.search_recompute();
                            }
                        }
                    }
                }
            }
            return;
        }
        // Cmd-C -> copy the selection to the system clipboard (before the
        // snap-to-bottom: copying must neither clear the selection nor move
        // the viewport). With no selection it falls through to normal handling.
        if self.mods.super_key() {
            if let Key::Character(s) = &ev.logical_key {
                if s.eq_ignore_ascii_case("c") && self.copy_selection() {
                    return;
                }
            }
        }
        // Any key press jumps back to the live view if scrolled into history.
        self.snap_to_bottom();
        // Cmd-V -> paste the system clipboard (bracketed when the app enabled
        // it). Pasting does not clear the selection.
        if self.mods.super_key() {
            if let Key::Character(s) = &ev.logical_key {
                if s.eq_ignore_ascii_case("v") {
                    self.paste_clipboard();
                    return;
                }
            }
        }
        // Cmd-= / Cmd-+ / Cmd-- / Cmd-0 -> live font zoom (grow / shrink / reset).
        if self.mods.super_key() {
            if let Key::Character(s) = &ev.logical_key {
                match s.as_str() {
                    "=" | "+" => {
                        self.set_font_px(self.font_px + FONT_ZOOM_STEP);
                        return;
                    }
                    "-" => {
                        self.set_font_px(self.font_px - FONT_ZOOM_STEP);
                        return;
                    }
                    "0" => {
                        self.set_font_px(self.default_font_px);
                        return;
                    }
                    _ => {}
                }
            }
        }
        // IME-1: while a composition (CJK / dead key) is in flight, SUPPRESS the
        // direct key send — the keystrokes belong to the composer; the resulting
        // text arrives via `Ime::Commit` (encoded through the same engine path).
        // Without this the composing keys would ALSO emit raw bytes (double
        // input). ASCII typing with no active composition is unaffected (preedit
        // is empty), so normal keys still send below. The Ctrl+letter `& 0x1f`
        // branch is intentionally GONE: K-1 routing (below) encodes Ctrl, Alt,
        // named keys, and Kitty CSI-u via the engine's `keymap` encoder.
        if keymap::suppress_direct_send(&self.preedit) {
            return;
        }
        // K-1: route EVERY remaining key through the engine's encoder via the
        // pure `keymap::encode_key_event`. This replaces the in-GUI `& 0x1f`
        // Ctrl branch and the raw `ev.text` write: Alt/Option now gets its ESC
        // prefix, Ctrl on non-alpha keys (Ctrl+Space=NUL, Ctrl+\=FS) is correct,
        // and the Kitty CSI-u / alternate-key / base-layout encoding matches the
        // engine protocol. Named keys (arrows/Home/End/keypad/F-keys, the FULL
        // K-2 set) flow through the same call, respecting DECCKM/app-keypad/Kitty.
        let mods = keymap::modifiers_from_winit(self.mods);
        let mode = term_lock(&self.term).keyboard_mode();
        if let Some(bytes) = keymap::encode_winit_key_event(&ev, mods, mode) {
            // Any key that writes to the PTY deselects, like any other typing.
            self.clear_selection();
            self.write_pty(&bytes);
            return;
        }
        // IME/dead-key fallback: the engine produced nothing (an unencodable key
        // or a layout-composed character that `key_without_modifiers` stripped).
        // Honor winit's resolved `text` so a plain layout character still types
        // when no IME composition is active — but NEVER for Ctrl/Alt/Super, whose
        // ESC/control encoding the engine already owns above.
        let bare = !self.mods.control_key() && !self.mods.alt_key() && !self.mods.super_key();
        if let Some(text) = &ev.text {
            if bare && !text.is_empty() {
                self.clear_selection();
                self.write_pty(text.as_bytes());
            }
        }
    }

    /// IME-1: a composition update (`Ime::Preedit`) — track the marked text so a
    /// preedit indicator can render and direct key sends stay suppressed while
    /// composing. An empty preedit ends the composition. Requests a repaint so
    /// the (minimal) on-screen indicator follows the composition.
    fn on_ime_preedit(&mut self, text: String) {
        let changed = self.preedit != text;
        self.preedit = text;
        if changed {
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }
    }

    /// IME-1: composition committed (`Ime::Commit`) — the finished CJK/dead-key
    /// text. End the composition and send the committed text to the PTY via the
    /// engine path (each grapheme encoded as a `Character` key, NOT `& 0x1f`), so
    /// it goes out exactly as typed text. Clears the selection like any typing.
    fn on_ime_commit(&mut self, text: String) {
        self.preedit.clear();
        if text.is_empty() {
            return;
        }
        self.clear_selection();
        let mode = term_lock(&self.term).keyboard_mode();
        let out = keymap::encode_committed_text(&text, mode);
        if !out.is_empty() {
            self.write_pty(&out);
        }
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    /// Current keyboard modifiers as a mouse-report modifier mask (shift/alt/ctrl
    /// bits the engine ORs into the button byte).
    fn mouse_modifiers(&self) -> u8 {
        use aterm_types::mouse::{ALT_MASK, CTRL_MASK, SHIFT_MASK};
        let mut m = 0u8;
        if self.mods.shift_key() {
            m |= SHIFT_MASK;
        }
        if self.mods.alt_key() {
            m |= ALT_MASK;
        }
        if self.mods.control_key() {
            m |= CTRL_MASK;
        }
        m
    }

    /// Map a pixel position to a 0-based (row, col) grid cell, clamped to the grid.
    fn pixel_to_cell(&self, x: f64, y: f64) -> (u16, u16) {
        let (cw, ch) = self.cell_size();
        let col = (x as usize / cw.max(1)).min(self.cols.saturating_sub(1) as usize) as u16;
        let row = (y as usize / ch.max(1)).min(self.rows.saturating_sub(1) as usize) as u16;
        (row, col)
    }

    /// `CursorMoved` -> remember the cell under the pointer; mid-drag, grow the
    /// text selection to that cell (and, when motion tracking is on, report the
    /// move to the app instead).
    /// Show the "pointer" cursor while Cmd-hovering a link, else the default. Only
    /// touches the OS cursor on a state CHANGE (not every mouse move). Updated on
    /// both pointer motion and Cmd press/release so the affordance tracks the key.
    fn update_hover_cursor(&mut self) {
        let over_link = self.mods.super_key() && self.link_under_pointer().is_some();
        if over_link != self.hover_pointer {
            self.hover_pointer = over_link;
            if let Some(w) = &self.window {
                w.set_cursor(if over_link { CursorIcon::Pointer } else { CursorIcon::Default });
            }
        }
    }

    fn on_cursor_moved(&mut self, x: f64, y: f64) {
        let (row, col) = self.pixel_to_cell(x, y);
        self.last_mouse_cell = (row, col);
        self.update_hover_cursor();
        // Which half of the cell the pointer is in: the right half includes
        // the hovered cell, the left half stops before it. Remembered so a
        // shift-click press (which has no pixel position of its own) can
        // anchor by the half that was pressed.
        let cw = self.cell_size().0.max(1);
        self.last_mouse_side = if (x as usize % cw) * 2 >= cw {
            SelectionSide::Right
        } else {
            SelectionSide::Left
        };
        if self.selecting {
            self.drag_selection(row, col);
            return;
        }
        let mods = self.mouse_modifiers();
        let bytes = {
            let term = term_lock(&self.term);
            if !term.mouse_tracking_enabled() {
                return;
            }
            // No-button motion: button code 3 means "no buttons" for the report.
            term.encode_mouse_motion(3, col, row, mods)
        };
        if let Some(b) = bytes {
            self.write_pty(&b);
        }
    }

    /// Mid-drag: grow the selection to the hovered viewport cell — by cell for
    /// simple/block drags, by whole words/lines when the drag began as a
    /// double/triple click (the gesture origin stays fully selected whichever
    /// direction the drag goes).
    fn drag_selection(&mut self, row: u16, col: u16) {
        let sel_row = {
            let mut term = term_lock(&self.term);
            let sel_row = i32::from(row) - term.grid().display_offset() as i32;
            match self.gesture {
                None => {
                    term.text_selection_mut()
                        .update_selection(sel_row, col, self.last_mouse_side);
                }
                // Triple-click drag: whole rows from the origin line to the
                // hovered line. Rebuilt from the origin each move so the
                // anchor sides stay inclusive in either drag direction.
                Some(g) if g.kind == SelectionType::Lines => {
                    let max_col = term.cols().saturating_sub(1);
                    let sel = term.text_selection_mut();
                    if sel_row < g.row {
                        sel.start_selection(
                            g.row,
                            max_col,
                            SelectionSide::Right,
                            SelectionType::Lines,
                        );
                        sel.update_selection(sel_row, 0, SelectionSide::Left);
                    } else {
                        sel.start_selection(g.row, 0, SelectionSide::Left, SelectionType::Lines);
                        sel.update_selection(sel_row, max_col, SelectionSide::Right);
                    }
                }
                // Double-click drag: snap the moving end to the hovered word
                // (or bare cell on whitespace); the origin word stays fully
                // selected by anchoring at its far boundary.
                Some(g) => {
                    let (ws, we) = control::word_cols(&term, sel_row, col).unwrap_or((col, col));
                    let sel = term.text_selection_mut();
                    if (sel_row, col) < (g.row, g.start_col) {
                        sel.start_selection(
                            g.row,
                            g.end_col,
                            SelectionSide::Right,
                            SelectionType::Semantic,
                        );
                        sel.update_selection(sel_row, ws, SelectionSide::Left);
                    } else {
                        sel.start_selection(
                            g.row,
                            g.start_col,
                            SelectionSide::Left,
                            SelectionType::Semantic,
                        );
                        sel.update_selection(sel_row, we, SelectionSide::Right);
                    }
                }
            }
            sel_row
        };
        if (sel_row, col) != self.sel_press_cell {
            self.sel_dragged = true;
        }
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    /// Left press with mouse tracking OFF — the selection-gesture dispatcher.
    ///
    /// Shift with an existing selection extends it to the pressed cell;
    /// otherwise the multi-click count picks the gesture: 1 starts a simple
    /// drag (rectangular block with alt/option held), 2 selects the word under
    /// the press, 3 selects the whole line. Word/line selections stay
    /// draggable until release (extending by whole words/lines).
    fn on_left_press(&mut self) {
        let (row, col) = self.last_mouse_cell;
        let sel_row =
            i32::from(row) - term_lock(&self.term).grid().display_offset() as i32;
        let now = Instant::now();
        // Shift-click: extend the existing selection instead of starting a new
        // one, the moving end anchored by the pressed cell-half. Resets the
        // multi-click streak (the press is not part of a double-click).
        if self.mods.shift_key() && self.extend_selection_to(sel_row, col) {
            self.last_press = Some((now, (sel_row, col)));
            self.click_count = 1;
            if let Some(w) = &self.window {
                w.request_redraw();
            }
            return;
        }
        // Multi-click detection: a press within MULTI_CLICK_MS of the previous
        // press in the SAME cell advances the count 1 -> 2 -> 3 -> 1; any
        // other press starts a new streak.
        self.click_count = match self.last_press {
            Some((t, cell))
                if cell == (sel_row, col)
                    && now.duration_since(t).as_millis() <= MULTI_CLICK_MS =>
            {
                self.click_count % 3 + 1
            }
            _ => 1,
        };
        self.last_press = Some((now, (sel_row, col)));
        match self.click_count {
            2 => self.select_word_click(sel_row, col),
            3 => self.select_line_click(sel_row, col),
            _ => self.begin_selection(if self.mods.alt_key() {
                SelectionType::Block
            } else {
                SelectionType::Simple
            }),
        }
    }

    /// Shift-click: extend an EXISTING non-empty selection so the pressed cell
    /// becomes its new endpoint (side by cell half), then complete it again.
    /// Returns false (no-op) when there is nothing to extend.
    fn extend_selection_to(&mut self, sel_row: i32, col: u16) -> bool {
        let mut term = term_lock(&self.term);
        let sel = term.text_selection_mut();
        if !sel.has_selection() || sel.is_empty() {
            return false;
        }
        sel.extend_selection(sel_row, col, self.last_mouse_side);
        sel.complete_selection();
        true
    }

    /// Double-click: word-select the pressed cell (builtin smart rules — URLs,
    /// paths, words; just the cell on whitespace), completed immediately, and
    /// arm the gesture so a drag before release extends by whole words.
    fn select_word_click(&mut self, sel_row: i32, col: u16) {
        let (start_col, end_col) = {
            let mut term = term_lock(&self.term);
            control::select_word(&mut term, sel_row, col)
        };
        self.gesture = Some(GestureOrigin {
            row: sel_row,
            start_col,
            end_col,
            kind: SelectionType::Semantic,
        });
        self.arm_gesture_drag(sel_row, col);
    }

    /// Triple-click: select the full line under the press, completed
    /// immediately, and arm the gesture so a drag extends by whole lines.
    fn select_line_click(&mut self, sel_row: i32, col: u16) {
        let end_col = {
            let mut term = term_lock(&self.term);
            control::select_line(&mut term, sel_row);
            term.cols().saturating_sub(1)
        };
        self.gesture = Some(GestureOrigin {
            row: sel_row,
            start_col: 0,
            end_col,
            kind: SelectionType::Lines,
        });
        self.arm_gesture_drag(sel_row, col);
    }

    /// Keep a completed double/triple-click selection draggable while the
    /// button stays down: `sel_dragged` is pre-set so the release completes
    /// the selection instead of treating it as a deselecting plain click.
    fn arm_gesture_drag(&mut self, sel_row: i32, col: u16) {
        self.selecting = true;
        self.sel_dragged = true;
        self.sel_press_cell = (sel_row, col);
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    /// Single press with mouse tracking OFF: start a text selection of `kind`
    /// (`Simple`, or `Block` for alt-drag) at the cell under the pointer,
    /// mapped to live-screen selection coords (viewport row minus
    /// `display_offset`, so a scrolled-back press lands in scrollback).
    fn begin_selection(&mut self, kind: SelectionType) {
        let (row, col) = self.last_mouse_cell;
        let sel_row = {
            let mut term = term_lock(&self.term);
            let sel_row = i32::from(row) - term.grid().display_offset() as i32;
            term.text_selection_mut()
                .start_selection(sel_row, col, SelectionSide::Left, kind);
            sel_row
        };
        self.selecting = true;
        self.sel_dragged = false;
        self.sel_press_cell = (sel_row, col);
        self.gesture = None;
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    /// Left release ending a drag: complete the selection — unless the pointer
    /// never left the press cell, in which case a plain click deselects.
    fn finish_selection(&mut self) {
        {
            let mut term = term_lock(&self.term);
            let sel = term.text_selection_mut();
            if self.sel_dragged {
                sel.complete_selection();
            } else {
                sel.clear();
            }
        }
        self.selecting = false;
        self.gesture = None;
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    /// The URL under the pointer, if any: an (authorized) OSC 8 hyperlink on the
    /// cell wins; else a plain-text `http(s)://` URL detected in the row. Used by
    /// Cmd-click (open) and Cmd-hover (pointer cursor).
    fn link_under_pointer(&self) -> Option<String> {
        let (row, col) = self.last_mouse_cell;
        let term = term_lock(&self.term);
        term.hyperlink_at(row, col).map(str::to_owned).or_else(|| {
            plain_url_at(&term.render_row(row as usize), col as usize).map(|(u, _, _)| u)
        })
    }

    /// Cmd-click: if there is a link under the pointer with a safe scheme, open it
    /// via the OS and report `true`. The `is_safe_url` allowlist is the security
    /// boundary — a hostile program's link can never make `open` launch an app or
    /// touch the filesystem (covers both OSC 8 and auto-detected plain-text URLs).
    fn open_link_under_pointer(&self) -> bool {
        let Some(url) = self.link_under_pointer() else {
            return false;
        };
        if !is_safe_url(&url) {
            return false;
        }
        #[cfg(target_os = "macos")]
        let _ = std::process::Command::new("/usr/bin/open").arg(&url).spawn();
        true
    }

    /// `MouseInput` -> when no app is tracking the mouse, left presses run the
    /// selection gestures (drag select, double-click word, triple-click line,
    /// shift-click extend, alt-drag block; a plain left click deselects); when
    /// tracking is on, encode the press/release for the cell under the pointer
    /// and write it to the PTY.
    fn on_mouse_input(&mut self, state: ElementState, button: WinitMouseButton) {
        if button == WinitMouseButton::Left {
            let tracking = term_lock(&self.term).mouse_tracking_enabled();
            if state == ElementState::Pressed && !tracking {
                // Cmd-click an OSC 8 hyperlink opens it (safe schemes only) instead
                // of starting a selection.
                if self.mods.super_key() && self.open_link_under_pointer() {
                    return;
                }
                self.on_left_press();
                return;
            }
            // Always settle an in-flight drag on release, even if the app
            // enabled tracking mid-drag.
            if state == ElementState::Released && self.selecting {
                self.finish_selection();
                return;
            }
        }
        let code = match button {
            WinitMouseButton::Left => 0u8,
            WinitMouseButton::Middle => 1,
            WinitMouseButton::Right => 2,
            _ => return,
        };
        let (row, col) = self.last_mouse_cell;
        let mods = self.mouse_modifiers();
        let bytes = {
            let term = term_lock(&self.term);
            if !term.mouse_tracking_enabled() {
                return;
            }
            match state {
                ElementState::Pressed => term.encode_mouse_press(code, col, row, mods),
                ElementState::Released => term.encode_mouse_release(code, col, row, mods),
            }
        };
        if let Some(b) = bytes {
            self.write_pty(&b);
        }
    }

    /// `MouseWheel` -> when an app is tracking the mouse, report wheel up/down at
    /// the cell under the pointer; otherwise scroll the scrollback viewport (the
    /// everyday "scroll up to see history" gesture).
    fn on_mouse_wheel(&mut self, delta: MouseScrollDelta) {
        // Lines to move per event: one line per LineDelta notch, or a fraction of
        // the cell height for trackpad PixelDelta (min 1 so a flick always moves).
        let (dir_up, lines) = match delta {
            MouseScrollDelta::LineDelta(_, y) => (y > 0.0, y.abs().round().max(1.0) as i32),
            MouseScrollDelta::PixelDelta(p) => {
                let ch = self.cell_size().1.max(1) as f64;
                (p.y > 0.0, (p.y.abs() / ch).round().max(1.0) as i32)
            }
        };
        if lines == 0 {
            return;
        }
        let (row, col) = self.last_mouse_cell;
        let mods = self.mouse_modifiers();
        let report = {
            let mut term = term_lock(&self.term);
            if term.mouse_tracking_enabled() {
                term.encode_mouse_wheel(dir_up, col, row, mods)
            } else {
                // Positive display_offset = older content. Wheel up -> into history.
                term.scroll_display(if dir_up { lines } else { -lines });
                None
            }
        };
        match report {
            // One wheel report PER line of scroll, so a fast notch or trackpad
            // flick moves a mouse-tracking app (vim/less/htop) by the same amount
            // it would move our own scrollback in the untracked branch above —
            // previously this sent a single report regardless of `lines`, so the
            // identical gesture scrolled N rows in the shell but 1 row in a TUI.
            Some(b) => {
                for _ in 0..lines {
                    self.write_pty(&b);
                }
            }
            None => {
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
        }
    }

    /// Snap the viewport back to the live bottom (called on keyboard input, the
    /// standard "start typing and jump to the prompt" behavior).
    fn snap_to_bottom(&mut self) {
        let scrolled = {
            let mut term = term_lock(&self.term);
            if term.grid().display_offset() != 0 {
                term.scroll_to_bottom();
                true
            } else {
                false
            }
        };
        if scrolled {
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }
    }

    fn on_resize(&mut self, size: PhysicalSize<u32>) {
        let (cw, ch) = self.cell_size();
        let cols = (size.width as usize / cw.max(1)).max(1) as u16;
        let rows = (size.height as usize / ch.max(1)).max(1) as u16;
        self.apply_term_resize(rows, cols);
    }

    /// Apply a `(rows, cols)` grid resize to the engine + PTY + GPU swapchain
    /// (the geometry the main thread owns). The CPU softbuffer resizes itself in
    /// `redraw` from the Frame dims. No-op when the geometry is unchanged. Shared
    /// by the window `Resized` path and the control-socket resize (RES-1).
    fn apply_term_resize(&mut self, rows: u16, cols: u16) -> bool {
        if (rows, cols) == (self.rows, self.cols) {
            return false;
        }
        let (cw, ch) = self.cell_size();
        self.rows = rows;
        self.cols = cols;
        term_lock(&self.term).resize(rows, cols);
        aterm_pty::resize(self.master, rows, cols);
        // GPU mode: reconfigure the swapchain to the new framebuffer pixel size
        // (rows/cols x cell size) so the blit target matches the frame.
        if let (Some(gpu), Some(gpu_surface)) =
            (self.backend.gpu_mut(), self.gpu_surface.as_mut())
        {
            let (w_px, h_px) = (cols as u32 * cw as u32, rows as u32 * ch as u32);
            gpu.resize_surface(gpu_surface, w_px, h_px);
        }
        true
    }

    /// RES-1: a control-socket `resize` verb landed on the main thread (via
    /// [`Wake::Resize`]). Apply the term/PTY/framebuffer resize, then ask the
    /// window to match the new grid pixel size so the on-screen geometry tracks
    /// the engine (the window `Resized` event that follows is a no-op — the grid
    /// already matches). Finally request a redraw so the resized screen is
    /// presented. Without this the verb left `App.rows/cols` + framebuffer stale
    /// and sent no Wake, so a follow-up `image`/`dims` disagreed.
    fn apply_grid_resize(&mut self, rows: u16, cols: u16) {
        let changed = self.apply_term_resize(rows, cols);
        if !changed {
            return;
        }
        if let Some(w) = &self.window {
            let (cw, ch) = self.cell_size();
            let size = PhysicalSize::new(cols as u32 * cw as u32, rows as u32 * ch as u32);
            // A best-effort request; the WM may clamp. The engine/PTY geometry is
            // already authoritative regardless of what the window settles on.
            let _ = w.request_inner_size(size);
            w.request_redraw();
        }
    }

    /// Live font zoom (Cmd-+/Cmd--/Cmd-0): rebuild the [`Backend`] at `px`, then
    /// re-grid for the new cell size in the SAME window (more/fewer rows+cols) and
    /// tell the PTY. A failed rebuild (GPU hiccup / no font) keeps the current size
    /// — zoom never crashes. No-op without a window (headless).
    fn set_font_px(&mut self, px: f32) {
        let px = px.clamp(FONT_PX_MIN, FONT_PX_MAX);
        if (px - self.font_px).abs() < 0.5 {
            return;
        }
        let Some(backend) = build_backend(px, self.use_gpu, self.theme) else {
            return;
        };
        self.font_px = px;
        self.backend = backend;
        // GPU mode: the old `gpu_surface` belongs to the now-dropped GpuRenderer
        // (its wgpu device is gone), so re-create the swapchain on the NEW renderer
        // for the current window before the next present.
        if self.backend.is_gpu() {
            self.gpu_surface = None;
            if let (Some(window), Some(gpu)) = (self.window.clone(), self.backend.gpu_mut()) {
                let size = window.inner_size();
                match gpu.create_window_surface(window.clone(), size.width, size.height) {
                    Ok(surf) => self.gpu_surface = Some(surf),
                    Err(e) => eprintln!("aterm-gui: GPU surface re-creation on zoom failed: {e}"),
                }
            }
        }
        // A new backend starts with a fresh damage cache, so force the next redraw
        // to actually paint (the D-1 early-out compares to the last present key).
        self.last_present = None;
        // Re-grid for the new cell metrics, then repaint. Read the window size out
        // first so `on_resize(&mut self)` doesn't overlap the `self.window` borrow.
        if let Some(size) = self.window.as_ref().map(|w| w.inner_size()) {
            self.on_resize(size);
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }
    }
}

impl ApplicationHandler<Wake> for App {
    fn new_events(&mut self, _el: &ActiveEventLoop, cause: StartCause) {
        // A `WaitUntil` deadline fired: a bell-flash end and/or a blink tick.
        if matches!(cause, StartCause::ResumeTimeReached { .. }) {
            let now = Instant::now();
            // Flash over: repaint the normal (un-inverted) frame.
            if self.bell_flash.expire(now) {
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            // Blink tick: flip the phase, repaint, and arm the next
            // half-period (`about_to_wait` re-schedules). Gated on the armed
            // deadline so an earlier bell-flash wake doesn't clip the period.
            if self.next_blink.is_some_and(|d| now >= d) {
                self.blink_phase = !self.blink_phase;
                self.next_blink = Some(now + BLINK_INTERVAL);
                self.sync_blink_phase();
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        // Blink scheduling: a blink deadline ONLY while a focused window
        // shows a visible Blinking* cursor.
        let mut deadline = if self.blink_active() {
            Some(
                *self
                    .next_blink
                    .get_or_insert_with(|| Instant::now() + BLINK_INTERVAL),
            )
        } else {
            self.next_blink = None;
            // Leave the cursor solid so a steady style is never stuck "off".
            if !self.blink_phase {
                self.blink_phase = true;
                self.sync_blink_phase();
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            None
        };
        // A pending bell flash needs a wake at its end to un-invert the frame.
        if let Some(d) = self.bell_flash.deadline() {
            deadline = Some(deadline.map_or(d, |b| b.min(d)));
        }
        // `WaitUntil` the earliest deadline; with neither armed, sleep in pure
        // `Wait` — zero timer wakeups, preserving the 0%-idle property for
        // steady/unfocused/hidden/headless sessions.
        match deadline {
            Some(d) => el.set_control_flow(ControlFlow::WaitUntil(d)),
            None => el.set_control_flow(ControlFlow::Wait),
        }
    }

    fn resumed(&mut self, el: &ActiveEventLoop) {
        // Event-driven: sleep until the PTY produces output or the user acts,
        // instead of busy-polling. Redraws are requested explicitly on Wake.
        el.set_control_flow(ControlFlow::Wait);
        // Headless: run the engine + control socket + offscreen rendering, but
        // never open a window. `redraw()` is a no-op while `surface` is None,
        // and the winit run loop still delivers `user_event` (Wake::Control for
        // `image`, Wake::Snapshot, Wake::Exit) with no window present.
        if self.headless {
            return;
        }
        if self.window.is_some() {
            return;
        }
        let (cw, ch) = self.cell_size();
        let size = PhysicalSize::new(self.cols as u32 * cw as u32, self.rows as u32 * ch as u32);
        let attrs = Window::default_attributes().with_title("aterm").with_inner_size(size);
        let window = Arc::new(el.create_window(attrs).expect("create window"));
        // IME-1: opt into IME so the window receives `WindowEvent::Ime`
        // (Preedit/Commit) for CJK/dead-key/Option composition. Never enabled
        // before, so composition input was impossible.
        window.set_ime_allowed(true);
        // HiDPI note: aterm rasterizes glyphs at `font_px` PHYSICAL pixels and
        // works in physical units throughout. On a 2× Retina display a program
        // with the default 16 px font therefore renders at ~8 logical points —
        // crisp (native resolution) but small. Set $ATERM_FONT_PX (e.g. 32 on a
        // 2× display) for comfortable text. A scale-factor of >1 here is the
        // signal that the default will look small; surfaced once, not fatal.
        let scale = window.scale_factor();
        if scale > 1.0 && std::env::var_os("ATERM_FONT_PX").is_none() {
            eprintln!(
                "aterm-gui: display scale {scale}× — default {FONT_PX}px font renders small; \
                 set ATERM_FONT_PX={} for native-size text",
                (FONT_PX * scale as f32).round()
            );
        }
        if self.backend.is_gpu() {
            // GPU mode: a wgpu swapchain on the SAME instance/adapter as the
            // offscreen renderer. The offscreen frame is blitted into it and
            // presented on the GPU — no softbuffer surface is created.
            let (w_px, h_px) = (size.width, size.height);
            match self.backend.gpu_mut().unwrap().create_window_surface(window.clone(), w_px, h_px) {
                Ok(surf) => self.gpu_surface = Some(surf),
                Err(e) => {
                    // A swapchain failure is fatal for the GPU present path (the
                    // CPU softbuffer surface is not built in GPU mode). Surface it
                    // loudly rather than presenting a black window silently.
                    eprintln!("aterm-gui: GPU surface creation failed: {e}");
                }
            }
            self.window = Some(window);
            return;
        }
        let context = softbuffer::Context::new(window.clone()).expect("softbuffer context");
        let surface = softbuffer::Surface::new(&context, window.clone()).expect("softbuffer surface");
        // Drop CoreAnimation's per-frame colour-space conversion (see fn docs):
        // softbuffer tags its content device-RGB; match the window so the
        // compositor doesn't CMS-convert every frame on the main thread.
        #[cfg(target_os = "macos")]
        match_window_colorspace_to_content(&window);
        self.window = Some(window);
        self._context = Some(context);
        self.surface = Some(surface);
    }

    fn user_event(&mut self, el: &ActiveEventLoop, ev: Wake) {
        match ev {
            Wake::Output => {
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            Wake::Exit => el.exit(),
            Wake::Snapshot => self.snapshot(),
            Wake::Bell => self.on_bell(),
            Wake::Control => {
                // Drain off-lock so the control thread can keep queuing, then
                // render each request and reply with the frame dimensions. A
                // dropped receiver (dead client) just makes send() fail; ignore.
                let reqs: Vec<control::ImageReq> =
                    self.image_queue.lock().unwrap().drain(..).collect();
                for req in reqs {
                    let dims = self.render_image(&req.target);
                    let _ = req.reply.send(dims);
                }
            }
            // RES-1: apply a control-socket resize on the geometry-owning main
            // thread — term + PTY + window + framebuffer all updated together, then
            // a redraw requested, so a follow-up `image`/`dims` agrees.
            Wake::Resize { rows, cols } => self.apply_grid_resize(rows, cols),
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::RedrawRequested => self.redraw(),
            WindowEvent::Focused(f) => self.on_focus(f),
            WindowEvent::ModifiersChanged(m) => {
                self.mods = m.state();
                self.update_hover_cursor();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                self.on_key(event);
                if self.should_exit {
                    el.exit();
                }
            }
            // IME-1: composition events. Without this arm winit's IME was dropped
            // by the catch-all, so CJK/dead-key/Option composition never worked.
            WindowEvent::Ime(ime) => match ime {
                Ime::Preedit(text, _cursor) => self.on_ime_preedit(text),
                Ime::Commit(text) => self.on_ime_commit(text),
                // Enabled/Disabled: clear any stale composition so suppression
                // can't get stuck on (e.g. focus loss mid-composition).
                Ime::Enabled | Ime::Disabled => self.on_ime_preedit(String::new()),
            },
            WindowEvent::CursorMoved { position, .. } => {
                self.on_cursor_moved(position.x, position.y);
            }
            WindowEvent::MouseInput { state, button, .. } => self.on_mouse_input(state, button),
            WindowEvent::MouseWheel { delta, .. } => self.on_mouse_wheel(delta),
            WindowEvent::Resized(size) => {
                self.on_resize(size);
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            _ => {}
        }
    }
}

// PTY spawn lives in `aterm-pty` (the single WS-G spawn seam); the frontend
// passes it the shell-integration injection computed below.

/// I-2: invert a frame's RGB in place when a visual-bell flash is `active`,
/// matching the on-screen present's invert (CPU `src ^ 0x00ff_ffff`; the GPU
/// blit shader does the same). Packed `0x00RRGGBB`, so XOR the low 24 bits and
/// leave the unused top byte clear. A no-op when no flash is active, so the
/// steady-screen snapshot path is byte-identical to before.
fn apply_bell_invert(frame: &mut Frame, active: bool) {
    if !active {
        return;
    }
    for px in &mut frame.pixels {
        *px ^= 0x00ff_ffff;
    }
}

/// Make the window's colour space match softbuffer's device-RGB content so
/// CoreAnimation does NOT run a per-frame colour-space conversion on the main
/// thread. softbuffer (`backends/cg.rs`) builds its CGImage with
/// `CGColorSpace::new_device_rgb()`; on a wide-gamut (P3) display CoreAnimation
/// otherwise converts device-RGB → display-P3 on *every* commit
/// (`CA::Render::prepare_image` → `vImageConvert_AnyToAny`) — profiled at ~half of
/// all present cost during heavy output. Tagging the NSWindow device-RGB makes
/// content and window the same space, so the conversion is skipped; the final
/// space→panel mapping is done once by the WindowServer, not per app frame.
/// aterm's framebuffer pixels are unchanged — only the redundant gamut round-trip
/// is removed. `$ATERM_NO_COLORSPACE_MATCH` opts out.
#[cfg(target_os = "macos")]
fn match_window_colorspace_to_content(window: &Window) {
    if std::env::var_os("ATERM_NO_COLORSPACE_MATCH").is_some() {
        return;
    }
    use objc2_app_kit::{NSColorSpace, NSView};
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let Ok(handle) = window.window_handle() else { return };
    let RawWindowHandle::AppKit(h) = handle.as_raw() else { return };
    // SAFETY: `ns_view` points at this window's live NSView (owned by winit for
    // the window's lifetime); we only borrow it — on the main thread, as AppKit
    // requires — to read its `window` and set the colour space.
    let view: &NSView = unsafe { &*(h.ns_view.as_ptr() as *const NSView) };
    let Some(ns_window) = view.window() else { return };
    // SAFETY: standard AppKit calls; device-RGB matches softbuffer's content.
    unsafe {
        let cs = NSColorSpace::deviceRGBColorSpace();
        ns_window.setColorSpace(Some(&cs));
    }
}

/// Prepare OSC 133/633 shell integration for `$SHELL`: returns the `(key, value)`
/// environment additions + an optional argv override (bash's `--rcfile`) to
/// inject into the spawned shell so it emits the command marks the
/// `blocks`/`blocktext`/`wait` introspection verbs surface, plus the raw
/// capability nonce for `Terminal::authorize_shell_integration` so ONLY this
/// shell's marks are trusted. `None` for an unknown shell or on I/O error (the
/// shell still spawns, just without command-block tracking). Runs in the PARENT,
/// before spawn — its file I/O is not async-signal-constrained.
fn prepare_shell_integration() -> Option<(Vec<(String, String)>, Option<Vec<String>>, [u8; 32])> {
    use aterm_core::shell_integration as si;
    let shell = si::ShellType::detect_current();
    let mut injection = si::prepare(shell).ok().flatten()?;
    let nonce = si::generate_nonce();
    si::augment_with_nonce(&mut injection, nonce.hex());
    Some((injection.env_add, injection.argv_override, nonce.into_parts().0))
}

/// Parsed CLI: the `-e` command to run instead of `$SHELL` (if any), the
/// `--working-directory` to start it in (if any), and whether to `--hold` the
/// window open after the command exits.
struct Cli {
    exec_command: Option<Vec<String>>,
    cwd: Option<String>,
    hold: bool,
}

/// Minimal CLI: `aterm-gui [-d DIR] [-e CMD ARGS… | --help | --version]`.
/// `--help`/`--version` print and exit; an unknown option, a `-d` without a valid
/// directory, or `-e` without a command prints a hint and exits non-zero. With no
/// args (a Finder/.app launch) this is a no-op and a normal interactive shell
/// starts in the inherited working directory.
fn parse_cli() -> Cli {
    let mut args = std::env::args().skip(1);
    let mut cwd: Option<String> = None;
    let mut hold = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print!(
                    "{}",
                    concat!(
                        "aterm-gui — a fast, hardened terminal\n\n",
                        "USAGE:\n",
                        "    aterm-gui [OPTIONS]\n",
                        "    aterm-gui [-d <dir>] -e <command> [args...]\n\n",
                        "OPTIONS:\n",
                        "    -e, --command <cmd> [args...]  Run <cmd> in the terminal instead of\n",
                        "                                   $SHELL; the window closes when it exits.\n",
                        "                                   Consumes the rest of the command line.\n",
                        "    -d, --working-directory <dir>  Start the shell/command in <dir>.\n",
                        "        --hold                     Keep the window open after the -e\n",
                        "                                   command exits (close it manually).\n",
                        "    -h, --help                     Print this help and exit.\n",
                        "    -V, --version                  Print the version and exit.\n\n",
                        "KEYS (in the window):\n",
                        "    Cmd-C / Cmd-V     Copy selection / paste (control-stripped, bracketed).\n",
                        "    Cmd-= / Cmd--     Zoom the font in / out.   Cmd-0  Reset zoom.\n",
                        "    Cmd-click         Open a hyperlink / detected URL (http/https/mailto).\n",
                        "    Cmd-F             Find (screen + scrollback): type, Enter/Shift-Enter, Esc.\n",
                        "    Cmd-N / Cmd-W     Open a new window / close this window.\n\n",
                        "ENVIRONMENT:\n",
                        "    ATERM_GPU=1       GPU (Metal) rendering.\n",
                        "    ATERM_FONT_PX=N   Glyph size in physical pixels.\n",
                        "    ATERM_HEADLESS=1  No window; engine + control socket only.\n\n",
                        "CONFIG:\n",
                        "    ~/.config/aterm/aterm.toml  (font_px, gpu, scrollback_lines,\n",
                        "                                cursor_style, cursor_blink, foreground,\n",
                        "                                background, cursor_color,\n",
                        "                                selection_color [#RRGGBB],\n",
                        "                                palette [array of #RRGGBB],\n",
                        "                                columns, lines [initial size]).\n",
                    )
                );
                std::process::exit(0);
            }
            "-V" | "--version" => {
                println!("aterm-gui {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "-d" | "--working-directory" => {
                let Some(dir) = args.next() else {
                    eprintln!("aterm-gui: -d/--working-directory requires a directory (try --help)");
                    std::process::exit(2);
                };
                if !std::path::Path::new(&dir).is_dir() {
                    eprintln!("aterm-gui: not a directory: {dir}");
                    std::process::exit(2);
                }
                cwd = Some(dir);
            }
            "--hold" => hold = true,
            "-e" | "--command" => {
                let cmd: Vec<String> = args.by_ref().collect();
                if cmd.is_empty() {
                    eprintln!("aterm-gui: -e/--command requires a command (try --help)");
                    std::process::exit(2);
                }
                return Cli { exec_command: Some(cmd), cwd, hold };
            }
            other => {
                eprintln!("aterm-gui: unknown option '{other}' (try --help)");
                std::process::exit(2);
            }
        }
    }
    Cli { exec_command: None, cwd, hold }
}

fn main() {
    // CLI first: `-e <cmd>` to run a command instead of $SHELL, `-d <dir>` to set
    // the working directory; `--help`/`--version` print and exit before any setup.
    // A Finder/.app launch passes no args, so this is a no-op there and a normal
    // interactive shell starts.
    let Cli { exec_command, cwd, hold } = parse_cli();
    // Diagnostics first, before any thread spawns: without a logger every
    // aterm_log record — including containment_audit denials — is discarded.
    logging::init();
    // SEC-1: establish the containment mode ONCE, here in the trusted launcher,
    // before any subsystem (the spawn seam, the control socket) queries it. The
    // launcher owns the mode (ATERM_DESIGN §5): `ATERM_CONTAINMENT_MODE` selects
    // it, defaulting to `User` (standard safeguards) for an interactive launch.
    // A bad/unparseable value fails closed: we fall back to `Containment`, the
    // most restrictive mode, and log the rejection rather than silently widening.
    let containment_mode = aterm_containment::init_mode_from_env(ContainmentMode::User)
        .unwrap_or_else(|e| {
            eprintln!(
                "aterm-gui: invalid ATERM_CONTAINMENT_MODE ({e}); falling back to \
                 Containment (most restrictive)"
            );
            // The parse error happened before init_mode ran, so the OnceLock is
            // still unset — set it to the fail-closed default now and proceed.
            let _ = aterm_containment::init_mode(ContainmentMode::Containment);
            ContainmentMode::Containment
        });
    eprintln!("aterm-gui: containment mode = {containment_mode}");
    // $ATERM_HEADLESS: bind the control socket and run the engine + offscreen
    // renderer without ever opening a window (clean automated introspection).
    let headless = std::env::var_os("ATERM_HEADLESS").is_some();
    // User config (~/.config/aterm/aterm.toml). Precedence: env > config > default.
    let config = load_config();
    // Initial grid size: config `columns`/`lines` (clamped sane), else 24×80.
    let cols = config.columns.unwrap_or(80).clamp(20, 500);
    let rows = config.lines.unwrap_or(24).clamp(5, 300);
    // Glyph rasterization size in PHYSICAL pixels. $ATERM_FONT_PX overrides the
    // config / 16 px default (clamped to a sane 6..=200), e.g. 32 on a 2× Retina
    // display for native-size text — see the HiDPI note at window creation.
    let font_px: f32 = std::env::var("ATERM_FONT_PX")
        .ok()
        .and_then(|s| s.parse::<f32>().ok())
        .or(config.font_px)
        .filter(|p| p.is_finite() && *p >= FONT_PX_MIN && *p <= FONT_PX_MAX)
        .unwrap_or(FONT_PX);
    // GPU rasterization: $ATERM_GPU presence wins; else the config's `gpu = true`.
    let want_gpu = std::env::var_os("ATERM_GPU").is_some() || config.gpu == Some(true);
    // Renderer theme (window clear colour, cursor, selection) from config; the
    // engine themes the CELLS, this themes the chrome around them.
    let theme = config.theme();
    // Opt into GPU (wgpu/Metal) rasterization with ATERM_GPU=1; falls back to
    // the CPU renderer if no GPU/font is available. Built FIRST so we can skip
    // the standalone CPU renderer entirely when the GPU path is live (the GPU
    // renderer already carries its own CPU face — a second is wasted RSS).
    // The injected rasterizer (ATERM_DESIGN WS-F): the GPU renderer when
    // ATERM_GPU is set and a device initializes, else the CPU renderer. Exactly
    // ONE is built — the GPU path already carries its own CPU face, so a
    // standalone CPU `Renderer` alongside it would parse the font and build a
    // glyph cache twice for nothing (~several MB idle RSS).
    let build_cpu = || -> Renderer {
        Renderer::from_system(font_px, theme).unwrap_or_else(|| {
            eprintln!("aterm-gui: no system monospace font found (set $ATERM_FONT)");
            std::process::exit(1);
        })
    };
    // `use_gpu` records which path is LIVE (GPU init can fail and fall back to
    // CPU), so live font-zoom rebuilds the backend as the same kind.
    let mut use_gpu = false;
    let backend: Backend = if want_gpu {
        match aterm_gpu::GpuRenderer::new(font_px, theme) {
            Ok(g) => {
                let (name, backend) = g.adapter();
                eprintln!("aterm-gui: GPU rendering on {name} ({backend})");
                use_gpu = true;
                Backend::Gpu(g)
            }
            Err(e) => {
                eprintln!("aterm-gui: GPU unavailable ({e}); using CPU renderer");
                Backend::Cpu(build_cpu())
            }
        }
    } else {
        Backend::Cpu(build_cpu())
    };
    // Fixed per-glyph cell size (from the font); the `dims` verb multiplies it
    // by the grid to report the framebuffer pixel size the renderer produces.
    let (cell_w, cell_h) = backend.cell_size();
    let cell_size = (cell_w as u32, cell_h as u32);
    // Baseline terminal-identity environment for the spawned child, set
    // UNCONDITIONALLY (the deleted `spawn_env::build_spawn_plan` did this): so
    // programs detect an xterm-256color truecolor terminal named aterm, and a
    // Finder/.app launch — which inherits no locale — still gets a UTF-8 one.
    // The pty seam applies these via `setenv(overwrite=1)` in vector order, so
    // the shell-integration vars appended below win on any key collision.
    let mut env_add: Vec<(String, String)> = vec![
        ("TERM".to_string(), "xterm-256color".to_string()),
        ("COLORTERM".to_string(), "truecolor".to_string()),
        ("TERM_PROGRAM".to_string(), "aterm".to_string()),
        ("TERM_PROGRAM_VERSION".to_string(), env!("CARGO_PKG_VERSION").to_string()),
    ];
    // LANG default ONLY when no locale is inherited — never clobber the user's.
    if ["LANG", "LC_ALL", "LC_CTYPE"].iter().all(|k| std::env::var_os(k).is_none()) {
        env_add.push(("LANG".to_string(), "en_US.UTF-8".to_string()));
    }
    // Shell integration (OSC 133/633 command blocks for the AI `blocks` verb) is
    // injected when there is no interactive user to surprise — headless — or on
    // explicit $ATERM_SHELL_INTEGRATION; $ATERM_NO_SHELL_INTEGRATION always opts
    // out. The shell sources the user's own rc and adds the marks; its loader
    // vars are appended after the baseline (no collision with TERM/LANG/…).
    // No shell integration when `-e` runs a command directly: there is no
    // interactive shell to inject OSC 133/633 marks into.
    let integrate = exec_command.is_none()
        && std::env::var_os("ATERM_NO_SHELL_INTEGRATION").is_none()
        && (headless || std::env::var_os("ATERM_SHELL_INTEGRATION").is_some());
    let (argv_override, shell_nonce) = match integrate.then(prepare_shell_integration).flatten() {
        Some((si_env, argv_override, nonce)) => {
            env_add.extend(si_env);
            (argv_override, Some(nonce))
        }
        None => (None, None),
    };
    // SEC-1: gate the single spawn seam on the containment decision. The mode
    // was resolved once at startup (`init_mode_from_env`, see `main`); here we
    // ask the actuator whether the initial shell may spawn for that mode, which
    // ALSO audits the chosen mode and the (honest) fact that no OS-level sandbox
    // is actuated yet — so an unconfined posture is a logged, explicit choice,
    // not a silent gap. A `Deny` fails closed (no shell).
    let mode = aterm_containment::mode_or_containment();
    match aterm_containment::decide_spawn(mode) {
        aterm_containment::SpawnDecision::Permit { os_sandbox, .. } => {
            if !os_sandbox {
                eprintln!(
                    "aterm-gui: containment mode {mode}: OS sandbox NOT actuated \
                     (rlimits + capability gate only); see aterm-containment::actuator"
                );
            }
        }
        // Deny — or any future non-exhaustive variant — fails closed: no shell.
        other => {
            debug_assert!(matches!(other, aterm_containment::SpawnDecision::Deny { .. }));
            eprintln!(
                "aterm-gui: containment mode {mode} denies spawning a shell (fail-closed); \
                 refusing to start an unconfined child"
            );
            std::process::exit(1);
        }
    }
    // The process minting authority is created ONCE here, in the trusted
    // launcher, before any untrusted input is processed. It is the SINGLE
    // `unsafe` root-authority mint in the product (CAP-1): trusted-launcher mint,
    // not a §5.4 sealed-by-reference mint (that is RED roadmap work). It grants
    // the spawn + sandbox capabilities the PTY seam requires.
    // SAFETY: this is the trusted process entry point, called exactly once here
    // before the shell is spawned and before any control-socket/PTY input runs.
    let authority = unsafe { aterm_cap::Authority::root_authority() };
    let spawn_cap = authority.grant::<aterm_cap::effects::Spawn>(aterm_cap::Tier::Trusted);
    let sandbox_cap = authority.grant::<aterm_sandbox::Sandbox>(aterm_cap::Tier::Trusted);
    let master =
        aterm_pty::spawn_shell(rows, cols, &spawn_cap, &sandbox_cap, &env_add, argv_override.as_deref(), exec_command.as_deref(), cwd.as_deref())
            .unwrap_or_else(|e| {
                eprintln!("aterm-gui: spawn failed: {e}");
                std::process::exit(1);
            });
    let term = {
        let mut t = Terminal::new(rows, cols);
        // Apply engine-side config (scrollback, …) before the reader thread starts.
        if let Some(tc) = config.terminal_config() {
            t.apply_config(&tc);
        }
        Arc::new(Mutex::new(t))
    };
    // Trust ONLY this shell's command marks: install its nonce and require it.
    if let Some(nonce) = shell_nonce {
        let mut t = term.lock().unwrap();
        t.authorize_shell_integration(nonce);
        t.set_require_shell_integration_nonce(true);
    }

    // OSC 52 system-clipboard integration: programs (tmux `set-clipboard on`,
    // vim, an ssh+tmux yank) place text on the clipboard via `ESC]52;c;<base64>`.
    // The engine decodes it and invokes this callback; we forward WRITES to
    // pbcopy on a dedicated thread so the blocking subprocess never runs under
    // the Terminal lock (the callback fires inside `process()`). READS (Query)
    // are denied — returning the user's clipboard to a program is a security
    // risk and stays off (gated separately by `allow_osc52_query`, default off).
    {
        let (clip_tx, clip_rx) = std::sync::mpsc::channel::<String>();
        std::thread::spawn(move || {
            while let Ok(content) = clip_rx.recv() {
                control::pbcopy(&content);
            }
        });
        let mut t = term_lock(&term);
        // Opt into OSC 52 WRITE (revoked by default). Write is the expected,
        // useful path (tmux/vim set the clipboard); the lower risk is clipboard
        // injection. QUERY (read) stays revoked — handing the user's clipboard
        // back to a program is the dangerous capability and is left off.
        t.authorize_clipboard_access(ClipboardAccess::Write);
        t.set_clipboard_callback(move |op| {
            match op {
                ClipboardOperation::Set { content, .. } => {
                    let _ = clip_tx.send(content);
                }
                ClipboardOperation::Clear { .. } => {
                    let _ = clip_tx.send(String::new());
                }
                ClipboardOperation::Query { .. } => {}
            }
            None
        });
    }

    // POL-1: install the DEFAULT OSC / escape-sequence policy profile so the
    // profile-based policy engine is LIVE (not just the legacy allow_* booleans).
    // It was built + wired into core but no product ever installed it, leaving
    // the enforcement path dead. Installed HERE — before the PTY reader thread
    // spawns and produces any bytes — so every PTY byte is evaluated against it.
    //
    // The `standard` profile is the sensible interactive default and stays
    // FAIL-CLOSED: the deny-by-default capability gates (OSC 52 clipboard,
    // XTWINOPS) fall back to the legacy authorization bits when no specific rule
    // matches a PTY-origin sequence (`engine_decision_deny_by_default_capability`
    // returns `Fallback`), so the GUI's explicit `authorize_clipboard_access`
    // above still governs clipboard writes — the policy only ADDS enforcement
    // (CSI t dropped for host-origin, notification/palette/response rate limits)
    // on top of the existing posture, never widens it.
    term_lock(&term).apply_policy_engine(aterm_policy::engine::PolicyEngine::new(
        aterm_policy::profiles::standard(),
    ));

    // Block SIGUSR1 process-wide (in the main thread, before spawning any thread,
    // so all threads inherit the block) — a dedicated thread sigwait()s it and
    // requests a self-introspection snapshot. Default SIGUSR1 action would kill
    // the process, so blocking is required.
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        libc::sigaddset(&mut set, libc::SIGUSR1);
        libc::pthread_sigmask(libc::SIG_BLOCK, &set, ptr::null_mut());
    }

    let event_loop = EventLoop::<Wake>::with_user_event().build().expect("event loop");
    let proxy: EventLoopProxy<Wake> = event_loop.create_proxy();

    // BEL -> Wake::Bell. The callback fires inside `process()` on the PTY
    // reader thread, under the Terminal lock, so it must only wake the UI;
    // the main thread does the beep/flash/attention. The engine throttles
    // the callback itself (one per 100ms) against BEL floods.
    {
        let proxy = event_loop.create_proxy();
        term_lock(&term).set_bell_callback(move || {
            let _ = proxy.send_event(Wake::Bell);
        });
    }

    // SIGUSR1 listener -> Wake::Snapshot (introspect the live screen to PNG+txt).
    {
        let proxy = event_loop.create_proxy();
        std::thread::spawn(move || {
            let mut set: libc::sigset_t = unsafe { std::mem::zeroed() };
            unsafe {
                libc::sigemptyset(&mut set);
                libc::sigaddset(&mut set, libc::SIGUSR1);
            }
            loop {
                let mut sig: libc::c_int = 0;
                if unsafe { libc::sigwait(&set, &mut sig) } != 0 {
                    break;
                }
                if proxy.send_event(Wake::Snapshot).is_err() {
                    break;
                }
            }
        });
    }

    // Live introspection control socket: a thread binds a Unix socket and serves
    // newline-delimited requests against this SAME running terminal (read the
    // screen, drive the shell, snapshot pixels, resize). Access control is
    // DEFAULT-ON (see `control_auth`): the socket lives in a per-user 0700 dir
    // ($XDG_RUNTIME_DIR/aterm, else ~/Library/Application Support/aterm), is
    // chmod 0600, accepts only same-uid peers, and requires the per-launch
    // capability token before any verb. Each instance binds its own
    // aterm-<pid>.sock (+ matching token) behind an atomically-updated
    // aterm.sock symlink, so concurrent instances never collide.
    // $ATERM_CONTROL_SOCK overrides the path, or disables the socket entirely
    // with `0`/`off` ($ATERM_NO_CONTROL_SOCK=1 works too).
    let image_queue: control::ImageQueue = Arc::new(Mutex::new(VecDeque::new()));
    let sock_plan = match control_auth::resolve_socket_plan() {
        control_auth::SocketResolution::Enabled(plan) => {
            control::spawn(
                term.clone(),
                master,
                event_loop.create_proxy(),
                image_queue.clone(),
                plan.clone(),
                cell_size,
            );
            Some(plan)
        }
        control_auth::SocketResolution::Disabled => {
            eprintln!("aterm-gui: control socket disabled by environment");
            None
        }
        control_auth::SocketResolution::NoDir => {
            eprintln!(
                "aterm-gui: no per-user runtime dir (set XDG_RUNTIME_DIR, HOME, or \
                 ATERM_CONTROL_SOCK); control socket disabled"
            );
            None
        }
    };

    // Latency self-introspection state (see App::trace_latency). The epoch is a
    // shared monotonic origin so the reader thread and the UI thread produce
    // comparable nanosecond stamps.
    let trace_latency = std::env::var_os("ATERM_TRACE_LATENCY").is_some();
    let lat_epoch = Instant::now();
    let last_output_ns = Arc::new(AtomicU64::new(0));

    // PTY reader thread: read -> feed engine -> wake UI.
    {
        let term = term.clone();
        let alive = Arc::new(AtomicBool::new(true));
        let _alive = alive.clone();
        let last_output_ns = last_output_ns.clone();
        std::thread::spawn(move || {
            // PTY read buffer. The macOS PTY output queue holds far more than the
            // old 8 KiB, so draining it in one syscall (instead of 8×) cuts read
            // round-trips on a heavy output burst — the dominant cost when the
            // engine keeps up and the reader is otherwise read()-blocked.
            // $ATERM_PTY_READ_BUF (bytes, 4 KiB..=4 MiB) overrides for tuning.
            let bufsz = std::env::var("ATERM_PTY_READ_BUF")
                .ok()
                .and_then(|s| s.parse::<usize>().ok())
                .filter(|n| (4096..=4 * 1024 * 1024).contains(n))
                .unwrap_or(65536);
            let mut buf = vec![0u8; bufsz];
            loop {
                let r = aterm_pty::read(master, &mut buf);
                if r <= 0 {
                    // PTY closed (the `-e` command or the shell exited). Normally
                    // close the app; with `--hold`, keep the window so the final
                    // output stays visible — the user closes it themselves
                    // (WindowEvent::CloseRequested → el.exit()).
                    if !hold {
                        let _ = proxy.send_event(Wake::Exit);
                    }
                    break;
                }
                let response = {
                    let mut t = term_lock(&term);
                    t.process(&buf[..r as usize]);
                    t.take_response()
                };
                // Terminal query REPLIES (DSR cursor position, primary/secondary
                // DA, OSC color/theme queries, DECRQM mode reports) must be
                // written back to the program as if typed — apps that query and
                // WAIT for the answer (zsh prompts, fzf, vim theme/cursor probes,
                // readline) otherwise hang or mis-detect. take_response() returns
                // the whole buffer; write it to the PTY master, off the lock.
                if let Some(resp) = response {
                    aterm_pty::write_all(master, &resp);
                }
                // Stamp only the LEADING edge of a burst (set iff currently 0):
                // the first unrendered output marks when content became ready;
                // redraw() clears it after present. Cheap; gated by the reader.
                if trace_latency {
                    let now = lat_epoch.elapsed().as_nanos() as u64;
                    let _ = last_output_ns.compare_exchange(
                        0,
                        now.max(1),
                        Ordering::Relaxed,
                        Ordering::Relaxed,
                    );
                }
                let _ = proxy.send_event(Wake::Output);
            }
        });
    }

    let mut app = App {
        term,
        backend,
        font_px,
        default_font_px: font_px,
        use_gpu,
        theme,
        master,
        rows,
        cols,
        mods: ModifiersState::empty(),
        last_mouse_cell: (0, 0),
        hover_pointer: false,
        selecting: false,
        sel_dragged: false,
        sel_press_cell: (0, 0),
        last_press: None,
        click_count: 0,
        last_mouse_side: SelectionSide::Left,
        gesture: None,
        headless,
        focused: true,
        blink_phase: true,
        next_blink: None,
        bell_flash: BellFlash::new(),
        bell_beep: BellRateLimiter::new(BELL_BEEP_INTERVAL),
        window: None,
        surface: None,
        _context: None,
        gpu_surface: None,
        current_title: "aterm".to_string(),
        image_queue,
        trace_latency,
        lat_epoch,
        last_output_ns,
        input_scratch: RenderInput::empty(),
        last_present: None,
        preedit: String::new(),
        search: None,
        should_exit: false,
    };
    event_loop.run_app(&mut app).expect("run");
    // Graceful-exit cleanup: this instance's socket + token, and the `latest`
    // symlink only while it still points at us (a newer instance may own it).
    // Crash exits are covered by the stale sweep at the next spawn.
    if let Some(plan) = &sock_plan {
        control_auth::cleanup_socket(plan);
    }
}

#[cfg(test)]
mod early_out_tests {
    use super::{RepaintKey, SelectionFingerprint, should_repaint};
    use aterm_core::terminal::{CursorStyle, Terminal};

    /// Build the `RepaintKey` for the current frame exactly as `redraw()` does:
    /// observe the damage epoch, the selection, and the supplied visual-only
    /// state. Returns the key; the caller decides whether to "present" (which in
    /// `redraw()` consumes the damage via `take_damage`).
    fn frame_key(
        term: &mut Terminal,
        blink_phase: bool,
        invert: bool,
        cursor_override: Option<CursorStyle>,
    ) -> RepaintKey {
        RepaintKey {
            damage_epoch: term.damage_epoch(),
            blink_phase,
            invert,
            cursor_override,
            selection: SelectionFingerprint::of(term.text_selection()),
        }
    }

    /// The first frame always paints (no previous present recorded).
    #[test]
    fn first_frame_always_repaints() {
        let mut term = Terminal::new(4, 10);
        let key = frame_key(&mut term, true, false, None);
        assert!(should_repaint(None, key), "the first frame must repaint");
    }

    /// THE acceptance behavior: after a present with NO subsequent terminal
    /// mutation and unchanged blink/flash/selection/focus, the next redraw
    /// decision is "skip" — i.e. `redraw()` would NOT re-rasterize.
    #[test]
    fn steady_screen_skips_rerasterize_after_present() {
        let mut term = Terminal::new(4, 10);
        term.process(b"steady");

        // Frame 1: present (record the key, consume the damage like redraw does).
        let k1 = frame_key(&mut term, true, false, None);
        assert!(should_repaint(None, k1), "first present");
        term.take_damage();
        let last_present = Some(k1);

        // Frame 2: nothing changed (no write, same blink/flash/selection/focus).
        let k2 = frame_key(&mut term, true, false, None);
        assert!(
            !should_repaint(last_present, k2),
            "an unchanged steady screen must skip the extract + rasterize + present"
        );
    }

    /// A blink-phase flip (the cursor-blink-only wake) STILL repaints — the
    /// regression this early-out must not introduce.
    #[test]
    fn blink_flip_still_repaints() {
        let mut term = Terminal::new(4, 10);
        term.process(b"x");
        let k1 = frame_key(&mut term, true, false, None);
        term.take_damage();
        // Blink toggles off; grid is otherwise unchanged.
        let k2 = frame_key(&mut term, false, false, None);
        assert!(should_repaint(Some(k1), k2), "a blink flip must repaint");
    }

    /// A bell-flash toggle, a focus change (cursor override), a selection change,
    /// a scroll, and a write each STILL repaint.
    #[test]
    fn visual_and_grid_changes_still_repaint() {
        let mut term = Terminal::new(4, 10);
        for _ in 0..8 {
            term.process(b"row\r\n");
        }
        let base = frame_key(&mut term, true, false, None);
        term.take_damage();

        // Bell flash on.
        let flash = frame_key(&mut term, true, true, None);
        assert!(should_repaint(Some(base), flash), "a bell flash must repaint");

        // Unfocused: hollow cursor override.
        let unfocused = frame_key(&mut term, true, false, Some(CursorStyle::HollowBlock));
        assert!(should_repaint(Some(base), unfocused), "a focus change must repaint");

        // Selection change (mutates the selection, NOT the grid damage tracker).
        term.text_selection_mut()
            .start_selection(0, 0, aterm_core::selection::SelectionSide::Left, aterm_core::selection::SelectionType::Simple);
        let sel = frame_key(&mut term, true, false, None);
        assert!(should_repaint(Some(base), sel), "a selection change must repaint");
        term.text_selection_mut().clear();
        term.take_damage();

        // A scroll (damages the grid => advances the epoch).
        let pre_scroll = frame_key(&mut term, true, false, None);
        term.take_damage();
        term.grid_mut().scroll_display(2);
        let scrolled = frame_key(&mut term, true, false, None);
        assert!(should_repaint(Some(pre_scroll), scrolled), "a scroll must repaint");
        term.take_damage();

        // A write (damages the grid => advances the epoch).
        let pre_write = frame_key(&mut term, true, false, None);
        term.take_damage();
        term.process(b"more");
        let written = frame_key(&mut term, true, false, None);
        assert!(should_repaint(Some(pre_write), written), "a write must repaint");
    }
}

#[cfg(test)]
mod tests {
    use super::{Config, find_url_span, is_safe_url};

    fn url_at(line: &str, col: usize) -> Option<String> {
        let chars: Vec<char> = line.chars().collect();
        find_url_span(&chars, col).map(|(u, _, _)| u)
    }

    #[test]
    fn config_parsing() {
        // Full config.
        let c: Config = toml::from_str("font_px = 24.0\ngpu = true\nscrollback_lines = 5000").unwrap();
        assert_eq!(c.font_px, Some(24.0));
        assert_eq!(c.gpu, Some(true));
        assert_eq!(c.scrollback_lines, Some(5000));
        // scrollback maps into the engine config: N -> cap, 0 -> unlimited (None).
        assert_eq!(c.terminal_config().unwrap().scrollback_limit, Some(5000));
        let unlimited: Config = toml::from_str("scrollback_lines = 0").unwrap();
        assert_eq!(unlimited.terminal_config().unwrap().scrollback_limit, None);
        // No engine-side keys -> no TerminalConfig to apply.
        assert!(toml::from_str::<Config>("font_px = 18.0").unwrap().terminal_config().is_none());
        // Cursor: shape + blink -> CursorStyle variant.
        use aterm_core::terminal::CursorStyle;
        let c: Config = toml::from_str("cursor_style = \"bar\"\ncursor_blink = false").unwrap();
        assert_eq!(c.terminal_config().unwrap().cursor_style, CursorStyle::SteadyBar);
        let c: Config = toml::from_str("cursor_style = \"underline\"").unwrap();
        assert_eq!(c.terminal_config().unwrap().cursor_style, CursorStyle::BlinkingUnderline);
        // Bad shape falls back (doesn't crash).
        let c: Config = toml::from_str("cursor_style = \"weird\"").unwrap();
        assert_eq!(c.terminal_config().unwrap().cursor_style, CursorStyle::BlinkingBlock);
        // Partial + UNKNOWN keys/tables ignored (forward-compatible).
        let c: Config =
            toml::from_str("gpu = false\nfuture_key = \"x\"\n[unknown]\nk = 1").unwrap();
        assert_eq!(c.font_px, None);
        assert_eq!(c.gpu, Some(false));
        // Empty → all defaults (None).
        let c: Config = toml::from_str("").unwrap();
        assert!(c.font_px.is_none() && c.gpu.is_none());
        // Wrong type → parse error (load_config warns + falls back to defaults).
        assert!(toml::from_str::<Config>("font_px = \"big\"").is_err());
        // Initial size.
        let c: Config = toml::from_str("columns = 120\nlines = 40").unwrap();
        assert_eq!((c.columns, c.lines), (Some(120), Some(40)));
        // Theme colours: hex parses; bad hex warns + is skipped (no crash).
        use super::parse_hex_color;
        assert_eq!(parse_hex_color("#FF8800"), super::Rgb::new(0xFF, 0x88, 0x00).into());
        assert_eq!(parse_hex_color("102030"), super::Rgb::new(0x10, 0x20, 0x30).into());
        assert!(parse_hex_color("#xyz").is_none() && parse_hex_color("#12345").is_none());
        // Renderer theme from config: hex → Theme u32 (0x00RRGGBB).
        let c: Config = toml::from_str(
            "foreground = \"#102030\"\nbackground = \"#405060\"\nselection_color = \"#708090\"",
        )
        .unwrap();
        let th = c.theme();
        assert_eq!((th.fg, th.bg, th.selection), (0x0010_2030, 0x0040_5060, 0x0070_8090));
        // Unset → built-in defaults preserved.
        assert_eq!(Config::default().theme().selection, super::Theme::default().selection);
    }

    /// The real proof themes work: a config colour flows config → engine →
    /// the rendered `RenderCell.fg/bg` (no renderer change involved).
    #[test]
    fn theme_colors_reach_rendercell() {
        use aterm_core::terminal::Terminal;
        let c: Config =
            toml::from_str("foreground = \"#FF8800\"\nbackground = \"#102030\"").unwrap();
        let mut t = Terminal::new(4, 10);
        t.apply_config(&c.terminal_config().unwrap());
        t.process(b"X"); // a default-styled glyph uses the configured theme colours
        let cells = t.render_row(0);
        assert_eq!(cells[0].ch, 'X');
        assert_eq!(cells[0].fg, [0xFF, 0x88, 0x00]);
        assert_eq!(cells[0].bg, [0x10, 0x20, 0x30]);
    }

    /// The palette flows config → engine → rendered cell too: SGR 31 (ANSI index 1)
    /// resolves to the configured palette colour.
    #[test]
    fn palette_colors_reach_rendercell() {
        use aterm_core::terminal::Terminal;
        let c: Config = toml::from_str("palette = [\"#000000\", \"#AB12CD\"]").unwrap();
        let mut t = Terminal::new(4, 10);
        t.apply_config(&c.terminal_config().unwrap());
        t.process(b"\x1b[31mR"); // SGR fg = ANSI 1 (red) → palette index 1
        let cells = t.render_row(0);
        assert_eq!(cells[0].ch, 'R');
        assert_eq!(cells[0].fg, [0xAB, 0x12, 0xCD]);
    }

    /// Cmd-F find core: `find_line_matches` is case-insensitive + non-overlapping,
    /// and highlighting a match via the selection (what `search_apply_current`
    /// does) round-trips to the matched text — so the find genuinely works.
    #[test]
    fn find_matches_and_highlight() {
        use super::find_line_matches;
        let lines = vec![
            (0i32, "Hello hello HELLO".to_string()),
            (1, "world".to_string()),
        ];
        assert_eq!(find_line_matches(&lines, "hello"), vec![(0, 0, 4), (0, 6, 10), (0, 12, 16)]);
        assert!(find_line_matches(&lines, "xyz").is_empty());
        assert!(find_line_matches(&lines, "").is_empty());

        use aterm_core::selection::{SelectionSide, SelectionType};
        use aterm_core::terminal::Terminal;
        let mut t = Terminal::new(4, 20);
        t.process(b"find ME here");
        let row0 = t.get_line_text(0, None).unwrap_or_default();
        let matches = find_line_matches(&[(0i32, row0)], "me");
        assert_eq!(matches.len(), 1);
        let (r, c0, c1) = matches[0];
        let sel = t.text_selection_mut();
        sel.start_selection(r, c0, SelectionSide::Left, SelectionType::Simple);
        sel.update_selection(r, c1, SelectionSide::Right);
        assert_eq!(t.selection_to_string().as_deref(), Some("ME"));
    }

    /// Scrollback find + scroll-to-match (what `search_recompute`/`apply_current`
    /// do): a match in HISTORY has a negative row, and `scroll_to_bottom` +
    /// `scroll_display(-row)` brings it into view (display_offset == -row) with the
    /// selection round-tripping to the matched text.
    #[test]
    fn search_scrollback_scroll_to_match() {
        use super::{MAX_SEARCH_HISTORY, find_line_matches};
        use aterm_core::selection::{SelectionSide, SelectionType};
        use aterm_core::terminal::Terminal;
        let mut t = Terminal::new(4, 20); // 4 visible rows
        for i in 0..12 {
            t.process(format!("LINE{i:02}\r\n").as_bytes()); // ~8 lines scroll off
        }
        t.scroll_to_bottom();
        // Gather scrollback (oldest→newest) + live, like search_recompute.
        let mut lines: Vec<(i32, String)> = Vec::new();
        let mut r = -1;
        while r >= -MAX_SEARCH_HISTORY {
            match t.get_line_text(r, None) {
                Some(s) => lines.push((r, s)),
                None => break,
            }
            r -= 1;
        }
        lines.reverse();
        for r in 0..4 {
            lines.push((r, t.get_line_text(r, None).unwrap_or_default()));
        }
        let matches = find_line_matches(&lines, "line03");
        assert_eq!(matches.len(), 1);
        let (row, c0, c1) = matches[0];
        assert!(row < 0, "LINE03 must be in scrollback, got row {row}");
        // Apply (mirrors search_apply_current).
        t.scroll_to_bottom();
        let sel = t.text_selection_mut();
        sel.start_selection(row, c0, SelectionSide::Left, SelectionType::Simple);
        sel.update_selection(row, c1, SelectionSide::Right);
        t.scroll_display(-row);
        assert_eq!(t.grid().display_offset() as i32, -row, "scrolled the match into view");
        assert_eq!(t.selection_to_string().as_deref(), Some("LINE03"));
    }

    #[test]
    fn url_detection_in_text() {
        let line = "see (http://example.com/p?q=1). end";
        // "http" starts at col 5; the URL spans into the query.
        assert_eq!(url_at(line, 5).as_deref(), Some("http://example.com/p?q=1"));
        assert_eq!(url_at(line, 12).as_deref(), Some("http://example.com/p?q=1"));
        // Trailing `).` is trimmed, so the close-paren col is NOT in the URL.
        let close_paren = line.find(')').unwrap();
        assert_eq!(url_at(line, close_paren), None);
        // Outside any URL.
        assert_eq!(url_at(line, 0), None);
        assert_eq!(url_at(line, 2), None);
        // https + bare URL, whole-span membership.
        let bare = "https://a.b/c";
        assert_eq!(url_at(bare, 0).as_deref(), Some("https://a.b/c"));
        assert_eq!(url_at(bare, bare.len() - 1).as_deref(), Some("https://a.b/c"));
        // Not a URL scheme.
        assert_eq!(url_at("ftp://x.y not-a-link", 0), None);
    }

    #[test]
    fn safe_url_allowlist() {
        // Allowed: http/https/mailto, case-insensitive.
        for ok in [
            "http://example.com",
            "https://example.com/path?q=1#frag",
            "HTTPS://EXAMPLE.COM",
            "mailto:user@example.com",
            "  https://example.com  ", // surrounding whitespace is trimmed
        ] {
            assert!(is_safe_url(ok), "should allow: {ok:?}");
        }
        // Rejected: filesystem, app/custom schemes, injection, empties.
        for bad in [
            "file:///etc/passwd",
            "x-apple-systempreferences:com.apple.preference",
            "javascript:alert(1)",
            "tel:+15551234",
            "ftp://example.com",
            "http://exa mple.com",   // internal whitespace
            "https://e\nvil.com",    // control byte
            "://noscheme",
            "",
            "   ",
        ] {
            assert!(!is_safe_url(bad), "should reject: {bad:?}");
        }
    }
}
