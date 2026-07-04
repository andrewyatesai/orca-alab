//! Parity dispatch for `orca_git::git_history_log_parser` vs
//! `src/shared/git-history-log-parser.ts`. Shapes the Rust ports' output to
//! match `JSON.stringify` of the TS return (optionals omitted when `None`).

use orca_git::git_history_log_parser::{
    git_history_log_to_json, git_history_ref_from_full_name, parse_git_history_log, short_git_hash,
};
use serde_json::{json, Map, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        // The napi export and this harness share one canonical serializer
        // (orca-git) so the production path and the parity check can't diverge.
        "parseGitHistoryLog" => git_history_log_to_json(&parse_git_history_log(input.as_str().unwrap_or(""))),
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

/// The `gitHistoryRefFromFullName` parity case returns a single ref, so it keeps a
/// local serializer (the shared one covers the item-list path).
fn ref_to_json(reference: &orca_git::git_history_types::GitHistoryItemRef) -> Value {
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
