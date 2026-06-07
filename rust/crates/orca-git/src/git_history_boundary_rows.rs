//! Synthetic "Incoming Changes" / "Outgoing Changes" boundary rows for the
//! swimlane graph, ported from `src/shared/git-history-boundary-rows.ts`.
//!
//! HEAD-only history omits upstream commits, so the graph must still synthesize
//! the remote lane that those hidden rows used to carry. This module splices the
//! boundary view-models (and their remote lanes) into the already-built models.

use crate::git_history_graph::{GitHistoryGraphKind, GitHistoryGraphNode, GitHistoryItemViewModel};
use crate::git_history_types::{
    GitHistoryItem, GitHistoryItemRef, GIT_HISTORY_REF_COLOR, GIT_HISTORY_REMOTE_REF_COLOR,
};

pub const GIT_HISTORY_INCOMING_CHANGES_ID: &str = "git-history-incoming-changes";
pub const GIT_HISTORY_OUTGOING_CHANGES_ID: &str = "git-history-outgoing-changes";

fn utf16_len(value: &str) -> usize {
    value.encode_utf16().count()
}

fn find_last_index<T>(items: &[T], mut predicate: impl FnMut(&T) -> bool) -> Option<usize> {
    (0..items.len()).rev().find(|&index| predicate(&items[index]))
}

fn has_node(
    nodes: &[GitHistoryGraphNode],
    id: &str,
    color: Option<crate::git_history_types::GitHistoryGraphColorId>,
) -> bool {
    nodes
        .iter()
        .any(|node| node.id == id && color.is_none_or(|c| node.color == c))
}

fn remote_boundary_input_node(node: &GitHistoryGraphNode, merge_base: &str) -> GitHistoryGraphNode {
    if node.id == merge_base && node.color == GIT_HISTORY_REMOTE_REF_COLOR {
        GitHistoryGraphNode { id: GIT_HISTORY_INCOMING_CHANGES_ID.to_string(), color: node.color }
    } else {
        node.clone()
    }
}

fn ensure_incoming_remote_lane(
    input_swimlanes: &mut Vec<GitHistoryGraphNode>,
    output_swimlanes: &mut Vec<GitHistoryGraphNode>,
    merge_base: &str,
) {
    if !has_node(output_swimlanes.as_slice(), merge_base, Some(GIT_HISTORY_REMOTE_REF_COLOR)) {
        let local_merge_base_index = output_swimlanes
            .iter()
            .position(|node| node.id == merge_base && node.color == GIT_HISTORY_REF_COLOR);
        let remote_merge_base_index = match local_merge_base_index {
            None => input_swimlanes.len(),
            Some(index) => index + 1,
        };
        let insert_at = remote_merge_base_index.min(output_swimlanes.len());
        output_swimlanes.insert(
            insert_at,
            GitHistoryGraphNode {
                id: merge_base.to_string(),
                color: GIT_HISTORY_REMOTE_REF_COLOR,
            },
        );
    }

    if has_node(
        input_swimlanes.as_slice(),
        GIT_HISTORY_INCOMING_CHANGES_ID,
        Some(GIT_HISTORY_REMOTE_REF_COLOR),
    ) {
        return;
    }

    let remote_merge_base_index = output_swimlanes
        .iter()
        .position(|node| node.id == merge_base && node.color == GIT_HISTORY_REMOTE_REF_COLOR);
    let insert_at = match remote_merge_base_index {
        None => input_swimlanes.len(),
        Some(index) => index,
    };
    let insert_at = insert_at.min(input_swimlanes.len());
    input_swimlanes.insert(
        insert_at,
        GitHistoryGraphNode {
            id: GIT_HISTORY_INCOMING_CHANGES_ID.to_string(),
            color: GIT_HISTORY_REMOTE_REF_COLOR,
        },
    );
}

pub fn add_incoming_outgoing_changes_history_items(
    view_models: &mut Vec<GitHistoryItemViewModel>,
    current_ref: Option<&GitHistoryItemRef>,
    remote_ref: Option<&GitHistoryItemRef>,
    add_incoming_changes: bool,
    add_outgoing_changes: bool,
    merge_base: Option<&str>,
) {
    let current_revision = current_ref.and_then(|r| r.revision.as_deref());
    let remote_revision = remote_ref.and_then(|r| r.revision.as_deref());

    // Why: `!mergeBase` in TS is true for both `undefined` and `''`.
    let merge_base = match merge_base {
        Some(value) if !value.is_empty() => value,
        _ => return,
    };
    if current_revision == remote_revision {
        return;
    }

    if add_incoming_changes {
        if let Some(remote) = remote_ref {
            if remote.revision.as_deref() != Some(merge_base) {
                add_incoming_changes_history_item(view_models, remote, merge_base);
            }
        }
    }

    if add_outgoing_changes {
        if let Some(current) = current_ref {
            if let Some(revision) = current.revision.as_deref() {
                if !revision.is_empty() && revision != merge_base {
                    add_outgoing_changes_history_item(view_models, current);
                }
            }
        }
    }
}

fn add_incoming_changes_history_item(
    view_models: &mut Vec<GitHistoryItemViewModel>,
    remote_ref: &GitHistoryItemRef,
    merge_base: &str,
) {
    let before_history_item_index = find_last_index(view_models.as_slice(), |view_model| {
        view_model.output_swimlanes.iter().any(|node| node.id == merge_base)
    });
    let Some(after_history_item_index) = view_models
        .iter()
        .position(|view_model| view_model.history_item.id == merge_base)
    else {
        return;
    };

    if let Some(before_index) = before_history_item_index {
        let before = &view_models[before_index];
        let incoming_change_merged = before.history_item.parent_ids.len() == 2
            && before.history_item.parent_ids.iter().any(|id| id == merge_base);
        if incoming_change_merged {
            return;
        }
    }

    let mut input_swimlanes: Vec<GitHistoryGraphNode> = match before_history_item_index {
        Some(before_index) => view_models[before_index]
            .output_swimlanes
            .iter()
            .map(|node| remote_boundary_input_node(node, merge_base))
            .collect(),
        None => view_models[after_history_item_index].input_swimlanes.clone(),
    };
    let mut output_swimlanes: Vec<GitHistoryGraphNode> =
        view_models[after_history_item_index].input_swimlanes.clone();
    ensure_incoming_remote_lane(&mut input_swimlanes, &mut output_swimlanes, merge_base);

    if let Some(before_index) = before_history_item_index {
        let new_input: Vec<GitHistoryGraphNode> = view_models[before_index]
            .input_swimlanes
            .iter()
            .map(|node| remote_boundary_input_node(node, merge_base))
            .collect();
        view_models[before_index].input_swimlanes = new_input;
        view_models[before_index].output_swimlanes = input_swimlanes.clone();
    }

    let display_id_length = view_models
        .first()
        .and_then(|view_model| view_model.history_item.display_id.as_deref())
        .map_or(0, utf16_len);
    let incoming_changes_history_item = GitHistoryItem {
        id: GIT_HISTORY_INCOMING_CHANGES_ID.to_string(),
        display_id: Some("0".repeat(display_id_length)),
        parent_ids: vec![merge_base.to_string()],
        author: Some(remote_ref.name.clone()),
        subject: "Incoming Changes".to_string(),
        message: String::new(),
        ..Default::default()
    };

    view_models.insert(
        after_history_item_index,
        GitHistoryItemViewModel {
            history_item: incoming_changes_history_item,
            kind: GitHistoryGraphKind::IncomingChanges,
            input_swimlanes,
            output_swimlanes: output_swimlanes.clone(),
        },
    );

    view_models[after_history_item_index + 1].input_swimlanes = output_swimlanes;
}

fn add_outgoing_changes_history_item(
    view_models: &mut Vec<GitHistoryItemViewModel>,
    current_ref: &GitHistoryItemRef,
) {
    let Some(current_revision) = current_ref.revision.as_deref() else {
        return;
    };
    if current_revision.is_empty() {
        return;
    }

    let Some(current_ref_index) = view_models.iter().position(|view_model| {
        view_model.kind == GitHistoryGraphKind::Head
            && view_model.history_item.id == current_revision
    }) else {
        return;
    };

    let display_id_length = view_models
        .first()
        .and_then(|view_model| view_model.history_item.display_id.as_deref())
        .map_or(0, utf16_len);
    let outgoing_changes_history_item = GitHistoryItem {
        id: GIT_HISTORY_OUTGOING_CHANGES_ID.to_string(),
        display_id: Some("0".repeat(display_id_length)),
        parent_ids: vec![current_revision.to_string()],
        author: Some(current_ref.name.clone()),
        subject: "Outgoing Changes".to_string(),
        message: String::new(),
        ..Default::default()
    };

    let input_swimlanes: Vec<GitHistoryGraphNode> =
        view_models[current_ref_index].input_swimlanes.clone();
    let mut output_swimlanes = input_swimlanes.clone();
    output_swimlanes.push(GitHistoryGraphNode {
        id: current_revision.to_string(),
        color: GIT_HISTORY_REF_COLOR,
    });

    view_models.insert(
        current_ref_index,
        GitHistoryItemViewModel {
            history_item: outgoing_changes_history_item,
            kind: GitHistoryGraphKind::OutgoingChanges,
            input_swimlanes,
            output_swimlanes,
        },
    );

    view_models[current_ref_index + 1].input_swimlanes.push(GitHistoryGraphNode {
        id: current_revision.to_string(),
        color: GIT_HISTORY_REF_COLOR,
    });
}
