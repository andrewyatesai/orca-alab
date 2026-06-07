//! Parity dispatch for `orca_git::git_history_log_parser` vs
//! `src/shared/git-history-log-parser.ts`. Shapes the Rust ports' output to
//! match `JSON.stringify` of the TS return (optionals omitted when `None`).

use orca_git::git_history_log_parser::{
    git_history_ref_from_full_name, parse_git_history_log, short_git_hash,
};
use orca_git::git_history_types::{GitHistoryItem, GitHistoryItemRef};
use serde_json::{json, Map, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "parseGitHistoryLog" => Value::Array(
            parse_git_history_log(input.as_str().unwrap_or(""))
                .iter()
                .map(item_to_json)
                .collect(),
        ),
        "shortGitHash" => Value::String(short_git_hash(input.as_str().unwrap_or(""))),
        "gitHistoryRefFromFullName" => {
            let full_name = input.get("fullName").and_then(Value::as_str);
            let fallback_name = input.get("fallbackName").and_then(Value::as_str).unwrap_or("");
            let revision = input.get("revision").and_then(Value::as_str).unwrap_or("");
            ref_to_json(&git_history_ref_from_full_name(full_name, fallback_name, revision))
        }
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

/// Match `JSON.stringify` of a TS `GitHistoryItemRef` (undefined fields dropped).
fn ref_to_json(reference: &GitHistoryItemRef) -> Value {
    let mut map = Map::new();
    map.insert("id".to_string(), Value::String(reference.id.clone()));
    map.insert("name".to_string(), Value::String(reference.name.clone()));
    if let Some(revision) = &reference.revision {
        map.insert("revision".to_string(), Value::String(revision.clone()));
    }
    if let Some(category) = reference.category {
        map.insert("category".to_string(), Value::String(category.as_str().to_string()));
    }
    if let Some(description) = &reference.description {
        map.insert("description".to_string(), Value::String(description.clone()));
    }
    if let Some(color) = reference.color {
        map.insert("color".to_string(), Value::String(color.as_str().to_string()));
    }
    Value::Object(map)
}

/// Match `JSON.stringify` of a TS `GitHistoryItem` (undefined fields dropped).
fn item_to_json(item: &GitHistoryItem) -> Value {
    let mut map = Map::new();
    map.insert("id".to_string(), Value::String(item.id.clone()));
    map.insert(
        "parentIds".to_string(),
        Value::Array(item.parent_ids.iter().map(|id| Value::String(id.clone())).collect()),
    );
    map.insert("subject".to_string(), Value::String(item.subject.clone()));
    map.insert("message".to_string(), Value::String(item.message.clone()));
    if let Some(display_id) = &item.display_id {
        map.insert("displayId".to_string(), Value::String(display_id.clone()));
    }
    if let Some(author) = &item.author {
        map.insert("author".to_string(), Value::String(author.clone()));
    }
    if let Some(author_email) = &item.author_email {
        map.insert("authorEmail".to_string(), Value::String(author_email.clone()));
    }
    if let Some(timestamp) = item.timestamp {
        map.insert("timestamp".to_string(), Value::Number(timestamp.into()));
    }
    if let Some(statistics) = item.statistics {
        map.insert(
            "statistics".to_string(),
            json!({
                "files": statistics.files,
                "insertions": statistics.insertions,
                "deletions": statistics.deletions,
            }),
        );
    }
    if let Some(references) = &item.references {
        map.insert("references".to_string(), Value::Array(references.iter().map(ref_to_json).collect()));
    }
    Value::Object(map)
}
