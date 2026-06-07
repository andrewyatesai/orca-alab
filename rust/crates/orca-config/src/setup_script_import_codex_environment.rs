//! Codex environment setup-script import, ported from
//! `src/shared/setup-script-import-codex-environment.ts`.
//!
//! Parses the hand-rolled minimal subset of TOML used by Codex environment
//! files (`.codex/environments/environment.toml`) to extract the `[setup]` and
//! `[cleanup]` `script = …` values (basic / literal / multiline strings), and
//! flags unsupported `actions` config. The file read is injected (the IO
//! boundary), so this stays pure and testable.

use crate::setup_script_imports::SetupScriptImportCandidate;

const CODEX_ENVIRONMENT_PATH: &str = ".codex/environments/environment.toml";

struct CodexEnvironmentToml {
    setup_script: Option<String>,
    cleanup_script: Option<String>,
    unsupported_fields: Vec<String>,
}

/// `read_file(path) -> Some(contents)` / `None`. Returns `Some(candidate)` only
/// when a non-empty `[setup]` script is present; the returned candidate always
/// carries a non-empty `setup`.
#[cfg_attr(trust_verify, trust::ensures(|out: &Option<SetupScriptImportCandidate>|
    out.as_ref().map_or(true, |candidate| !candidate.setup.is_empty())))]
pub fn inspect_codex_environment_config(
    read_file: &dyn Fn(&str) -> Option<String>,
) -> Option<SetupScriptImportCandidate> {
    // `!content` in TS treats both a missing file and an empty string as absent.
    let content = read_file(CODEX_ENVIRONMENT_PATH).filter(|text| !text.is_empty())?;

    let parsed = parse_codex_environment_toml(&content);
    let setup = parsed.setup_script.as_deref().map(str::trim).unwrap_or("");
    if setup.is_empty() {
        return None;
    }

    let archive = parsed
        .cleanup_script
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string);

    Some(SetupScriptImportCandidate {
        provider: "codex".to_string(),
        label: "Codex environment".to_string(),
        files: vec![CODEX_ENVIRONMENT_PATH.to_string()],
        setup: setup.to_string(),
        archive,
        unsupported_fields: parsed.unsupported_fields,
    })
}

fn parse_codex_environment_toml(content: &str) -> CodexEnvironmentToml {
    // `content.split(/\r?\n/)`: normalize CRLF then split on LF (a bare CR that
    // is not followed by LF is preserved, matching the regex).
    let normalized = content.replace("\r\n", "\n");
    let lines: Vec<&str> = normalized.split('\n').collect();

    let mut unsupported_fields: Vec<String> = Vec::new();
    let mut section = String::new();
    let mut setup_script: Option<String> = None;
    let mut cleanup_script: Option<String> = None;

    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        let trimmed = line.trim();
        if matches_actions_assignment(trimmed) {
            unsupported_fields.push("actions".to_string());
        }
        if let Some(name) = parse_section_header(trimmed) {
            section = name.to_string();
            if section == "actions" || section.starts_with("actions.") {
                unsupported_fields.push(format!("[{section}]"));
            }
            index += 1;
            continue;
        }

        if section == "setup" || section == "cleanup" {
            if let Some(raw_value) = parse_script_assignment(line) {
                let parsed = parse_toml_string_value(&lines, index, raw_value);
                // Skip the lines a multiline string consumed.
                index = parsed.end_line_index;
                if section == "setup" {
                    setup_script = Some(parsed.value);
                } else {
                    cleanup_script = Some(parsed.value);
                }
            }
        }
        index += 1;
    }

    CodexEnvironmentToml { setup_script, cleanup_script, unsupported_fields }
}

/// `/^actions\s*=/` against the trimmed line.
fn matches_actions_assignment(trimmed: &str) -> bool {
    match trimmed.strip_prefix("actions") {
        Some(rest) => rest.trim_start().starts_with('='),
        None => false,
    }
}

/// `/^\[([A-Za-z0-9_.-]+)\]\s*(?:#.*)?$/` — returns the section name on match.
fn parse_section_header(trimmed: &str) -> Option<&str> {
    let rest = trimmed.strip_prefix('[')?;
    let name_end = rest.find(|c: char| !is_section_char(c)).unwrap_or(rest.len());
    if name_end == 0 {
        return None;
    }
    let name = &rest[..name_end];
    let after = rest[name_end..].strip_prefix(']')?.trim_start();
    (after.is_empty() || after.starts_with('#')).then_some(name)
}

fn is_section_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-'
}

/// `/^\s*script\s*=\s*(.*)$/` against the raw line — returns the captured value.
fn parse_script_assignment(line: &str) -> Option<&str> {
    let rest = line.trim_start().strip_prefix("script")?;
    let rest = rest.trim_start().strip_prefix('=')?;
    Some(rest.trim_start())
}

struct ParsedTomlValue {
    value: String,
    end_line_index: usize,
}

fn parse_toml_string_value(lines: &[&str], start_line_index: usize, raw_value: &str) -> ParsedTomlValue {
    let value = raw_value.trim_start();
    if value.starts_with("\"\"\"") || value.starts_with("'''") {
        let delimiter = if value.starts_with("\"\"\"") { "\"\"\"" } else { "'''" };
        return parse_toml_multiline_string(lines, start_line_index, &value[3..], delimiter);
    }
    if value.starts_with('"') {
        return ParsedTomlValue { value: parse_toml_basic_string(value), end_line_index: start_line_index };
    }
    if value.starts_with('\'') {
        return ParsedTomlValue { value: parse_toml_literal_string(value), end_line_index: start_line_index };
    }
    ParsedTomlValue { value: strip_inline_comment_and_trim(value), end_line_index: start_line_index }
}

fn parse_toml_multiline_string(
    lines: &[&str],
    start_line_index: usize,
    first_line_remainder: &str,
    delimiter: &str,
) -> ParsedTomlValue {
    let mut content = String::new();
    let mut remainder = first_line_remainder;
    let mut index = start_line_index;
    while index < lines.len() {
        if index > start_line_index {
            remainder = lines[index];
        }
        if let Some(close_index) = remainder.find(delimiter) {
            return ParsedTomlValue {
                value: format!("{content}{}", &remainder[..close_index]),
                end_line_index: index,
            };
        }
        content.push_str(remainder);
        content.push('\n');
        index += 1;
    }
    ParsedTomlValue {
        value: content.trim_end().to_string(),
        end_line_index: lines.len().saturating_sub(1),
    }
}

fn parse_toml_basic_string(value: &str) -> String {
    let close = find_toml_string_close(value, '"');
    let raw = &value[..close];
    // `JSON.parse(raw)` decodes the escapes; fall back to stripping the quotes.
    match serde_json::from_str::<String>(raw) {
        Ok(parsed) => parsed,
        Err(_) => drop_first_last_char(raw),
    }
}

fn parse_toml_literal_string(value: &str) -> String {
    let chars: Vec<(usize, char)> = value.char_indices().collect();
    let count = chars.len();
    // `findTomlStringEnd(value, "'")`: first `'` after index 0, else `length - 1`.
    let mut end_char_index = count.saturating_sub(1);
    let mut k = 1;
    while k < count {
        if chars[k].1 == '\'' {
            end_char_index = k;
            break;
        }
        k += 1;
    }
    let start_byte = chars.get(1).map_or(value.len(), |&(byte, _)| byte);
    let end_byte = chars.get(end_char_index).map_or(value.len(), |&(byte, _)| byte);
    if start_byte >= end_byte {
        return String::new();
    }
    value[start_byte..end_byte].to_string()
}

/// `value.slice(0, findTomlStringEnd(value, quote) + 1)` end offset (in bytes).
/// Always `<= value.len()`.
#[cfg_attr(trust_verify, trust::ensures(|out: &usize| *out <= value.len()))]
fn find_toml_string_close(value: &str, quote: char) -> usize {
    let chars: Vec<(usize, char)> = value.char_indices().collect();
    let mut k = 1;
    while k < chars.len() {
        let (byte_idx, c) = chars[k];
        if c == quote && (quote == '\'' || !is_escaped(&chars, k)) {
            return byte_idx + c.len_utf8();
        }
        k += 1;
    }
    value.len()
}

fn is_escaped(chars: &[(usize, char)], index: usize) -> bool {
    let mut slash_count = 0usize;
    let mut cursor = index;
    while cursor > 0 && chars[cursor - 1].1 == '\\' {
        slash_count += 1;
        cursor -= 1;
    }
    slash_count % 2 == 1
}

/// `value.replace(/\s+#.*$/, '').trim()` — drop the first inline comment
/// (whitespace-run followed by `#`) and trim.
fn strip_inline_comment_and_trim(value: &str) -> String {
    let mut run_start: Option<usize> = None;
    let mut cut: Option<usize> = None;
    for (i, c) in value.char_indices() {
        if c == '#' {
            if let Some(start) = run_start {
                cut = Some(start);
                break;
            }
        }
        if c.is_whitespace() {
            if run_start.is_none() {
                run_start = Some(i);
            }
        } else {
            run_start = None;
        }
    }
    match cut {
        Some(start) => value[..start].trim().to_string(),
        None => value.trim().to_string(),
    }
}

/// `raw.slice(1, -1)` — drop the first and last char (the surrounding quotes).
fn drop_first_last_char(value: &str) -> String {
    let mut chars = value.chars();
    chars.next();
    chars.next_back();
    chars.as_str().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reader(files: Vec<(&'static str, &'static str)>) -> impl Fn(&str) -> Option<String> {
        move |path| {
            files
                .iter()
                .find(|(name, _)| *name == path)
                .map(|(_, content)| content.to_string())
        }
    }

    // Derived from `setup-script-imports.test.ts`
    // "imports setup and cleanup scripts from Codex environment config".
    #[test]
    fn imports_setup_and_cleanup_scripts() {
        let toml = "\n[setup]\nscript = \"\"\"\nnpm ci\npnpm build\n\"\"\"\n\n[cleanup]\nscript = \"pnpm clean\"\n\n[actions.test]\ncommand = \"pnpm test\"\n";
        let read = reader(vec![(".codex/environments/environment.toml", toml)]);
        assert_eq!(
            inspect_codex_environment_config(&read),
            Some(SetupScriptImportCandidate {
                provider: "codex".to_string(),
                label: "Codex environment".to_string(),
                files: vec![".codex/environments/environment.toml".to_string()],
                setup: "npm ci\npnpm build".to_string(),
                archive: Some("pnpm clean".to_string()),
                unsupported_fields: vec!["[actions.test]".to_string()],
            })
        );
    }

    // Derived from the "ignores malformed or setup-less configs" Codex case:
    // a `[cleanup]`-only file yields no candidate (no `[setup]` script).
    #[test]
    fn ignores_cleanup_only_config() {
        let read = reader(vec![(".codex/environments/environment.toml", "[cleanup]\nscript = \"pnpm clean\"")]);
        assert_eq!(inspect_codex_environment_config(&read), None);
    }

    #[test]
    fn ignores_missing_config() {
        let read = reader(vec![]);
        assert_eq!(inspect_codex_environment_config(&read), None);
    }
}
