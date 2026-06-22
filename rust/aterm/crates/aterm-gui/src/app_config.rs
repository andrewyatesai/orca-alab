// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! Config subsystem: the `aterm.toml` model (`Config`) plus the loaders and
//! precedence resolvers (font px, force scale, grid size, tab-strip rows), AND
//! the `App`-side font/scale/backend/config methods that consume them
//! (set_font_px, rebuild_backend, on_scale_factor_changed, on_resize,
//! reload_config, toggle_fullscreen, hide_app). A verbatim inherent-impl split.

use aterm_core::terminal::{ColorPalette, CursorStyle, Rgb};
use aterm_render::Theme;
use winit::dpi::PhysicalSize;

use crate::input::{InputEvent, Source};
use crate::{
    App, Backend, FONT_PX, FONT_PX_MAX, FONT_PX_MIN, PresentTarget, WindowId, build_backend,
    keybinding, pad_for_scale, term_lock,
};

/// User config file (`$XDG_CONFIG_HOME/aterm/aterm.toml`, else
/// `~/.config/aterm/aterm.toml`). Every field is optional; unknown keys are
/// ignored (forward-compatible). Precedence at startup is env var > config >
/// built-in default, so existing `ATERM_*` usage and `-e`/`-d` flags still win.
/// v1 exposes the settings that were previously env-only; it will grow to mirror
/// the engine's `TerminalConfig` (colours, cursor, scrollback) as themes land.
#[derive(Default, serde::Deserialize)]
#[serde(default)]
pub(crate) struct Config {
    /// Glyph size in physical px (like `$ATERM_FONT_PX`).
    pub(crate) font_px: Option<f32>,
    /// GPU (Metal) rendering (like `$ATERM_GPU`).
    pub(crate) gpu: Option<bool>,
    /// Scrollback history limit, in lines (engine `TerminalConfig.scrollback_limit`;
    /// default 100 000). 0 means unlimited (bounded only by the memory budget).
    pub(crate) scrollback_lines: Option<usize>,
    /// Cursor shape: `"block"` (default), `"underline"`, or `"bar"` (alias `"beam"`).
    pub(crate) cursor_style: Option<String>,
    /// Whether the cursor blinks (default true). Combined with `cursor_style`.
    pub(crate) cursor_blink: Option<bool>,
    /// Theme: default text colour, `#RRGGBB` (engine `default_foreground`).
    pub(crate) foreground: Option<String>,
    /// Theme: default background colour, `#RRGGBB` (engine `default_background`).
    pub(crate) background: Option<String>,
    /// Theme: cursor colour, `#RRGGBB` (engine `cursor_color`).
    pub(crate) cursor_color: Option<String>,
    /// Theme: selection-highlight colour, `#RRGGBB` (renderer `Theme.selection`).
    pub(crate) selection_color: Option<String>,
    /// Theme: indexed palette colours, `#RRGGBB`, by 0-based index (0–15 are the
    /// ANSI/bright set; up to 256). e.g. `palette = ["#1d1f21", "#cc6666", …]`.
    pub(crate) palette: Option<Vec<String>>,
    /// Named built-in colour scheme ("theme palette"), e.g. `theme = "Dracula"`
    /// (case-insensitive). One key sets the default fg/bg, cursor, selection AND the
    /// full ANSI 0–15 palette. The individual `foreground`/`background`/`cursor_color`/
    /// `selection_color`/`palette` keys still layer ON TOP (last-wins). An unknown
    /// name warns and falls back to the built-in default. See [`aterm_types::scheme`].
    pub(crate) theme: Option<String>,
    /// Initial window width in columns (default 80, clamped 20..=500).
    pub(crate) columns: Option<u16>,
    /// Initial window height in rows (default 24, clamped 5..=300).
    pub(crate) lines: Option<u16>,
    /// How many scrollback lines back Cmd-F find scans, plus the live screen
    /// (default 5000 = [`MAX_SEARCH_HISTORY`]). Raise it to search deeper history;
    /// the cost is that each keystroke re-scans up to this many lines, so very
    /// large values can make the find box feel sluggish on a huge scrollback. 0 =
    /// live screen only. Clamped to `i32::MAX`.
    pub(crate) search_history_lines: Option<u32>,
    /// Primary font FAMILY name (e.g. `"JetBrains Mono"`). Resolved to a font
    /// file via [`resolve_font_family`]; on a miss the loader falls back to
    /// `$ATERM_FONT` then the built-in [`FONT_CANDIDATES`], so an unset / unknown
    /// family is byte-identical to before.
    pub(crate) font_family: Option<String>,
    /// macOS: when `true`, the Option (Alt) modifier sends ESC-prefixed (Meta)
    /// key sequences — the standard terminal expectation. When `false`, Option
    /// produces the OS-composed character (`å`) instead. ABSENT keeps the current
    /// default (Meta), so no config = byte-identical. See [`Config::option_as_meta_or_default`].
    pub(crate) option_as_meta: Option<bool>,
    /// User keyboard shortcuts: a `[keybindings]` table mapping chord strings
    /// (`"cmd+shift+t"`) to action names (`"new_tab"`). Parsed into a
    /// `HashMap<Chord, Action>` checked first in `on_key`; a miss falls through to
    /// the hardcoded defaults, and a malformed entry is warned + skipped. ABSENT =
    /// an empty map (the hardcoded path is reached unchanged).
    pub(crate) keybindings: Option<std::collections::BTreeMap<String, String>>,
    /// Rows reserved at the TOP of the window for the in-grid tab strip. DEFAULT is
    /// now `0` ([`DEFAULT_TAB_STRIP_ROWS`]) — the in-grid strip read as a non-native
    /// "ugly frame" drawn inside the terminal, and the native macOS window TOOLBAR
    /// (toolbar.rs) now carries the New Tab affordance. Set `tab_strip_rows = 1` in
    /// config to bring the in-grid strip back. Clamped to [`MAX_TAB_STRIP_ROWS`].
    pub(crate) tab_strip_rows: Option<u16>,
}

/// Default rows reserved for the in-grid tab strip. `0`: OFF by default — tabs live
/// in the native window toolbar (toolbar.rs), not an in-terminal frame. (Opt back in
/// with config `tab_strip_rows = 1`.)
pub(crate) const DEFAULT_TAB_STRIP_ROWS: u16 = 0;
/// Upper clamp on `tab_strip_rows` so a mis-set config can't starve the terminal.
pub(crate) const MAX_TAB_STRIP_ROWS: u16 = 4;

/// Resolve the configured tab-strip row count (env `ATERM_TAB_STRIP_ROWS` wins, then
/// config, then [`DEFAULT_TAB_STRIP_ROWS`]), clamped to `0..=MAX_TAB_STRIP_ROWS`.
/// Env precedence mirrors the other window settings (env > config > default).
pub(crate) fn resolve_tab_strip_rows(config: &Config) -> u16 {
    let raw = std::env::var("ATERM_TAB_STRIP_ROWS")
        .ok()
        .and_then(|v| v.trim().parse::<u16>().ok())
        .or(config.tab_strip_rows)
        .unwrap_or(DEFAULT_TAB_STRIP_ROWS);
    raw.min(MAX_TAB_STRIP_ROWS)
}

impl Config {
    /// Resolve the BASE color scheme this config selects: the named built-in `theme`
    /// (case-insensitive) or the built-in [`aterm_types::ColorScheme::default`] when
    /// no theme — or an unknown one — is set. The per-key color overrides
    /// (`foreground`/…/`palette`) are layered ON TOP of this base by the callers
    /// ([`Self::theme`] and [`Self::terminal_config`]), so they always win.
    fn base_scheme(&self) -> aterm_types::ColorScheme {
        // Resolves SILENTLY (unknown name → Default): both `theme()` and
        // `terminal_config()` call this, so warning here would double-print. The
        // single "unknown theme" diagnostic is emitted in `terminal_config`.
        match self.theme.as_deref() {
            None => aterm_types::ColorScheme::default(),
            Some(name) => aterm_types::scheme::builtin(name).unwrap_or_default(),
        }
    }

    /// The RENDERER theme (window clear colour, cursor, selection highlight). Starts
    /// from the selected scheme's chrome ([`Self::base_scheme`]); the per-key color
    /// keys then override individual slots (unchanged precedence) so the window CLEAR
    /// colour matches a configured `background` and `selection_color` themes the
    /// highlight.
    pub(crate) fn theme(&self) -> Theme {
        let tp = self.base_scheme().to_theme_parts();
        let mut t = Theme {
            fg: tp.fg,
            bg: tp.bg,
            cursor: tp.cursor,
            selection: tp.selection,
        };
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

    /// Whether the Option/Alt modifier should send ESC-prefixed (Meta) sequences.
    /// The DEFAULT when the key is absent is `true` — aterm already routes Option
    /// through the engine encoder, which ESC-prefixes Alt, so "absent = Meta" is
    /// exactly today's behavior (no regression). Setting `option_as_meta = false`
    /// opts into OS-composed characters (`å`) instead.
    pub(crate) fn option_as_meta_or_default(&self) -> bool {
        self.option_as_meta.unwrap_or(true)
    }
}

/// Parse a `#RRGGBB` (or bare `RRGGBB`) hex colour; `None` on malformed input.
pub(crate) fn parse_hex_color(s: &str) -> Option<Rgb> {
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
    pub(crate) fn terminal_config(&self) -> Option<aterm_core::config::TerminalConfig> {
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
                    if blink {
                        CursorStyle::BlinkingBlock
                    } else {
                        CursorStyle::SteadyBlock
                    }
                }
            };
            tc.cursor_blink = blink;
            any = true;
        }
        // A named theme seeds the engine default fg/bg, cursor, and the full ANSI
        // palette; the per-key color blocks below then override individual slots
        // (last-wins). No theme = this block is skipped, so the per-key path stays
        // byte-identical to before.
        if let Some(name) = self.theme.as_deref() {
            // Single point that warns on an unknown theme name (base_scheme resolves
            // silently, so this never double-prints from theme() + here).
            if !name.eq_ignore_ascii_case("default") && aterm_types::scheme::builtin(name).is_none()
            {
                eprintln!(
                    "aterm-gui: config theme: unknown theme {name:?}; using Default (available: {})",
                    aterm_types::scheme::builtin_names().join(", ")
                );
            }
            let s = self.base_scheme();
            tc.default_foreground = s.foreground;
            tc.default_background = s.background;
            if let Some(cur) = s.cursor {
                tc.cursor_color = Some(cur);
            }
            tc.custom_palette = Some(s.to_color_palette());
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
        // Explicit overrides layer ON TOP of the theme's ANSI palette (if a theme set
        // one); without a theme this starts empty — byte-identical to before.
        if let Some(entries) = &self.palette {
            let mut pal = tc.custom_palette.take().unwrap_or_else(ColorPalette::new);
            let mut ok = false;
            for (i, hex) in entries.iter().take(256).enumerate() {
                match parse_hex_color(hex) {
                    Some(rgb) => {
                        pal.set(i as u8, rgb);
                        ok = true;
                    }
                    None => {
                        eprintln!("aterm-gui: config palette[{i}]: expected #RRGGBB, got {hex:?}")
                    }
                }
            }
            // Keep the (possibly theme-seeded) palette if any override landed OR a
            // theme already populated it; else leave custom_palette unset (as before).
            if ok || self.theme.is_some() {
                tc.custom_palette = Some(pal);
                any = true;
            }
        }
        any.then_some(tc)
    }

    /// The engine [`TerminalConfig`] to actually APPLY to terminals: the optional
    /// config deltas ([`Self::terminal_config`]) with the engine's default fg/bg
    /// ALWAYS pinned to the renderer [`Self::theme`].
    ///
    /// The engine's spec default background is black (`0,0,0`) — correct VT
    /// semantics — but the GUI clears the window (and the interior padding) to the
    /// THEME background (`#111318`). Left unsynced, an unstyled cell paints spec-black
    /// while the margins paint the theme bg, so the text area reads visibly *blacker*
    /// than its surroundings (two visual judges flagged this "black-backed text" — see
    /// tools/visual-judge). Pinning the engine defaults to the theme makes a default
    /// cell paint exactly the colour the window clears to. `theme()` already folds in
    /// any `foreground`/`background` config, so an explicit theme is honoured too.
    pub(crate) fn applied_terminal_config(&self) -> aterm_core::config::TerminalConfig {
        let mut tc = self.terminal_config().unwrap_or_default();
        let theme = self.theme();
        let rgb = |c: u32| {
            Rgb::new(
                ((c >> 16) & 0xff) as u8,
                ((c >> 8) & 0xff) as u8,
                (c & 0xff) as u8,
            )
        };
        tc.default_foreground = rgb(theme.fg);
        tc.default_background = rgb(theme.bg);
        tc
    }
}

/// Resolve the config file path without creating anything.
pub(crate) fn config_path() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    if let Some(x) = std::env::var_os("XDG_CONFIG_HOME").filter(|x| !x.is_empty()) {
        return Some(PathBuf::from(x).join("aterm").join("aterm.toml"));
    }
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config/aterm/aterm.toml"))
}

/// Load the user config. A missing file is fine (defaults); a malformed file is
/// reported and ignored rather than aborting the launch.
pub(crate) fn load_config() -> Config {
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

/// Resolve the glyph size in physical px with the canonical precedence
/// `$ATERM_FONT_PX > config.font_px > FONT_PX default`, clamped to the sane
/// `FONT_PX_MIN..=FONT_PX_MAX` bounds. Shared by startup (`main`) and live
/// hot-reload (`App::reload_config`) so a reload re-applies the SAME precedence —
/// an env override still wins after the user edits the config file.
pub(crate) fn resolve_font_px(config: &Config) -> f32 {
    resolve_font_px_with(
        std::env::var("ATERM_FONT_PX").ok().as_deref(),
        config.font_px,
    )
}

/// Parse a non-zero `u16` from an environment variable, returning `None` when the
/// var is unset, empty, unparseable, or zero. Used to let `--columns`/`--lines`
/// (which set `ATERM_COLUMNS`/`ATERM_LINES`) override the config grid size while
/// keeping the same clamp + default fallback the config path already applies.
pub(crate) fn env_u16(key: &str) -> Option<u16> {
    std::env::var(key)
        .ok()?
        .parse::<u16>()
        .ok()
        .filter(|&n| n != 0)
}

/// An explicit render-scale override from `$ATERM_FORCE_SCALE` (set directly or by
/// the `--scale` flag). `Some(f)` for a finite, positive value; `None` when unset
/// or invalid. When set it overrides BOTH the headless 1.0 default and a real
/// window's `scale_factor()`, driving the auto-scaled font (`round(FONT_PX·f)`) and the
/// interior padding (`pad_for_scale(f)`) so an offscreen `image` capture renders at
/// the same DPI a real window of that scale would (e.g. `--scale 2` ≈ 2× Retina).
pub(crate) fn resolve_force_scale() -> Option<f64> {
    std::env::var("ATERM_FORCE_SCALE")
        .ok()?
        .parse::<f64>()
        .ok()
        .filter(|f| f.is_finite() && *f > 0.0)
}

/// Pure precedence core for [`resolve_font_px`], with the `$ATERM_FONT_PX` env
/// value and the config value passed in explicitly so it is deterministically
/// unit-testable (no process-global env mutation). Order: a finite, in-range env
/// value wins; else a finite, in-range config value; else the built-in default.
/// A present-but-unparseable/out-of-range env value falls through to the config,
/// matching the startup `.parse().ok().or(config).filter(in_range)` chain.
pub(crate) fn resolve_font_px_with(env: Option<&str>, config: Option<f32>) -> f32 {
    env.and_then(|s| s.parse::<f32>().ok())
        .or(config)
        .filter(|p| p.is_finite() && *p >= FONT_PX_MIN && *p <= FONT_PX_MAX)
        .unwrap_or(FONT_PX)
}

impl App {
    pub(crate) fn on_resize(&mut self, wid: WindowId, size: PhysicalSize<u32>) {
        let (cw, ch) = self.cell_size();
        // The grid occupies the window MINUS the `2·pad` interior border, so the
        // column/row count divides the inset area, not the raw window size — the
        // inverse of `frame_px`. (`pad == 0` ⇒ byte-identical to before.)
        let pad2 = 2 * self.backend.pad();
        let usable_w = (size.width as usize).saturating_sub(pad2);
        let usable_h = (size.height as usize).saturating_sub(pad2);
        let cols = (usable_w / cw.max(1)).max(1) as u16;
        // The window holds the tab strip ABOVE the terminal: subtract its rows so the
        // terminal grid is the remaining height. Clamp to >=1 row so a window shorter
        // than the strip still leaves one terminal row. With `tab_strip_rows == 0`
        // this is the original full-window grid (byte-identical).
        let win_rows = (usable_h / ch.max(1)).max(1) as u16;
        let rows = win_rows.saturating_sub(self.tab_strip_rows).max(1);
        // Phase 0.5: route through the seam so the window-resize and the control
        // `resize` verb share the one clamp + apply path. `echo_to_window: false`
        // is the KEY (RES-1 regression fix): the window ALREADY has this size (the
        // user is dragging the edge / the WM resized us), so the seam applies the
        // term+PTY+framebuffer resize via `apply_term_resize` WITHOUT calling
        // `request_inner_size` — re-requesting the size would fight an interactive
        // edge-drag and risk a resize feedback loop. Only the `resize` VERB (no
        // window event of its own) sets `echo_to_window: true`. This is a transport
        // flag, not a `Source` branch — both a human-issued and a controller-issued
        // `resize` verb echo the same way.
        self.input(
            wid,
            InputEvent::Resize {
                rows,
                cols,
                echo_to_window: false,
            },
            Source::Human,
        );
    }

    /// Live font zoom (Cmd-+/Cmd--/Cmd-0): rebuild the [`Backend`] at `px`, then
    /// re-grid for the new cell size in the SAME window (more/fewer rows+cols) and
    /// tell the PTY. A failed rebuild (GPU hiccup / no font) keeps the current size
    /// — zoom never crashes. No-op without a window (headless).
    pub(crate) fn set_font_px(&mut self, px: f32) {
        let px = px.clamp(FONT_PX_MIN, FONT_PX_MAX);
        if (px - self.font_px).abs() < 0.5 {
            return;
        }
        self.font_px = px;
        self.rebuild_backend();
    }

    /// Rebuild the [`Backend`] from the CURRENT `self.font_px` + `self.theme`,
    /// re-grid the window for the new cell metrics, and repaint. The single proven
    /// rebuild path shared by live font-zoom ([`Self::set_font_px`]) and live
    /// config hot-reload ([`Self::reload_config`]) — a font-size OR theme change.
    /// A failed rebuild (GPU hiccup / no font) keeps the current backend, so a
    /// reload/zoom never crashes. No-op re-grid without a window (headless).
    pub(crate) fn rebuild_backend(&mut self) {
        // Preserve the interior padding across the rebuild — a fresh backend starts
        // at `pad == 0`, so a font-zoom / config-reload would otherwise drop the
        // border. (The pad is a device-px constant for the session's scale; it does
        // not change with the font size.)
        let pad = self.backend.pad();
        match &mut self.backend {
            Backend::Gpu(g) => {
                // In-place: keep the device + EVERY window's swapchain. Dropping the
                // device would orphan every other window's surface, so the GPU path
                // rebuilds the font/theme on the SAME device.
                if let Err(e) = g.set_font_theme(self.font_px, self.theme) {
                    eprintln!("aterm-gui: GPU font/theme rebuild failed: {e}");
                    return; // keep the current backend; never crash a zoom/reload
                }
            }
            Backend::Cpu(_) => {
                // The CPU renderer owns no device, so a full rebuild is free and safe.
                let Some(backend) = build_backend(
                    self.font_px,
                    self.use_gpu,
                    self.theme,
                    self.font_family.as_deref(),
                ) else {
                    return;
                };
                self.backend = backend;
            }
        }
        self.backend.set_pad(pad);
        // The atlas/face changed, so every window's offscreen + dirty-gate are stale.
        // Reset the per-window GPU caches (the swapchain stays valid — same device) and
        // the introspection scratch, and force a repaint. NOTE: the swapchains and OS
        // windows are untouched, so no surface is orphaned.
        self.introspect_gpu = aterm_gpu::WindowGpu::new();
        for ws in self.windows.values_mut() {
            if let Some(PresentTarget::Gpu { window_gpu, .. }) = &mut ws.present {
                *window_gpu = aterm_gpu::WindowGpu::new();
            }
            ws.last_present = None;
        }
        // Re-grid EVERY window that has an OS window for the new cell metrics (from
        // ITS OWN inner_size), then repaint it. At n==1 this is the one window —
        // identical to the old front-only re-grid.
        let sized: Vec<(WindowId, PhysicalSize<u32>)> = self
            .windows
            .iter()
            .filter_map(|(wid, ws)| ws.os_window.as_ref().map(|w| (*wid, w.inner_size())))
            .collect();
        for (wid, size) in sized {
            self.on_resize(wid, size);
            if let Some(w) = self.windows.get(&wid).and_then(|ws| ws.os_window.as_ref()) {
                w.request_redraw();
            }
        }
    }

    /// HiDPI follow-through for `WindowEvent::ScaleFactorChanged` — a window moved to
    /// a display with a different scale factor (or its display's scale changed). winit
    /// hands us the new factor; re-derive the auto-scaled font (`round(FONT_PX·scale)`) and
    /// interior pad and rebuild, so glyphs stay crisp and correctly sized at the new
    /// DPI. This is the SAME derivation [`Self::attach_os_window`] runs once at window
    /// creation, now applied on the fly instead of being frozen at the creation DPI.
    ///
    /// Honored only for the AUTO font (no `$ATERM_FONT_PX` / `config.font_px`) and
    /// when no scale is force-pinned (`--scale` / `$ATERM_FORCE_SCALE` deliberately
    /// ignore the real monitor — a forced scale must render identically everywhere).
    /// A no-op when neither the font nor the pad would change (a spurious event, or
    /// the initial post-creation event whose scale `attach_os_window` already applied,
    /// or returning to a display at the same DPI).
    ///
    /// SHARED-BACKEND LIMITATION (honest): aterm renders every window through ONE
    /// backend with a single font size and pad, so this re-scales that shared renderer
    /// to the changed window's DPI. For the dominant cases — a single window, or all
    /// windows on equal-DPI displays — it is exactly right. With two windows
    /// simultaneously on displays of DIFFERENT DPI, the most-recently-scaled window
    /// wins; truly independent per-window DPI would require per-window renderers (out
    /// of scope). `rebuild_backend` re-grids every window from its own inner size, and
    /// the natural `Resized` winit emits after this settles the changed window.
    pub(crate) fn on_scale_factor_changed(&mut self, scale: f64) {
        // Explicit font or a force-pinned scale ⇒ DPI is intentionally fixed; ignore.
        if self.font_px_explicit || resolve_force_scale().is_some() {
            return;
        }
        let scaled = (FONT_PX * scale as f32)
            .round()
            .clamp(FONT_PX_MIN, FONT_PX_MAX);
        let new_pad = pad_for_scale(scale);
        // Skip the rebuild when nothing actually changes (e.g. the spurious initial
        // event at the creation DPI, which `attach_os_window` already applied).
        if (scaled - self.font_px).abs() < 0.5 && new_pad == self.backend.pad() {
            return;
        }
        // Seed the new pad BEFORE the rebuild (`rebuild_backend` preserves the live
        // pad across the font swap), and make Cmd-0 reset to this scaled default
        // rather than the tiny FONT_PX base.
        self.backend.set_pad(new_pad);
        self.default_font_px = scaled;
        self.font_px = scaled;
        self.rebuild_backend();
    }

    /// Toggle the window's full-screen state (View ▸ Enter Full Screen). Uses
    /// winit's borderless full-screen on the current monitor — the same path a
    /// future keybinding would use. No-op before a window exists.
    pub(crate) fn toggle_fullscreen(&self) {
        if let Some(w) = self.front().and_then(|ws| ws.os_window.as_ref()) {
            let next = match w.fullscreen() {
                Some(_) => None,
                None => Some(winit::window::Fullscreen::Borderless(None)),
            };
            w.set_fullscreen(next);
        }
    }

    /// App ▸ Hide aterm: hide every aterm window (the standard ⌘H). macOS-only —
    /// AppKit's `NSApplication::hide`; a no-op off macOS (no platform app object).
    #[cfg(target_os = "macos")]
    pub(crate) fn hide_app(&self) {
        use objc2_foundation::MainThreadMarker;
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
        // `[NSApp hide:nil]` on the main thread (the winit loop guarantees
        // `user_event` runs on it). `None` is the `nil` sender. `hide` is a safe
        // binding in objc2-app-kit, so no `unsafe` is needed here.
        app.hide(None);
    }

    /// Non-macOS: no AppKit app object to hide.
    #[cfg(not(target_os = "macos"))]
    pub(crate) fn hide_app(&self) {}

    /// Live config hot-reload (`Wake::ConfigReload`): the user edited
    /// `~/.config/aterm/aterm.toml` and the watcher saw its mtime change. Re-read +
    /// VALIDATE the file, then apply the new settings to every live session
    /// WITHOUT a restart.
    ///
    /// VALIDATION / FAIL-SAFE: `load_config` is the same parser the startup path
    /// uses — a malformed or partial mid-edit file fails to parse, is logged, and
    /// yields `Config::default()`. We must NOT clobber the running config with
    /// those defaults, so a parse failure is detected (re-read the raw text and
    /// re-parse strictly) and the reload is REJECTED, leaving every session
    /// exactly as it was. A missing/unreadable file is treated the same as a parse
    /// failure here: a reload that produced all-defaults is a no-op against the
    /// live state rather than a silent reset to built-ins.
    ///
    /// PRECEDENCE (no regression): font size flows through [`resolve_font_px`] —
    /// the SAME `$ATERM_FONT_PX > config > default` order as startup — so an env
    /// override still wins after an edit. GPU is a launch-time decision and is NOT
    /// hot-swapped here (`self.use_gpu` is fixed); only font size, the renderer
    /// theme, and the engine `TerminalConfig` (scrollback/cursor/colours/palette,
    /// diffed by `Terminal::apply_config`) are re-applied.
    pub(crate) fn reload_config(&mut self) {
        // Re-read + strictly re-parse. A parse error (malformed/partial mid-edit
        // file) or an unreadable/absent file is REJECTED so the live config is
        // never replaced by defaults; the previous config stays intact.
        let Some(path) = config_path() else { return };
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                aterm_log::warn!(
                    "config reload: {} unreadable ({e}); keeping current config",
                    path.display()
                );
                return;
            }
        };
        let config: Config = match toml::from_str(&text) {
            Ok(c) => c,
            Err(e) => {
                aterm_log::warn!(
                    "config reload: {} is invalid ({e}); keeping current config",
                    path.display()
                );
                return;
            }
        };

        // Engine-side config (scrollback/cursor/theme colours/palette). Clearing a
        // previously-set key reverts it: `applied_terminal_config()` rebuilds from a
        // fresh default each reload (the engine diffs via `apply_config`, so a no-op
        // delta is free) while ALWAYS pinning the engine default fg/bg to the theme,
        // so a revert lands on the themed background, never spec-black. Apply to EVERY
        // live tab — window-level config, like a resize — and refresh the factory so
        // future Cmd-T tabs inherit the new config.
        let applied_tc = config.applied_terminal_config();
        for s in self.pool.iter() {
            term_lock(&s.term).apply_config(&applied_tc);
        }
        self.session_factory.terminal_config = Some(applied_tc);

        // Input-level config (no backend rebuild): the keybinding table and the
        // Option-as-Meta flag are re-applied live so an edit takes effect on the
        // next keypress. Clearing the `[keybindings]` table reverts to an empty
        // map (the hardcoded defaults), and dropping `option_as_meta` restores the
        // default (Meta) — both diff-free, costing nothing when unchanged.
        self.keybindings = keybinding::Keybindings::from_config(config.keybindings.as_ref());
        self.option_as_meta = config.option_as_meta_or_default();

        // Tab-strip rows are window chrome: a change re-splits the window between the
        // strip and the terminal, so re-grid (like a resize) if it changed.
        let new_strip = resolve_tab_strip_rows(&config);
        if new_strip != self.tab_strip_rows {
            self.tab_strip_rows = new_strip;
            // The strip is GLOBAL, but `tab_segments`/`last_present` are per-window:
            // clear each window's cache + force a repaint, and re-grid each from its
            // own OS window size (the strip now takes more/fewer rows).
            let sized: Vec<(WindowId, PhysicalSize<u32>)> = self
                .windows
                .iter_mut()
                .filter_map(|(wid, ws)| {
                    ws.tab_segments.clear();
                    ws.last_strip_fp = None; // E3: strip geometry changed
                    ws.last_present = None;
                    ws.os_window.as_ref().map(|w| (*wid, w.inner_size()))
                })
                .collect();
            for (wid, size) in sized {
                self.on_resize(wid, size);
                if let Some(w) = self.windows.get(&wid).and_then(|ws| ws.os_window.as_ref()) {
                    w.request_redraw();
                }
            }
        }

        // GUI-level: renderer theme (window clear colour, cursor, selection),
        // font size, and font family. Rebuild the backend ONLY when something it
        // bakes in actually changed (theme, resolved font px, or family) — a
        // metadata-only save (e.g. a comment edit) then costs nothing visible.
        let new_theme = config.theme();
        // Re-derive the AUTO default font with the SAME HiDPI logic
        // `attach_os_window` / `on_scale_factor_changed` use, so editing an
        // unrelated key (e.g. a colour) on a Retina display does NOT shrink the
        // font back to the 16px base. An explicit env/config font is honored
        // verbatim (and re-pins `font_px_explicit`).
        let font_explicit_now =
            std::env::var_os("ATERM_FONT_PX").is_some() || config.font_px.is_some();
        self.font_px_explicit = font_explicit_now;
        let new_default_font_px = if font_explicit_now {
            resolve_font_px(&config)
        } else {
            let scale = resolve_force_scale()
                .or_else(|| {
                    self.front()
                        .and_then(|ws| ws.os_window.as_ref())
                        .map(|w| w.scale_factor())
                })
                .unwrap_or(1.0);
            if scale > 1.0 {
                (FONT_PX * scale as f32)
                    .round()
                    .clamp(FONT_PX_MIN, FONT_PX_MAX)
            } else {
                FONT_PX
            }
        };
        // Only re-apply the font when the derived default ACTUALLY changed, so a
        // live Cmd-+/Cmd-- zoom survives an unrelated config edit (and Cmd-0 still
        // resets to the up-to-date scaled default). A metadata-only save is then a
        // true no-op (new == old) instead of forcing a font shrink.
        let default_changed = (new_default_font_px - self.default_font_px).abs() >= 0.5;
        self.default_font_px = new_default_font_px;
        let new_font_px = if default_changed {
            new_default_font_px
        } else {
            self.font_px
        };
        // `Theme` is a 4×u32 POD without `PartialEq`; compare its fields directly
        // (the renderer bakes these in, so any change needs a backend rebuild).
        let theme_changed = (
            new_theme.fg,
            new_theme.bg,
            new_theme.cursor,
            new_theme.selection,
        ) != (
            self.theme.fg,
            self.theme.bg,
            self.theme.cursor,
            self.theme.selection,
        );
        let font_changed = (new_font_px - self.font_px).abs() >= 0.5;
        let family_changed = config.font_family != self.font_family;
        if theme_changed || font_changed || family_changed {
            self.theme = new_theme;
            self.font_px = new_font_px;
            self.font_family = config.font_family.clone();
            // The strip rows are painted with the theme colours, so a theme change
            // invalidates every window's E3 strip-row cache (a font/width change
            // already differs the cache key, which folds in `cols`).
            if theme_changed {
                let bg = self.theme.bg;
                for ws in self.windows.values_mut() {
                    ws.last_strip_fp = None;
                    // Keep the seamless titlebar (set_window_background_color) in step
                    // with the new terminal bg, so a live theme change does not reopen a
                    // colour seam between the compact bar and the terminal body.
                    #[cfg(target_os = "macos")]
                    if let Some(w) = ws.os_window.as_ref() {
                        crate::app_window::set_window_background_color(w, bg);
                    }
                }
            }
            self.rebuild_backend();
        } else {
            // No backend rebuild, but the engine config may have changed cells, so
            // still request a repaint (the D-1 early-out skips it if nothing moved).
            if let Some(w) = self.front().and_then(|ws| ws.os_window.as_ref()) {
                w.request_redraw();
            }
        }
    }
}
