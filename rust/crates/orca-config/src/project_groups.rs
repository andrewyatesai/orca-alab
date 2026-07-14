//! Project-group organization, ported from `src/shared/project-groups.ts`.
//!
//! Creates/normalizes project groups (the repo-organization tree), clears dead
//! memberships, and computes subtree ids + next ordering. The group id and
//! timestamps are injected (the IO edge owns the RNG/clock); persisted-value
//! normalization reads `unknown` JSON via vendored `serde_json`.

use orca_core::execution_host::normalize_execution_host_id;
use orca_core::js_string::trim_js;
use serde_json::Value;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

/// Best-effort stand-in for JS `String.prototype.localeCompare` (default
/// locale/ICU) used only as the equal-`tabOrder` sort tiebreaker. Compares
/// case-insensitively first (so `apple` sorts before `Banana`, unlike raw
/// scalar order which puts uppercase first), then falls back to scalar order to
/// stay a deterministic total order. Not full ICU collation — accent adjacency
/// and non-Latin script ordering can still differ; that only surfaces on genuine
/// tabOrder ties between such names, a documented cosmetic divergence.
fn locale_compare_names(left: &str, right: &str) -> Ordering {
    left.to_lowercase().cmp(&right.to_lowercase()).then_with(|| left.cmp(right))
}

pub const UNGROUPED_PROJECT_GROUP_KEY: &str = "project-group:ungrouped";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectGroupCreatedFrom {
    Manual,
    FolderScan,
    Migration,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProjectGroup {
    pub id: String,
    pub name: String,
    pub parent_path: Option<String>,
    // Why: an SSH group's connection id and the runtime-owned host id must
    // survive normalization — otherwise a persisted remote group looks local on
    // reload. `execution_host_id` is emitted only when present (the TS spreads
    // `...(executionHostId ? { executionHostId } : {})`); `create_project_group`
    // never sets it, matching the TS factory.
    pub connection_id: Option<String>,
    pub parent_group_id: Option<String>,
    pub created_from: ProjectGroupCreatedFrom,
    pub tab_order: f64,
    pub is_collapsed: bool,
    pub color: Option<String>,
    pub created_at: f64,
    pub updated_at: f64,
    pub execution_host_id: Option<String>,
}

/// The repo fields the membership/order helpers read.
#[derive(Clone, Debug, PartialEq)]
pub struct Repo {
    pub id: String,
    pub project_group_id: Option<String>,
    pub project_group_order: Option<f64>,
}

/// `Pick<ProjectGroup, 'id' | 'parentGroupId'>` for subtree collection.
#[derive(Clone, Debug)]
pub struct ProjectGroupNode {
    pub id: String,
    pub parent_group_id: Option<String>,
}

pub fn normalize_project_group_name(name: &str, fallback: &str) -> String {
    // Why: JS `.trim()` (ECMAScript WhiteSpace) trims U+FEFF but keeps U+0085,
    // unlike Rust `str::trim` — mirror it so the twin is faithful.
    let trimmed = trim_js(name);
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
}

#[allow(clippy::too_many_arguments)]
pub fn create_project_group(
    id: &str,
    name: &str,
    parent_path: Option<&str>,
    connection_id: Option<&str>,
    parent_group_id: Option<&str>,
    created_from: ProjectGroupCreatedFrom,
    tab_order: f64,
    now: f64,
) -> ProjectGroup {
    ProjectGroup {
        id: id.to_string(),
        name: normalize_project_group_name(name, "Untitled group"),
        parent_path: parent_path.map(str::to_string),
        connection_id: connection_id.map(str::to_string),
        parent_group_id: parent_group_id.map(str::to_string),
        created_from,
        tab_order,
        is_collapsed: false,
        color: None,
        created_at: now,
        updated_at: now,
        // The TS factory never sets executionHostId.
        execution_host_id: None,
    }
}

fn finite_or(value: Option<&Value>, fallback: f64) -> f64 {
    value.and_then(Value::as_f64).filter(|number| number.is_finite()).unwrap_or(fallback)
}

pub fn normalize_project_groups(value: &Value, now: f64) -> Vec<ProjectGroup> {
    let Some(array) = value.as_array() else {
        return Vec::new();
    };
    let mut groups: Vec<ProjectGroup> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for candidate in array {
        let Some(object) = candidate.as_object() else {
            continue;
        };
        let Some(id) = object.get("id").and_then(Value::as_str) else {
            continue;
        };
        if !seen.insert(id.to_string()) {
            continue;
        }
        groups.push(ProjectGroup {
            id: id.to_string(),
            name: normalize_project_group_name(object.get("name").and_then(Value::as_str).unwrap_or(""), "Untitled group"),
            parent_path: object.get("parentPath").and_then(Value::as_str).map(str::to_string),
            // `typeof raw.connectionId === 'string' ? raw.connectionId : null`.
            connection_id: object.get("connectionId").and_then(Value::as_str).map(str::to_string),
            parent_group_id: object.get("parentGroupId").and_then(Value::as_str).map(str::to_string),
            created_from: match object.get("createdFrom").and_then(Value::as_str) {
                Some("folder-scan") => ProjectGroupCreatedFrom::FolderScan,
                Some("migration") => ProjectGroupCreatedFrom::Migration,
                _ => ProjectGroupCreatedFrom::Manual,
            },
            tab_order: finite_or(object.get("tabOrder"), 0.0),
            is_collapsed: object.get("isCollapsed") == Some(&Value::Bool(true)),
            color: object.get("color").and_then(Value::as_str).map(str::to_string),
            created_at: finite_or(object.get("createdAt"), now),
            updated_at: finite_or(object.get("updatedAt"), now),
            // Only carried when it normalizes; the dispatch serializer omits None.
            execution_host_id: object
                .get("executionHostId")
                .and_then(Value::as_str)
                .and_then(normalize_execution_host_id),
        });
    }

    groups.sort_by(|left, right| {
        left.tab_order
            .partial_cmp(&right.tab_order)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| locale_compare_names(&left.name, &right.name))
    });

    let group_ids: HashSet<String> = groups.iter().map(|group| group.id.clone()).collect();
    for group in &mut groups {
        let self_parent = group.parent_group_id.as_deref() == Some(group.id.as_str());
        let missing_parent = !group.parent_group_id.as_deref().is_some_and(|parent| group_ids.contains(parent));
        if self_parent || missing_parent {
            group.parent_group_id = None;
        }
    }
    groups
}

// clearMissingProjectGroupMemberships is not modeled here: it must preserve every
// Repo field verbatim (only nulling a dead projectGroupId), which the lean `Repo`
// struct can't express. Its single production impl is a serde_json::Value
// passthrough in orca-dispatch (modules::project_groups), the JSON layer.

pub fn get_project_group_subtree_ids(groups: &[ProjectGroupNode], root_group_id: &str) -> HashSet<String> {
    let mut children_by_parent: HashMap<&str, Vec<&str>> = HashMap::new();
    for group in groups {
        if let Some(parent) = &group.parent_group_id {
            children_by_parent.entry(parent.as_str()).or_default().push(group.id.as_str());
        }
    }
    let mut subtree: HashSet<String> = HashSet::new();
    let mut pending: Vec<&str> = vec![root_group_id];
    while let Some(group_id) = pending.pop() {
        if !subtree.insert(group_id.to_string()) {
            continue;
        }
        if let Some(children) = children_by_parent.get(group_id) {
            pending.extend(children.iter().copied());
        }
    }
    subtree
}

pub fn get_next_project_group_order(repos: &[Repo], group_id: Option<&str>) -> f64 {
    let mut max = -1.0_f64;
    for repo in repos {
        if repo.project_group_id.as_deref() != group_id {
            continue;
        }
        if let Some(order) = repo.project_group_order.filter(|order| order.is_finite()) {
            max = max.max(order);
        }
    }
    max + 1.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use ProjectGroupCreatedFrom::FolderScan;

    fn repo(id: &str, project_group_id: Option<&str>, project_group_order: Option<f64>) -> Repo {
        Repo { id: id.to_string(), project_group_id: project_group_id.map(str::to_string), project_group_order }
    }

    #[test]
    fn creates_a_durable_project_group_with_normalized_defaults() {
        let group = create_project_group("g1", "  Platform  ", Some("/srv/platform"), Some("conn-1"), None, FolderScan, 3.0, 100.0);
        assert_eq!(group.name, "Platform");
        assert_eq!(group.parent_path.as_deref(), Some("/srv/platform"));
        assert_eq!(group.connection_id.as_deref(), Some("conn-1"));
        assert_eq!(group.parent_group_id, None);
        assert_eq!(group.created_from, FolderScan);
        assert_eq!(group.tab_order, 3.0);
        assert!(!group.is_collapsed);
        assert_eq!(group.color, None);
        assert_eq!(group.created_at, 100.0);
        assert_eq!(group.updated_at, 100.0);
        // The factory never sets executionHostId.
        assert_eq!(group.execution_host_id, None);
    }

    #[test]
    fn trims_empty_group_names_to_a_fallback() {
        assert_eq!(normalize_project_group_name("   ", "Existing"), "Existing");
    }

    #[test]
    fn trims_js_whitespace_not_rust_whitespace() {
        // JS `.trim()` strips U+FEFF (BOM) but keeps U+0085 (NEL); mirror it.
        assert_eq!(normalize_project_group_name("\u{FEFF}Platform", "x"), "Platform");
        assert_eq!(normalize_project_group_name("Platform\u{0085}", "x"), "Platform\u{0085}");
        assert_eq!(normalize_project_group_name("\u{FEFF}", "Untitled group"), "Untitled group");
    }

    #[test]
    fn normalizes_persisted_groups_and_drops_malformed_entries() {
        let groups = normalize_project_groups(
            &json!([
                { "id": "b", "name": "B", "tabOrder": 2 },
                { "id": "a", "name": "A", "tabOrder": 1, "parentGroupId": "missing", "createdFrom": "folder-scan", "isCollapsed": true },
                { "id": "a", "name": "duplicate" },
                { "name": "missing id" }
            ]),
            0.0,
        );
        assert_eq!(groups.iter().map(|g| g.id.as_str()).collect::<Vec<_>>(), ["a", "b"]);
        assert_eq!(groups[0].created_from, FolderScan);
        assert!(groups[0].is_collapsed);
        assert_eq!(groups[0].parent_group_id, None);
    }

    #[test]
    fn preserves_connection_id_and_normalizes_execution_host_id() {
        let groups = normalize_project_groups(
            &json!([
                { "id": "remote", "name": "Remote", "connectionId": "conn-9", "executionHostId": "ssh:host%20a" },
                { "id": "bad-host", "name": "Bad", "connectionId": 42, "executionHostId": "ssh:" },
                { "id": "local", "name": "Local", "executionHostId": "local" }
            ]),
            0.0,
        );
        let remote = groups.iter().find(|g| g.id == "remote").unwrap();
        assert_eq!(remote.connection_id.as_deref(), Some("conn-9"));
        assert_eq!(remote.execution_host_id.as_deref(), Some("ssh:host%20a"));
        let bad = groups.iter().find(|g| g.id == "bad-host").unwrap();
        // Non-string connectionId → null; malformed ssh: host → dropped.
        assert_eq!(bad.connection_id, None);
        assert_eq!(bad.execution_host_id, None);
        assert_eq!(groups.iter().find(|g| g.id == "local").unwrap().execution_host_id.as_deref(), Some("local"));
    }

    #[test]
    fn orders_equal_tab_order_names_case_insensitively() {
        let groups = normalize_project_groups(
            &json!([
                { "id": "b", "name": "Banana", "tabOrder": 0 },
                { "id": "a", "name": "apple", "tabOrder": 0 }
            ]),
            0.0,
        );
        // localeCompare puts `apple` before `Banana`; raw scalar order would not.
        assert_eq!(groups.iter().map(|g| g.name.as_str()).collect::<Vec<_>>(), ["apple", "Banana"]);
    }

    #[test]
    fn computes_next_order_inside_a_group_independently_from_ungrouped_repos() {
        assert_eq!(
            get_next_project_group_order(&[repo("a", Some("g"), Some(2.0)), repo("b", None, Some(9.0))], Some("g")),
            3.0
        );
    }

    fn node(id: &str, parent: Option<&str>) -> ProjectGroupNode {
        ProjectGroupNode { id: id.to_string(), parent_group_id: parent.map(str::to_string) }
    }

    #[test]
    fn collects_descendant_group_ids_for_subtree_deletion() {
        let mut ids: Vec<String> = get_project_group_subtree_ids(
            &[node("root", None), node("child", Some("root")), node("grandchild", Some("child")), node("sibling", None)],
            "root",
        )
        .into_iter()
        .collect();
        ids.sort();
        assert_eq!(ids, ["child", "grandchild", "root"]);
    }

    #[test]
    fn collects_wide_descendant_groups_without_overflowing() {
        let mut groups = vec![node("root", None)];
        groups.extend((0..130_000).map(|index| node(&format!("child-{index}"), Some("root"))));
        let subtree = get_project_group_subtree_ids(&groups, "root");
        assert_eq!(subtree.len(), 130_001);
        assert!(subtree.contains("root"));
        assert!(subtree.contains("child-129999"));
    }
}
