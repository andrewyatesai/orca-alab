//! Agent recognition from terminal titles and process names, ported from
//! `src/shared/agent-name-token-match.ts` and the config-free parts of
//! `agent-process-recognition.ts`.
//!
//! Agent names must match as **whole tokens**, never substrings — substring
//! matching mis-fired on cwd/worktree titles like `opencode-blinker` (⊃
//! `opencode`) or `openclaude` (⊃ `claude`). The TS uses regex lookbehind
//! (`(?<![\w./\\-])…`), which Rust's `regex` crate does not support, so the
//! boundary guard is hand-rolled here.

use crate::js_string::trim_js;

/// Agent names matched in OSC terminal titles. Intentionally narrower than the
/// full launchable set (short names like `amp` would mis-classify shell titles).
pub const AGENT_NAMES: &[&str] = &[
    "claude",
    "openclaude",
    "codex",
    "copilot",
    "cursor",
    "gemini",
    "antigravity",
    "opencode",
    "mimo",
    "openclaw",
    "aider",
    "grok",
    "devin",
];

const WINDOWS_EXE_SUFFIXES: &[&str] = &[".exe", ".cmd", ".bat", ".ps1"];

/// A char that may NOT abut an agent token: `[\w./\\-]` (ASCII word char, `.`,
/// `/`, `\`, `-`).
fn is_boundary_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '/' | '\\' | '-')
}

/// Whole-token match of `name` in `title` (case-insensitive). When
/// `allow_exe_suffix`, an optional `.exe`/`.cmd`/`.bat`/`.ps1` may follow the
/// name before the right boundary (Windows launcher process names).
pub fn title_has_token(title: &str, name: &str, allow_exe_suffix: bool) -> bool {
    // The TS twin (`buildAgentNameRe`) matches with a case-insensitive REGEX
    // ('i' flag = ECMAScript simple case fold), NOT `.toLowerCase()`. Rust's
    // full `to_lowercase()` would fold e.g. U+212A KELVIN→'k' (and length-change
    // U+0130→"i̇"), which the JS regex does not — so use ASCII folding to match
    // the regex for these ASCII agent tokens.
    let haystack: Vec<char> = title.to_ascii_lowercase().chars().collect();
    let needle: Vec<char> = name.to_ascii_lowercase().chars().collect();
    if needle.is_empty() || needle.len() > haystack.len() {
        return false;
    }
    let suffixes: Vec<Vec<char>> = if allow_exe_suffix {
        WINDOWS_EXE_SUFFIXES.iter().map(|s| s.chars().collect()).collect()
    } else {
        Vec::new()
    };

    for start in 0..=(haystack.len() - needle.len()) {
        if haystack[start..start + needle.len()] != needle[..] {
            continue;
        }
        // Left boundary: start of string, or a non-boundary char before.
        if start != 0 && is_boundary_char(haystack[start - 1]) {
            continue;
        }
        // Optionally consume an exe-like suffix, then require a right boundary.
        let after = start + needle.len();
        let mut end = after;
        for suffix in &suffixes {
            if after + suffix.len() <= haystack.len()
                && haystack[after..after + suffix.len()] == suffix[..]
            {
                end = after + suffix.len();
                break;
            }
        }
        if end == haystack.len() || !is_boundary_char(haystack[end]) {
            return true;
        }
    }
    false
}

/// True when `title` contains `name` (an `AGENT_NAMES` entry) as a whole token.
pub fn title_has_agent_name(title: &str, name: &str) -> bool {
    if !AGENT_NAMES.contains(&name) {
        return false;
    }
    title_has_token(title, name, true)
}

/// True when `title` contains any `AGENT_NAMES` entry as a whole token.
pub fn title_has_any_legacy_agent_name(title: &str) -> bool {
    AGENT_NAMES.iter().any(|name| title_has_token(title, name, true))
}

/// `droid`/`hermes`/`agy` are token-matched without an exe suffix — `android`
/// contains `droid`, and cwd titles like `~/hermes/working` must not count.
pub fn title_has_droid(title: &str) -> bool {
    title_has_token(title, "droid", false)
}
pub fn title_has_hermes(title: &str) -> bool {
    title_has_token(title, "hermes", false)
}
pub fn title_has_agy(title: &str) -> bool {
    title_has_token(title, "agy", false)
}

const PROCESS_EXTENSIONS: &[&str] = &[".exe", ".cmd", ".bat", ".ps1"];

/// Normalise a process name to a comparable basename: unquote, take the path
/// basename, lowercase, strip a trailing executable extension.
pub fn normalize_process_name(process_name: Option<&str>) -> String {
    let raw = match process_name {
        // Twins TS `normalizeProcessName`'s `.trim()` — JS trim set, not Rust's
        // `str::trim` (they differ on U+FEFF / U+0085; see js_string).
        Some(name) => trim_js(name),
        None => return String::new(),
    };
    if raw.is_empty() {
        return String::new();
    }
    // Strip one leading and one trailing quote.
    let mut unquoted = raw;
    if unquoted.starts_with('"') || unquoted.starts_with('\'') {
        unquoted = &unquoted[1..];
    }
    if unquoted.ends_with('"') || unquoted.ends_with('\'') {
        unquoted = &unquoted[..unquoted.len() - 1];
    }
    let basename = unquoted.rsplit(['/', '\\']).next().unwrap_or(unquoted);
    let lower = basename.to_lowercase();
    for ext in PROCESS_EXTENSIONS {
        if let Some(stripped) = lower.strip_suffix(ext) {
            return stripped.to_string();
        }
    }
    lower
}

/// True when `process_name` is the `expected_process` (basename-normalised),
/// allowing a dotted sub-variant (`expected.foo`).
pub fn is_expected_agent_process(process_name: Option<&str>, expected_process: &str) -> bool {
    let normalized = normalize_process_name(process_name);
    let expected = normalize_process_name(Some(expected_process));
    if normalized.is_empty() || expected.is_empty() {
        return false;
    }
    normalized == expected || normalized.starts_with(&format!("{expected}."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_match_rejects_substrings_and_compounds() {
        // substring of another name / hyphen compound / path segment must NOT match
        assert!(!title_has_agent_name("openclaude", "claude"));
        assert!(!title_has_agent_name("opencode-blinker", "opencode"));
        assert!(!title_has_agent_name("~/opencode/working", "opencode"));
    }

    #[test]
    fn token_match_accepts_whole_tokens_and_exe_suffix() {
        assert!(title_has_agent_name("claude working", "claude"));
        assert!(title_has_agent_name("running codex now", "codex"));
        assert!(title_has_agent_name("claude.exe ready", "claude"));
        assert!(title_has_agent_name("claude", "claude"));
    }

    #[test]
    fn any_legacy_agent_name_detection() {
        assert!(title_has_any_legacy_agent_name("codex • ~/repo"));
        assert!(!title_has_any_legacy_agent_name("timestamp ready"));
        assert!(!title_has_any_legacy_agent_name("openclaude-blinker")); // compound of openclaude
    }

    #[test]
    fn droid_hermes_agy_token_matching_without_exe_suffix() {
        assert!(title_has_droid("droid ready"));
        assert!(!title_has_droid("android ready"));
        assert!(!title_has_droid("droid.exe")); // no exe suffix allowed for droid
        assert!(title_has_hermes("hermes working"));
        assert!(!title_has_hermes("~/hermes/working"));
        assert!(title_has_agy("agy now"));
    }

    #[test]
    fn normalize_process_name_cases() {
        assert_eq!(normalize_process_name(Some("/usr/local/bin/claude")), "claude");
        assert_eq!(
            normalize_process_name(Some(r"C:\Users\dev\AppData\Roaming\npm\claude.exe")),
            "claude"
        );
        assert_eq!(normalize_process_name(Some("\"command-code.cmd\"")), "command-code");
        assert_eq!(normalize_process_name(Some("cmd.exe")), "cmd");
        assert_eq!(normalize_process_name(None), "");
    }

    #[test]
    fn is_expected_agent_process_cases() {
        assert!(is_expected_agent_process(
            Some(r"C:\Users\dev\AppData\Roaming\npm\claude.exe"),
            "claude"
        ));
        assert!(is_expected_agent_process(Some("/usr/local/bin/claude"), "claude"));
        assert!(!is_expected_agent_process(Some("powershell.exe"), "claude"));
        assert!(!is_expected_agent_process(Some("/usr/local/bin/openclaude"), "claude"));
    }

    #[test]
    fn token_match_uses_ascii_fold_like_the_js_regex_i_flag() {
        // ASCII case folding still works.
        assert!(title_has_token("GROK ready", "grok", false));
        // The TS twin matches via a case-insensitive regex ('i' = simple fold),
        // which does NOT fold U+212A KELVIN SIGN to 'k' — so neither must Rust,
        // or a title ending in the Kelvin sign would spuriously match `grok`.
        assert!(!title_has_token("gro\u{212A}", "grok", false));
    }

    #[test]
    fn normalize_process_name_trims_the_js_whitespace_set() {
        // JS `.trim()` strips U+FEFF (BOM) but NOT U+0085 (NEL); Rust `str::trim`
        // does the opposite. normalize_process_name twins the TS `.trim()`.
        assert_eq!(normalize_process_name(Some("\u{FEFF}claude")), "claude");
        assert!(is_expected_agent_process(Some("\u{FEFF}claude"), "claude"));
        // NEL is not in the JS trim set, so it is retained (matching JS).
        assert_eq!(normalize_process_name(Some("\u{85}claude")), "\u{85}claude");
    }
}
