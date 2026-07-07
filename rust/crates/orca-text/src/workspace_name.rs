//! Workspace name/seed derivation, ported from `src/shared/workspace-name.ts`.
//!
//! Slugifies free text into git-ref-safe workspace seeds and derives a single
//! human "intent" name (e.g. `Fix Issue 2635`) for first-create identity, so the
//! folder/branch/sidebar names don't drift. Regex-backed; pure.

use regex::Regex;
use std::sync::OnceLock;

/// Curly apostrophes → ASCII (TS `normalizeApostrophes`).
fn normalize_apostrophes(input: &str) -> String {
    input.replace(['\u{2018}', '\u{2019}'], "'")
}

/// Contractions/possessives must not become stray `t`/`s` tokens in display
/// names or extra hyphen segments in branch-safe seeds (TS
/// `removeIntraWordApostrophes`: drop `'` between letter/number neighbours).
fn remove_intra_word_apostrophes(input: &str) -> String {
    let chars: Vec<char> = normalize_apostrophes(input).chars().collect();
    let mut out = String::with_capacity(chars.len());
    for (i, &c) in chars.iter().enumerate() {
        if c == '\''
            && i > 0
            && chars[i - 1].is_alphanumeric()
            && chars.get(i + 1).is_some_and(|n| n.is_alphanumeric())
        {
            continue;
        }
        out.push(c);
    }
    out
}

/// TS `stripDanglingDisplayApostrophes`: drop `'` when it does not sit between
/// two letter/number neighbours (leading, trailing, or bracketing a token edge).
fn strip_dangling_display_apostrophes(input: &str) -> String {
    let chars: Vec<char> = normalize_apostrophes(input).chars().collect();
    let mut out = String::with_capacity(chars.len());
    for (i, &c) in chars.iter().enumerate() {
        if c == '\'' {
            let prev_word = i > 0 && chars[i - 1].is_alphanumeric();
            let next_word = chars.get(i + 1).is_some_and(|n| n.is_alphanumeric());
            if !(prev_word && next_word) {
                continue;
            }
        }
        out.push(c);
    }
    out
}

/// The TS `isWorkspaceNameWhitespace` code list — NOT Unicode White_Space: it
/// includes U+FEFF (BOM) and excludes U+0085 (NEL), so `\s` is not a faithful
/// substitute. (Same list as the agent-tab-title fold set.)
fn is_workspace_name_whitespace(c: char) -> bool {
    let code = c as u32;
    code == 32
        || (9..=13).contains(&code)
        || code == 160
        || code == 5760
        || (8192..=8202).contains(&code)
        || code == 8232
        || code == 8233
        || code == 8239
        || code == 8287
        || code == 12288
        || code == 65279
}

/// Mirror of `foldWorkspaceNameWhitespaceToHyphen`: collapse fold-set runs to
/// single hyphens. A LEADING run also emits a hyphen (the TS fires
/// `pendingHyphen` unconditionally) — unreachable from `slugify` (input is
/// pre-trimmed) but kept exact; a trailing run emits nothing.
fn fold_workspace_name_whitespace_to_hyphen(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut pending_hyphen = false;
    for c in input.chars() {
        if is_workspace_name_whitespace(c) {
            pending_hyphen = true;
            continue;
        }
        if pending_hyphen {
            result.push('-');
            pending_hyphen = false;
        }
        result.push(c);
    }
    result
}

/// ECMAScript `String.prototype.trim` (Unicode White_Space − NEL + BOM) — the
/// JS set differs from Rust `str::trim` on exactly those two code points.
fn js_trim(value: &str) -> &str {
    value.trim_matches(|c: char| (c.is_whitespace() && c != '\u{0085}') || c == '\u{FEFF}')
}

pub fn slugify_for_workspace_name(input: &str) -> String {
    // TS order: strip intra-word apostrophes, trim, lowercase, backslash/slash
    // → hyphen, fold whitespace → hyphen, then the ref-safe character passes.
    let lowered = js_trim(&remove_intra_word_apostrophes(input)).to_lowercase();
    let s = re(r"[\\/]+").replace_all(&lowered, "-").into_owned();
    let s = fold_workspace_name_whitespace_to_hyphen(&s);
    let s = re(r"[^a-z0-9._-]+").replace_all(&s, "-");
    let s = re(r"-+").replace_all(&s, "-");
    let s = re(r"\.{2,}").replace_all(&s, ".");
    let s = re(r"^[.-]+|[.-]+$").replace_all(&s, "");
    let truncated: String = s.chars().take(48).collect();
    re(r"[-._]+$").replace_all(&truncated, "").into_owned()
}

/// TS `getLinkedWorkItemTitleSubject`: the title with issue/PR/MR prefixes and
/// duplicated `#123`-style numbers stripped. Empty when nothing useful remains.
fn get_linked_work_item_title_subject(title: &str) -> String {
    let trimmed = js_trim(title);
    let without = re(r"(?i)^(?:issue|pr|pull request|mr|merge request)\s*[#!]?\d+\s*[:-]\s*")
        .replace(trimmed, "");
    let without = re(r"^#\d+\s*[:-]\s*").replace(&without, "").into_owned();
    let without = re(r"\([#!]?\d+\)").replace_all(&without, "");
    let without = re(r"\b#\d+\b").replace_all(&without, "");
    js_trim(&without).to_string()
}

pub fn get_linked_work_item_suggested_name(title: &str) -> String {
    let subject = get_linked_work_item_title_subject(title);
    let seed = if subject.is_empty() { js_trim(title).to_string() } else { subject };
    slugify_for_workspace_name(&seed)
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
    let normalized = normalize_apostrophes(word);
    if re(r"^[A-Z]{2,}\d*$").is_match(&normalized) || re(r"(?i)^[A-Z]+-\d+$").is_match(&normalized)
    {
        return normalized.to_uppercase();
    }
    // Acronym possessive keeps its `'s` un-uppercased: `ABC's`, not `ABC'S`.
    if let Some(caps) = re(r"^([A-Z]{2,}\d*)'[sS]$").captures(&normalized) {
        return format!("{}'s", caps[1].to_uppercase());
    }
    let lower = normalized.to_lowercase();
    // Single-letter contractions title-case the letter before the apostrophe
    // (`i'm` → `I'm`), matching the TS split-on-apostrophe branch.
    let parts: Vec<&str> = lower.split('\'').collect();
    if parts.len() == 2 && parts[0].chars().count() == 1 && !parts[1].is_empty() {
        return format!("{}'{}", parts[0].to_uppercase(), parts[1]);
    }
    let mut chars = lower.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// TS `isCompactWorkspaceWordSeparator`: the fold-whitespace set plus the
/// explicit separator codes (quote, hash, parens, slash, colon, brackets,
/// backslash, underscore, braces, hyphen). Apostrophes are NOT separators —
/// they ride inside tokens and are handled by the apostrophe passes.
fn is_compact_word_separator(c: char) -> bool {
    is_workspace_name_whitespace(c)
        || matches!(c, '"' | '#' | '(' | ')' | '/' | ':' | '[' | '\\' | ']' | '_' | '{' | '}' | '-')
}

fn starts_with_http_url(chars: &[char], index: usize) -> bool {
    ["http://", "https://"].iter().any(|prefix| {
        let p: Vec<char> = prefix.chars().collect();
        index + p.len() <= chars.len()
            && chars[index..index + p.len()]
                .iter()
                .zip(&p)
                .all(|(a, b)| a.to_ascii_lowercase() == *b)
    })
}

/// Mirror of `collectCompactWorkspaceWords`: separator-delimited tokens with
/// http(s) URL skipping (a URL ends the current token and is consumed through
/// the next whitespace), stop-word filtering, and a `max_words` early stop.
fn collect_compact_workspace_words(input: &str, max_words: usize) -> Vec<String> {
    let chars: Vec<char> = input.chars().collect();
    let mut words: Vec<String> = Vec::new();
    let finish = |start: Option<usize>, end: usize, words: &mut Vec<String>, chars: &[char]| {
        if let Some(start) = start {
            if words.len() < max_words {
                let word: String = chars[start..end].iter().collect();
                if !word.is_empty() && !STOP_WORDS.contains(&word.to_lowercase().as_str()) {
                    words.push(word);
                }
            }
        }
    };
    let mut token_start: Option<usize> = None;
    let mut index = 0usize;
    while index <= chars.len() {
        let is_end = index == chars.len();
        if !is_end && starts_with_http_url(&chars, index) {
            finish(token_start, index, &mut words, &chars);
            token_start = None;
            while index < chars.len() && !is_workspace_name_whitespace(chars[index]) {
                index += 1;
            }
            if words.len() >= max_words {
                break;
            }
            continue;
        }
        if !is_end && !is_compact_word_separator(chars[index]) {
            if token_start.is_none() {
                token_start = Some(index);
            }
            index += 1;
            continue;
        }
        if token_start.is_some() {
            finish(token_start, index, &mut words, &chars);
            token_start = None;
            if words.len() >= max_words {
                break;
            }
        }
        index += 1;
    }
    words
}

fn compact_words(input: &str, max_words: usize) -> String {
    collect_compact_workspace_words(&strip_dangling_display_apostrophes(input), max_words)
        .iter()
        .map(|word| title_case_word(word))
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

/// TS `getLinkedWorkItemWorkspaceName`: display+seed for a linked work item —
/// `[identifier, subject]` joined (the RAW identifier, unlike
/// `work_item_identity`'s uppercased form), falling back to the identity when
/// both are empty. `None` when no git-safe seed can be derived.
pub fn get_linked_work_item_workspace_name(
    item: &WorkspaceIntentWorkItem,
) -> Option<WorkspaceIntentName> {
    let identifier = item.linear_identifier.as_deref().or(item.jira_identifier.as_deref());
    let subject_raw = get_linked_work_item_title_subject(&item.title);
    let mut subject =
        if subject_raw.is_empty() { js_trim(&item.title).to_string() } else { subject_raw };
    if let Some(identifier) = identifier {
        if let Ok(identifier_re) =
            Regex::new(&format!(r"(?i)^{}\s*[:-]?\s*", regex::escape(identifier)))
        {
            subject = js_trim(&identifier_re.replace(&subject, "")).to_string();
        }
    }
    let joined = [identifier.unwrap_or(""), &subject]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let display_name = if joined.is_empty() { work_item_identity(item) } else { joined };
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
    fn slugs_drop_intra_word_apostrophes_instead_of_splitting_tokens() {
        // "don't" must become "dont", not "don-t" (the resynced apostrophe pass).
        assert_eq!(slugify_for_workspace_name("Don't break the user's flow"), "dont-break-the-users-flow");
        assert_eq!(slugify_for_workspace_name("Fix \u{2019}quoted\u{2019} names"), "fix-quoted-names");
    }

    #[test]
    fn compact_words_skips_urls_and_dangling_apostrophes_and_title_cases_possessives() {
        // URL tokens are consumed whole; possessive acronyms keep 's.
        assert_eq!(compact_words("fix ABC's bug at https://example.com/x now", 5), "Fix ABC's Bug At Now");
        // Single-letter contraction keeps its apostrophe with the letter cased.
        assert_eq!(compact_words("i'm testing", 3), "I'm Testing");
        // Dangling apostrophes are stripped from display tokens.
        assert_eq!(compact_words("'quoted' words", 3), "Quoted Words");
    }

    #[test]
    fn linked_title_subject_strips_mr_prefixes_and_bang_numbers() {
        // The resynced prefix set covers GitLab MRs and [#!] markers.
        assert_eq!(
            get_linked_work_item_suggested_name("MR !42: Fix pipeline caching"),
            "fix-pipeline-caching"
        );
        assert_eq!(
            get_linked_work_item_suggested_name("Add drawer (!812)"),
            "add-drawer"
        );
    }

    #[test]
    fn linked_work_item_workspace_name_joins_identifier_and_subject() {
        let item = WorkspaceIntentWorkItem {
            kind: Some(WorkItemType::Issue),
            number: 7,
            title: "ENG-42: Fix the flaky sync".to_string(),
            linear_identifier: Some("ENG-42".to_string()),
            ..Default::default()
        };
        let name = get_linked_work_item_workspace_name(&item).unwrap();
        assert_eq!(name.display_name, "ENG-42 Fix the flaky sync");
        assert_eq!(name.seed_name, "eng-42-fix-the-flaky-sync");

        // A bare "#12" title is NOT the fallback case: the subject survives the
        // strips (no `\b` before `#`), so it becomes the display name directly.
        let hash_title = WorkspaceIntentWorkItem {
            kind: Some(WorkItemType::Pr),
            number: 12,
            title: "#12".to_string(),
            ..Default::default()
        };
        let name = get_linked_work_item_workspace_name(&hash_title).unwrap();
        assert_eq!(name.display_name, "#12");
        assert_eq!(name.seed_name, "12");

        // No identifier + EMPTY title falls back to the work-item identity.
        let bare = WorkspaceIntentWorkItem {
            kind: Some(WorkItemType::Pr),
            number: 12,
            title: String::new(),
            ..Default::default()
        };
        let name = get_linked_work_item_workspace_name(&bare).unwrap();
        assert_eq!(name.display_name, "PR 12");
        assert_eq!(name.seed_name, "pr-12");
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
