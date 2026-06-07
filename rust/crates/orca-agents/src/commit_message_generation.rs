//! Commit-message draft prompt + response splitting, ported from
//! `src/shared/commit-message-generation.ts`.
//!
//! Builds the prompt from staged context (so the agent doesn't inspect git
//! itself) and splits the generated text into subject/body. Reuses
//! `commit_message_prompt`'s diff truncation + output cleanup.

use crate::commit_message_prompt::{clean_generated_commit_message, truncate_diff_for_prompt, STAGED_DIFF_BYTE_BUDGET};

#[derive(Clone, Debug, Default)]
pub struct CommitMessageDraftContext {
    pub branch: Option<String>,
    pub staged_summary: String,
    pub staged_patch: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeneratedCommitMessage {
    pub subject: String,
    pub body: String,
    pub message: String,
}

fn limit_section(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let omitted = value.chars().count() - max_chars;
    let kept: String = value.chars().take(max_chars).collect();
    format!("{kept}\n\n[truncated: {omitted} characters omitted]")
}

pub fn build_commit_message_prompt(context: &CommitMessageDraftContext, custom_prompt: &str) -> String {
    let base = [
        "You are generating a single git commit message.".to_string(),
        "Return only the commit message text. Do not include a preamble, quotes, or code fences.".to_string(),
        String::new(),
        "Rules:".to_string(),
        "- First line: imperative mood, <= 72 chars, no trailing period.".to_string(),
        "- Optional body: blank line, then short wrapped bullet points or prose explaining WHY.".to_string(),
        "- Capture the primary user-visible or developer-visible change.".to_string(),
        "- Use only the staged changes below as context.".to_string(),
        "- Do not include \"Co-authored-by\" or other git trailers.".to_string(),
        String::new(),
        format!("Branch: {}", context.branch.as_deref().unwrap_or("(detached)")),
        String::new(),
        "Staged files:".to_string(),
        limit_section(&context.staged_summary, 6_000),
        String::new(),
        "Staged patch:".to_string(),
        "```diff".to_string(),
        truncate_diff_for_prompt(&context.staged_patch, STAGED_DIFF_BYTE_BUDGET),
        "```".to_string(),
    ]
    .join("\n");

    let trimmed = custom_prompt.trim();
    if trimmed.is_empty() {
        base
    } else {
        [base, String::new(), "Additional user prompt:".to_string(), limit_section(trimmed, 4_000)].join("\n")
    }
}

pub fn split_generated_commit_message(message: &str) -> GeneratedCommitMessage {
    let normalized = clean_generated_commit_message(message);
    let mut lines = normalized.split('\n');
    let subject_line = lines.next().unwrap_or("");
    let body_lines: Vec<&str> = lines.collect();

    // trim → strip trailing dots → cap at 72 chars → trim trailing whitespace.
    let capped: String = subject_line.trim().trim_end_matches('.').chars().take(72).collect();
    let subject = capped.trim_end();
    let body = body_lines.join("\n").trim().to_string();

    let safe_subject = if subject.is_empty() { "Update project files".to_string() } else { subject.to_string() };
    let message =
        if body.is_empty() { safe_subject.clone() } else { format!("{safe_subject}\n\n{body}") };
    GeneratedCommitMessage { subject: safe_subject, body, message }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_a_prompt_from_staged_context_instead_of_asking_the_agent_to_inspect_git() {
        let prompt = build_commit_message_prompt(
            &CommitMessageDraftContext {
                branch: Some("feature/commit-drafts".to_string()),
                staged_summary: "M\tsrc/main/ipc/filesystem.ts".to_string(),
                staged_patch: "diff --git a/src/main/ipc/filesystem.ts b/src/main/ipc/filesystem.ts\n+hello".to_string(),
            },
            "",
        );
        assert!(prompt.contains("Branch: feature/commit-drafts"));
        assert!(prompt.contains("Staged files:\nM\tsrc/main/ipc/filesystem.ts"));
        assert!(prompt.contains("Staged patch:\n```diff"));
        assert!(prompt.contains("+hello"));
        assert!(prompt.contains("Use only the staged changes below as context."));
        assert!(!prompt.contains("Additional user prompt:"));
    }

    #[test]
    fn keeps_a_custom_prompt_in_a_separate_bounded_section() {
        let prompt = build_commit_message_prompt(
            &CommitMessageDraftContext {
                branch: None,
                staged_summary: "A\tREADME.md".to_string(),
                staged_patch: "+docs".to_string(),
            },
            "Use Conventional Commits.",
        );
        assert!(prompt.contains("Branch: (detached)"));
        assert!(prompt.contains("Additional user prompt:\nUse Conventional Commits."));
    }

    #[test]
    fn normalizes_subject_and_preserves_body_text() {
        assert_eq!(
            split_generated_commit_message("Fix source control generation.\n\n- Move planning into main"),
            GeneratedCommitMessage {
                subject: "Fix source control generation".to_string(),
                body: "- Move planning into main".to_string(),
                message: "Fix source control generation\n\n- Move planning into main".to_string(),
            }
        );
    }
}
