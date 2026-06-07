//! Task search-query parse/serialize/transform, ported from `src/shared/task-query.ts`.
//!
//! Parses a GitHub-style task search string into structured filters (scope,
//! state, draft, assignee/author/review, labels, free text), serializes back
//! (round-tripping known qualifiers), applies single-filter edits, and strips
//! `repo:` qualifiers before cross-repo fan-out. Pure (hand-rolled tokenizer;
//! no regex).

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TaskScope {
    #[default]
    All,
    Issue,
    Pr,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskState {
    Open,
    Closed,
    All,
    Merged,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParsedTaskQuery {
    pub scope: TaskScope,
    pub state: Option<TaskState>,
    pub draft: bool,
    pub assignee: Option<String>,
    pub author: Option<String>,
    pub review_requested: Option<String>,
    pub reviewed_by: Option<String>,
    pub labels: Vec<String>,
    pub free_text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskQueryFilterKey {
    Author,
    Assignee,
    ReviewRequested,
    ReviewedBy,
    Labels,
    State,
    Draft,
}

/// The value for [`with_qualifier`]: a single value, a label list, or clear.
#[derive(Clone, Debug)]
pub enum QualifierValue {
    Str(String),
    List(Vec<String>),
    Clear,
}

struct SearchQueryToken {
    value: String,
    raw: String,
}

fn tokenize_with_raw(raw_query: &str) -> Vec<SearchQueryToken> {
    let mut tokens = Vec::new();
    let mut value = String::new();
    let mut raw = String::new();
    let mut quote: Option<char> = None;

    for ch in raw_query.chars() {
        if ch.is_whitespace() && quote.is_none() {
            if !value.is_empty() || !raw.is_empty() {
                tokens.push(SearchQueryToken { value: std::mem::take(&mut value), raw: std::mem::take(&mut raw) });
            }
            continue;
        }
        raw.push(ch);
        if (ch == '"' || ch == '\'') && quote.is_none() {
            quote = Some(ch);
            continue;
        }
        if Some(ch) == quote {
            quote = None;
            continue;
        }
        value.push(ch);
    }
    if !value.is_empty() || !raw.is_empty() {
        tokens.push(SearchQueryToken { value, raw });
    }
    tokens
}

pub fn tokenize_search_query(raw_query: &str) -> Vec<String> {
    tokenize_with_raw(raw_query).into_iter().map(|token| token.value).collect()
}

pub fn parse_task_query(raw_query: &str) -> ParsedTaskQuery {
    let mut query = ParsedTaskQuery::default();
    let mut free_text_tokens: Vec<String> = Vec::new();
    let mut saw_issue_scope = false;
    let mut saw_pr_scope = false;

    for token in tokenize_with_raw(raw_query.trim()) {
        let normalized = token.value.to_lowercase();
        match normalized.as_str() {
            "is:issue" => {
                saw_issue_scope = true;
                query.scope = if saw_pr_scope { TaskScope::All } else { TaskScope::Issue };
                continue;
            }
            "is:pr" | "is:pull-request" => {
                saw_pr_scope = true;
                query.scope = if saw_issue_scope { TaskScope::All } else { TaskScope::Pr };
                continue;
            }
            "is:open" => {
                query.state = Some(TaskState::Open);
                continue;
            }
            "is:closed" => {
                query.state = Some(TaskState::Closed);
                continue;
            }
            "is:merged" => {
                query.state = Some(TaskState::Merged);
                continue;
            }
            "is:draft" => {
                query.scope = TaskScope::Pr;
                query.state = Some(TaskState::Open);
                query.draft = true;
                continue;
            }
            _ => {}
        }

        let (raw_key, value) = match token.value.split_once(':') {
            Some((key, rest)) => (key, rest.trim()),
            None => (token.value.as_str(), ""),
        };
        let key = raw_key.to_lowercase();
        if value.is_empty() {
            free_text_tokens.push(token.raw.clone());
            continue;
        }

        match key.as_str() {
            "assignee" => query.assignee = Some(value.to_string()),
            "author" => query.author = Some(value.to_string()),
            "review-requested" => {
                query.scope = TaskScope::Pr;
                query.review_requested = Some(value.to_string());
            }
            "reviewed-by" => {
                query.scope = TaskScope::Pr;
                query.reviewed_by = Some(value.to_string());
            }
            "label" => query.labels.push(value.to_string()),
            "state" => match value.to_lowercase().as_str() {
                "open" => query.state = Some(TaskState::Open),
                "closed" => query.state = Some(TaskState::Closed),
                "merged" => query.state = Some(TaskState::Merged),
                "all" => query.state = Some(TaskState::All),
                // Unknown qualifiers/exact phrases pass through to search as-is.
                _ => free_text_tokens.push(token.raw.clone()),
            },
            _ => free_text_tokens.push(token.raw.clone()),
        }
    }

    if query.draft {
        query.scope = TaskScope::Pr;
        query.state = Some(TaskState::Open);
    } else if query.state == Some(TaskState::Merged) || query.review_requested.is_some() || query.reviewed_by.is_some() {
        query.scope = TaskScope::Pr;
    }
    query.free_text = free_text_tokens.join(" ").trim().to_string();
    query
}

fn quote_if_needed(value: &str) -> String {
    if value.chars().any(char::is_whitespace) {
        format!("\"{}\"", value.replace('"', "\\\""))
    } else {
        value.to_string()
    }
}

pub fn serialize_task_query(query: &ParsedTaskQuery) -> String {
    let mut parts: Vec<String> = Vec::new();
    match query.scope {
        TaskScope::Pr => parts.push("is:pr".to_string()),
        TaskScope::Issue => parts.push("is:issue".to_string()),
        TaskScope::All => {}
    }
    match query.state {
        Some(TaskState::Open) => parts.push("is:open".to_string()),
        Some(TaskState::Closed) => parts.push("is:closed".to_string()),
        Some(TaskState::Merged) => parts.push("is:merged".to_string()),
        Some(TaskState::All) => parts.push("state:all".to_string()),
        None => {}
    }
    if query.draft {
        parts.push("is:draft".to_string());
    }
    if let Some(author) = &query.author {
        parts.push(format!("author:{}", quote_if_needed(author)));
    }
    if let Some(assignee) = &query.assignee {
        parts.push(format!("assignee:{}", quote_if_needed(assignee)));
    }
    if let Some(review_requested) = &query.review_requested {
        parts.push(format!("review-requested:{}", quote_if_needed(review_requested)));
    }
    if let Some(reviewed_by) = &query.reviewed_by {
        parts.push(format!("reviewed-by:{}", quote_if_needed(reviewed_by)));
    }
    for label in &query.labels {
        parts.push(format!("label:{}", quote_if_needed(label)));
    }
    if !query.free_text.is_empty() {
        parts.push(query.free_text.clone());
    }
    parts.join(" ")
}

pub fn with_qualifier(raw_query: &str, key: TaskQueryFilterKey, value: QualifierValue) -> String {
    let mut parsed = parse_task_query(raw_query);
    let as_str = |value: &QualifierValue| match value {
        QualifierValue::Str(text) => Some(text.clone()),
        _ => None,
    };
    match key {
        TaskQueryFilterKey::Author => parsed.author = as_str(&value),
        TaskQueryFilterKey::Assignee => parsed.assignee = as_str(&value),
        TaskQueryFilterKey::ReviewRequested => {
            parsed.review_requested = as_str(&value);
            if parsed.review_requested.is_some() {
                parsed.scope = TaskScope::Pr;
            }
        }
        TaskQueryFilterKey::ReviewedBy => {
            parsed.reviewed_by = as_str(&value);
            if parsed.reviewed_by.is_some() {
                parsed.scope = TaskScope::Pr;
            }
        }
        TaskQueryFilterKey::Labels => {
            parsed.labels = match value {
                QualifierValue::List(list) => list,
                _ => Vec::new(),
            };
        }
        TaskQueryFilterKey::State => {
            parsed.state = match as_str(&value).as_deref() {
                Some("open") => Some(TaskState::Open),
                Some("closed") => Some(TaskState::Closed),
                Some("merged") => Some(TaskState::Merged),
                Some("all") => Some(TaskState::All),
                _ => None,
            };
            if parsed.state == Some(TaskState::Merged) {
                parsed.scope = TaskScope::Pr;
            }
            if parsed.state != Some(TaskState::Open) {
                parsed.draft = false;
            }
        }
        TaskQueryFilterKey::Draft => {
            parsed.draft = matches!(&value, QualifierValue::Str(text) if text == "true");
            if parsed.draft {
                parsed.scope = TaskScope::Pr;
                parsed.state = Some(TaskState::Open);
            }
        }
    }
    serialize_task_query(&parsed)
}

fn is_repo_qualifier(token: &str) -> bool {
    match token.to_lowercase().strip_prefix("repo:") {
        Some(rest) => !rest.is_empty() && !rest.chars().any(char::is_whitespace),
        None => false,
    }
}

pub fn strip_repo_qualifiers(raw_query: &str) -> String {
    let mut kept: Vec<String> = Vec::new();
    for token in tokenize_search_query(raw_query.trim()) {
        if is_repo_qualifier(&token) {
            continue;
        }
        if token.chars().any(char::is_whitespace) {
            match token.split_once(':') {
                Some((raw_key, rest)) => kept.push(format!("{raw_key}:\"{rest}\"")),
                None => kept.push(format!("\"{token}\"")),
            }
        } else {
            kept.push(token);
        }
    }
    kept.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- tokenizeSearchQuery ---

    #[test]
    fn splits_on_whitespace() {
        assert_eq!(tokenize_search_query("is:open assignee:@me foo"), ["is:open", "assignee:@me", "foo"]);
    }

    #[test]
    fn unwraps_standalone_double_quoted_tokens() {
        assert_eq!(tokenize_search_query("\"needs review\" foo"), ["needs review", "foo"]);
    }

    #[test]
    fn unwraps_standalone_single_quoted_tokens() {
        assert_eq!(tokenize_search_query("'with spaces' bar"), ["with spaces", "bar"]);
    }

    #[test]
    fn keeps_quoted_qualifier_values_as_one_token() {
        assert_eq!(tokenize_search_query("label:\"needs review\" author:alice"), ["label:needs review", "author:alice"]);
    }

    #[test]
    fn returns_an_empty_list_for_an_empty_string() {
        assert!(tokenize_search_query("").is_empty());
    }

    // --- parseTaskQuery ---

    #[test]
    fn returns_defaults_for_an_empty_query() {
        let parsed = parse_task_query("");
        assert_eq!(parsed.scope, TaskScope::All);
        assert_eq!(parsed.state, None);
        assert!(parsed.labels.is_empty());
        assert_eq!(parsed.free_text, "");
    }

    #[test]
    fn parses_is_issue_and_is_open() {
        let parsed = parse_task_query("is:issue is:open");
        assert_eq!(parsed.scope, TaskScope::Issue);
        assert_eq!(parsed.state, Some(TaskState::Open));
    }

    #[test]
    fn parses_is_pull_request_as_a_pr_scope_alias() {
        let parsed = parse_task_query("is:pull-request is:open");
        assert_eq!(parsed.scope, TaskScope::Pr);
        assert_eq!(parsed.state, Some(TaskState::Open));
    }

    #[test]
    fn widens_scope_to_all_when_both_issue_and_pr_present() {
        assert_eq!(parse_task_query("is:issue is:pr").scope, TaskScope::All);
        assert_eq!(parse_task_query("is:pr is:issue").scope, TaskScope::All);
    }

    #[test]
    fn is_draft_forces_pr_scope_and_open_state() {
        let parsed = parse_task_query("is:draft");
        assert_eq!(parsed.scope, TaskScope::Pr);
        assert_eq!(parsed.state, Some(TaskState::Open));
        assert!(parsed.draft);
    }

    #[test]
    fn keeps_draft_scoped_to_open_prs_even_with_later_issue_token() {
        let parsed = parse_task_query("is:draft is:issue");
        assert_eq!(parsed.scope, TaskScope::Pr);
        assert_eq!(parsed.state, Some(TaskState::Open));
        assert!(parsed.draft);
    }

    #[test]
    fn is_pr_is_open_does_not_set_draft() {
        let parsed = parse_task_query("is:pr is:open");
        assert_eq!(parsed.scope, TaskScope::Pr);
        assert_eq!(parsed.state, Some(TaskState::Open));
        assert!(!parsed.draft);
    }

    #[test]
    fn extracts_assignee_author_label_and_review_qualifiers() {
        let parsed = parse_task_query("assignee:@me author:alice review-requested:@me label:bug free text");
        assert_eq!(parsed.assignee.as_deref(), Some("@me"));
        assert_eq!(parsed.author.as_deref(), Some("alice"));
        assert_eq!(parsed.review_requested.as_deref(), Some("@me"));
        assert_eq!(parsed.scope, TaskScope::Pr);
        assert_eq!(parsed.labels, ["bug"]);
        assert_eq!(parsed.free_text, "free text");
    }

    #[test]
    fn keeps_review_qualifiers_scoped_to_prs_even_with_later_issue_token() {
        let parsed = parse_task_query("review-requested:@me is:issue");
        assert_eq!(parsed.scope, TaskScope::Pr);
        assert_eq!(parsed.review_requested.as_deref(), Some("@me"));
    }

    #[test]
    fn leaves_unknown_qualifiers_and_bare_words_in_free_text() {
        assert_eq!(parse_task_query("custom:value hello").free_text, "custom:value hello");
    }

    #[test]
    fn parses_state_all_for_the_any_state_filter() {
        let parsed = parse_task_query("is:pr state:all");
        assert_eq!(parsed.scope, TaskScope::Pr);
        assert_eq!(parsed.state, Some(TaskState::All));
    }

    // --- stripRepoQualifiers ---

    #[test]
    fn removes_repo_owner_name_tokens() {
        assert_eq!(strip_repo_qualifiers("is:open repo:foo/bar assignee:@me"), "is:open assignee:@me");
    }

    #[test]
    fn is_case_insensitive_on_the_repo_key() {
        assert_eq!(strip_repo_qualifiers("REPO:Foo/Bar is:open"), "is:open");
    }

    #[test]
    fn keeps_other_qualifiers_intact() {
        assert_eq!(strip_repo_qualifiers("label:bug repo:a/b"), "label:bug");
    }

    #[test]
    fn re_quotes_a_standalone_token_that_contains_whitespace() {
        assert_eq!(strip_repo_qualifiers("\"needs review\" repo:x/y"), "\"needs review\"");
    }

    #[test]
    fn returns_empty_string_when_only_repo_qualifiers_present() {
        assert_eq!(strip_repo_qualifiers("repo:foo/bar repo:baz/qux"), "");
    }

    #[test]
    fn preserves_a_bare_word_containing_no_space() {
        assert_eq!(strip_repo_qualifiers("hello repo:a/b world"), "hello world");
    }

    // --- serializeTaskQuery ---

    #[test]
    fn round_trips_qualifiers_and_free_text() {
        let raw = "is:pr is:open author:alice label:bug review-requested:bob hello world";
        let reserialized = serialize_task_query(&parse_task_query(raw));
        assert_eq!(parse_task_query(&reserialized), parse_task_query(raw));
    }

    #[test]
    fn quotes_label_values_containing_whitespace() {
        let parsed = parse_task_query("label:\"needs review\"");
        assert_eq!(parsed.labels, ["needs review"]);
        assert_eq!(parsed.free_text, "");
        assert!(serialize_task_query(&parsed).contains("label:\"needs review\""));
    }

    #[test]
    fn serializes_all_state_so_filter_changes_do_not_fall_back_to_default_open() {
        assert_eq!(serialize_task_query(&parse_task_query("is:pr state:all")), "is:pr state:all");
    }

    // --- withQualifier ---

    #[test]
    fn sets_and_clears_the_author_qualifier_without_disturbing_free_text() {
        let set = with_qualifier("hello", TaskQueryFilterKey::Author, QualifierValue::Str("alice".to_string()));
        assert_eq!(parse_task_query(&set).author.as_deref(), Some("alice"));
        assert_eq!(parse_task_query(&set).free_text, "hello");
        let cleared = with_qualifier(&set, TaskQueryFilterKey::Author, QualifierValue::Clear);
        assert_eq!(parse_task_query(&cleared).author, None);
        assert_eq!(parse_task_query(&cleared).free_text, "hello");
    }

    #[test]
    fn replaces_the_labels_list() {
        let result = with_qualifier("label:bug label:enh", TaskQueryFilterKey::Labels, QualifierValue::List(vec!["triage".to_string()]));
        assert_eq!(parse_task_query(&result).labels, ["triage"]);
    }

    #[test]
    fn clears_labels_when_given_an_empty_array() {
        let result = with_qualifier("label:bug is:pr", TaskQueryFilterKey::Labels, QualifierValue::List(Vec::new()));
        assert!(parse_task_query(&result).labels.is_empty());
        assert_eq!(parse_task_query(&result).scope, TaskScope::Pr);
    }

    #[test]
    fn sets_the_all_state_filter() {
        let result = with_qualifier("is:pr is:open", TaskQueryFilterKey::State, QualifierValue::Str("all".to_string()));
        assert_eq!(parse_task_query(&result).state, Some(TaskState::All));
        assert!(result.contains("state:all"));
    }

    #[test]
    fn preserves_quoted_free_text_tokens_when_applying_a_filter() {
        let result = with_qualifier(
            "\"exact phrase\" milestone:\"next release\"",
            TaskQueryFilterKey::Author,
            QualifierValue::Str("alice".to_string()),
        );
        assert!(result.contains("\"exact phrase\""));
        assert!(result.contains("milestone:\"next release\""));
        assert_eq!(parse_task_query(&result).author.as_deref(), Some("alice"));
    }

    #[test]
    fn keeps_pr_only_filters_scoped_to_prs() {
        assert_eq!(
            parse_task_query(&with_qualifier("", TaskQueryFilterKey::Draft, QualifierValue::Str("true".to_string()))).scope,
            TaskScope::Pr
        );
        assert_eq!(
            parse_task_query(&with_qualifier("", TaskQueryFilterKey::State, QualifierValue::Str("merged".to_string()))).scope,
            TaskScope::Pr
        );
        assert_eq!(
            parse_task_query(&with_qualifier("", TaskQueryFilterKey::ReviewRequested, QualifierValue::Str("@me".to_string()))).scope,
            TaskScope::Pr
        );
    }

    #[test]
    fn forces_draft_filters_back_to_open_prs() {
        let parsed = parse_task_query(&with_qualifier("is:pr is:closed", TaskQueryFilterKey::Draft, QualifierValue::Str("true".to_string())));
        assert_eq!(parsed.scope, TaskScope::Pr);
        assert_eq!(parsed.state, Some(TaskState::Open));
        assert!(parsed.draft);
    }
}
