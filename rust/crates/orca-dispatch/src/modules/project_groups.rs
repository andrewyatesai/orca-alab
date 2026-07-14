//! Parity dispatch for `orca_config::project_groups` vs
//! `src/shared/project-groups.ts`.

use orca_config::project_groups::{
    get_next_project_group_order, get_project_group_subtree_ids, normalize_project_group_name,
    ProjectGroupNode, Repo,
};
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "normalizeProjectGroupName" => {
            let name = input.get("name").and_then(Value::as_str).unwrap_or_default();
            // Absent `fallback` mirrors the TS default parameter ('Untitled group').
            let fallback =
                input.get("fallback").and_then(Value::as_str).unwrap_or("Untitled group");
            Value::String(normalize_project_group_name(name, fallback))
        }
        "getNextProjectGroupOrder" => {
            let repos = parse_repos(input.get("repos"));
            // A JSON `null` groupId yields `None`, matching the TS `string | null`.
            let group_id = input.get("groupId").and_then(Value::as_str);
            json!(get_next_project_group_order(&repos, group_id))
        }
        "getProjectGroupSubtreeIds" => {
            let nodes = parse_nodes(input.get("groups"));
            let root = input.get("rootGroupId").and_then(Value::as_str).unwrap_or_default();
            // The TS returns a `Set` (membership-only at every call site); emit a
            // SORTED array so the JSON is deterministic — the wrapper rebuilds a Set.
            let mut ids: Vec<String> =
                get_project_group_subtree_ids(&nodes, root).into_iter().collect();
            ids.sort();
            json!(ids)
        }
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

/// Build subtree `ProjectGroupNode`s from the vector array; only `id` +
/// `parentGroupId` (a `Pick`) carry, matching the TS signature.
fn parse_nodes(value: Option<&Value>) -> Vec<ProjectGroupNode> {
    value
        .and_then(Value::as_array)
        .map(|array| {
            array
                .iter()
                .filter_map(Value::as_object)
                .map(|object| ProjectGroupNode {
                    id: object.get("id").and_then(Value::as_str).unwrap_or_default().to_string(),
                    parent_group_id: object
                        .get("parentGroupId")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Build the membership/order `Repo`s from the vector array; only the fields the
/// port reads (`projectGroupId`, `projectGroupOrder`) carry, plus `id`.
fn parse_repos(value: Option<&Value>) -> Vec<Repo> {
    value
        .and_then(Value::as_array)
        .map(|array| {
            array
                .iter()
                .filter_map(Value::as_object)
                .map(|object| Repo {
                    id: object.get("id").and_then(Value::as_str).unwrap_or_default().to_string(),
                    project_group_id: object
                        .get("projectGroupId")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    project_group_order: object.get("projectGroupOrder").and_then(Value::as_f64),
                })
                .collect()
        })
        .unwrap_or_default()
}
