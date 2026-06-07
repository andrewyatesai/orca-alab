//! Parity dispatch for `orca_core::task_query` vs `src/shared/task-query.ts`.

use orca_core::task_query::{
    parse_task_query, serialize_task_query, strip_repo_qualifiers, tokenize_search_query,
    with_qualifier, ParsedTaskQuery, QualifierValue, TaskQueryFilterKey, TaskScope, TaskState,
};
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "tokenizeSearchQuery" => {
            let tokens = tokenize_search_query(input.as_str().unwrap_or(""));
            Value::Array(tokens.into_iter().map(Value::String).collect())
        }
        "parseTaskQuery" => parsed_to_json(&parse_task_query(input.as_str().unwrap_or(""))),
        "serializeTaskQuery" => Value::String(serialize_task_query(&json_to_parsed(input))),
        "withQualifier" => {
            let raw = input.get("rawQuery").and_then(Value::as_str).unwrap_or("");
            match input.get("key").and_then(Value::as_str).and_then(filter_key_from_id) {
                Some(key) => {
                    let value = input.get("value").map(qualifier_value).unwrap_or(QualifierValue::Clear);
                    Value::String(with_qualifier(raw, key, value))
                }
                // Vectors only carry known keys; an unknown one is a vector bug.
                None => json!({ "__parity_error__": "unknown TaskQueryFilterKey in input.key" }),
            }
        }
        "stripRepoQualifiers" => Value::String(strip_repo_qualifiers(input.as_str().unwrap_or(""))),
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

/// Match `JSON.stringify` of the TS `ParsedTaskQuery`: every key present, with
/// `null` (not omitted) for the optional `state`/assignee/author/review fields,
/// and enums serialized to their TS string ids.
fn parsed_to_json(query: &ParsedTaskQuery) -> Value {
    json!({
        "scope": scope_id(query.scope),
        "state": state_value(query.state),
        "draft": query.draft,
        "assignee": optional_str(&query.assignee),
        "author": optional_str(&query.author),
        "reviewRequested": optional_str(&query.review_requested),
        "reviewedBy": optional_str(&query.reviewed_by),
        "labels": query.labels.clone(),
        "freeText": query.free_text,
    })
}

/// Rebuild a `ParsedTaskQuery` from the TS-shaped object so `serializeTaskQuery`
/// runs over the same input the TS reference receives.
fn json_to_parsed(input: &Value) -> ParsedTaskQuery {
    let str_field = |key: &str| input.get(key).and_then(Value::as_str).map(str::to_string);
    let labels: Vec<String> = input
        .get("labels")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    ParsedTaskQuery {
        scope: scope_from_id(input.get("scope").and_then(Value::as_str)),
        state: state_from_id(input.get("state").and_then(Value::as_str)),
        draft: input.get("draft").and_then(Value::as_bool).unwrap_or(false),
        assignee: str_field("assignee"),
        author: str_field("author"),
        review_requested: str_field("reviewRequested"),
        reviewed_by: str_field("reviewedBy"),
        labels,
        free_text: input.get("freeText").and_then(Value::as_str).unwrap_or("").to_string(),
    }
}

fn scope_id(scope: TaskScope) -> &'static str {
    match scope {
        TaskScope::All => "all",
        TaskScope::Issue => "issue",
        TaskScope::Pr => "pr",
    }
}

fn scope_from_id(id: Option<&str>) -> TaskScope {
    match id {
        Some("issue") => TaskScope::Issue,
        Some("pr") => TaskScope::Pr,
        _ => TaskScope::All,
    }
}

fn state_id(state: TaskState) -> &'static str {
    match state {
        TaskState::Open => "open",
        TaskState::Closed => "closed",
        TaskState::All => "all",
        TaskState::Merged => "merged",
    }
}

fn state_value(state: Option<TaskState>) -> Value {
    // TS keeps the key with an explicit `null` when there is no state filter.
    match state {
        Some(state) => Value::String(state_id(state).to_string()),
        None => Value::Null,
    }
}

fn state_from_id(id: Option<&str>) -> Option<TaskState> {
    match id {
        Some("open") => Some(TaskState::Open),
        Some("closed") => Some(TaskState::Closed),
        Some("merged") => Some(TaskState::Merged),
        Some("all") => Some(TaskState::All),
        _ => None,
    }
}

fn optional_str(value: &Option<String>) -> Value {
    match value {
        Some(text) => Value::String(text.clone()),
        None => Value::Null,
    }
}

fn filter_key_from_id(id: &str) -> Option<TaskQueryFilterKey> {
    match id {
        "author" => Some(TaskQueryFilterKey::Author),
        "assignee" => Some(TaskQueryFilterKey::Assignee),
        "reviewRequested" => Some(TaskQueryFilterKey::ReviewRequested),
        "reviewedBy" => Some(TaskQueryFilterKey::ReviewedBy),
        "labels" => Some(TaskQueryFilterKey::Labels),
        "state" => Some(TaskQueryFilterKey::State),
        "draft" => Some(TaskQueryFilterKey::Draft),
        _ => None,
    }
}

/// Mirror the TS union `string | string[] | null`: string -> Str, array -> List,
/// everything else (null/absent) -> Clear.
fn qualifier_value(value: &Value) -> QualifierValue {
    match value {
        Value::String(text) => QualifierValue::Str(text.clone()),
        Value::Array(items) => {
            QualifierValue::List(items.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        }
        _ => QualifierValue::Clear,
    }
}
