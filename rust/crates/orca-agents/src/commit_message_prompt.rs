//! Commit-message prompt assembly + agent-output cleanup, ported from
//! `src/shared/commit-message-prompt.ts`.
//!
//! Pure transformation from "diff + user prompt" into the prompt text and, for
//! custom commands, a spawn-ready binary + argv. Shared so the local generator
//! and the SSH/relay path build identical strings. Regex for the cleanup
//! heuristics; `serde_json` for the JSON error payloads agent CLIs emit.

use regex::Regex;
use serde_json::Value;
use std::sync::OnceLock;

const COMMIT_MESSAGE_BASE_PROMPT: &str = r#"You are generating a single git commit message.
Read the staged diff below and produce the message.

Rules:
- First line: imperative mood, <= 72 chars, no trailing period.
- Optional body: blank line, then wrapped at 72 chars explaining WHY.
- Output ONLY the commit message - no preamble, no code fences, no quotes.
- Do not include "Co-authored-by" trailers - Orca appends them after generation when configured.

Staged diff:
```diff
{{DIFF}}
```
"#;

pub const STAGED_DIFF_BYTE_BUDGET: usize = 200_000;
pub const CUSTOM_PROMPT_PLACEHOLDER: &str = "{prompt}";

/// Build the final prompt sent to the agent; a non-empty custom suffix is
/// appended verbatim so the user can override style.
pub fn build_commit_prompt(diff: &str, custom_suffix: &str) -> String {
    let base = COMMIT_MESSAGE_BASE_PROMPT.replace("{{DIFF}}", diff);
    let trimmed = custom_suffix.trim();
    if trimmed.is_empty() {
        base
    } else {
        format!("{base}\n\nAdditional user prompt:\n{trimmed}")
    }
}

/// Truncate a diff over `budget` bytes, appending a marker so the agent knows
/// the input was clipped. Truncation floors to a char boundary (panic-free).
pub fn truncate_diff_for_prompt(diff: &str, budget: usize) -> String {
    if diff.len() <= budget {
        return diff.to_string();
    }
    let omitted = diff.len() - budget;
    let mut end = budget;
    while end > 0 && !diff.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n...(diff truncated, {omitted} bytes omitted)", &diff[..end])
}

/// Strip noise around an agent's output: surrounding whitespace, a single
/// enclosing fenced code block, a lone "Generating…"/"Thinking…" preamble line,
/// and a leading list marker.
pub fn clean_generated_commit_message(raw: &str) -> String {
    let mut text = raw.replace("\r\n", "\n").trim().to_string();

    if let Some(newline) = text.find('\n') {
        let first_line = &text[..newline];
        if preamble_re().is_match(first_line) || ellipsis_re().is_match(first_line.trim()) {
            text = text[newline + 1..].trim().to_string();
        }
    }

    if let Some(captures) = fence_re().captures(&text) {
        text = captures.get(1).map(|m| m.as_str()).unwrap_or("").trim().to_string();
    }

    list_marker_re().replace(&text, "$1").trim().to_string()
}

/// Tokens from a custom command template. POSIX-style grouping only (single +
/// double quotes, backslash escapes inside double quotes); no variable/glob
/// expansion — the user's intent is "spawn this exact CLI".
pub fn tokenize_custom_command_template(template: &str) -> Result<Vec<String>, String> {
    let chars: Vec<char> = template.chars().collect();
    let mut tokens: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut in_token = false;
    let mut quote: Option<char> = None;
    let mut i = 0;

    while i < chars.len() {
        let ch = chars[i];
        if let Some(active) = quote {
            if ch == '\\' && active == '"' && i + 1 < chars.len() {
                current.push(chars[i + 1]);
                i += 2;
                continue;
            }
            if ch == active {
                quote = None;
                i += 1;
                // Leaving a quoted region keeps the token open: a"b"c → abc.
                in_token = true;
                continue;
            }
            current.push(ch);
            i += 1;
            continue;
        }

        if ch == '"' || ch == '\'' {
            quote = Some(ch);
            in_token = true;
            i += 1;
            continue;
        }
        if ch == '\\' && i + 1 < chars.len() {
            current.push(chars[i + 1]);
            in_token = true;
            i += 2;
            continue;
        }
        if ch.is_whitespace() {
            if in_token {
                tokens.push(std::mem::take(&mut current));
                in_token = false;
            }
            i += 1;
            continue;
        }
        current.push(ch);
        in_token = true;
        i += 1;
    }

    if quote.is_some() {
        return Err("Unclosed quote in command template.".to_string());
    }
    if in_token {
        tokens.push(current);
    }
    Ok(tokens)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CustomCommandPlan {
    pub binary: String,
    pub args: Vec<String>,
    /// `Some` when the prompt is delivered via stdin (no `{prompt}` in template).
    pub stdin_payload: Option<String>,
}

/// Parse a user command template into a spawn-ready binary + argv, substituting
/// `{prompt}`. With no `{prompt}`, the prompt is delivered via stdin.
pub fn plan_custom_command(template: &str, prompt: &str) -> Result<CustomCommandPlan, String> {
    let tokens = tokenize_custom_command_template(template)?;
    if tokens.is_empty() {
        return Err("Custom command is empty.".to_string());
    }
    if tokens[0].is_empty() {
        return Err("Custom command must start with a binary name.".to_string());
    }

    let substitute = |token: &str| -> String {
        if token.contains(CUSTOM_PROMPT_PLACEHOLDER) {
            token.split(CUSTOM_PROMPT_PLACEHOLDER).collect::<Vec<_>>().join(prompt)
        } else {
            token.to_string()
        }
    };

    if tokens.iter().any(|token| token.contains(CUSTOM_PROMPT_PLACEHOLDER)) {
        Ok(CustomCommandPlan {
            binary: substitute(&tokens[0]),
            args: tokens[1..].iter().map(|token| substitute(token)).collect(),
            stdin_payload: None,
        })
    } else {
        Ok(CustomCommandPlan {
            binary: tokens[0].clone(),
            args: tokens[1..].to_vec(),
            stdin_payload: Some(prompt.to_string()),
        })
    }
}

/// Pull the actionable error out of an agent CLI's stdout/stderr (which bury it
/// under config preamble and lifecycle noise). `None` if nothing error-shaped.
pub fn extract_agent_error_message(stdout: &str, stderr: &str) -> Option<String> {
    let combined = strip_ansi_control_sequences(&format!("{stdout}\n{stderr}"));

    // Pass 1: an `ERROR:`/`Error:` line, walked from the end so the most recent
    // (usually most meaningful) error wins.
    for line in crlf_lines(&combined).iter().rev() {
        let Some(captures) = error_line_re().captures(line) else {
            continue;
        };
        let payload = captures.get(1).map(|m| m.as_str().trim()).unwrap_or("");
        if payload.starts_with('{') {
            if let Ok(parsed) = serde_json::from_str::<Value>(payload) {
                let inner = parsed
                    .get("error")
                    .and_then(|error| error.get("message"))
                    .and_then(Value::as_str)
                    .or_else(|| parsed.get("message").and_then(Value::as_str));
                if let Some(inner) = inner {
                    let inner = inner.trim();
                    if !inner.is_empty() {
                        return Some(inner.to_string());
                    }
                }
            }
        }
        if !payload.is_empty() {
            return Some(payload.to_string());
        }
    }

    // Pass 2: a wrapped `Error code: NNN - {...}` payload with a quoted message.
    let joined = compact_newlines_re().replace_all(&combined, "$1$2");
    let compact = whitespace_re().replace_all(&joined, " ");
    if let Some(captures) = error_code_re().captures(&compact) {
        let payload = captures.get(1).map(|m| m.as_str().trim()).unwrap_or("");
        if let Some(message) = message_field_re().captures(payload) {
            let message = message.get(1).map(|m| m.as_str().trim()).unwrap_or("");
            if !message.is_empty() {
                return Some(message.to_string());
            }
        }
        if !payload.is_empty() {
            return Some(payload.to_string());
        }
    }

    None
}

fn strip_ansi_control_sequences(value: &str) -> String {
    ansi_re().replace_all(value, "").into_owned()
}

fn crlf_lines(value: &str) -> Vec<&str> {
    value.split('\n').map(|line| line.strip_suffix('\r').unwrap_or(line)).collect()
}

fn preamble_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)^(generating|thinking)\b").unwrap())
}

fn ellipsis_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[.…]+$").unwrap())
}

fn fence_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?s)^```[a-zA-Z0-9_-]*\n(.*?)\n```$").unwrap())
}

fn list_marker_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(\s*)(?:[-*•●]\s+|\d+[.)]\s+)").unwrap())
}

fn ansi_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\x1b\[[0-?]*[ -/]*[@-~]").unwrap())
}

fn error_line_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)^\s*(?:ERROR|Error(?:\s+during\s+[^:]+)?)\s*:\s*(.+)$").unwrap())
}

fn compact_newlines_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"([A-Za-z])\r?\n\s*([A-Za-z_])").unwrap())
}

fn whitespace_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\s+").unwrap())
}

fn error_code_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)\bError code:\s*\d+\s*-\s*(.+)$").unwrap())
}

fn message_field_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"(?i)['"]message['"]\s*:\s*['"]([^'"]+)['"]"#).unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- buildCommitPrompt ---

    #[test]
    fn embeds_the_diff_into_the_base_prompt() {
        let prompt = build_commit_prompt("diff --git a/foo b/foo\n+hello", "");
        assert!(prompt.contains("diff --git a/foo b/foo"));
        assert!(prompt.contains("+hello"));
        assert!(prompt.contains("First line: imperative mood"));
    }

    #[test]
    fn appends_a_custom_suffix_when_non_empty() {
        let prompt = build_commit_prompt("diff", "Use Conventional Commits.");
        assert!(prompt.contains("Additional user prompt:"));
        assert!(prompt.ends_with("Use Conventional Commits."));
    }

    #[test]
    fn does_not_append_the_suffix_block_for_whitespace_only_suffixes() {
        let prompt = build_commit_prompt("diff", "   \n  ");
        assert!(!prompt.contains("Additional user prompt:"));
    }

    // --- truncateDiffForPrompt ---

    #[test]
    fn returns_the_diff_unchanged_when_within_budget() {
        let diff = "line\n".repeat(10);
        assert_eq!(truncate_diff_for_prompt(&diff, STAGED_DIFF_BYTE_BUDGET), diff);
    }

    #[test]
    fn truncates_and_appends_a_marker_when_over_budget() {
        let oversized = "A".repeat(STAGED_DIFF_BYTE_BUDGET + 100);
        let result = truncate_diff_for_prompt(&oversized, STAGED_DIFF_BYTE_BUDGET);
        assert!(result.len() < oversized.len());
        assert!(result.contains("diff truncated, 100 bytes omitted"));
    }

    #[test]
    fn honors_a_custom_budget() {
        let result = truncate_diff_for_prompt("abcdefghij", 5);
        assert!(result.starts_with("abcde"));
        assert!(result.contains("diff truncated, 5 bytes omitted"));
    }

    // --- cleanGeneratedCommitMessage ---

    #[test]
    fn trims_whitespace() {
        assert_eq!(clean_generated_commit_message("  feat: hello  \n"), "feat: hello");
    }

    #[test]
    fn strips_a_single_enclosing_fenced_code_block() {
        assert_eq!(clean_generated_commit_message("```\nfeat: hello\n```"), "feat: hello");
    }

    #[test]
    fn strips_a_fenced_block_with_a_language_tag() {
        assert_eq!(clean_generated_commit_message("```text\nfix: bug\n```"), "fix: bug");
    }

    #[test]
    fn drops_a_leading_generating_preamble_line() {
        assert_eq!(clean_generated_commit_message("Generating…\nfeat: hello world"), "feat: hello world");
    }

    #[test]
    fn normalizes_crlf_line_endings() {
        assert_eq!(clean_generated_commit_message("feat: a\r\nbody line\r\n"), "feat: a\nbody line");
    }

    #[test]
    fn strips_a_leading_list_marker_from_the_commit_subject() {
        assert_eq!(
            clean_generated_commit_message("● Add Copilot entry to agent results"),
            "Add Copilot entry to agent results"
        );
        assert_eq!(clean_generated_commit_message("1. Add numbered entry"), "Add numbered entry");
    }

    #[test]
    fn returns_empty_string_when_input_is_whitespace() {
        assert_eq!(clean_generated_commit_message("   \n\t"), "");
    }

    // --- extractAgentErrorMessage ---

    #[test]
    fn returns_the_inner_message_from_a_codex_json_error_payload() {
        let stderr = [
            "--------",
            "workdir: C:\\Storage\\Projects\\bagplanner",
            "model: gpt-5.3-codex-spark",
            "reasoning effort: medium",
            "--------",
            "user",
            "You are generating a single git commit message...",
            "hook: SessionStart",
            "hook: SessionStart Completed",
            r#"ERROR: {"type":"error","status":400,"error":{"type":"invalid_request_error","message":"The 'gpt-5.3-codex-spark' model is not supported when using Codex with a ChatGPT account."}}"#,
        ]
        .join("\n");
        assert_eq!(
            extract_agent_error_message("", &stderr).as_deref(),
            Some("The 'gpt-5.3-codex-spark' model is not supported when using Codex with a ChatGPT account.")
        );
    }

    #[test]
    fn returns_the_payload_for_non_json_error_lines() {
        assert_eq!(
            extract_agent_error_message("preamble line\nERROR: {bad json oops", "").as_deref(),
            Some("{bad json oops")
        );
    }

    #[test]
    fn uses_the_last_error_line_when_several_are_emitted() {
        let out = ["ERROR: first failure", "retry message", "ERROR: second failure"].join("\n");
        assert_eq!(extract_agent_error_message(&out, "").as_deref(), Some("second failure"));
    }

    #[test]
    fn matches_an_error_line_emitted_on_stdout() {
        assert_eq!(
            extract_agent_error_message("Error: model unavailable\n", "").as_deref(),
            Some("model unavailable")
        );
    }

    #[test]
    fn matches_ansi_colored_error_lines_emitted_by_clis() {
        assert_eq!(
            extract_agent_error_message("", "\u{1b}[91m\u{1b}[1mError: \u{1b}[0mNo payment method\n").as_deref(),
            Some("No payment method")
        );
    }

    #[test]
    fn matches_tool_specific_error_during_lines() {
        assert_eq!(
            extract_agent_error_message(
                "",
                "Error during droid execution: Authentication failed. Please log into Factory.\n"
            )
            .as_deref(),
            Some("Authentication failed. Please log into Factory.")
        );
    }

    #[test]
    fn matches_wrapped_provider_error_code_payloads_with_quoted_message_fields() {
        let stdout = [
            "Error code: 401 - {'error': {'message': 'The API Key appears to be invalid or ma",
            "y have expired. Please verify your credentials and try again.', 'type': 'invalid",
            "_authentication_error'}}",
        ]
        .join("\n");
        assert_eq!(
            extract_agent_error_message(&stdout, "").as_deref(),
            Some("The API Key appears to be invalid or may have expired. Please verify your credentials and try again.")
        );
    }

    #[test]
    fn returns_null_when_no_error_line_is_present() {
        assert_eq!(extract_agent_error_message("plain log\nmore log\n", ""), None);
    }

    #[test]
    fn returns_the_json_payload_message_field_when_no_nested_error_is_set() {
        assert_eq!(
            extract_agent_error_message(r#"ERROR: {"message":"top-level only"}"#, "").as_deref(),
            Some("top-level only")
        );
    }

    // --- tokenizeCustomCommandTemplate ---

    #[test]
    fn splits_on_whitespace() {
        assert_eq!(tokenize_custom_command_template("claude -p"), Ok(vec!["claude".to_string(), "-p".to_string()]));
    }

    #[test]
    fn groups_double_quoted_segments_with_spaces() {
        assert_eq!(
            tokenize_custom_command_template(r#"claude --msg "hello world""#),
            Ok(vec!["claude".to_string(), "--msg".to_string(), "hello world".to_string()])
        );
    }

    #[test]
    fn groups_single_quoted_segments_verbatim() {
        assert_eq!(
            tokenize_custom_command_template(r#"agent --json '{"k":"v"}'"#),
            Ok(vec!["agent".to_string(), "--json".to_string(), r#"{"k":"v"}"#.to_string()])
        );
    }

    #[test]
    fn honors_backslash_escapes_inside_double_quotes() {
        assert_eq!(
            tokenize_custom_command_template(r#"claude --msg "she said \"hi\"""#),
            Ok(vec!["claude".to_string(), "--msg".to_string(), r#"she said "hi""#.to_string()])
        );
    }

    #[test]
    fn keeps_adjacent_quoted_unquoted_regions_in_one_token() {
        assert_eq!(
            tokenize_custom_command_template(r#"foo a"b"c"#),
            Ok(vec!["foo".to_string(), "abc".to_string()])
        );
    }

    #[test]
    fn returns_an_error_for_an_unclosed_quote() {
        let result = tokenize_custom_command_template(r#"claude --msg "no end"#);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_lowercase().contains("unclosed"));
    }

    #[test]
    fn returns_an_empty_token_list_for_whitespace_only_input() {
        assert_eq!(tokenize_custom_command_template("   \t  "), Ok(Vec::new()));
    }

    // --- planCustomCommand ---

    #[test]
    fn routes_prompt_via_stdin_when_placeholder_is_absent() {
        assert_eq!(
            plan_custom_command("claude -p", "COMMIT MSG"),
            Ok(CustomCommandPlan {
                binary: "claude".to_string(),
                args: vec!["-p".to_string()],
                stdin_payload: Some("COMMIT MSG".to_string()),
            })
        );
    }

    #[test]
    fn substitutes_placeholder_as_a_whole_token_via_argv() {
        assert_eq!(
            plan_custom_command("codex exec {prompt}", "PROMPT"),
            Ok(CustomCommandPlan {
                binary: "codex".to_string(),
                args: vec!["exec".to_string(), "PROMPT".to_string()],
                stdin_payload: None,
            })
        );
    }

    #[test]
    fn treats_quoted_placeholder_identically_to_bare_placeholder() {
        assert_eq!(
            plan_custom_command("codex exec {prompt}", "PROMPT"),
            plan_custom_command(r#"codex exec "{prompt}""#, "PROMPT")
        );
    }

    #[test]
    fn substitutes_placeholder_embedded_inside_a_token() {
        assert_eq!(
            plan_custom_command("agent --msg={prompt}", "PROMPT"),
            Ok(CustomCommandPlan {
                binary: "agent".to_string(),
                args: vec!["--msg=PROMPT".to_string()],
                stdin_payload: None,
            })
        );
    }

    #[test]
    fn errors_on_empty_templates() {
        assert!(plan_custom_command("   ", "PROMPT").is_err());
    }

    #[test]
    fn propagates_tokenizer_errors() {
        let result = plan_custom_command(r#"agent "unclosed"#, "PROMPT");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_lowercase().contains("unclosed"));
    }
}
