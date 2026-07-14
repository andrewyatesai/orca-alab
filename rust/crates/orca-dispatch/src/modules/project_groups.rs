//! Parity dispatch for `orca_config::project_groups` vs
//! `src/shared/project-groups.ts`.

use orca_config::project_groups::{
    create_project_group, get_next_project_group_order, get_project_group_subtree_ids,
    normalize_project_group_name, normalize_project_groups, ProjectGroup, ProjectGroupCreatedFrom,
    ProjectGroupNode, Repo,
};
use serde_json::{json, Map, Value};
use std::collections::HashSet;

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
        "normalizeProjectGroups" => {
            let value = input.get("value").cloned().unwrap_or(Value::Null);
            // Single injected clock (the TS reads Date.now() per-candidate, but
            // within one synchronous loop it's constant to the millisecond).
            let now = input.get("now").and_then(Value::as_f64).unwrap_or(0.0);
            let groups: Vec<Value> =
                normalize_project_groups(&value, now).iter().map(project_group_to_json).collect();
            json!(groups)
        }
        "createProjectGroup" => {
            let group = create_project_group(
                input.get("id").and_then(Value::as_str).unwrap_or_default(),
                input.get("name").and_then(Value::as_str).unwrap_or_default(),
                input.get("parentPath").and_then(Value::as_str),
                input.get("connectionId").and_then(Value::as_str),
                input.get("parentGroupId").and_then(Value::as_str),
                parse_created_from(input.get("createdFrom").and_then(Value::as_str)),
                input.get("tabOrder").and_then(Value::as_f64).unwrap_or(0.0),
                input.get("now").and_then(Value::as_f64).unwrap_or(0.0),
            );
            project_group_to_json(&group)
        }
        "clearMissingProjectGroupMemberships" => clear_missing_project_group_memberships_json(input),
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

fn parse_created_from(value: Option<&str>) -> ProjectGroupCreatedFrom {
    match value {
        Some("folder-scan") => ProjectGroupCreatedFrom::FolderScan,
        Some("migration") => ProjectGroupCreatedFrom::Migration,
        _ => ProjectGroupCreatedFrom::Manual,
    }
}

fn created_from_str(value: ProjectGroupCreatedFrom) -> &'static str {
    match value {
        ProjectGroupCreatedFrom::Manual => "manual",
        ProjectGroupCreatedFrom::FolderScan => "folder-scan",
        ProjectGroupCreatedFrom::Migration => "migration",
    }
}

/// Serialize a `ProjectGroup` to the TS object shape. `connectionId` is always
/// present (null when absent); `executionHostId` is emitted ONLY when set — the
/// TS spreads `...(executionHostId ? { executionHostId } : {})`.
fn project_group_to_json(group: &ProjectGroup) -> Value {
    let mut object = Map::new();
    object.insert("id".to_string(), json!(group.id));
    object.insert("name".to_string(), json!(group.name));
    object.insert("parentPath".to_string(), json!(group.parent_path));
    object.insert("connectionId".to_string(), json!(group.connection_id));
    object.insert("parentGroupId".to_string(), json!(group.parent_group_id));
    object.insert("createdFrom".to_string(), json!(created_from_str(group.created_from)));
    object.insert("tabOrder".to_string(), json!(group.tab_order));
    object.insert("isCollapsed".to_string(), json!(group.is_collapsed));
    object.insert("color".to_string(), json!(group.color));
    object.insert("createdAt".to_string(), json!(group.created_at));
    object.insert("updatedAt".to_string(), json!(group.updated_at));
    if let Some(execution_host_id) = &group.execution_host_id {
        object.insert("executionHostId".to_string(), json!(execution_host_id));
    }
    Value::Object(object)
}

/// Field-preserving passthrough: null out a repo's `projectGroupId` when it
/// points at a group that no longer exists (JS truthy check — null/empty are
/// left alone), else keep the repo object VERBATIM. Operates on raw `Value`s so
/// every other Repo field survives, which the lean typed `Repo` couldn't.
fn clear_missing_project_group_memberships_json(input: &Value) -> Value {
    let group_ids: HashSet<&str> = input
        .get("groups")
        .and_then(Value::as_array)
        .map(|groups| {
            groups.iter().filter_map(|group| group.get("id").and_then(Value::as_str)).collect()
        })
        .unwrap_or_default();
    let repos = input.get("repos").and_then(Value::as_array).cloned().unwrap_or_default();
    let cleared: Vec<Value> = repos
        .into_iter()
        .map(|mut repo| {
            let should_clear = repo
                .get("projectGroupId")
                .and_then(Value::as_str)
                .is_some_and(|group_id| !group_id.is_empty() && !group_ids.contains(group_id));
            if should_clear {
                if let Some(object) = repo.as_object_mut() {
                    object.insert("projectGroupId".to_string(), Value::Null);
                }
            }
            repo
        })
        .collect();
    json!(cleared)
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
