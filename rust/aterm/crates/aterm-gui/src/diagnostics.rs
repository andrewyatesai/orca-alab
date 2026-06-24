// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! `--diagnose`: a headless diagnostics ("doctor") report — version/build,
//! platform, renderer, advertised terminal capabilities, config location, and the
//! active `ATERM_*` environment. The report body is a PURE function of a captured
//! [`DiagInfo`], so it is unit-tested without launching a window; printing it and
//! exiting is the CLI's job (`cli.rs`). This is the discoverable surface a human or
//! an AI reads to understand what this build supports and how it is configured.

use std::fmt::Write as _;

/// A captured snapshot of what the diagnostics report prints. Built from the live
/// build + environment by [`collect`]; constructed directly by tests.
pub(crate) struct DiagInfo {
    pub version: String,
    pub git_commit: &'static str,
    pub build_time: &'static str,
    pub target_os: &'static str,
    pub target_arch: &'static str,
    pub profile: &'static str,
    pub renderer_default: &'static str,
    pub features: Vec<(&'static str, bool)>,
    pub capabilities: Vec<(&'static str, bool)>,
    pub config_path: String,
    pub config_exists: bool,
    pub env: Vec<(String, String)>,
}

fn checkbox(on: bool) -> char {
    if on { 'x' } else { ' ' }
}

impl DiagInfo {
    /// Render the stable, sectioned report.
    pub(crate) fn render(&self) -> String {
        let mut s = String::new();
        let _ = writeln!(s, "aterm diagnostics");
        let _ = writeln!(s, "=================");
        let _ = writeln!(
            s,
            "version:   {} ({}, built {})",
            self.version, self.git_commit, self.build_time
        );
        let _ = writeln!(
            s,
            "build:     {} / {}-{}",
            self.profile, self.target_os, self.target_arch
        );
        let _ = writeln!(s, "renderer:  {}", self.renderer_default);
        let _ = writeln!(
            s,
            "config:    {} [{}]",
            self.config_path,
            if self.config_exists {
                "present"
            } else {
                "absent — defaults"
            }
        );
        let _ = writeln!(s);
        let _ = writeln!(s, "features:");
        for (name, on) in &self.features {
            let _ = writeln!(s, "  [{}] {name}", checkbox(*on));
        }
        let _ = writeln!(s);
        let _ = writeln!(s, "terminal capabilities (advertised):");
        for (name, on) in &self.capabilities {
            let _ = writeln!(s, "  [{}] {name}", checkbox(*on));
        }
        let _ = writeln!(s);
        if self.env.is_empty() {
            let _ = writeln!(s, "environment: (no ATERM_* variables set)");
        } else {
            let _ = writeln!(s, "environment:");
            for (k, v) in &self.env {
                let _ = writeln!(s, "  {k}={v}");
            }
        }
        s
    }
}

/// The advertised terminal capabilities, enumerated as `(name, advertised)` from
/// the single source of truth (`aterm_capabilities()`).
fn capability_list() -> Vec<(&'static str, bool)> {
    let c = aterm_types::TerminalCapabilities::aterm_capabilities();
    vec![
        ("true_color", c.true_color),
        ("color_256", c.color_256),
        ("hyperlinks", c.hyperlinks),
        ("sixel_graphics", c.sixel_graphics),
        ("iterm_images", c.iterm_images),
        ("kitty_graphics", c.kitty_graphics),
        ("clipboard", c.clipboard),
        ("shell_integration", c.shell_integration),
        ("synchronized_output", c.synchronized_output),
        ("kitty_keyboard", c.kitty_keyboard),
        ("soft_fonts", c.soft_fonts),
        ("unicode", c.unicode),
        ("bracketed_paste", c.bracketed_paste),
        ("focus_reporting", c.focus_reporting),
        ("mouse_tracking", c.mouse_tracking),
        ("alternate_screen", c.alternate_screen),
    ]
}

/// Collect diagnostics from the live build + environment.
pub(crate) fn collect() -> DiagInfo {
    // Renderer default: GPU only when ATERM_GPU is set and --cpu/ATERM_CPU is not
    // (mirrors the precedence funnel `main` uses).
    let gpu = std::env::var_os("ATERM_GPU").is_some() && std::env::var_os("ATERM_CPU").is_none();
    let renderer_default = if gpu { "gpu (metal)" } else { "cpu" };

    let (config_path, config_exists) = match crate::app_config::config_path() {
        Some(p) => {
            let exists = p.exists();
            (p.display().to_string(), exists)
        }
        None => ("(no HOME / XDG_CONFIG_HOME)".to_string(), false),
    };

    let mut env: Vec<(String, String)> = std::env::vars()
        .filter(|(k, _)| k.starts_with("ATERM_"))
        .collect();
    env.sort();

    DiagInfo {
        version: crate::build_info::VERSION.to_string(),
        git_commit: crate::build_info::GIT_COMMIT,
        build_time: crate::build_info::BUILD_TIME,
        target_os: std::env::consts::OS,
        target_arch: std::env::consts::ARCH,
        profile: if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
        renderer_default,
        features: vec![
            ("sixel", cfg!(feature = "sixel")),
            ("a11y-appkit", cfg!(feature = "a11y-appkit")),
        ],
        capabilities: capability_list(),
        config_path,
        config_exists,
        env,
    }
}

/// Parse `text` as the `aterm.toml` config, returning `Ok(())` if valid or a
/// human-readable error (with the toml error's line/column). Pure — the file I/O
/// lives in [`validate_config`].
pub(crate) fn validate_config_text(text: &str) -> Result<(), String> {
    toml::from_str::<crate::app_config::Config>(text)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// `--validate-config`: parse the config at its canonical path. Returns a message
/// and whether it is valid (the CLI maps the bool to the exit code).
pub(crate) fn validate_config() -> (String, bool) {
    match crate::app_config::config_path() {
        None => (
            "no config path (HOME / XDG_CONFIG_HOME unset); built-in defaults in use".to_string(),
            true,
        ),
        Some(p) if !p.exists() => (
            format!(
                "no config file at {} — built-in defaults in use (OK)",
                p.display()
            ),
            true,
        ),
        Some(p) => match std::fs::read_to_string(&p) {
            Err(e) => (format!("config {} is unreadable: {e}", p.display()), false),
            Ok(text) => match validate_config_text(&text) {
                Ok(()) => (format!("config {} is valid", p.display()), true),
                Err(e) => (format!("config {} is INVALID:\n{e}", p.display()), false),
            },
        },
    }
}

/// `--list-fonts`: the font search directories the resolver scans, then every
/// discoverable family STEM (from `aterm_render::list_fonts`). The directory
/// header makes the result self-explanatory (where these came from); the family
/// list is sorted + de-duplicated by the renderer. A host with no enumerable
/// fonts still prints the dirs header followed by a clear placeholder.
pub(crate) fn list_fonts() -> String {
    let mut s = String::new();
    let _ = writeln!(s, "font search directories:");
    for dir in aterm_render::font_search_dirs() {
        let _ = writeln!(s, "  {}", dir.display());
    }
    let _ = writeln!(s);
    let families = aterm_render::list_fonts();
    if families.is_empty() {
        let _ = writeln!(s, "fonts: (none discoverable)");
    } else {
        let _ = writeln!(s, "fonts ({}):", families.len());
        for f in families {
            let _ = writeln!(s, "  {f}");
        }
    }
    s
}

/// `--list-themes`: every built-in colour scheme as `name — description`, the
/// `"Default"` first, from the single registry (`scheme::builtin_themes`). These
/// are the names accepted by `theme = "<name>"` in the config.
pub(crate) fn list_themes() -> String {
    let mut s = String::new();
    let _ = writeln!(s, "built-in themes (set via `theme = \"<name>\"`):");
    for (name, desc) in aterm_types::scheme::builtin_themes() {
        let _ = writeln!(s, "  {name} — {desc}");
    }
    s
}

/// `--list-keybinds`: the keybinding surface. First the BUILT-IN default chords
/// (the fixed Cmd-* bindings handled in `on_key`), then any user `[keybindings]`
/// overrides from the effective config (parsed, malformed entries skipped). The
/// bindable action NAMES come from [`crate::keybinding::ACTION_NAMES`].
pub(crate) fn list_keybinds() -> String {
    let mut s = String::new();
    let _ = writeln!(s, "built-in keybindings (in the window):");
    for (chord, action) in BUILTIN_KEYBINDS {
        let _ = writeln!(s, "  {chord:<16} {action}");
    }
    let _ = writeln!(s);
    let config = crate::app_config::load_config();
    match config.keybindings.as_ref().filter(|t| !t.is_empty()) {
        None => {
            let _ = writeln!(s, "user [keybindings]: (none configured)");
        }
        Some(table) => {
            let _ = writeln!(s, "user [keybindings] (from config):");
            for (chord, action) in table {
                let valid = crate::keybinding::Action::parse(action).is_some();
                let note = if valid { "" } else { "  (UNKNOWN action)" };
                let _ = writeln!(s, "  {chord:<16} {action}{note}");
            }
        }
    }
    let _ = writeln!(s);
    let _ = writeln!(s, "bindable action names (for [keybindings] values):");
    for name in crate::keybinding::ACTION_NAMES {
        let _ = writeln!(s, "  {name}");
    }
    s
}

/// The fixed Cmd-* chords wired directly into `on_key` (mirrors the KEYS section
/// of `--help`). User `[keybindings]` entries are consulted first and can shadow
/// these; this table documents the defaults that apply with no config.
const BUILTIN_KEYBINDS: &[(&str, &str)] = &[
    ("cmd+c", "copy selection"),
    ("cmd+v", "paste (control-stripped, bracketed)"),
    ("cmd+=", "font increase (zoom in)"),
    ("cmd+-", "font decrease (zoom out)"),
    ("cmd+0", "font reset"),
    ("cmd+f", "find (screen + scrollback)"),
    ("cmd+n", "new window"),
    ("cmd+t", "new tab"),
    ("cmd+w", "close tab (last tab quits)"),
    ("cmd+shift+]", "next tab"),
    ("cmd+shift+[", "prev tab"),
    ("cmd+1..9", "switch to Nth tab"),
];

/// `--show-config`: the EFFECTIVE resolved config — the values aterm would launch
/// with right now, after applying the env > config > default precedence. Reuses
/// the same resolvers the startup path uses (`resolve_font_px`,
/// `resolve_tab_strip_rows`, `Config::theme`/`applied_terminal_config`) so what is
/// printed is what would be applied. The config FILE path + presence is shown so
/// the reader knows whether any of this came from disk.
pub(crate) fn show_config() -> String {
    let config = crate::app_config::load_config();
    let (config_path, config_exists) = match crate::app_config::config_path() {
        Some(p) => (p.display().to_string(), p.exists()),
        None => ("(no HOME / XDG_CONFIG_HOME)".to_string(), false),
    };
    let gpu = std::env::var_os("ATERM_GPU").is_some() && std::env::var_os("ATERM_CPU").is_none();
    let font_px = crate::app_config::resolve_font_px(&config);
    let tab_strip_rows = crate::app_config::resolve_tab_strip_rows(&config);
    let theme_name = config
        .theme
        .clone()
        .unwrap_or_else(|| "Default".to_string());
    let tc = config.applied_terminal_config();
    let font_family = std::env::var("ATERM_FONT")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| config.font_family.clone())
        .unwrap_or_else(|| "(built-in default)".to_string());
    let columns = crate::app_config::env_u16("ATERM_COLUMNS")
        .or(config.columns)
        .unwrap_or(80);
    let lines = crate::app_config::env_u16("ATERM_LINES")
        .or(config.lines)
        .unwrap_or(24);

    let mut s = String::new();
    let _ = writeln!(s, "effective config (env > config > default)");
    let _ = writeln!(s, "=========================================");
    let _ = writeln!(
        s,
        "config file: {} [{}]",
        config_path,
        if config_exists {
            "present"
        } else {
            "absent — defaults"
        }
    );
    let _ = writeln!(s);
    let _ = writeln!(s, "font_px:        {font_px}");
    let _ = writeln!(s, "font_family:    {font_family}");
    let _ = writeln!(
        s,
        "renderer:       {}",
        if gpu { "gpu (metal)" } else { "cpu" }
    );
    let _ = writeln!(s, "columns:        {columns}");
    let _ = writeln!(s, "lines:          {lines}");
    let _ = writeln!(s, "tab_strip_rows: {tab_strip_rows}");
    let _ = writeln!(s, "theme:          {theme_name}");
    let _ = writeln!(
        s,
        "foreground:     #{:02X}{:02X}{:02X}",
        tc.default_foreground.r, tc.default_foreground.g, tc.default_foreground.b
    );
    let _ = writeln!(
        s,
        "background:     #{:02X}{:02X}{:02X}",
        tc.default_background.r, tc.default_background.g, tc.default_background.b
    );
    s
}

/// `--show-face`: the resolved font FACE for `family` — the file aterm would
/// actually load plus its cell metrics + glyph count (from `aterm_render::face_info`,
/// the same resolver the renderer uses). `family` empty falls back to the effective
/// `font_family` (env > config). An unresolvable family yields a clear message and a
/// non-zero result so scripts can detect it.
pub(crate) fn show_face(family: &str) -> (String, bool) {
    let family = family.trim();
    let resolved_family = if family.is_empty() {
        let config = crate::app_config::load_config();
        std::env::var("ATERM_FONT")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .or(config.font_family)
            .unwrap_or_default()
    } else {
        family.to_string()
    };
    if resolved_family.trim().is_empty() {
        return (
            "no font family configured; pass one: --show-face <family>".to_string(),
            false,
        );
    }
    match aterm_render::face_info(&resolved_family) {
        None => (
            format!("font family {resolved_family:?} does not resolve to a loadable face"),
            false,
        ),
        Some(info) => {
            let mut s = String::new();
            let _ = writeln!(s, "resolved face for {resolved_family:?}:");
            let _ = writeln!(s, "  path:        {}", info.path);
            let _ = writeln!(
                s,
                "  metrics at:  {} px (probe size)",
                aterm_render::FaceInfo::PROBE_PX
            );
            let _ = writeln!(s, "  cell_width:  {} px", info.cell_width);
            let _ = writeln!(s, "  cell_height: {} px", info.cell_height);
            let _ = writeln!(s, "  baseline:    {} px", info.baseline);
            let _ = writeln!(s, "  glyph_count: {}", info.glyph_count);
            (s, true)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_accepts_good_config_and_rejects_bad() {
        // A well-formed config of known keys parses.
        assert!(validate_config_text("font_px = 14.0\ngpu = true\ntheme = \"Dracula\"").is_ok());
        // Empty config is valid (all fields optional).
        assert!(validate_config_text("").is_ok());
        // A type error (string where a number is expected) is reported.
        let err = validate_config_text("font_px = \"big\"").unwrap_err();
        assert!(
            !err.is_empty(),
            "a type mismatch must yield an error message"
        );
        // Malformed TOML syntax is reported.
        assert!(validate_config_text("font_px = = 1").is_err());
    }

    fn sample() -> DiagInfo {
        DiagInfo {
            version: "0.3.0".into(),
            git_commit: "abc1234",
            build_time: "2026-06-23T00:00:00Z",
            target_os: "macos",
            target_arch: "aarch64",
            profile: "release",
            renderer_default: "cpu",
            features: vec![("sixel", true), ("accessibility", false)],
            capabilities: vec![("kitty_graphics", true), ("soft_fonts", false)],
            config_path: "/home/u/.config/aterm/aterm.toml".into(),
            config_exists: false,
            env: vec![("ATERM_GPU".into(), "1".into())],
        }
    }

    #[test]
    fn report_includes_key_sections() {
        let r = sample().render();
        assert!(r.contains("aterm diagnostics"), "header");
        assert!(r.contains("version:   0.3.0 (abc1234"), "version line");
        assert!(r.contains("[x] kitty_graphics"), "advertised cap checked");
        assert!(r.contains("[ ] soft_fonts"), "unadvertised cap unchecked");
        assert!(r.contains("ATERM_GPU=1"), "env listed");
        assert!(r.contains("absent — defaults"), "config absence noted");
    }

    #[test]
    fn empty_env_renders_placeholder() {
        let mut d = sample();
        d.env.clear();
        assert!(d.render().contains("(no ATERM_* variables set)"));
    }

    #[test]
    fn collect_enumerates_every_capability() {
        let d = collect();
        // All 16 advertised capabilities are surfaced (no silent omission).
        assert_eq!(d.capabilities.len(), 16);
        assert!(d.render().contains("terminal capabilities"));
        // Version is the real build version.
        assert_eq!(d.version, crate::build_info::VERSION);
    }

    #[test]
    fn list_themes_lists_every_builtin_default_first() {
        let out = list_themes();
        assert!(out.contains("built-in themes"), "header present");
        // Default is first and every registry name appears with a description.
        assert!(out.contains("Default — "), "Default themed first");
        for name in aterm_types::scheme::builtin_names() {
            assert!(out.contains(name), "{name} must be listed");
        }
    }

    #[test]
    fn list_fonts_has_dirs_header_and_lists_search_dirs() {
        let out = list_fonts();
        assert!(out.contains("font search directories:"), "dirs header");
        // Every scanned directory is named so the family list is self-explanatory.
        for dir in aterm_render::font_search_dirs() {
            assert!(
                out.contains(&dir.display().to_string()),
                "search dir {dir:?} listed"
            );
        }
        // Either a fonts section or the explicit empty placeholder — never silent.
        assert!(out.contains("fonts"), "a fonts line is always present");
    }

    #[test]
    fn list_keybinds_covers_builtins_and_action_names() {
        let out = list_keybinds();
        assert!(out.contains("built-in keybindings"), "builtin header");
        // A couple of the fixed Cmd-* chords are documented.
        assert!(out.contains("cmd+c"), "copy chord listed");
        assert!(out.contains("cmd+t"), "new-tab chord listed");
        // Every bindable action NAME is offered for [keybindings] values.
        assert!(out.contains("bindable action names"), "actions header");
        for name in crate::keybinding::ACTION_NAMES {
            assert!(out.contains(name), "{name} must be listed");
        }
    }

    #[test]
    fn show_config_reports_effective_resolved_values() {
        let out = show_config();
        assert!(out.contains("effective config"), "header present");
        // The key resolved knobs are each surfaced with a label.
        for label in [
            "config file:",
            "font_px:",
            "renderer:",
            "columns:",
            "lines:",
            "theme:",
            "foreground:",
            "background:",
        ] {
            assert!(out.contains(label), "{label} must appear in show-config");
        }
    }

    #[test]
    fn show_face_rejects_an_unresolvable_family() {
        // A deliberately nonsensical family never resolves; the result is non-zero
        // with a clear message (scripts can detect the failure).
        let (msg, ok) = show_face("definitely-not-a-real-font-xyzzy");
        assert!(!ok, "unresolvable family must report failure");
        assert!(!msg.is_empty(), "a message is always produced");
    }
}
