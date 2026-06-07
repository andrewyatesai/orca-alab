//! Model-discovery parsers for commit-message agents, ported from the parser
//! half of `src/shared/commit-message-agent-spec.ts`.
//!
//! Each agent CLI lists its models differently — Codex emits JSON, others one
//! id per line, Pi a whitespace table, Cursor `id - Label` lines. These parse
//! that stdout into the unified `CommitMessageModel` shape (with thinking-effort
//! levels where the model supports them). The per-agent spec table + `buildArgs`
//! are a separate, larger port.

use regex::Regex;
use serde_json::Value;
use std::collections::HashSet;
use std::sync::OnceLock;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThinkingLevel {
    pub id: String,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitMessageModel {
    pub id: String,
    pub label: String,
    pub thinking_levels: Option<Vec<ThinkingLevel>>,
    pub default_thinking_level: Option<String>,
}

fn level(id: &str, label: &str) -> ThinkingLevel {
    ThinkingLevel { id: id.to_string(), label: label.to_string() }
}

pub(crate) fn openai_thinking_levels() -> Vec<ThinkingLevel> {
    vec![level("low", "Low"), level("medium", "Medium"), level("high", "High"), level("xhigh", "Extra High")]
}

fn pi_thinking_levels() -> Vec<ThinkingLevel> {
    vec![
        level("off", "Off"),
        level("low", "Low"),
        level("medium", "Medium"),
        level("high", "High"),
        level("xhigh", "Extra High"),
    ]
}

fn capitalize_first(part: &str) -> String {
    let mut chars = part.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// Turn a model/provider id into a display label: split on `/` and `-`,
/// upper-case `gpt` and short numeric parts, capitalize the rest.
pub(crate) fn label_from_model_id(id: &str) -> String {
    id.split(['/', '-'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            if part.eq_ignore_ascii_case("gpt") {
                return "GPT".to_string();
            }
            let starts_with_digit = part.chars().next().is_some_and(|c| c.is_ascii_digit());
            if part.chars().count() <= 3 && starts_with_digit {
                part.to_uppercase()
            } else {
                capitalize_first(part)
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// OpenAI-family models (`gpt-5*`, `codex`) expose the standard effort levels.
pub(crate) fn with_openai_thinking(id: &str) -> (Option<Vec<ThinkingLevel>>, Option<String>) {
    let lowered = id.to_lowercase();
    if lowered.contains("gpt-5") || lowered.contains("codex") {
        (Some(openai_thinking_levels()), Some("low".to_string()))
    } else {
        (None, None)
    }
}

fn unique_models(models: Vec<CommitMessageModel>) -> Vec<CommitMessageModel> {
    let mut seen: HashSet<String> = HashSet::new();
    models.into_iter().filter(|model| !model.id.is_empty() && seen.insert(model.id.clone())).collect()
}

fn crlf_lines(stdout: &str) -> Vec<&str> {
    stdout.split('\n').map(|line| line.strip_suffix('\r').unwrap_or(line)).collect()
}

pub fn parse_codex_models(stdout: &str) -> Vec<CommitMessageModel> {
    let Ok(parsed) = serde_json::from_str::<Value>(stdout) else {
        return Vec::new();
    };
    let empty = Vec::new();
    let models = parsed.get("models").and_then(Value::as_array).unwrap_or(&empty);
    let mapped = models
        .iter()
        .filter_map(|model| {
            let slug = model.get("slug").and_then(Value::as_str).filter(|s| !s.is_empty())?;
            let display_name =
                model.get("display_name").and_then(Value::as_str).filter(|s| !s.is_empty())?;
            let reasoning = model.get("supported_reasoning_levels").and_then(Value::as_array);
            let (thinking_levels, default_thinking_level) = match reasoning {
                Some(levels) if !levels.is_empty() => {
                    let thinking = levels
                        .iter()
                        .filter_map(|item| item.get("effort").and_then(Value::as_str))
                        .filter(|effort| !effort.is_empty())
                        .map(|effort| ThinkingLevel {
                            id: effort.to_string(),
                            label: if effort == "xhigh" {
                                "Extra High".to_string()
                            } else {
                                label_from_model_id(effort)
                            },
                        })
                        .collect();
                    let default = model
                        .get("default_reasoning_level")
                        .and_then(Value::as_str)
                        .unwrap_or("low")
                        .to_string();
                    (Some(thinking), Some(default))
                }
                _ => (None, None),
            };
            Some(CommitMessageModel {
                id: slug.to_string(),
                label: display_name.to_string(),
                thinking_levels,
                default_thinking_level,
            })
        })
        .collect();
    unique_models(mapped)
}

pub fn parse_line_models(stdout: &str) -> Vec<CommitMessageModel> {
    let mapped = crlf_lines(stdout)
        .into_iter()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.contains(' '))
        .map(|id| {
            let (thinking_levels, default_thinking_level) = with_openai_thinking(id);
            CommitMessageModel {
                id: id.to_string(),
                label: label_from_model_id(id),
                thinking_levels,
                default_thinking_level,
            }
        })
        .collect();
    unique_models(mapped)
}

pub fn parse_pi_models(stdout: &str) -> Vec<CommitMessageModel> {
    let mapped = crlf_lines(stdout)
        .into_iter()
        .map(|line| line.split_whitespace().collect::<Vec<_>>())
        .filter(|parts| parts.len() >= 6 && parts[0] != "provider")
        .map(|parts| {
            let (provider, model, thinking) = (parts[0], parts[1], parts[4]);
            let (thinking_levels, default_thinking_level) = if thinking == "yes" {
                (Some(pi_thinking_levels()), Some("low".to_string()))
            } else {
                (None, None)
            };
            CommitMessageModel {
                id: format!("{provider}/{model}"),
                label: format!("{} {}", label_from_model_id(provider), label_from_model_id(model)),
                thinking_levels,
                default_thinking_level,
            }
        })
        .collect();
    unique_models(mapped)
}

pub fn parse_cursor_models(stdout: &str) -> Vec<CommitMessageModel> {
    let mapped = crlf_lines(stdout)
        .into_iter()
        .map(str::trim)
        .filter_map(|line| cursor_line_re().captures(line))
        .map(|captures| {
            let id = captures.get(1).map_or("", |m| m.as_str());
            let raw_label = captures.get(2).map_or("", |m| m.as_str());
            let (thinking_levels, default_thinking_level) = with_openai_thinking(id);
            CommitMessageModel {
                id: id.to_string(),
                label: cursor_default_re().replace(raw_label, "").into_owned(),
                thinking_levels,
                default_thinking_level,
            }
        })
        .collect();
    unique_models(mapped)
}

fn cursor_line_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(\S+)\s+-\s+(.+)$").unwrap())
}

fn cursor_default_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)\s+\((?:default|current)\)$").unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(id: &str, label: &str) -> CommitMessageModel {
        CommitMessageModel { id: id.to_string(), label: label.to_string(), thinking_levels: None, default_thinking_level: None }
    }

    fn model_with_thinking(id: &str, label: &str, levels: Vec<ThinkingLevel>, default: &str) -> CommitMessageModel {
        CommitMessageModel {
            id: id.to_string(),
            label: label.to_string(),
            thinking_levels: Some(levels),
            default_thinking_level: Some(default.to_string()),
        }
    }

    #[test]
    fn parses_codex_model_json() {
        let stdout = r#"{"models":[{"slug":"gpt-5.5","display_name":"GPT-5.5","default_reasoning_level":"low","supported_reasoning_levels":[{"effort":"low"},{"effort":"high"}]}]}"#;
        assert_eq!(
            parse_codex_models(stdout),
            vec![model_with_thinking(
                "gpt-5.5",
                "GPT-5.5",
                vec![level("low", "Low"), level("high", "High")],
                "low",
            )]
        );
    }

    #[test]
    fn parses_one_model_per_line_output() {
        assert_eq!(
            parse_line_models("opencode/gpt-5.4-mini\n\nopenai/gpt-5.5\n")
                .into_iter()
                .map(|m| m.id)
                .collect::<Vec<_>>(),
            vec!["opencode/gpt-5.4-mini".to_string(), "openai/gpt-5.5".to_string()]
        );
    }

    #[test]
    fn parses_pi_model_table_output_with_provider_qualified_ids() {
        let output = [
            "provider        model                   context  max-out  thinking  images",
            "github-copilot  gpt-5.4-mini            400K     128K     yes       yes",
            "github-copilot  gpt-4o                  128K     4.1K     no        yes",
        ]
        .join("\n");
        assert_eq!(
            parse_pi_models(&output),
            vec![
                model_with_thinking(
                    "github-copilot/gpt-5.4-mini",
                    "Github Copilot GPT 5.4 Mini",
                    pi_thinking_levels(),
                    "low",
                ),
                model("github-copilot/gpt-4o", "Github Copilot GPT 4O"),
            ]
        );
    }

    #[test]
    fn parses_cursor_model_output() {
        assert_eq!(
            parse_cursor_models("auto - Auto\ngpt-5.2 - GPT-5.2\n"),
            vec![
                model("auto", "Auto"),
                model_with_thinking("gpt-5.2", "GPT-5.2", openai_thinking_levels(), "low"),
            ]
        );
    }
}
