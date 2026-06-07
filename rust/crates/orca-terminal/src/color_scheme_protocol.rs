//! Terminal color-scheme (DEC mode 2031 / CSI 997) protocol, ported from
//! `src/shared/terminal-color-scheme-protocol.ts`.
//!
//! Contour/Kitty's "color-scheme update" protocol: a TUI subscribes via
//! `CSI ?2031h`, and the terminal replies `CSI ?997;1n` (dark) / `;2n` (light).
//! [`scan_mode_2031_sequences`] watches a child's output for subscribe/
//! unsubscribe of mode 2031, carrying a `tail` across chunk boundaries so a
//! sequence split mid-stream is still recognized.

use regex::Regex;
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalColorSchemeMode {
    Dark,
    Light,
}

const MODE_2031_SCAN_TAIL_LIMIT: usize = 128;

/// The `CSI 997` status reply the daemon sends for a given scheme.
pub fn mode_2031_sequence_for(mode: TerminalColorSchemeMode) -> &'static str {
    match mode {
        TerminalColorSchemeMode::Dark => "\x1b[?997;1n",
        TerminalColorSchemeMode::Light => "\x1b[?997;2n",
    }
}

/// Resolve the effective scheme from the app `theme` setting (`None` =
/// absent/unset → `"system"`) and the OS preference.
pub fn resolve_terminal_color_scheme_mode(theme: Option<&str>, system_prefers_dark: bool) -> TerminalColorSchemeMode {
    match theme.unwrap_or("system") {
        "dark" => TerminalColorSchemeMode::Dark,
        "light" => TerminalColorSchemeMode::Light,
        _ => {
            if system_prefers_dark {
                TerminalColorSchemeMode::Dark
            } else {
                TerminalColorSchemeMode::Light
            }
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Mode2031ScanResult {
    pub subscribe: bool,
    pub unsubscribe: bool,
    /// Trailing partial escape sequence to prepend to the next chunk.
    pub tail: String,
}

pub fn scan_mode_2031_sequences(previous_tail: &str, data: &str) -> Mode2031ScanResult {
    // Fast path: no carried tail and no ESC/CSI byte means nothing to scan.
    if previous_tail.is_empty() && !data.contains('\x1b') && !data.contains('\u{9b}') {
        return Mode2031ScanResult::default();
    }
    let input = format!("{previous_tail}{data}");
    let mut result =
        Mode2031ScanResult { subscribe: false, unsubscribe: false, tail: extract_private_mode_scan_tail(&input) };
    for captures in private_mode_re().captures_iter(&input) {
        let params = captures.get(1).or_else(|| captures.get(3)).map_or("", |m| m.as_str());
        if !has_mode_2031(params) {
            continue;
        }
        if captures.get(2).or_else(|| captures.get(4)).map_or("", |m| m.as_str()) == "h" {
            result.subscribe = true;
        } else {
            result.unsubscribe = true;
        }
    }
    result
}

fn has_mode_2031(params: &str) -> bool {
    params.split(';').any(|param| param.parse::<u32>() == Ok(2031))
}

fn extract_private_mode_scan_tail(input: &str) -> String {
    let start = match (input.rfind('\x1b'), input.rfind('\u{9b}')) {
        (Some(esc), Some(csi)) => esc.max(csi),
        (Some(esc), None) => esc,
        (None, Some(csi)) => csi,
        (None, None) => return String::new(),
    };
    let tail = &input[start..];
    if tail.chars().count() > MODE_2031_SCAN_TAIL_LIMIT {
        return String::new();
    }
    if tail == "\x1b" || tail == "\x1b[" || tail == "\u{9b}" {
        return tail.to_string();
    }
    if let Some(params) = tail.strip_prefix("\x1b[?") {
        return if is_incomplete_private_mode_params(params) { tail.to_string() } else { String::new() };
    }
    if let Some(params) = tail.strip_prefix("\u{9b}?") {
        return if is_incomplete_private_mode_params(params) { tail.to_string() } else { String::new() };
    }
    String::new()
}

fn is_incomplete_private_mode_params(params: &str) -> bool {
    params.chars().all(|c| c.is_ascii_digit() || c == ';')
}

fn private_mode_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\x1b\[\?([0-9;]+)([hl])|\x9b\?([0-9;]+)([hl])").unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;
    use TerminalColorSchemeMode::{Dark, Light};

    #[test]
    fn maps_mode_2031_replies_to_csi_997_status_reports() {
        assert_eq!(mode_2031_sequence_for(Dark), "\x1b[?997;1n");
        assert_eq!(mode_2031_sequence_for(Light), "\x1b[?997;2n");
    }

    #[test]
    fn resolves_system_color_scheme_from_app_settings_and_system_preference() {
        assert_eq!(resolve_terminal_color_scheme_mode(Some("dark"), false), Dark);
        assert_eq!(resolve_terminal_color_scheme_mode(Some("light"), true), Light);
        assert_eq!(resolve_terminal_color_scheme_mode(Some("system"), true), Dark);
        assert_eq!(resolve_terminal_color_scheme_mode(Some("system"), false), Light);
    }

    #[test]
    fn detects_mode_2031_subscribes_in_compound_and_split_private_mode_sequences() {
        let compound = scan_mode_2031_sequences("", "\x1b[?25;2031h");
        assert!(compound.subscribe);
        assert_eq!(compound.tail, "");

        let first = scan_mode_2031_sequences("", "\x1b[?20");
        assert!(!first.subscribe);
        assert_eq!(first.tail, "\x1b[?20");

        let second = scan_mode_2031_sequences(&first.tail, "31h");
        assert!(second.subscribe);
        assert_eq!(second.tail, "");
    }
}
