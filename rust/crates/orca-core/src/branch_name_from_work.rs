//! Work-derived branch naming, ported from `src/shared/branch-name-from-work.ts`.
//!
//! Turns raw model output into a short, safe kebab-case branch leaf, recognises
//! Orca's auto-generated creature names (so auto-rename only overwrites names
//! Orca itself picked), humanises a slug for the sidebar label, and builds the
//! identical generation prompt used by both local and SSH targets.

use crate::marine_creatures::MARINE_CREATURES;

/// Two-to-four words is the sweet spot the feature targets.
pub const MAX_BRANCH_NAME_WORDS: usize = 4;
const MIN_BRANCH_NAME_WORDS: usize = 2;

/// Strip a single trailing `-<digits>` collision suffix (`-2`, `-17`, …).
fn strip_collision_suffix(s: &str) -> &str {
    if let Some(idx) = s.rfind('-') {
        let suffix = &s[idx + 1..];
        if !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit()) {
            return &s[..idx];
        }
    }
    s
}

/// True when `branch_leaf` is an Orca auto-generated creature name, e.g.
/// `Nautilus` or `Octopus-2`.
pub fn is_auto_generated_creature_branch_name(branch_leaf: &str) -> bool {
    let normalized = branch_leaf.trim().to_lowercase();
    if normalized.is_empty() {
        return false;
    }
    let base = strip_collision_suffix(&normalized);
    MARINE_CREATURES.iter().any(|c| c.eq_ignore_ascii_case(base))
}

/// Turn raw model output into a safe, short kebab-case branch leaf. Returns ""
/// when nothing usable remains (callers treat that as "skip the rename").
pub fn sanitize_branch_slug(raw: &str, max_words: usize) -> String {
    let lower = raw.to_lowercase();
    lower
        .split(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit()))
        .filter(|s| !s.is_empty())
        .take(max_words)
        .collect::<Vec<_>>()
        .join("-")
}

/// `sanitize_branch_slug` with the default word cap.
pub fn sanitize_branch_slug_default(raw: &str) -> String {
    sanitize_branch_slug(raw, MAX_BRANCH_NAME_WORDS)
}

/// Turn a branch slug into a readable label: `supported-models-list` →
/// `Supported models list`.
pub fn humanize_branch_slug(slug: &str) -> String {
    let joined = slug
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let mut chars = joined.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// Context for the branch-name generation prompt.
#[derive(Clone, Debug, Default)]
pub struct BranchNameWorkContext {
    /// The user's first prompt to the agent in this workspace.
    pub first_prompt: String,
    /// The agent's first response, when it has already arrived.
    pub assistant_message: Option<String>,
}

/// Build the text-generation prompt asking the agent to summarise the work into
/// a branch name. Kept identical across local and SSH generation targets.
pub fn build_branch_name_prompt(context: &BranchNameWorkContext, custom_prompt: &str) -> String {
    let mut sections: Vec<String> = vec![
        "Generate a git branch name that summarizes the coding task described below.".to_string(),
        "Rules:".to_string(),
        format!("- Use between {MIN_BRANCH_NAME_WORDS} and {MAX_BRANCH_NAME_WORDS} words."),
        "- Lowercase kebab-case only (words joined by single hyphens).".to_string(),
        "- No slashes, no prefixes, no quotes, no trailing punctuation.".to_string(),
        "- Describe the work itself, not the agent or the repository.".to_string(),
        "- Output ONLY the branch name on a single line, nothing else.".to_string(),
        String::new(),
        "User request:".to_string(),
        context.first_prompt.trim().to_string(),
    ];
    if let Some(assistant) = context.assistant_message.as_deref() {
        let assistant = assistant.trim();
        if !assistant.is_empty() {
            sections.push(String::new());
            sections.push("Agent's initial response:".to_string());
            sections.push(assistant.to_string());
        }
    }
    let prompt = custom_prompt.trim();
    if !prompt.is_empty() {
        sections.push(String::new());
        sections.push("Additional user prompt:".to_string());
        sections.push(prompt.to_string());
    }
    sections.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_lowercases_and_kebab_cases_free_form_output() {
        assert_eq!(sanitize_branch_slug_default("Fix the auth bug"), "fix-the-auth-bug");
    }

    #[test]
    fn sanitize_caps_at_four_words() {
        assert_eq!(
            sanitize_branch_slug_default("add dark mode toggle to settings page"),
            "add-dark-mode-toggle"
        );
    }

    #[test]
    fn sanitize_honors_custom_word_cap() {
        assert_eq!(sanitize_branch_slug("add dark mode toggle", 2), "add-dark");
    }

    #[test]
    fn sanitize_collapses_punctuation_quotes_and_slashes() {
        assert_eq!(sanitize_branch_slug_default("\"feat/login_flow!!\""), "feat-login-flow");
    }

    #[test]
    fn sanitize_returns_empty_when_nothing_usable_remains() {
        assert_eq!(sanitize_branch_slug_default("   !!! ___ "), "");
        assert_eq!(sanitize_branch_slug_default(""), "");
    }

    #[test]
    fn creature_match_is_case_insensitive() {
        assert!(is_auto_generated_creature_branch_name("Nautilus"));
        assert!(is_auto_generated_creature_branch_name("octopus"));
    }

    #[test]
    fn creature_match_handles_numbered_collisions() {
        assert!(is_auto_generated_creature_branch_name("Nautilus-2"));
        assert!(is_auto_generated_creature_branch_name("Seahorse-17"));
    }

    #[test]
    fn creature_match_rejects_user_and_work_derived_names() {
        assert!(!is_auto_generated_creature_branch_name("fix-auth-bug"));
        assert!(!is_auto_generated_creature_branch_name("my-feature"));
        assert!(!is_auto_generated_creature_branch_name(""));
    }

    #[test]
    fn humanize_turns_kebab_into_readable_label() {
        assert_eq!(humanize_branch_slug("supported-models-list"), "Supported models list");
        assert_eq!(humanize_branch_slug("fix-auth"), "Fix auth");
    }

    #[test]
    fn humanize_returns_empty_for_empty_slug() {
        assert_eq!(humanize_branch_slug(""), "");
    }

    #[test]
    fn prompt_includes_user_prompt_and_omits_absent_assistant() {
        let prompt = build_branch_name_prompt(
            &BranchNameWorkContext {
                first_prompt: "Add a logout button".to_string(),
                assistant_message: None,
            },
            "",
        );
        assert!(prompt.contains("Add a logout button"));
        assert!(!prompt.contains("Agent's initial response"));
    }

    #[test]
    fn prompt_includes_assistant_response_when_present() {
        let prompt = build_branch_name_prompt(
            &BranchNameWorkContext {
                first_prompt: "Add a logout button".to_string(),
                assistant_message: Some("I'll wire it into the header.".to_string()),
            },
            "",
        );
        assert!(prompt.contains("Agent's initial response"));
        assert!(prompt.contains("I'll wire it into the header."));
    }

    #[test]
    fn prompt_appends_custom_branch_name_prompt_when_present() {
        let prompt = build_branch_name_prompt(
            &BranchNameWorkContext {
                first_prompt: "Add a logout button".to_string(),
                assistant_message: None,
            },
            "Prefer product nouns.",
        );
        assert!(prompt.contains("Additional user prompt:"));
        assert!(prompt.contains("Prefer product nouns."));
    }
}
