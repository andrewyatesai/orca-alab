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
}
