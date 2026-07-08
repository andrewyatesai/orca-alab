//! Parity dispatch for `orca_core::gitlab_projects` vs
//! `src/shared/gitlab-projects.ts`.
//!
//! The TS `now: Date` is carried as the ISO string it persists to
//! (`nowIso`); the Rust port already takes that string verbatim, so both sides
//! produce the same `lastOpenedAt` on the freshly prepended entry.

use orca_core::gitlab_projects::{compute_next_gitlab_recents, GitLabRecentEntry};
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "computeNextGitLabRecents" => {
            let existing = parse_entries(input.get("existing"));
            let host = input.get("host").and_then(Value::as_str).unwrap_or("");
            let path = input.get("path").and_then(Value::as_str).unwrap_or("");
            let now_iso = input.get("nowIso").and_then(Value::as_str).unwrap_or("");
            let max = input.get("max").and_then(Value::as_u64).unwrap_or(0) as usize;
            entries_to_json(&compute_next_gitlab_recents(&existing, host, path, now_iso, max))
        }
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

/// Parse the TS `recent` array (`{ host, path, lastOpenedAt }[]`) into ports.
fn parse_entries(value: Option<&Value>) -> Vec<GitLabRecentEntry> {
    value
        .and_then(Value::as_array)
        .map(|items| items.iter().map(parse_entry).collect())
        .unwrap_or_default()
}

fn parse_entry(item: &Value) -> GitLabRecentEntry {
    let field = |key: &str| item.get(key).and_then(Value::as_str).unwrap_or("").to_string();
    GitLabRecentEntry {
        host: field("host"),
        path: field("path"),
        last_opened_at: field("lastOpenedAt"),
    }
}

/// Match `JSON.stringify` of the TS `recent` array, including the `lastOpenedAt`
/// field name (the Rust field is `last_opened_at`).
fn entries_to_json(entries: &[GitLabRecentEntry]) -> Value {
    Value::Array(
        entries
            .iter()
            .map(|entry| {
                json!({
                    "host": entry.host,
                    "path": entry.path,
                    "lastOpenedAt": entry.last_opened_at,
                })
            })
            .collect(),
    )
}
