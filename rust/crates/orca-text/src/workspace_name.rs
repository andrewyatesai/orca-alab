//! Workspace name/seed derivation, ported from `src/shared/workspace-name.ts`.
//!
//! Slugifies free text into git-ref-safe workspace seeds and derives a single
//! human "intent" name (e.g. `Fix Issue 2635`) for first-create identity, so the
//! folder/branch/sidebar names don't drift. Regex-backed; pure.

use regex::Regex;
use std::sync::OnceLock;

pub fn slugify_for_workspace_name(input: &str) -> String {
    let lowered = input.trim().to_lowercase();
    let s = re(r"[\\/]+").replace_all(&lowered, "-");
    let s = re(r"\s+").replace_all(&s, "-");
    let s = re(r"[^a-z0-9._-]+").replace_all(&s, "-");
    let s = re(r"-+").replace_all(&s, "-");
    let s = re(r"\.{2,}").replace_all(&s, ".");
    let s = re(r"^[.-]+|[.-]+$").replace_all(&s, "");
    let truncated: String = s.chars().take(48).collect();
    re(r"[-._]+$").replace_all(&truncated, "").into_owned()
}

pub fn get_linked_work_item_suggested_name(title: &str) -> String {
    let trimmed = title.trim();
    let without = re(r"(?i)^(?:issue|pr|pull request)\s*#?\d+\s*[:-]\s*").replace(trimmed, "");
    let without = re(r"^#\d+\s*[:-]\s*").replace(&without, "").into_owned();
    let without = re(r"(?i)\(#\d+\)").replace_all(&without, "");
    let without = re(r"\b#\d+\b").replace_all(&without, "");
    let without = without.trim();
    let seed = if without.is_empty() { trimmed } else { without };
    slugify_for_workspace_name(seed)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkItemType {
    Issue,
    Pr,
    Mr,
}

#[derive(Clone, Debug, Default)]
pub struct WorkspaceIntentWorkItem {
    pub kind: Option<WorkItemType>,
    pub number: u64,
    pub title: String,
    pub linear_identifier: Option<String>,
    pub jira_identifier: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct WorkspaceIntentArgs {
    pub source_text: Option<String>,
    pub work_item: Option<WorkspaceIntentWorkItem>,
    pub fallback_name: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceIntentName {
    pub display_name: String,
    pub seed_name: String,
}

const STOP_WORDS: [&str; 16] =
    ["a", "an", "and", "for", "from", "in", "is", "it", "of", "on", "or", "the", "this", "to", "with", ""];

fn action_labels() -> &'static [(Regex, &'static str)] {
    static LABELS: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    LABELS.get_or_init(|| {
        // Generated slugs are hyphenated, so the boundaries exclude `-`/`_` (not
        // `\b`): `issue-123-fix-title` must NOT read as a "fix" action.
        let bound = |word: &str| Regex::new(&format!(r"(?i)(?:^|[^a-z0-9_-])(?:{word})(?:$|[^a-z0-9_-])")).unwrap();
        vec![
            (bound("fix(?:e[sd])?|resolve|repair"), "Fix"),
            (bound("debug|diagnose"), "Debug"),
            (bound(r"review|look\s+over|inspect|check|safe|safety"), "Review"),
            (bound("implement|build|ship"), "Implement"),
            (bound("investigate|understand|triage"), "Investigate"),
            (bound("add|create"), "Add"),
            (bound("update|change"), "Update"),
            (bound("refactor|simplify"), "Refactor"),
            (bound("test|verify|validate"), "Test"),
        ]
    })
}

fn detect_intent_action(source_text: &str) -> Option<&'static str> {
    action_labels().iter().find(|(pattern, _)| pattern.is_match(source_text)).map(|(_, label)| *label)
}

fn title_case_word(word: &str) -> String {
    if re(r"^[A-Z]{2,}\d*$").is_match(word) || re(r"(?i)^[A-Z]+-\d+$").is_match(word) {
        return word.to_uppercase();
    }
    let lower = word.to_lowercase();
    let mut chars = lower.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn compact_words(input: &str, max_words: usize) -> String {
    let without_urls = re(r"(?i)https?://\S+").replace_all(input, " ");
    let without_brackets = re(r#"[()\[\]{}"']"#).replace_all(&without_urls, " ");
    let without_seps = re(r"[#/\\:_-]+").replace_all(&without_brackets, " ");
    without_seps
        .split_whitespace()
        .filter(|word| !STOP_WORDS.contains(&word.to_lowercase().as_str()))
        .take(max_words)
        .map(title_case_word)
        .collect::<Vec<_>>()
        .join(" ")
}

fn compact_work_item_title(title: &str, item: &WorkspaceIntentWorkItem) -> String {
    let mut without = re(r"(?i)^(?:issue|pr|pull request|mr|merge request)\s*[#!]?\d+\s*[:-]\s*")
        .replace(title.trim(), "")
        .into_owned();
    without = re(r"\([#!]?\d+\)").replace_all(&without, "").into_owned();
    without = re(r"^[^:]{1,32}:\s*").replace(&without, "").trim().to_string();
    if item.number > 0 {
        if let Ok(number_re) = Regex::new(&format!(r"\b[#!]?{}\b", item.number)) {
            without = number_re.replace_all(&without, "").trim().to_string();
        }
    }
    if let Some(identifier) = item.linear_identifier.as_deref().or(item.jira_identifier.as_deref()) {
        if let Ok(identifier_re) = Regex::new(&format!(r"(?i)^{}\s*[:-]?\s*", regex::escape(identifier))) {
            without = identifier_re.replace(&without, "").trim().to_string();
        }
    }
    let seed = if without.is_empty() { title } else { &without };
    compact_words(seed, 3)
}

fn work_item_identity(item: &WorkspaceIntentWorkItem) -> String {
    if let Some(identifier) = &item.linear_identifier {
        return identifier.to_uppercase();
    }
    if let Some(identifier) = &item.jira_identifier {
        return identifier.to_uppercase();
    }
    match item.kind {
        Some(WorkItemType::Pr) => format!("PR {}", item.number),
        Some(WorkItemType::Mr) => format!("MR {}", item.number),
        _ => format!("Issue {}", item.number),
    }
}

fn default_action_for_work_item(item: &WorkspaceIntentWorkItem) -> Option<&'static str> {
    matches!(item.kind, Some(WorkItemType::Pr | WorkItemType::Mr)).then_some("Review")
}

pub fn get_workspace_intent_name(args: &WorkspaceIntentArgs) -> Option<WorkspaceIntentName> {
    let source_text = args.source_text.as_deref().map(str::trim).unwrap_or("");
    let mut display_name = String::new();

    if let Some(item) = &args.work_item {
        let action = detect_intent_action(source_text).or_else(|| default_action_for_work_item(item));
        let identity = work_item_identity(item);
        display_name = match action {
            Some(action) => format!("{action} {identity}"),
            None => {
                let subject = compact_work_item_title(&item.title, item);
                [identity, subject].into_iter().filter(|part| !part.is_empty()).collect::<Vec<_>>().join(" ")
            }
        };
    } else if !source_text.is_empty() {
        display_name = compact_words(source_text, 5);
    }

    if display_name.is_empty() {
        if let Some(fallback) = args.fallback_name.as_deref().map(str::trim).filter(|f| !f.is_empty()) {
            display_name = fallback.to_string();
        }
    }
    if display_name.is_empty() {
        return None;
    }

    let seed_name = slugify_for_workspace_name(&display_name);
    if seed_name.is_empty() {
        return None;
    }
    Some(WorkspaceIntentName { display_name, seed_name })
}

pub fn get_linear_issue_workspace_name(identifier: &str, title: &str) -> String {
    let key = slugify_for_workspace_name(identifier);
    let title_slug = get_linked_work_item_suggested_name(title);
    if key.is_empty() {
        return title_slug;
    }
    let deduped = if title_slug == key {
        String::new()
    } else if let Some(rest) = title_slug.strip_prefix(&format!("{key}-")) {
        rest.to_string()
    } else {
        title_slug
    };
    let combined = [key, deduped].into_iter().filter(|part| !part.is_empty()).collect::<Vec<_>>().join("-");
    slugify_for_workspace_name(&combined)
}

pub fn resolve_workspace_create_name(draft: Option<&str>, fallback: &str) -> String {
    match draft.map(str::trim).filter(|d| !d.is_empty()) {
        Some(draft) => draft.to_string(),
        None => fallback.to_string(),
    }
}

// Workspace naming is a rare, non-hot path, so compiling these patterns on use
// (rather than caching each) keeps the code simple; the literal patterns are
// known-good so `unwrap` cannot fire.
fn re(pattern: &str) -> Regex {
    Regex::new(pattern).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issue(number: u64, title: &str) -> WorkspaceIntentWorkItem {
        WorkspaceIntentWorkItem { kind: Some(WorkItemType::Issue), number, title: title.to_string(), ..Default::default() }
    }

    #[test]
    fn keeps_workspace_seed_slugs_short_ascii_safe_and_git_ref_safe() {
        assert_eq!(slugify_for_workspace_name("../../Fix mobile Tasks 🚀"), "fix-mobile-tasks");
        assert_eq!(slugify_for_workspace_name("feature/add issue drawer"), "feature-add-issue-drawer");
        assert_eq!(slugify_for_workspace_name(&"a".repeat(80)), "a".repeat(48));
    }

    #[test]
    fn removes_duplicated_issue_and_pr_numbers_from_linked_titles() {
        assert_eq!(get_linked_work_item_suggested_name("Issue #123: Fix mobile Tasks"), "fix-mobile-tasks");
        assert_eq!(get_linked_work_item_suggested_name("Add mobile drawer (#812)"), "add-mobile-drawer");
    }

    fn intent(source_text: &str, item: Option<WorkspaceIntentWorkItem>) -> Option<WorkspaceIntentName> {
        get_workspace_intent_name(&WorkspaceIntentArgs {
            source_text: (!source_text.is_empty()).then(|| source_text.to_string()),
            work_item: item,
            fallback_name: None,
        })
    }

    fn name(display: &str, seed: &str) -> Option<WorkspaceIntentName> {
        Some(WorkspaceIntentName { display_name: display.to_string(), seed_name: seed.to_string() })
    }

    #[test]
    fn uses_explicit_user_intent_for_linked_issues() {
        assert_eq!(
            intent(
                "https://github.com/mvanhorn/cli-printing-press/issues/2635 and fix it",
                Some(issue(2635, "scorer/dogfood: live acceptance can't authenticate via the CLI's config/cookie credentials (scoped-home is env-only)")),
            ),
            name("Fix Issue 2635", "fix-issue-2635")
        );
    }

    #[test]
    fn defaults_pr_and_mr_work_to_review_oriented_identities() {
        assert_eq!(
            intent(
                "https://github.com/acme/app/pull/1234 and check whether this is safe",
                Some(WorkspaceIntentWorkItem { kind: Some(WorkItemType::Pr), number: 1234, title: "Refactor account settings panel".to_string(), ..Default::default() }),
            ),
            name("Review PR 1234", "review-pr-1234")
        );
        assert_eq!(
            intent(
                "fix https://gitlab.com/acme/app/-/merge_requests/77",
                Some(WorkspaceIntentWorkItem { kind: Some(WorkItemType::Mr), number: 77, title: "Resolve sync race".to_string(), ..Default::default() }),
            ),
            name("Fix MR 77", "fix-mr-77")
        );
    }

    #[test]
    fn uses_a_compressed_subject_when_a_linked_issue_has_no_action() {
        assert_eq!(
            intent("https://github.com/acme/app/issues/9876", Some(issue(9876, "Make importer handle archived rows"))),
            name("Issue 9876 Make Importer Handle", "issue-9876-make-importer-handle")
        );
    }

    #[test]
    fn does_not_treat_an_auto_generated_slug_as_explicit_user_intent() {
        assert_eq!(
            intent("issue-123-fix-navbar", Some(issue(456, "Make importer handle archived rows"))),
            name("Issue 456 Make Importer Handle", "issue-456-make-importer-handle")
        );
    }

    #[test]
    fn uses_external_provider_identifiers_without_duplicating_in_the_subject() {
        assert_eq!(
            get_workspace_intent_name(&WorkspaceIntentArgs {
                work_item: Some(WorkspaceIntentWorkItem {
                    kind: Some(WorkItemType::Issue),
                    number: 0,
                    title: "PROJ-7 Fix flaky import".to_string(),
                    jira_identifier: Some("PROJ-7".to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            name("PROJ-7 Fix Flaky Import", "proj-7-fix-flaky-import")
        );
    }

    #[test]
    fn summarizes_unlinked_task_text_into_a_shared_display_and_seed() {
        assert_eq!(
            intent("add keyboard shortcut settings", None),
            name("Add Keyboard Shortcut Settings", "add-keyboard-shortcut-settings")
        );
    }

    #[test]
    fn keeps_the_linear_identifier_in_the_workspace_seed() {
        assert_eq!(get_linear_issue_workspace_name("ENG-42", "Ship Linear parity"), "eng-42-ship-linear-parity");
    }

    #[test]
    fn does_not_duplicate_an_identifier_already_present_in_the_linear_title() {
        assert_eq!(get_linear_issue_workspace_name("ENG-42", "ENG-42 Ship Linear parity"), "eng-42-ship-linear-parity");
    }

    #[test]
    fn keeps_the_combined_linear_seed_within_the_workspace_name_limit() {
        let seed = get_linear_issue_workspace_name("ENG-42", "Implement a very long Linear issue title that should be truncated");
        assert!(seed.chars().count() <= 48);
        assert!(seed.starts_with("eng-42-"));
    }

    #[test]
    fn preserves_explicit_user_entered_names_for_the_host_worktree_sanitizer() {
        assert_eq!(resolve_workspace_create_name(Some("feature/something"), "issue-123"), "feature/something");
        assert_eq!(resolve_workspace_create_name(Some("日本語 テスト"), "issue-123"), "日本語 テスト");
    }

    #[test]
    fn uses_the_stable_fallback_when_the_draft_is_blank() {
        assert_eq!(resolve_workspace_create_name(Some("   "), "pr-9"), "pr-9");
        assert_eq!(resolve_workspace_create_name(None, "issue-4"), "issue-4");
    }
}
