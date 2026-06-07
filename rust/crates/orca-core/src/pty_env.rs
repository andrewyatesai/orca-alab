//! PTY environment construction, ported from `src/main/pty/{terminal-color-env,
//! wsl-orca-env, codex-home-wsl-env}.ts`. Pure env-map manipulation — the actual
//! PTY spawn lives in the IO tier (`orca-pty`).

use std::collections::HashMap;

/// A process environment map.
pub type Env = HashMap<String, String>;

/// Drop a parent-inherited `NO_COLOR`: a terminal emulator shouldn't inherit a
/// launching agent/dev shell's logging choice (login startup files still may).
pub fn remove_inherited_no_color(env: &mut Env) {
    env.remove("NO_COLOR");
}

const WSLENV_ENTRY_SEPARATOR: char = ':';

/// Opt the trusted `ORCA_TERMINAL_HANDLE` into WSL's Windows→Linux env import
/// (`WSLENV`), without duplicating it or disturbing existing entries.
pub fn add_orca_wsl_interop_env(env: &mut Env) {
    let existing = env.get("WSLENV").cloned().unwrap_or_default();
    let mut entries: Vec<String> = existing
        .split(WSLENV_ENTRY_SEPARATOR)
        .filter(|entry| !entry.is_empty())
        .map(str::to_string)
        .collect();
    let has_handle = entries
        .iter()
        .any(|entry| entry.split('/').next() == Some("ORCA_TERMINAL_HANDLE"));
    if !has_handle {
        entries.push("ORCA_TERMINAL_HANDLE/u".to_string());
    }
    env.insert(
        "WSLENV".to_string(),
        entries.join(&WSLENV_ENTRY_SEPARATOR.to_string()),
    );
}

/// `^[A-Za-z]:(?:[\\/]|$)` or `\\` — a Windows path WSL Codex can't use as `CODEX_HOME`.
pub fn is_host_codex_home_for_wsl(value: Option<&str>) -> bool {
    let trimmed = value.map(str::trim).unwrap_or("");
    if trimmed.is_empty() {
        return false;
    }
    let b = trimmed.as_bytes();
    let drive = b.len() >= 2
        && b[0].is_ascii_alphabetic()
        && b[1] == b':'
        && (b.len() == 2 || b[2] == b'\\' || b[2] == b'/');
    drive || trimmed.starts_with("\\\\")
}

/// A Linux path host Codex can't use on Windows.
pub fn is_wsl_codex_home_for_host(value: Option<&str>) -> bool {
    let trimmed = value.map(str::trim).unwrap_or("");
    !trimmed.is_empty() && trimmed.starts_with('/')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> Env {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn removes_inherited_no_color() {
        let mut e = env(&[("NO_COLOR", "1"), ("TERM", "xterm")]);
        remove_inherited_no_color(&mut e);
        assert!(!e.contains_key("NO_COLOR"));
        assert_eq!(e.get("TERM").map(String::as_str), Some("xterm"));
        // Absent is fine.
        remove_inherited_no_color(&mut e);
    }

    #[test]
    fn marks_orca_terminal_handle_for_wsl_import() {
        let mut e = env(&[("ORCA_TERMINAL_HANDLE", "term_wsl")]);
        add_orca_wsl_interop_env(&mut e);
        assert_eq!(e.get("WSLENV").map(String::as_str), Some("ORCA_TERMINAL_HANDLE/u"));
    }

    #[test]
    fn preserves_existing_wslenv_and_does_not_duplicate() {
        let mut e = env(&[("WSLENV", "FOO/u:ORCA_TERMINAL_HANDLE/u:BAR/p")]);
        add_orca_wsl_interop_env(&mut e);
        assert_eq!(
            e.get("WSLENV").map(String::as_str),
            Some("FOO/u:ORCA_TERMINAL_HANDLE/u:BAR/p")
        );
    }

    #[test]
    fn host_codex_home_for_wsl_matches_windows_paths() {
        assert!(is_host_codex_home_for_wsl(Some("C:\\Users\\jin\\.codex")));
        assert!(is_host_codex_home_for_wsl(Some("C:/Users/jin/.codex")));
        assert!(is_host_codex_home_for_wsl(Some("C:")));
        assert!(is_host_codex_home_for_wsl(Some("\\\\server\\share\\.codex")));
        assert!(!is_host_codex_home_for_wsl(Some("/home/jin/.codex")));
        assert!(!is_host_codex_home_for_wsl(Some("")));
        assert!(!is_host_codex_home_for_wsl(None));
    }

    #[test]
    fn wsl_codex_home_for_host_matches_linux_paths() {
        assert!(is_wsl_codex_home_for_host(Some(
            "/home/jin/.local/share/orca/codex-accounts/a/home"
        )));
        assert!(!is_wsl_codex_home_for_host(Some("C:\\Users\\jin\\.codex")));
        assert!(!is_wsl_codex_home_for_host(None));
    }
}
