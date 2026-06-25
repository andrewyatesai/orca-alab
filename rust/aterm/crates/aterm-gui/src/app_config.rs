// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Andrew Yates

//! Config subsystem: the `aterm.toml` model (`Config`) plus the loaders and
//! precedence resolvers (font px, force scale, grid size, tab-strip rows), AND
//! the `App`-side font/scale/backend/config methods that consume them
//! (set_font_px, rebuild_backend, on_scale_factor_changed, on_resize,
//! reload_config, toggle_fullscreen, hide_app). A verbatim inherent-impl split.

use aterm_core::terminal::{ColorPalette, CursorStyle, Rgb};
use aterm_render::Theme;
use winit::dpi::PhysicalSize;

use crate::input::{InputEvent, Source};
use crate::platform::AppRt;
use crate::{
    App, Backend, FONT_PX, FONT_PX_MAX, FONT_PX_MIN, PresentTarget, WindowId, build_backend,
    hud_bar, keybinding, pad_for_scale, term_lock,
};

/// User config file (`$XDG_CONFIG_HOME/aterm/aterm.toml`, else
/// `~/.config/aterm/aterm.toml`). Every field is optional; unknown keys are
/// ignored (forward-compatible). Precedence at startup is env var > config >
/// built-in default, so existing `ATERM_*` usage and `-e`/`-d` flags still win.
/// v1 exposes the settings that were previously env-only; it will grow to mirror
/// the engine's `TerminalConfig` (colours, cursor, scrollback) as themes land.
#[derive(Default, Clone, serde::Deserialize)]
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
    ///
    /// AUTO LIGHT/DARK SPLIT: `theme = "dark:<name>,light:<name>"` follows the live
    /// OS appearance — aterm switches schemes when the desktop toggles light↔dark
    /// (the same `winit` signal that drives [`crate::app_colorscheme`]). A plain
    /// `theme = "<name>"` is used for BOTH appearances. A split that omits one side
    /// uses the built-in Default for that side. See [`Self::resolve_theme_name`].
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
    /// Window CHROME appearance (titlebar / traffic lights), independent of the
    /// terminal body theme: `"auto"` (default — follow the OS light/dark setting,
    /// including live day-night switches), `"light"`, or `"dark"`. Maps to
    /// [`WindowTheme`] via [`Config::window_theme_or_default`]; an unknown value
    /// warns and falls back to `auto`. macOS-only today (the field is parsed but
    /// inert on other platforms). Replaces the old unconditional dark-chrome force.
    pub(crate) window_theme: Option<String>,
    /// macOS: when `true`, the Option (Alt) modifier sends ESC-prefixed (Meta)
    /// key sequences — the standard terminal expectation. When `false`, Option
    /// produces the OS-composed character (`å`) instead. ABSENT keeps the current
    /// default (Meta), so no config = byte-identical. See [`Config::option_as_meta_or_default`].
    pub(crate) option_as_meta: Option<bool>,
    /// Copy a mouse selection to the system clipboard automatically the moment a
    /// drag-select completes (mouse-up), so no explicit Cmd-C is needed. DEFAULT
    /// OFF (ghostty's own default `copy-on-select = false` — macOS users expect an
    /// explicit copy and the primary-selection convention is X11, not macOS). The
    /// selection is left highlighted either way, so Cmd-C still works. See
    /// [`Config::copy_on_select_or_default`].
    pub(crate) copy_on_select: Option<bool>,
    /// Show the bottom PERFORMANCE HUD (streaming fps/latency/sparkline). Default
    /// ON — the performance GUI ships enabled. Toggleable live via the Performance
    /// control panel or View ▸ Show Performance HUD. See
    /// [`Config::show_perf_hud_or_default`].
    pub(crate) show_perf_hud: Option<bool>,
    /// Show the system-load HUD panel (CPU load + memory). Default ON (ships with the
    /// perf HUD).
    pub(crate) show_sysload_hud: Option<bool>,
    /// Show the network HUD panel (whole-machine rx/tx rate). Default OFF.
    pub(crate) show_network_hud: Option<bool>,
    /// Show the app-fed HUD panel (process-reported metrics, e.g. AI token spend).
    /// Default OFF.
    pub(crate) show_appfed_hud: Option<bool>,
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
    /// BiDi (right-to-left) text handling: `"implicit"` (default — automatic
    /// per-line UAX#9 reordering, so Hebrew/Arabic display in visual order),
    /// `"disabled"` (keep logical order), or `"explicit"` (app-controlled). Maps to
    /// the engine `BiDiConfig.mode`. ABSENT keeps the engine default (Implicit).
    pub(crate) bidi: Option<String>,
    /// East-Asian Ambiguous-width characters: `"narrow"` (default, 1 cell) or
    /// `"wide"` (2 cells). Maps to the engine `ambiguous_width_double`. CJK users
    /// who expect ambiguous glyphs (some punctuation, line-drawing) to be
    /// double-width set `"wide"`. ghostty has no equivalent knob.
    pub(crate) ambiguous_width: Option<String>,
    /// Security opt-in: allow apps to READ the system clipboard via OSC 52
    /// (`Pd = "?"`). Default OFF (fail-closed) — a clipboard read is an
    /// exfiltration vector from untrusted output. Maps to `allow_osc52_query`.
    pub(crate) allow_osc52_query: Option<bool>,
    /// Security opt-in: allow XTWINOPS window manipulation + geometry/title
    /// reports (`CSI t`). Default OFF — title reports can fingerprint and window
    /// moves can hide the window. Maps to `allow_window_ops`.
    pub(crate) allow_window_ops: Option<bool>,
    /// Security opt-in: allow desktop notifications (OSC 9 / 99 / 777). Default
    /// OFF. Maps to `allow_notifications`.
    pub(crate) allow_notifications: Option<bool>,
    /// Security opt-in: allow apps to reconfigure the color palette (OSC 4/104).
    /// Default OFF. Maps to `allow_palette_reconfigure`.
    pub(crate) allow_palette_reconfigure: Option<bool>,
    /// Security opt-in: allow Kitty graphics NON-DIRECT transmission mediums to read
    /// host files / shared memory (`t=f` file, `t=t` temp file, `t=s` POSIX shm).
    /// Default OFF (fail-closed) — letting a program make the terminal read arbitrary
    /// user-readable files off an escape sequence is an exfiltration/abuse surface.
    /// When enabled, a size-capped resolver (`spawn::configure_kitty_file_transfer`)
    /// is installed; otherwise non-direct mediums are skipped cleanly.
    pub(crate) allow_kitty_file_transfer: Option<bool>,
}

/// Default rows reserved for the in-grid tab strip, PER PLATFORM. On macOS this is
/// `0` — tabs live in the native window toolbar (toolbar.rs), so an in-terminal
/// frame would be redundant. On every other platform (Linux/X11) the native toolbar
/// is a no-op ([`crate::platform::AppRtLinux`]), so the in-grid strip is the ONLY
/// tab UI: it defaults to `1` row, otherwise a second/third tab is completely
/// invisible and un-switchable by mouse. Override either way with config
/// `tab_strip_rows = N` or `ATERM_TAB_STRIP_ROWS`.
#[cfg(target_os = "macos")]
pub(crate) const DEFAULT_TAB_STRIP_ROWS: u16 = 0;
/// See the macOS variant above — non-macOS defaults the in-grid strip ON.
#[cfg(not(target_os = "macos"))]
pub(crate) const DEFAULT_TAB_STRIP_ROWS: u16 = 1;
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
    /// Resolve the scheme NAME this config selects for `appearance`, honouring the
    /// optional OS-appearance SPLIT `theme = "dark:<name>,light:<name>"`.
    ///
    /// A plain value with no `dark:`/`light:` prefix is the single theme for BOTH
    /// appearances (unchanged behavior). In the split form the segment matching
    /// `appearance` wins; an omitted side resolves to `None` (the built-in Default).
    /// Keys and the surrounding whitespace are case/space-insensitive; the theme
    /// NAME keeps its original case (so `light:GitHub Light` resolves correctly).
    pub(crate) fn resolve_theme_name(&self, appearance: aterm_types::Appearance) -> Option<String> {
        let raw = self.theme.as_deref()?;
        // A "split" is any comma-segment whose key (before ':') is dark|light.
        let is_split = raw.split(',').any(|seg| {
            seg.split_once(':').is_some_and(|(k, _)| {
                matches!(k.trim().to_ascii_lowercase().as_str(), "dark" | "light")
            })
        });
        if !is_split {
            return Some(raw.trim().to_string());
        }
        let want = match appearance {
            aterm_types::Appearance::Light => "light",
            aterm_types::Appearance::Dark => "dark",
        };
        for seg in raw.split(',') {
            if let Some((key, name)) = seg.split_once(':')
                && key.trim().eq_ignore_ascii_case(want)
                && !name.trim().is_empty()
            {
                return Some(name.trim().to_string());
            }
        }
        None // split form that omits this appearance's side → built-in Default
    }

    /// Resolve the BASE color scheme this config selects for `appearance` (see
    /// [`Self::resolve_theme_name`]): the named built-in (case-insensitive), a user
    /// theme FILE of that name, or the built-in [`aterm_types::ColorScheme::default`]
    /// when no theme — or an unresolvable / malformed one — is set. The per-key color
    /// overrides (`foreground`/…/`palette`) are layered ON TOP of this base by the
    /// callers, so they always win.
    fn base_scheme_for(&self, appearance: aterm_types::Appearance) -> aterm_types::ColorScheme {
        // Resolves SILENTLY (unresolvable/malformed name → Default): both `theme()`
        // and `terminal_config()` call this, so warning here would double-print. The
        // single "unknown theme" diagnostic is emitted in `terminal_config`.
        match self.resolve_theme_name(appearance) {
            None => aterm_types::ColorScheme::default(),
            Some(name) => aterm_types::scheme::load(&name).unwrap_or_default(),
        }
    }

    /// The RENDERER theme (window clear colour, cursor, selection highlight). Starts
    /// from the selected scheme's chrome ([`Self::base_scheme_for`]); the per-key color
    /// keys then override individual slots (unchanged precedence) so the window CLEAR
    /// colour matches a configured `background` and `selection_color` themes the
    /// highlight.
    pub(crate) fn theme(&self) -> Theme {
        self.theme_for(aterm_types::Appearance::Dark)
    }

    /// [`Self::theme`] resolved for a specific OS `appearance` — drives the live
    /// light↔dark scheme switch (see [`Self::resolve_theme_name`]).
    pub(crate) fn theme_for(&self, appearance: aterm_types::Appearance) -> Theme {
        let tp = self.base_scheme_for(appearance).to_theme_parts();
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

    /// Whether a completed mouse selection auto-copies to the clipboard. DEFAULT
    /// when absent is `false` — ghostty's own default (`copy-on-select = false`),
    /// and the macOS expectation is an explicit copy. Setting `copy_on_select =
    /// true` opts into the X11-style copy-on-select convenience.
    pub(crate) fn copy_on_select_or_default(&self) -> bool {
        self.copy_on_select.unwrap_or(false)
    }

    /// Whether to show the bottom performance HUD. Default ON — the performance GUI
    /// ships enabled (toggle it from the Performance control panel, the View menu, or
    /// `show_perf_hud = false` in aterm.toml).
    pub(crate) fn show_perf_hud_or_default(&self) -> bool {
        self.show_perf_hud.unwrap_or(true)
    }

    /// Whether to show the system-load HUD panel (CPU + memory). Default ON — ships with
    /// the perf HUD; override with `show_sysload_hud = false`.
    pub(crate) fn show_sysload_hud_or_default(&self) -> bool {
        self.show_sysload_hud.unwrap_or(true)
    }

    /// Whether to show the network HUD panel (default OFF).
    pub(crate) fn show_network_hud_or_default(&self) -> bool {
        self.show_network_hud.unwrap_or(false)
    }

    /// Whether to show the app-fed HUD panel (default OFF).
    pub(crate) fn show_appfed_hud_or_default(&self) -> bool {
        self.show_appfed_hud.unwrap_or(false)
    }

    /// Resolve the window-chrome appearance ([`WindowTheme`]) from config. The
    /// DEFAULT when the key is absent is [`WindowTheme::Auto`] — follow the OS
    /// effective appearance — so an unset config no longer forces dark chrome on a
    /// light desktop. An unknown / malformed value warns and falls back to `Auto`.
    pub(crate) fn window_theme_or_default(&self) -> WindowTheme {
        match self.window_theme.as_deref() {
            None => WindowTheme::Auto,
            Some(s) => match WindowTheme::parse(s) {
                Some(t) => t,
                None => {
                    eprintln!(
                        "aterm-gui: config window_theme: expected auto|light|dark, got {s:?}; using auto"
                    );
                    WindowTheme::Auto
                }
            },
        }
    }
}

/// Window-CHROME appearance (titlebar + traffic lights), distinct from the
/// terminal-body color scheme. Resolved from config `window_theme` via
/// [`Config::window_theme_or_default`] and applied to the NSWindow appearance in
/// `platform::AppRtMacOS::window_set_appearance` (macOS).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum WindowTheme {
    /// Follow the OS light/dark setting (no `NSAppearance` override), so the
    /// chrome tracks live day-night appearance switches. The default.
    #[default]
    Auto,
    /// Force light chrome (`NSAppearanceNameAqua`).
    Light,
    /// Force dark chrome (`NSAppearanceNameDarkAqua`).
    Dark,
}

impl WindowTheme {
    /// Parse a config `window_theme` value (case-insensitive, trimmed): `auto`,
    /// `light`, or `dark`. `None` on any other value (caller defaults to `Auto`).
    pub(crate) fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "light" => Some(Self::Light),
            "dark" => Some(Self::Dark),
            _ => None,
        }
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
    /// [`Self::terminal_config_for`] at the default (Dark) appearance. Test-only: the
    /// runtime always resolves for the live OS appearance via the `_for` variant.
    #[cfg(test)]
    pub(crate) fn terminal_config(&self) -> Option<aterm_core::config::TerminalConfig> {
        self.terminal_config_for(aterm_types::Appearance::Dark)
    }

    /// [`Self::terminal_config`] resolved for a specific OS `appearance` — picks the
    /// matching side of a `dark:…,light:…` split theme (see [`Self::resolve_theme_name`]).
    pub(crate) fn terminal_config_for(
        &self,
        appearance: aterm_types::Appearance,
    ) -> Option<aterm_core::config::TerminalConfig> {
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
        if let Some(name) = self.resolve_theme_name(appearance) {
            // Single point that warns on a theme that does not resolve to a built-in
            // OR a parseable user theme file (base_scheme_for resolves silently, so this
            // never double-prints from theme() + here). A NotFound names the built-in
            // set + the user theme dir; a Parse error surfaces the offending line.
            if !name.eq_ignore_ascii_case("default") {
                match aterm_types::scheme::load(&name) {
                    Ok(_) => {}
                    Err(aterm_types::scheme::ThemeError::NotFound(_)) => {
                        let where_ = aterm_types::scheme::user_theme_dir()
                            .map(|p| format!(" or a file in {}", p.display()))
                            .unwrap_or_default();
                        eprintln!(
                            "aterm-gui: config theme: unknown theme {name:?}; using Default (built-ins: {}{where_})",
                            aterm_types::scheme::builtin_names().join(", ")
                        );
                    }
                    Err(e) => {
                        eprintln!(
                            "aterm-gui: config theme: failed to load {name:?} ({e}); using Default"
                        );
                    }
                }
            }
            let s = self.base_scheme_for(appearance);
            tc.default_foreground = s.foreground;
            tc.default_background = s.background;
            if let Some(cur) = s.cursor {
                tc.cursor_color = Some(cur);
            }
            if let Some(sel) = s.selection {
                tc.selection_background = Some(sel);
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
        // Selection highlight → engine `selection_background` (OSC-21 queryable). The
        // renderer Theme already carries it for drawing; mirror it into the engine so a
        // configured selection colour is also reported on query, not left as `None`.
        if let Some(s) = &self.selection_color {
            match parse_hex_color(s) {
                Some(rgb) => {
                    tc.selection_background = Some(rgb);
                    any = true;
                }
                None => {
                    eprintln!("aterm-gui: config selection_color: expected #RRGGBB, got {s:?}")
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
        // BiDi mode (engine `BiDiConfig.mode`; applied by Terminal::apply_config).
        if let Some(b) = self.bidi.as_deref() {
            use aterm_core::config::BiDiMode;
            match b.to_ascii_lowercase().as_str() {
                "disabled" | "off" => tc.bidi.mode = BiDiMode::Disabled,
                "implicit" | "on" => tc.bidi.mode = BiDiMode::Implicit,
                "explicit" => tc.bidi.mode = BiDiMode::Explicit,
                other => eprintln!(
                    "aterm-gui: config bidi: expected implicit|disabled|explicit, got {other:?}"
                ),
            }
            any = true;
        }
        // East-Asian Ambiguous width (engine `ambiguous_width_double`).
        if let Some(w) = self.ambiguous_width.as_deref() {
            match w.to_ascii_lowercase().as_str() {
                "narrow" | "single" => tc.ambiguous_width_double = false,
                "wide" | "double" => tc.ambiguous_width_double = true,
                other => eprintln!(
                    "aterm-gui: config ambiguous_width: expected narrow|wide, got {other:?}"
                ),
            }
            any = true;
        }
        // Security opt-ins (all fail-closed by default in TerminalConfig). Only a
        // present key changes the flag, so omitting them keeps the safe default.
        if let Some(v) = self.allow_osc52_query {
            tc.allow_osc52_query = v;
            any = true;
        }
        if let Some(v) = self.allow_window_ops {
            tc.allow_window_ops = v;
            any = true;
        }
        if let Some(v) = self.allow_notifications {
            tc.allow_notifications = v;
            any = true;
        }
        if let Some(v) = self.allow_palette_reconfigure {
            tc.allow_palette_reconfigure = v;
            any = true;
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
        self.applied_terminal_config_for(aterm_types::Appearance::Dark)
    }

    /// [`Self::applied_terminal_config`] resolved for a specific OS `appearance` — the
    /// engine config the GUI applies live when the desktop toggles light↔dark under a
    /// `dark:…,light:…` split theme (see [`Self::resolve_theme_name`]).
    pub(crate) fn applied_terminal_config_for(
        &self,
        appearance: aterm_types::Appearance,
    ) -> aterm_core::config::TerminalConfig {
        let mut tc = self.terminal_config_for(appearance).unwrap_or_default();
        let theme = self.theme_for(appearance);
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

/// Max HUD rows a `win_rows`-tall window can show below a `strip`-row tab strip,
/// always leaving at least one terminal row. The bottom of the HUD stack is dropped
/// past this so the composed frame never exceeds the window (no off-glass clip).
pub(crate) fn hud_cap_for(win_rows: u16, strip: u16) -> u16 {
    win_rows.saturating_sub(strip).saturating_sub(1)
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
        // Reserve the tab strip (top) AND the HUD stack (bottom) so the terminal grid —
        // hence the PTY/shell — never draws under either chrome band. Both are 0 by
        // default ⇒ byte-identical full-window grid. FIT THE CHROME TO THE WINDOW: keep
        // >=1 terminal row and the tab strip, then show as many HUD rows as remain
        // (`hud_cap`); on a window too short for the whole stack the bottom panels are
        // dropped rather than rendering a frame TALLER than the window (which clips
        // off-glass). `hud_cap` is stored so the splice + swapchain agree.
        let hud_cap = hud_cap_for(win_rows, self.tab_strip_rows);
        let eff_hud = self.hud_rows.min(hud_cap);
        if let Some(ws) = self.windows.get_mut(&wid) {
            ws.hud_cap = hud_cap;
        }
        let rows = win_rows
            .saturating_sub(self.tab_strip_rows)
            .saturating_sub(eff_hud)
            .max(1);
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

    /// `hud_rows` = the count of ENABLED HUD panels (each reserves one bottom row).
    /// Kept in sync after any panel toggle / config reload.
    pub(crate) fn recompute_hud_rows(&mut self) {
        self.hud_rows = self.panels.iter().filter(|p| p.enabled()).count() as u16;
    }

    /// Whether the panel with `id` is currently enabled (for menu state + toggles).
    #[must_use]
    pub(crate) fn panel_enabled(&self, id: hud_bar::PanelId) -> bool {
        self.panels.iter().any(|p| p.id() == id && p.enabled())
    }

    /// Toggle a HUD panel on/off, re-gridding every window so the terminal grid
    /// releases / reclaims the panel's bottom row (the bottom analog of changing
    /// `tab_strip_rows`). Shared by the View-menu items and config reload. No-op when
    /// already in the requested state.
    pub(crate) fn set_panel(&mut self, id: hud_bar::PanelId, on: bool) {
        let changed = self.panels.iter_mut().any(|p| {
            if p.id() == id && p.enabled() != on {
                p.set_enabled(on);
                true
            } else {
                false
            }
        });
        if !changed {
            return;
        }
        self.recompute_hud_rows();
        // Re-grid each window from its own OS size (the HUD now takes/frees rows),
        // forcing a fresh present so the band appears/disappears immediately.
        let sized: Vec<(WindowId, PhysicalSize<u32>)> = self
            .windows
            .iter_mut()
            .filter_map(|(wid, ws)| {
                ws.last_present = None;
                ws.next_hud_tick = None; // re-armed by about_to_wait if now on
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
        // Retain the parsed config so a later OS light↔dark switch can re-resolve a
        // `dark:…,light:…` split theme without re-reading disk (see
        // `App::sync_app_theme_to_appearance`). Resolve the engine/renderer theme for
        // the CURRENT OS appearance so a reload preserves the active light/dark side.
        self.config = config.clone();
        let applied_tc = config.applied_terminal_config_for(self.os_appearance);
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
        // Copy-on-select is a live input-policy toggle: a reload that flips it takes
        // effect on the next selection (dropping the key reverts to the off default).
        self.copy_on_select = config.copy_on_select_or_default();

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

        // HUD panels are bottom chrome: toggling any re-grids the window between the
        // terminal and the HUD stack, exactly like the tab strip above.
        for (id, want) in [
            (hud_bar::PanelId::Perf, config.show_perf_hud_or_default()),
            (
                hud_bar::PanelId::SysLoad,
                config.show_sysload_hud_or_default(),
            ),
            (
                hud_bar::PanelId::Network,
                config.show_network_hud_or_default(),
            ),
            (
                hud_bar::PanelId::AppFed,
                config.show_appfed_hud_or_default(),
            ),
        ] {
            if want != self.panel_enabled(id) {
                self.set_panel(id, want);
            }
        }

        // GUI-level: renderer theme (window clear colour, cursor, selection),
        // font size, and font family. Rebuild the backend ONLY when something it
        // bakes in actually changed (theme, resolved font px, or family) — a
        // metadata-only save (e.g. a comment edit) then costs nothing visible.
        let new_theme = config.theme_for(self.os_appearance);
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
                let apprt = &self.apprt;
                for ws in self.windows.values_mut() {
                    ws.last_strip_fp = None;
                    // Keep the seamless titlebar (window_set_background_color) in step
                    // with the new terminal bg, so a live theme change does not reopen a
                    // colour seam between the compact bar and the terminal body. No-op
                    // off macOS (the Linux apprt does nothing here).
                    if let Some(w) = ws.os_window.as_ref() {
                        apprt.window_set_background_color(w, bg);
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

#[cfg(test)]
mod cfg_engine_tests {
    use super::Config;
    use aterm_core::config::BiDiMode;

    fn cfg(toml: &str) -> Config {
        toml::from_str(toml).expect("valid toml")
    }

    #[test]
    fn bidi_disabled_maps_to_engine() {
        let tc = cfg("bidi = \"disabled\"")
            .terminal_config()
            .expect("bidi sets engine config");
        assert_eq!(tc.bidi.mode, BiDiMode::Disabled);
    }

    #[test]
    fn bidi_explicit_and_case_insensitive() {
        let tc = cfg("bidi = \"Explicit\"").terminal_config().unwrap();
        assert_eq!(tc.bidi.mode, BiDiMode::Explicit);
    }

    #[test]
    fn ambiguous_width_wide_maps_to_double() {
        let tc = cfg("ambiguous_width = \"wide\"").terminal_config().unwrap();
        assert!(tc.ambiguous_width_double);
        let tc = cfg("ambiguous_width = \"narrow\"")
            .terminal_config()
            .unwrap();
        assert!(!tc.ambiguous_width_double);
    }

    #[test]
    fn absent_keys_leave_engine_defaults_and_no_config() {
        // No engine-affecting keys -> terminal_config() is None (GUI skips apply).
        assert!(cfg("font_px = 14.0").terminal_config().is_none());
        // bidi default stays Implicit when only an unrelated key is set elsewhere.
        let tc = cfg("bidi = \"implicit\"").terminal_config().unwrap();
        assert_eq!(tc.bidi.mode, BiDiMode::Implicit);
    }

    #[test]
    fn security_flags_opt_in() {
        let tc = cfg("allow_osc52_query = true\nallow_window_ops = true\n\
             allow_notifications = true\nallow_palette_reconfigure = true")
        .terminal_config()
        .unwrap();
        assert!(tc.allow_osc52_query);
        assert!(tc.allow_window_ops);
        assert!(tc.allow_notifications);
        assert!(tc.allow_palette_reconfigure);
    }

    #[test]
    fn security_flags_fail_closed_when_absent() {
        // A config that sets only an unrelated engine key must NOT enable any
        // security flag — they stay fail-closed (default false).
        let tc = cfg("scrollback_lines = 5000").terminal_config().unwrap();
        assert!(!tc.allow_osc52_query);
        assert!(!tc.allow_window_ops);
        assert!(!tc.allow_notifications);
        assert!(!tc.allow_palette_reconfigure);
    }
}

#[cfg(test)]
mod window_theme_tests {
    use super::{Config, WindowTheme};

    fn cfg(toml: &str) -> Config {
        toml::from_str(toml).expect("valid toml")
    }

    #[test]
    fn window_theme_defaults_to_auto_when_absent() {
        // No key at all -> Auto (follow the OS), so a light desktop is no longer
        // forced dark.
        assert_eq!(
            Config::default().window_theme_or_default(),
            WindowTheme::Auto
        );
        assert_eq!(
            cfg("font_px = 14.0").window_theme_or_default(),
            WindowTheme::Auto
        );
    }

    #[test]
    fn window_theme_auto_light_dark_parse() {
        assert_eq!(
            cfg("window_theme = \"auto\"").window_theme_or_default(),
            WindowTheme::Auto
        );
        assert_eq!(
            cfg("window_theme = \"light\"").window_theme_or_default(),
            WindowTheme::Light
        );
        assert_eq!(
            cfg("window_theme = \"dark\"").window_theme_or_default(),
            WindowTheme::Dark
        );
    }

    #[test]
    fn window_theme_is_case_insensitive_and_trimmed() {
        assert_eq!(
            cfg("window_theme = \" Dark \"").window_theme_or_default(),
            WindowTheme::Dark
        );
        assert_eq!(
            cfg("window_theme = \"LIGHT\"").window_theme_or_default(),
            WindowTheme::Light
        );
    }

    #[test]
    fn window_theme_invalid_defaults_to_auto() {
        assert_eq!(
            cfg("window_theme = \"midnight\"").window_theme_or_default(),
            WindowTheme::Auto
        );
        assert_eq!(
            cfg("window_theme = \"\"").window_theme_or_default(),
            WindowTheme::Auto
        );
        // Direct parser: unknown -> None (caller defaults).
        assert_eq!(WindowTheme::parse("nope"), None);
        assert_eq!(WindowTheme::parse("auto"), Some(WindowTheme::Auto));
    }
}

#[cfg(test)]
mod hud_fit_tests {
    use super::hud_cap_for;

    #[test]
    fn chrome_fits_the_window_and_never_clips() {
        // Plenty of room: all desired HUD rows fit.
        assert_eq!(hud_cap_for(100, 1), 98);
        // Exactly enough for a 4-panel stack + 1-row strip + 1 terminal row.
        assert_eq!(hud_cap_for(6, 1), 4);
        // One row too short for 4 panels → cap drops to 3 (bottom panel hidden).
        assert_eq!(hud_cap_for(5, 1), 3);
        // Window only fits terminal + strip → no HUD rows.
        assert_eq!(hud_cap_for(2, 1), 0);
        assert_eq!(hud_cap_for(1, 0), 0);

        // Invariant across sizes: terminal stays >=1 row AND the composed frame
        // (terminal + strip + effective HUD) never exceeds the window.
        for win in 1u16..=40 {
            for strip in 0u16..=2 {
                for desired_hud in 0u16..=4 {
                    let eff = desired_hud.min(hud_cap_for(win, strip));
                    let term = win.saturating_sub(strip).saturating_sub(eff).max(1);
                    assert!(term >= 1, "win={win} strip={strip}: terminal underflowed");
                    if win > strip {
                        assert!(
                            term + strip + eff <= win,
                            "win={win} strip={strip} hud={desired_hud}: frame {} > window",
                            term + strip + eff
                        );
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod split_theme_tests {
    use super::Config;
    use aterm_types::Appearance;

    fn cfg(toml: &str) -> Config {
        toml::from_str(toml).expect("valid toml")
    }

    /// A plain `theme = "<name>"` (no `dark:`/`light:` prefix) resolves to the SAME
    /// name for both appearances — unchanged behavior, even for a multi-word name.
    #[test]
    fn plain_theme_used_for_both_appearances() {
        let c = cfg(r#"theme = "Tokyo Night""#);
        assert_eq!(
            c.resolve_theme_name(Appearance::Dark).as_deref(),
            Some("Tokyo Night")
        );
        assert_eq!(
            c.resolve_theme_name(Appearance::Light).as_deref(),
            Some("Tokyo Night")
        );
        // …and the resolved renderer Theme is identical across appearances.
        assert_eq!(
            c.theme_for(Appearance::Dark).bg,
            c.theme_for(Appearance::Light).bg
        );
    }

    /// The split form picks the segment matching the OS appearance; the two sides
    /// resolve to DIFFERENT schemes (different rendered background).
    #[test]
    fn split_picks_matching_side() {
        let c = cfg(r#"theme = "dark:Dracula,light:GitHub Light""#);
        assert_eq!(
            c.resolve_theme_name(Appearance::Dark).as_deref(),
            Some("Dracula")
        );
        assert_eq!(
            c.resolve_theme_name(Appearance::Light).as_deref(),
            Some("GitHub Light")
        );
        // End-to-end: each side equals naming that scheme directly, and they differ.
        assert_eq!(
            c.theme_for(Appearance::Dark).bg,
            cfg(r#"theme = "Dracula""#).theme_for(Appearance::Dark).bg
        );
        assert_eq!(
            c.theme_for(Appearance::Light).bg,
            cfg(r#"theme = "GitHub Light""#)
                .theme_for(Appearance::Light)
                .bg
        );
        assert_ne!(
            c.theme_for(Appearance::Dark).bg,
            c.theme_for(Appearance::Light).bg,
            "the two sides must render different backgrounds"
        );
        // GitHub Light's background is pure white (#ffffff) on the light side.
        assert_eq!(c.theme_for(Appearance::Light).bg, 0x00FF_FFFF);
    }

    /// Keys are case/whitespace-insensitive; the theme NAME keeps its original case
    /// and surrounding spaces are trimmed.
    #[test]
    fn split_keys_case_and_whitespace_insensitive() {
        let c = cfg(r#"theme = " DARK : Solarized Dark , Light : Solarized Light ""#);
        assert_eq!(
            c.resolve_theme_name(Appearance::Dark).as_deref(),
            Some("Solarized Dark")
        );
        assert_eq!(
            c.resolve_theme_name(Appearance::Light).as_deref(),
            Some("Solarized Light")
        );
    }

    /// A split that OMITS one side resolves that appearance to `None` (built-in
    /// Default), while the present side still resolves.
    #[test]
    fn split_omitted_side_is_default() {
        let c = cfg(r#"theme = "light:GitHub Light""#);
        assert_eq!(c.resolve_theme_name(Appearance::Dark), None);
        assert_eq!(
            c.resolve_theme_name(Appearance::Light).as_deref(),
            Some("GitHub Light")
        );
        // The dark side renders the built-in Default background.
        assert_eq!(
            c.theme_for(Appearance::Dark).bg,
            aterm_types::ColorScheme::default().to_theme_parts().bg
        );
    }

    /// No `theme` key → `None` for both appearances (built-in Default everywhere).
    #[test]
    fn absent_theme_is_none() {
        let c = cfg("font_px = 14.0");
        assert_eq!(c.resolve_theme_name(Appearance::Dark), None);
        assert_eq!(c.resolve_theme_name(Appearance::Light), None);
    }

    /// The engine config (palette + default bg) also tracks the split, so the live
    /// switch re-colours cells, not just the chrome.
    #[test]
    fn applied_terminal_config_tracks_split() {
        let c = cfg(r#"theme = "dark:Dracula,light:GitHub Light""#);
        let dark = c.applied_terminal_config_for(Appearance::Dark);
        let light = c.applied_terminal_config_for(Appearance::Light);
        assert_ne!(
            dark.default_background, light.default_background,
            "the engine default background must differ between the two sides"
        );
        // Light side's engine default bg is GitHub Light's white.
        assert_eq!(
            light.default_background,
            aterm_types::Rgb::new(0xff, 0xff, 0xff)
        );
    }
}
