//! Pull-request field generation, ported from `src/shared/pull-request-generation.ts`.
//!
//! Builds the prompt that asks an agent for PR `base`/`title`/`body`/`draft` as
//! compact JSON, and parses that JSON back (fence-tolerant) with fallbacks to
//! the current PR fields. Reuses `commit_message_prompt::truncate_diff_for_prompt`
//! for the patch budget. Provider-neutral.

use crate::commit_message_prompt::{truncate_diff_for_prompt, STAGED_DIFF_BYTE_BUDGET};
use regex::Regex;
use serde_json::Value;
use std::sync::OnceLock;

#[derive(Clone, Debug, Default)]
pub struct PullRequestDraftContext {
    pub branch: Option<String>,
    pub base: String,
    pub branch_changed_by_preparation: bool,
    pub current_title: String,
    pub current_body: String,
    pub current_draft: bool,
    pub commit_summary: String,
    pub change_summary: String,
    pub patch: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeneratedPullRequestFields {
    pub base: String,
    pub title: String,
    pub body: String,
    pub draft: bool,
}

fn limit_section(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let omitted = value.chars().count() - max_chars;
    let kept: String = value.chars().take(max_chars).collect();
    format!("{kept}\n\n[truncated: {omitted} characters omitted]")
}

pub fn build_pull_request_fields_prompt(context: &PullRequestDraftContext, custom_prompt: &str) -> String {
    let or_empty = |value: &str| if value.is_empty() { "(empty)" } else { value }.to_string();
    let base = [
        "You are generating pull request details.".to_string(),
        "Return ONLY compact JSON with this exact shape:".to_string(),
        r#"{"base":"branch-name","title":"short title","body":"markdown description","draft":false}"#.to_string(),
        String::new(),
        "Rules:".to_string(),
        "- Use the branch diff and commits below as source of truth.".to_string(),
        "- Keep the base branch as the current base unless the diff clearly targets a different branch.".to_string(),
        "- Title: concise, specific, no trailing period.".to_string(),
        "- Body: useful Markdown summary for reviewers. Include testing notes only when evidence exists.".to_string(),
        "- draft: true only when the changes clearly look unfinished, WIP, or unsafe to review.".to_string(),
        "- Do not include labels, reviewers, code fences, prose, or any keys beyond base/title/body/draft.".to_string(),
        String::new(),
        format!("Head branch: {}", context.branch.as_deref().unwrap_or("(detached)")),
        format!("Current base: {}", context.base),
        format!("Current title: {}", or_empty(&context.current_title)),
        format!("Current description: {}", or_empty(&context.current_body)),
        format!("Current draft: {}", if context.current_draft { "true" } else { "false" }),
        String::new(),
        "Commits:".to_string(),
        limit_section(if context.commit_summary.is_empty() { "(none)" } else { &context.commit_summary }, 8_000),
        String::new(),
        "Changed files:".to_string(),
        limit_section(if context.change_summary.is_empty() { "(none)" } else { &context.change_summary }, 8_000),
        String::new(),
        "Patch:".to_string(),
        "```diff".to_string(),
        truncate_diff_for_prompt(&context.patch, STAGED_DIFF_BYTE_BUDGET),
        "```".to_string(),
    ]
    .join("\n");

    let final_requirement =
        "Return compact JSON only with keys base, title, body, and draft. No prose or code fences."
            .to_string();
    let trimmed = custom_prompt.trim();
    if trimmed.is_empty() {
        [base, String::new(), "Final output requirement:".to_string(), final_requirement].join("\n")
    } else {
        [
            base,
            String::new(),
            "Additional user prompt:".to_string(),
            limit_section(trimmed, 4_000),
            String::new(),
            "Final output requirement:".to_string(),
            final_requirement,
        ]
        .join("\n")
    }
}

fn strip_json_fence(raw: &str) -> String {
    let normalized = raw.replace("\r\n", "\n");
    let text = normalized.trim();
    let text = match json_fence_re().captures(text) {
        Some(captures) => captures.get(1).map_or("", |m| m.as_str()).trim().to_string(),
        None => text.to_string(),
    };
    match (text.find('{'), text.rfind('}')) {
        (Some(start), Some(end)) if end > start => text[start..=end].to_string(),
        _ => text,
    }
}

/// Parse the agent's JSON reply into PR fields, falling back to the current
/// values for missing/blank fields. `Err` if the payload is not a JSON object.
pub fn parse_generated_pull_request_fields(
    raw: &str,
    fallback: &PullRequestDraftContext,
) -> Result<GeneratedPullRequestFields, String> {
    let parsed: Value = serde_json::from_str(&strip_json_fence(raw)).map_err(|error| error.to_string())?;
    let record = parsed.as_object().ok_or_else(|| "Expected a JSON object.".to_string())?;

    let base = record.get("base").and_then(Value::as_str).map(str::trim).unwrap_or(fallback.base.as_str());
    let base = if base.is_empty() { fallback.base.clone() } else { base.to_string() };

    let title = match record.get("title").and_then(Value::as_str) {
        Some(raw) if !raw.trim().is_empty() => trailing_dots_re().replace(raw.trim(), "").into_owned(),
        _ => fallback.current_title.trim().to_string(),
    };
    let title = if title.is_empty() { "Update project files".to_string() } else { title };

    let body = match record.get("body").and_then(Value::as_str) {
        Some(raw) => trailing_whitespace_re().replace(raw, "").into_owned(),
        None => fallback.current_body.clone(),
    };

    let draft = record.get("draft").and_then(Value::as_bool).unwrap_or(fallback.current_draft);

    Ok(GeneratedPullRequestFields { base, title, body, draft })
}

fn json_fence_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?is)^```(?:json)?\n(.*?)\n```$").unwrap())
}

fn trailing_dots_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"[.]+$").unwrap())
}

fn trailing_whitespace_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\s+$").unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> PullRequestDraftContext {
        PullRequestDraftContext {
            branch: Some("feature/pr-details".to_string()),
            base: "main".to_string(),
            branch_changed_by_preparation: false,
            current_title: "Feature pr details".to_string(),
            current_body: "- Add form".to_string(),
            current_draft: false,
            commit_summary: "- feat: add generated PR details".to_string(),
            change_summary: "M\tsrc/file.ts".to_string(),
            patch: "diff --git a/src/file.ts b/src/file.ts\n+export const value = true".to_string(),
        }
    }

    #[test]
    fn asks_for_compact_json_and_includes_pr_context() {
        let prompt = build_pull_request_fields_prompt(&context(), "Use conventional PR titles.");
        assert!(prompt.contains("Return ONLY compact JSON"));
        assert!(prompt.contains("Head branch: feature/pr-details"));
        assert!(prompt.contains("Current base: main"));
        assert!(prompt.contains("Additional user prompt:"));
        assert!(prompt.contains("Use conventional PR titles."));
    }

    #[test]
    fn parses_fenced_json_output() {
        let fields = parse_generated_pull_request_fields(
            "```json\n{\"base\":\"main\",\"title\":\"fix: add details.\",\"body\":\"Summary\",\"draft\":true}\n```",
            &context(),
        )
        .unwrap();
        assert_eq!(
            fields,
            GeneratedPullRequestFields {
                base: "main".to_string(),
                title: "fix: add details".to_string(),
                body: "Summary".to_string(),
                draft: true,
            }
        );
    }

    #[test]
    fn falls_back_for_missing_optional_values() {
        let fields = parse_generated_pull_request_fields("{\"title\":\"\"}", &context()).unwrap();
        assert_eq!(
            fields,
            GeneratedPullRequestFields {
                base: "main".to_string(),
                title: "Feature pr details".to_string(),
                body: "- Add form".to_string(),
                draft: false,
            }
        );
    }
}
