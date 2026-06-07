//! Swimlane graph view-models, ported from `src/shared/git-history-graph.ts`.
//! Allocates a lane per commit, rotates through [`GIT_HISTORY_LANE_COLORS`] for
//! merge parents, keeps the current/remote/base ref colors stable, then defers
//! to `git_history_boundary_rows` for the incoming/outgoing boundary rows.

use std::cmp::Ordering;
use std::collections::HashMap;

use crate::git_history_boundary_rows::{
    add_incoming_outgoing_changes_history_items, GIT_HISTORY_INCOMING_CHANGES_ID,
    GIT_HISTORY_OUTGOING_CHANGES_ID,
};
use crate::git_history_types::{
    GitHistoryGraphColorId, GitHistoryItem, GitHistoryItemRef, GIT_HISTORY_BASE_REF_COLOR,
    GIT_HISTORY_LANE_COLORS, GIT_HISTORY_REF_COLOR, GIT_HISTORY_REMOTE_REF_COLOR,
};

/// `Map<string, GitHistoryGraphColorId | undefined>`: the inner `Option`
/// distinguishes "present but undefined" (key exists, value `None`) from
/// "absent" (no key) — the TS graph branches on `colorMap.has(id)`.
pub type GitHistoryColorMap = HashMap<String, Option<GitHistoryGraphColorId>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GitHistoryGraphKind {
    Head,
    Node,
    IncomingChanges,
    OutgoingChanges,
}

impl GitHistoryGraphKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Head => "HEAD",
            Self::Node => "node",
            Self::IncomingChanges => "incoming-changes",
            Self::OutgoingChanges => "outgoing-changes",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitHistoryGraphNode {
    pub id: String,
    pub color: GitHistoryGraphColorId,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitHistoryItemViewModel {
    pub history_item: GitHistoryItem,
    pub input_swimlanes: Vec<GitHistoryGraphNode>,
    pub output_swimlanes: Vec<GitHistoryGraphNode>,
    pub kind: GitHistoryGraphKind,
}

fn rotate(index: i64, length: i64) -> i64 {
    ((index % length) + length) % length
}

fn find_last_node_index(nodes: &[GitHistoryGraphNode], id: &str) -> Option<usize> {
    (0..nodes.len()).rev().find(|&index| nodes[index].id == id)
}

fn get_label_color_identifier(
    history_item: &GitHistoryItem,
    color_map: &GitHistoryColorMap,
) -> Option<GitHistoryGraphColorId> {
    if history_item.id == GIT_HISTORY_INCOMING_CHANGES_ID {
        return Some(GIT_HISTORY_REMOTE_REF_COLOR);
    }
    if history_item.id == GIT_HISTORY_OUTGOING_CHANGES_ID {
        return Some(GIT_HISTORY_REF_COLOR);
    }
    for ref_ in history_item.references.as_deref().unwrap_or(&[]) {
        if let Some(color) = color_map.get(&ref_.id).copied().flatten() {
            return Some(color);
        }
    }
    None
}

pub fn compare_git_history_refs(
    ref1: &GitHistoryItemRef,
    ref2: &GitHistoryItemRef,
    current_ref: Option<&GitHistoryItemRef>,
    remote_ref: Option<&GitHistoryItemRef>,
    base_ref: Option<&GitHistoryItemRef>,
) -> Ordering {
    let order = |ref_: &GitHistoryItemRef| -> i32 {
        if Some(&ref_.id) == current_ref.map(|r| &r.id) {
            return 1;
        }
        if Some(&ref_.id) == remote_ref.map(|r| &r.id) {
            return 2;
        }
        if Some(&ref_.id) == base_ref.map(|r| &r.id) {
            return 3;
        }
        if ref_.color.is_some() {
            return 4;
        }
        99
    };

    order(ref1).cmp(&order(ref2))
}

#[allow(clippy::too_many_arguments)]
pub fn build_git_history_view_models(
    history_items: &[GitHistoryItem],
    color_map: &GitHistoryColorMap,
    current_ref: Option<&GitHistoryItemRef>,
    remote_ref: Option<&GitHistoryItemRef>,
    base_ref: Option<&GitHistoryItemRef>,
    add_incoming_changes: bool,
    add_outgoing_changes: bool,
    merge_base: Option<&str>,
) -> Vec<GitHistoryItemViewModel> {
    let mut color_index: i64 = -1;
    let mut view_models: Vec<GitHistoryItemViewModel> = Vec::new();

    for history_item in history_items {
        let kind = if Some(history_item.id.as_str())
            == current_ref.and_then(|r| r.revision.as_deref())
        {
            GitHistoryGraphKind::Head
        } else {
            GitHistoryGraphKind::Node
        };

        let input_swimlanes: Vec<GitHistoryGraphNode> = view_models
            .last()
            .map(|view_model| view_model.output_swimlanes.clone())
            .unwrap_or_default();
        let mut output_swimlanes: Vec<GitHistoryGraphNode> = Vec::new();
        let mut first_parent_added = false;

        if !history_item.parent_ids.is_empty() {
            for node in &input_swimlanes {
                if node.id == history_item.id {
                    if !first_parent_added {
                        let color =
                            get_label_color_identifier(history_item, color_map).unwrap_or(node.color);
                        output_swimlanes.push(GitHistoryGraphNode {
                            id: history_item.parent_ids[0].clone(),
                            color,
                        });
                        first_parent_added = true;
                    }
                    continue;
                }
                output_swimlanes.push(node.clone());
            }
        }

        let mut index = if first_parent_added { 1 } else { 0 };
        while index < history_item.parent_ids.len() {
            let color_identifier = if index == 0 {
                get_label_color_identifier(history_item, color_map)
            } else {
                let parent_id = &history_item.parent_ids[index];
                history_items
                    .iter()
                    .find(|item| &item.id == parent_id)
                    .and_then(|parent| get_label_color_identifier(parent, color_map))
            };

            let color = match color_identifier {
                Some(color) => color,
                None => {
                    color_index = rotate(color_index + 1, GIT_HISTORY_LANE_COLORS.len() as i64);
                    GIT_HISTORY_LANE_COLORS[color_index as usize]
                }
            };

            output_swimlanes.push(GitHistoryGraphNode {
                id: history_item.parent_ids[index].clone(),
                color,
            });
            index += 1;
        }

        let mut references: Vec<GitHistoryItemRef> = history_item
            .references
            .clone()
            .unwrap_or_default()
            .into_iter()
            .map(|mut ref_| {
                let mut color = color_map.get(&ref_.id).copied().flatten();
                if color_map.contains_key(&ref_.id) && color.is_none() {
                    let input_index =
                        input_swimlanes.iter().position(|node| node.id == history_item.id);
                    let circle_index = input_index.unwrap_or(input_swimlanes.len());
                    color = if circle_index < output_swimlanes.len() {
                        Some(output_swimlanes[circle_index].color)
                    } else if circle_index < input_swimlanes.len() {
                        Some(input_swimlanes[circle_index].color)
                    } else {
                        Some(GIT_HISTORY_REF_COLOR)
                    };
                }
                ref_.color = color;
                ref_
            })
            .collect();
        references
            .sort_by(|ref1, ref2| compare_git_history_refs(ref1, ref2, current_ref, remote_ref, base_ref));

        let mut history_item = history_item.clone();
        history_item.references = Some(references);
        view_models.push(GitHistoryItemViewModel {
            history_item,
            kind,
            input_swimlanes,
            output_swimlanes,
        });
    }

    add_incoming_outgoing_changes_history_items(
        &mut view_models,
        current_ref,
        remote_ref,
        add_incoming_changes,
        add_outgoing_changes,
        merge_base,
    );

    view_models
}

#[cfg_attr(trust_verify, trust::ensures(|out: &i64| *out >= 0))]
pub fn get_git_history_item_lane_index(view_model: &GitHistoryItemViewModel) -> i64 {
    match view_model
        .input_swimlanes
        .iter()
        .position(|node| node.id == view_model.history_item.id)
    {
        Some(index) => index as i64,
        None => view_model.input_swimlanes.len() as i64,
    }
}

pub fn get_git_history_merge_parent_lane_index(
    view_model: &GitHistoryItemViewModel,
    parent_id: &str,
) -> i64 {
    find_last_node_index(&view_model.output_swimlanes, parent_id).map_or(-1, |index| index as i64)
}

pub fn build_default_git_history_color_map(
    current_ref: Option<&GitHistoryItemRef>,
    remote_ref: Option<&GitHistoryItemRef>,
    base_ref: Option<&GitHistoryItemRef>,
) -> GitHistoryColorMap {
    let mut color_map: GitHistoryColorMap = HashMap::new();
    if let Some(current) = current_ref {
        color_map.insert(current.id.clone(), Some(GIT_HISTORY_REF_COLOR));
    }
    if let Some(remote) = remote_ref {
        color_map.insert(remote.id.clone(), Some(GIT_HISTORY_REMOTE_REF_COLOR));
    }
    if let Some(base) = base_ref {
        color_map.insert(base.id.clone(), Some(GIT_HISTORY_BASE_REF_COLOR));
    }
    color_map
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git_history_types::GitHistoryRefCategory;

    fn item(id: &str, parent_ids: &[&str], references: Vec<GitHistoryItemRef>) -> GitHistoryItem {
        GitHistoryItem {
            id: id.to_string(),
            parent_ids: parent_ids.iter().map(|p| p.to_string()).collect(),
            subject: id.to_string(),
            message: id.to_string(),
            display_id: Some(id.to_string()),
            references: Some(references),
            ..Default::default()
        }
    }

    fn branch(name: &str, revision: &str) -> GitHistoryItemRef {
        GitHistoryItemRef {
            id: format!("refs/heads/{name}"),
            name: name.to_string(),
            revision: Some(revision.to_string()),
            category: Some(GitHistoryRefCategory::Branches),
            ..Default::default()
        }
    }

    fn remote(name: &str, revision: &str) -> GitHistoryItemRef {
        GitHistoryItemRef {
            id: format!("refs/remotes/{name}"),
            name: name.to_string(),
            revision: Some(revision.to_string()),
            category: Some(GitHistoryRefCategory::RemoteBranches),
            ..Default::default()
        }
    }

    fn node(id: &str, color: GitHistoryGraphColorId) -> GitHistoryGraphNode {
        GitHistoryGraphNode { id: id.to_string(), color }
    }

    fn kinds(view_models: &[GitHistoryItemViewModel]) -> Vec<GitHistoryGraphKind> {
        view_models.iter().map(|view_model| view_model.kind).collect()
    }

    #[test]
    fn preserves_the_current_branch_lane_through_linear_history() {
        let current_ref = branch("main", "A");
        let view_models = build_git_history_view_models(
            &[
                item("A", &["B"], vec![current_ref.clone()]),
                item("B", &["C"], vec![]),
                item("C", &[], vec![]),
            ],
            &build_default_git_history_color_map(Some(&current_ref), None, None),
            Some(&current_ref),
            None,
            None,
            false,
            false,
            None,
        );

        assert_eq!(
            kinds(&view_models),
            vec![
                GitHistoryGraphKind::Head,
                GitHistoryGraphKind::Node,
                GitHistoryGraphKind::Node
            ]
        );
        assert_eq!(view_models[0].input_swimlanes, Vec::<GitHistoryGraphNode>::new());
        assert_eq!(view_models[0].output_swimlanes, vec![node("B", GIT_HISTORY_REF_COLOR)]);
        assert_eq!(view_models[1].input_swimlanes, vec![node("B", GIT_HISTORY_REF_COLOR)]);
        assert_eq!(view_models[1].output_swimlanes, vec![node("C", GIT_HISTORY_REF_COLOR)]);
        assert_eq!(
            view_models[0].history_item.references.as_ref().unwrap()[0].color,
            Some(GIT_HISTORY_REF_COLOR)
        );
    }

    #[test]
    fn allocates_a_side_lane_for_a_merge_parent() {
        let current_ref = branch("feature", "M");
        let view_models = build_git_history_view_models(
            &[
                item("M", &["A", "B"], vec![current_ref.clone()]),
                item("A", &["C"], vec![]),
                item("B", &["C"], vec![]),
                item("C", &[], vec![]),
            ],
            &build_default_git_history_color_map(Some(&current_ref), None, None),
            Some(&current_ref),
            None,
            None,
            false,
            false,
            None,
        );

        assert_eq!(view_models[0].kind, GitHistoryGraphKind::Head);
        assert_eq!(
            view_models[0].output_swimlanes,
            vec![node("A", GIT_HISTORY_REF_COLOR), node("B", GIT_HISTORY_LANE_COLORS[0])]
        );
        assert_eq!(get_git_history_merge_parent_lane_index(&view_models[0], "B"), 1);
    }

    #[test]
    fn inserts_incoming_and_outgoing_boundary_rows_at_the_merge_base() {
        let current_ref = branch("feature", "A");
        let remote_ref = remote("origin/feature", "R");
        let view_models = build_git_history_view_models(
            &[
                item("A", &["B"], vec![current_ref.clone()]),
                item("R", &["B"], vec![remote_ref.clone()]),
                item("B", &["C"], vec![]),
                item("C", &[], vec![]),
            ],
            &build_default_git_history_color_map(Some(&current_ref), Some(&remote_ref), None),
            Some(&current_ref),
            Some(&remote_ref),
            None,
            true,
            true,
            Some("B"),
        );

        assert_eq!(
            kinds(&view_models),
            vec![
                GitHistoryGraphKind::OutgoingChanges,
                GitHistoryGraphKind::Head,
                GitHistoryGraphKind::Node,
                GitHistoryGraphKind::IncomingChanges,
                GitHistoryGraphKind::Node,
                GitHistoryGraphKind::Node
            ]
        );
        assert_eq!(view_models[0].history_item.id, GIT_HISTORY_OUTGOING_CHANGES_ID);
        assert_eq!(view_models[3].history_item.id, GIT_HISTORY_INCOMING_CHANGES_ID);
        assert!(view_models[3].input_swimlanes.contains(&node(
            GIT_HISTORY_INCOMING_CHANGES_ID,
            GIT_HISTORY_REMOTE_REF_COLOR
        )));
    }

    #[test]
    fn inserts_an_incoming_boundary_when_head_only_history_is_behind_upstream() {
        let current_ref = branch("feature", "B");
        let remote_ref = remote("origin/feature", "R");
        let view_models = build_git_history_view_models(
            &[item("B", &["C"], vec![current_ref.clone()]), item("C", &[], vec![])],
            &build_default_git_history_color_map(Some(&current_ref), Some(&remote_ref), None),
            Some(&current_ref),
            Some(&remote_ref),
            None,
            true,
            false,
            Some("B"),
        );

        assert_eq!(
            kinds(&view_models),
            vec![
                GitHistoryGraphKind::IncomingChanges,
                GitHistoryGraphKind::Head,
                GitHistoryGraphKind::Node
            ]
        );
        assert!(view_models[0].input_swimlanes.contains(&node(
            GIT_HISTORY_INCOMING_CHANGES_ID,
            GIT_HISTORY_REMOTE_REF_COLOR
        )));
        assert!(view_models[0].output_swimlanes.contains(&node("B", GIT_HISTORY_REMOTE_REF_COLOR)));
        assert!(view_models[1].input_swimlanes.contains(&node("B", GIT_HISTORY_REMOTE_REF_COLOR)));
        let incoming_lane_index = view_models[0]
            .input_swimlanes
            .iter()
            .position(|node| node.id == GIT_HISTORY_INCOMING_CHANGES_ID)
            .unwrap();
        assert_eq!(
            view_models[0].output_swimlanes.get(incoming_lane_index).map(|node| node.color),
            Some(GIT_HISTORY_REMOTE_REF_COLOR)
        );
    }

    #[test]
    fn colors_incoming_boundary_lanes_as_remote_when_upstream_commits_are_omitted() {
        let current_ref = branch("feature", "A");
        let remote_ref = remote("origin/feature", "R");
        let view_models = build_git_history_view_models(
            &[
                item("A", &["B"], vec![current_ref.clone()]),
                item("B", &["C"], vec![]),
                item("C", &[], vec![]),
            ],
            &build_default_git_history_color_map(Some(&current_ref), Some(&remote_ref), None),
            Some(&current_ref),
            Some(&remote_ref),
            None,
            true,
            true,
            Some("B"),
        );

        assert_eq!(
            kinds(&view_models),
            vec![
                GitHistoryGraphKind::OutgoingChanges,
                GitHistoryGraphKind::Head,
                GitHistoryGraphKind::IncomingChanges,
                GitHistoryGraphKind::Node,
                GitHistoryGraphKind::Node
            ]
        );
        assert!(view_models[2].input_swimlanes.contains(&node(
            GIT_HISTORY_INCOMING_CHANGES_ID,
            GIT_HISTORY_REMOTE_REF_COLOR
        )));
        assert!(view_models[2].output_swimlanes.contains(&node("B", GIT_HISTORY_REMOTE_REF_COLOR)));
        assert!(view_models[3].input_swimlanes.contains(&node("B", GIT_HISTORY_REMOTE_REF_COLOR)));
        let incoming_lane_index = view_models[2]
            .input_swimlanes
            .iter()
            .position(|node| node.id == GIT_HISTORY_INCOMING_CHANGES_ID)
            .unwrap();
        assert_eq!(
            view_models[2].output_swimlanes.get(incoming_lane_index).map(|node| node.color),
            Some(GIT_HISTORY_REMOTE_REF_COLOR)
        );
    }

    #[test]
    fn assigns_stable_colors_to_current_remote_and_base_refs() {
        let current_ref = branch("feature", "A");
        let remote_ref = remote("origin/feature", "R");
        let base_ref = remote("origin/main", "B");

        let color_map = build_default_git_history_color_map(
            Some(&current_ref),
            Some(&remote_ref),
            Some(&base_ref),
        );

        assert_eq!(color_map.get(&current_ref.id), Some(&Some(GIT_HISTORY_REF_COLOR)));
        assert_eq!(color_map.get(&remote_ref.id), Some(&Some(GIT_HISTORY_REMOTE_REF_COLOR)));
        assert_eq!(color_map.get(&base_ref.id), Some(&Some(GIT_HISTORY_BASE_REF_COLOR)));
    }
}
