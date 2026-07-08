//! Parity dispatch for `orca_config::workspace_statuses` vs
//! `src/shared/workspace-statuses.ts`. Only the pure functions are exercised
//! here (normalization, id checks, group-key round-tripping).

use orca_config::workspace_statuses::{
    get_default_workspace_status_id, get_workspace_status_from_group_key,
    get_workspace_status_group_key, is_workspace_status_id, normalize_workspace_statuses,
    WorkspaceStatusDefinition,
};
use serde_json::{json, Map, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        // Single arg: the value to normalize is the input itself.
        "normalizeWorkspaceStatuses" => statuses_to_json(&normalize_workspace_statuses(input)),
        "isWorkspaceStatusId" => {
            let value = input.get("value").and_then(Value::as_str).unwrap_or_default();
            let statuses = statuses_from_json(input.get("statuses"));
            Value::Bool(is_workspace_status_id(value, &statuses))
        }
        // Single arg: the status list is the input itself.
        "getDefaultWorkspaceStatusId" => {
            let statuses = statuses_from_json(Some(input));
            Value::String(get_default_workspace_status_id(&statuses))
        }
        // Single arg: the status string is the input itself.
        "getWorkspaceStatusGroupKey" => {
            Value::String(get_workspace_status_group_key(input.as_str().unwrap_or_default()))
        }
        "getWorkspaceStatusFromGroupKey" => {
            let group_key = input.get("groupKey").and_then(Value::as_str).unwrap_or_default();
            let statuses = statuses_from_json(input.get("statuses"));
            // TS returns `WorkspaceStatus | null`; None maps to JSON null.
            match get_workspace_status_from_group_key(group_key, &statuses) {
                Some(status) => Value::String(status),
                None => Value::Null,
            }
        }
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

/// Match `JSON.stringify` of the TS `WorkspaceStatusDefinition[]`.
fn statuses_to_json(statuses: &[WorkspaceStatusDefinition]) -> Value {
    Value::Array(
        statuses
            .iter()
            .map(|status| {
                let mut object = Map::new();
                object.insert("id".to_string(), Value::String(status.id.clone()));
                object.insert("label".to_string(), Value::String(status.label.clone()));
                object.insert("color".to_string(), Value::String(status.color.clone()));
                object.insert("icon".to_string(), Value::String(status.icon.clone()));
                Value::Object(object)
            })
            .collect(),
    )
}

/// Rebuild the status list from the vector's JSON. Only `id` drives the pure
/// id/group-key functions, so absent fields default to empty strings.
fn statuses_from_json(value: Option<&Value>) -> Vec<WorkspaceStatusDefinition> {
    let Some(Value::Array(items)) = value else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let object = item.as_object()?;
            let field = |key: &str| object.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
            Some(WorkspaceStatusDefinition {
                id: field("id"),
                label: field("label"),
                color: field("color"),
                icon: field("icon"),
            })
        })
        .collect()
}
