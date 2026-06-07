//! Project-group organization, ported from `src/shared/project-groups.ts`.
//!
//! Creates/normalizes project groups (the repo-organization tree), clears dead
//! memberships, and computes subtree ids + next ordering. The group id and
//! timestamps are injected (the IO edge owns the RNG/clock); persisted-value
//! normalization reads `unknown` JSON via vendored `serde_json`.

use serde_json::Value;
use std::collections::{HashMap, HashSet};

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
    pub parent_group_id: Option<String>,
    pub created_from: ProjectGroupCreatedFrom,
    pub tab_order: f64,
    pub is_collapsed: bool,
    pub color: Option<String>,
    pub created_at: f64,
    pub updated_at: f64,
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
    let trimmed = name.trim();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
}

pub fn create_project_group(
    id: &str,
    name: &str,
    parent_path: Option<&str>,
    parent_group_id: Option<&str>,
    created_from: ProjectGroupCreatedFrom,
    tab_order: f64,
    now: f64,
) -> ProjectGroup {
    ProjectGroup {
        id: id.to_string(),
        name: normalize_project_group_name(name, "Untitled group"),
        parent_path: parent_path.map(str::to_string),
        parent_group_id: parent_group_id.map(str::to_string),
        created_from,
        tab_order,
        is_collapsed: false,
        color: None,
        created_at: now,
        updated_at: now,
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
        });
    }

    groups.sort_by(|left, right| {
        left.tab_order
            .partial_cmp(&right.tab_order)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.name.cmp(&right.name))
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

pub fn clear_missing_project_group_memberships(repos: &[Repo], groups: &[ProjectGroup]) -> Vec<Repo> {
    let group_ids: HashSet<&str> = groups.iter().map(|group| group.id.as_str()).collect();
    repos
        .iter()
        .map(|repo| match &repo.project_group_id {
            Some(group_id) if !group_ids.contains(group_id.as_str()) => Repo { project_group_id: None, ..repo.clone() },
            _ => repo.clone(),
        })
        .collect()
}

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
    use ProjectGroupCreatedFrom::{FolderScan, Manual};

    fn repo(id: &str, project_group_id: Option<&str>, project_group_order: Option<f64>) -> Repo {
        Repo { id: id.to_string(), project_group_id: project_group_id.map(str::to_string), project_group_order }
    }

    #[test]
    fn creates_a_durable_project_group_with_normalized_defaults() {
        let group = create_project_group("g1", "  Platform  ", Some("/srv/platform"), None, FolderScan, 3.0, 100.0);
        assert_eq!(group.name, "Platform");
        assert_eq!(group.parent_path.as_deref(), Some("/srv/platform"));
        assert_eq!(group.parent_group_id, None);
        assert_eq!(group.created_from, FolderScan);
        assert_eq!(group.tab_order, 3.0);
        assert!(!group.is_collapsed);
        assert_eq!(group.color, None);
        assert_eq!(group.created_at, 100.0);
        assert_eq!(group.updated_at, 100.0);
    }

    #[test]
    fn trims_empty_group_names_to_a_fallback() {
        assert_eq!(normalize_project_group_name("   ", "Existing"), "Existing");
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
    fn clears_repo_memberships_whose_group_no_longer_exists() {
        let groups = vec![create_project_group("known-group", "Known", None, None, Manual, 0.0, 0.0)];
        let repos = clear_missing_project_group_memberships(
            &[repo("known", Some("known-group"), None), repo("missing", Some("x"), None)],
            &groups,
        );
        assert_eq!(repos.iter().find(|r| r.id == "known").unwrap().project_group_id.as_deref(), Some("known-group"));
        assert_eq!(repos.iter().find(|r| r.id == "missing").unwrap().project_group_id, None);
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
