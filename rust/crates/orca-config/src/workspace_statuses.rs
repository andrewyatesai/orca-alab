//! Workspace status definitions + normalization, ported from
//! `src/shared/workspace-statuses.ts` (with its `workspace-status-defaults` and
//! `workspace-status-default-migration` dependencies folded in).
//!
//! Normalizes persisted status columns (sanitize id/label/color/icon, dedupe,
//! cap), with one-shot migrations for the legacy default visuals and the
//! reversed-default-order regression. Over vendored `serde_json`; percent
//! en/decoding for group keys via `orca-core`.

use orca_core::uri_component::{decode_uri_component, encode_uri_component};
use serde_json::Value;
use std::collections::HashSet;

pub const DEFAULT_WORKSPACE_STATUS_ID: &str = "in-progress";
pub const DEFAULT_WORKSPACE_STATUS_COLOR_ID: &str = "neutral";
pub const DEFAULT_WORKSPACE_STATUS_ICON_ID: &str = "circle-dot";
pub const WORKSPACE_BOARD_COLUMN_WIDTH_DEFAULT: i64 = 308;
pub const WORKSPACE_BOARD_COLUMN_WIDTH_MIN: i64 = 220;
pub const WORKSPACE_BOARD_COLUMN_WIDTH_MAX: i64 = 520;
pub const WORKSPACE_BOARD_COLUMN_WIDTH_STEP: i64 = 20;

const MAX_STATUS_LABEL_LENGTH: usize = 32;
const MAX_WORKSPACE_STATUSES: usize = 12;
const WORKSPACE_STATUS_GROUP_PREFIX: &str = "workspace-status:";

pub const WORKSPACE_STATUS_COLOR_IDS: [&str; 11] = [
    "neutral", "blue", "sky", "violet", "amber", "emerald", "rose", "zinc", "conductor-done",
    "conductor-review", "conductor-progress",
];

pub const WORKSPACE_STATUS_ICON_IDS: [&str; 16] = [
    "circle", "circle-dot", "circle-progress", "circle-dashed", "circle-ellipsis", "git-pull-request",
    "timer", "flag", "circle-alert", "circle-pause", "circle-play", "circle-check", "ban", "conductor-done",
    "conductor-review", "conductor-progress",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceStatusDefinition {
    pub id: String,
    pub label: String,
    pub color: String,
    pub icon: String,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct WorkspaceStatusNormalizeOptions {
    pub migrate_default_workflow_statuses: bool,
    pub repair_reordered_default_statuses: bool,
    pub migrate_legacy_default_status_visuals: bool,
}

fn status(id: &str, label: &str, color: &str, icon: &str) -> WorkspaceStatusDefinition {
    WorkspaceStatusDefinition { id: id.to_string(), label: label.to_string(), color: color.to_string(), icon: icon.to_string() }
}

pub fn clone_default_workspace_statuses() -> Vec<WorkspaceStatusDefinition> {
    vec![
        status("completed", "Done", "conductor-done", "conductor-done"),
        status("in-review", "In review", "conductor-review", "conductor-review"),
        status("in-progress", "In progress", "conductor-progress", "conductor-progress"),
        status("todo", "Todo", "neutral", "circle"),
    ]
}

/// `DEFAULT_STATUS_VISUALS[id]` (the Conductor default visuals).
fn default_status_visual(id: &str) -> Option<(&'static str, &'static str)> {
    match id {
        "todo" => Some(("neutral", "circle")),
        "in-progress" => Some(("conductor-progress", "conductor-progress")),
        "in-review" => Some(("conductor-review", "conductor-review")),
        "completed" => Some(("conductor-done", "conductor-done")),
        _ => None,
    }
}

fn legacy_status_visual(id: &str) -> Option<(&'static str, &'static str)> {
    match id {
        "todo" => Some(("neutral", "circle")),
        "in-progress" => Some(("blue", "circle-dot")),
        "in-review" => Some(("violet", "git-pull-request")),
        "completed" => Some(("emerald", "circle-check")),
        _ => None,
    }
}

fn legacy_status_label(id: &str) -> Option<&'static str> {
    match id {
        "todo" => Some("Todo"),
        "in-progress" => Some("In progress"),
        "in-review" => Some("In review"),
        "completed" => Some("Completed"),
        _ => None,
    }
}

const LEGACY_TODO_FIRST_IDS: [&str; 4] = ["todo", "in-progress", "in-review", "completed"];
const WORKFLOW_IDS: [&str; 4] = ["completed", "in-review", "in-progress", "todo"];

fn is_legacy_default_status_payload(
    value: &Value,
    ordered_ids: &[&str],
    visual: fn(&str) -> Option<(&'static str, &'static str)>,
) -> bool {
    let Some(array) = value.as_array() else {
        return false;
    };
    if array.len() != ordered_ids.len() {
        return false;
    }
    array.iter().enumerate().all(|(index, raw)| {
        let Some(object) = raw.as_object() else {
            return false;
        };
        let expected_id = ordered_ids[index];
        let Some((color, icon)) = visual(expected_id) else {
            return false;
        };
        object.len() == 4
            && object.get("id").and_then(Value::as_str) == Some(expected_id)
            && object.get("label").and_then(Value::as_str) == legacy_status_label(expected_id)
            && object.get("color").and_then(Value::as_str) == Some(color)
            && object.get("icon").and_then(Value::as_str) == Some(icon)
    })
}

fn is_legacy_default_workflow_status_payload(value: &Value) -> bool {
    is_legacy_default_status_payload(value, &LEGACY_TODO_FIRST_IDS, default_status_visual)
        || is_legacy_default_status_payload(value, &LEGACY_TODO_FIRST_IDS, legacy_status_visual)
        || is_legacy_default_status_payload(value, &WORKFLOW_IDS, default_status_visual)
        || is_legacy_default_status_payload(value, &WORKFLOW_IDS, legacy_status_visual)
}

fn is_known_bad_pr_reordered_default_status_payload(value: &Value) -> bool {
    is_legacy_default_status_payload(value, &WORKFLOW_IDS, default_status_visual)
}

fn collapse_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn replace_runs(value: &str, keep: impl Fn(char) -> bool) -> String {
    let mut out = String::new();
    let mut in_run = false;
    for ch in value.chars() {
        if keep(ch) {
            out.push(ch);
            in_run = false;
        } else if !in_run {
            out.push('-');
            in_run = true;
        }
    }
    out
}

fn slug_status_label(label: &str) -> String {
    let slug = replace_runs(&label.trim().to_lowercase(), |c| c.is_ascii_alphanumeric());
    let trimmed = slug.trim_matches('-');
    if trimmed.is_empty() { "status".to_string() } else { trimmed.to_string() }
}

fn sanitize_status_label(value: Option<&str>, fallback: &str) -> String {
    let Some(value) = value else {
        return fallback.to_string();
    };
    let collapsed = collapse_whitespace(value);
    if collapsed.is_empty() {
        fallback.to_string()
    } else {
        collapsed.chars().take(MAX_STATUS_LABEL_LENGTH).collect()
    }
}

fn sanitize_status_id(value: Option<&str>, fallback_label: &str) -> String {
    let Some(value) = value else {
        return slug_status_label(fallback_label);
    };
    let trimmed = value.trim().to_lowercase();
    if trimmed.is_empty() {
        return slug_status_label(fallback_label);
    }
    let replaced = replace_runs(&trimmed, |c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    let stripped = replaced.trim_matches('-');
    if stripped.is_empty() { "status".to_string() } else { stripped.to_string() }
}

fn sanitize_status_color(value: Option<&str>, status_id: &str, label: &str, index: usize, migrate_legacy_visuals: bool) -> String {
    let migrate = migrate_legacy_visuals
        && ((status_id == "in-progress" && label == "In progress" && value == Some("blue"))
            || (status_id == "in-review" && label == "In review" && value == Some("violet"))
            || (status_id == "completed" && (label == "Completed" || label == "Done") && value == Some("emerald")));
    if migrate {
        if let Some((color, _)) = default_status_visual(status_id) {
            return color.to_string();
        }
    }
    if let Some(value) = value {
        if WORKSPACE_STATUS_COLOR_IDS.contains(&value) {
            return value.to_string();
        }
    }
    if let Some((color, _)) = default_status_visual(status_id) {
        return color.to_string();
    }
    WORKSPACE_STATUS_COLOR_IDS[index % WORKSPACE_STATUS_COLOR_IDS.len()].to_string()
}

fn sanitize_status_icon(value: Option<&str>, status_id: &str, label: &str, migrate_legacy_visuals: bool) -> String {
    let migrate = migrate_legacy_visuals
        && ((status_id == "in-progress" && label == "In progress" && (value == Some("circle-dot") || value == Some("circle-progress")))
            || (status_id == "in-review" && label == "In review" && value == Some("git-pull-request"))
            || (status_id == "completed" && (label == "Completed" || label == "Done") && value == Some("circle-check")));
    if migrate {
        if let Some((_, icon)) = default_status_visual(status_id) {
            return icon.to_string();
        }
    }
    if let Some(value) = value {
        if WORKSPACE_STATUS_ICON_IDS.contains(&value) {
            return value.to_string();
        }
    }
    default_status_visual(status_id).map_or_else(|| DEFAULT_WORKSPACE_STATUS_ICON_ID.to_string(), |(_, icon)| icon.to_string())
}

pub fn make_workspace_status_id(label: &str, existing_statuses: &[WorkspaceStatusDefinition]) -> String {
    let base = slug_status_label(label);
    let existing_ids: HashSet<&str> = existing_statuses.iter().map(|status| status.id.as_str()).collect();
    if !existing_ids.contains(base.as_str()) {
        return base;
    }
    for index in 2..100 {
        let candidate = format!("{base}-{index}");
        if !existing_ids.contains(candidate.as_str()) {
            return candidate;
        }
    }
    // Degenerate fallback (clock-free; the TS uses Date.now() here).
    format!("{base}-{}", existing_statuses.len())
}

fn normalize_internal(value: &Value, migrate_legacy_visuals: bool) -> Vec<WorkspaceStatusDefinition> {
    let Some(array) = value.as_array() else {
        return clone_default_workspace_statuses();
    };
    let mut statuses: Vec<WorkspaceStatusDefinition> = Vec::new();
    let mut used_ids: HashSet<String> = HashSet::new();
    for raw in array.iter().take(MAX_WORKSPACE_STATUSES) {
        let Some(object) = raw.as_object() else {
            continue;
        };
        let fallback_label = format!("Status {}", statuses.len() + 1);
        let label = sanitize_status_label(object.get("label").and_then(Value::as_str), &fallback_label);
        let mut id = sanitize_status_id(object.get("id").and_then(Value::as_str), &label);
        if used_ids.contains(&id) {
            id = make_workspace_status_id(&label, &statuses);
        }
        used_ids.insert(id.clone());
        let color = sanitize_status_color(object.get("color").and_then(Value::as_str), &id, &label, statuses.len(), migrate_legacy_visuals);
        let icon = sanitize_status_icon(object.get("icon").and_then(Value::as_str), &id, &label, migrate_legacy_visuals);
        statuses.push(WorkspaceStatusDefinition { id, label, color, icon });
    }
    if statuses.is_empty() {
        clone_default_workspace_statuses()
    } else {
        statuses
    }
}

pub fn normalize_workspace_statuses(value: &Value) -> Vec<WorkspaceStatusDefinition> {
    normalize_internal(value, false)
}

pub fn normalize_persisted_workspace_statuses(value: &Value, options: WorkspaceStatusNormalizeOptions) -> Vec<WorkspaceStatusDefinition> {
    if options.migrate_default_workflow_statuses && is_legacy_default_workflow_status_payload(value) {
        return clone_default_workspace_statuses();
    }
    if options.repair_reordered_default_statuses && is_known_bad_pr_reordered_default_status_payload(value) {
        return clone_default_workspace_statuses();
    }
    normalize_internal(value, options.migrate_legacy_default_status_visuals)
}

pub fn clamp_workspace_board_opacity(value: Option<f64>) -> f64 {
    match value {
        Some(value) if value.is_finite() => ((value * 100.0).round() / 100.0).clamp(0.2, 1.0),
        _ => 1.0,
    }
}

pub fn clamp_workspace_board_column_width(value: Option<f64>) -> i64 {
    match value {
        Some(value) if value.is_finite() => {
            value.round().clamp(WORKSPACE_BOARD_COLUMN_WIDTH_MIN as f64, WORKSPACE_BOARD_COLUMN_WIDTH_MAX as f64) as i64
        }
        _ => WORKSPACE_BOARD_COLUMN_WIDTH_DEFAULT,
    }
}

pub fn is_workspace_status_id(value: &str, statuses: &[WorkspaceStatusDefinition]) -> bool {
    statuses.iter().any(|status| status.id == value)
}

pub fn get_default_workspace_status_id(statuses: &[WorkspaceStatusDefinition]) -> String {
    if statuses.iter().any(|status| status.id == DEFAULT_WORKSPACE_STATUS_ID) {
        DEFAULT_WORKSPACE_STATUS_ID.to_string()
    } else {
        statuses.first().map_or_else(|| DEFAULT_WORKSPACE_STATUS_ID.to_string(), |status| status.id.clone())
    }
}

pub fn get_workspace_status(workspace_status: Option<&str>, statuses: &[WorkspaceStatusDefinition]) -> String {
    match workspace_status {
        Some(status) if is_workspace_status_id(status, statuses) => status.to_string(),
        _ => get_default_workspace_status_id(statuses),
    }
}

pub fn get_workspace_status_group_key(status: &str) -> String {
    format!("{WORKSPACE_STATUS_GROUP_PREFIX}{}", encode_uri_component(status))
}

pub fn get_workspace_status_from_group_key(group_key: &str, statuses: &[WorkspaceStatusDefinition]) -> Option<String> {
    let encoded = group_key.strip_prefix(WORKSPACE_STATUS_GROUP_PREFIX)?;
    let status = decode_uri_component(encoded);
    is_workspace_status_id(&status, statuses).then_some(status)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ids(statuses: &[WorkspaceStatusDefinition]) -> Vec<&str> {
        statuses.iter().map(|s| s.id.as_str()).collect()
    }

    #[test]
    fn keeps_the_default_workflow_order() {
        assert_eq!(ids(&clone_default_workspace_statuses()), ["completed", "in-review", "in-progress", "todo"]);
        let first = &clone_default_workspace_statuses()[0];
        assert_eq!((first.id.as_str(), first.label.as_str()), ("completed", "Done"));
    }

    #[test]
    fn migrates_legacy_default_statuses_to_the_default_workflow_order() {
        let statuses = normalize_persisted_workspace_statuses(
            &json!([
                { "id": "todo", "label": "Todo", "color": "neutral", "icon": "circle" },
                { "id": "in-progress", "label": "In progress", "color": "conductor-progress", "icon": "conductor-progress" },
                { "id": "in-review", "label": "In review", "color": "conductor-review", "icon": "conductor-review" },
                { "id": "completed", "label": "Completed", "color": "conductor-done", "icon": "conductor-done" }
            ]),
            WorkspaceStatusNormalizeOptions { migrate_default_workflow_statuses: true, ..Default::default() },
        );
        assert_eq!(statuses, clone_default_workspace_statuses());
    }

    #[test]
    fn migrates_the_old_default_status_visuals_without_reordering() {
        let statuses = normalize_persisted_workspace_statuses(
            &json!([
                { "id": "todo", "label": "Todo", "color": "neutral", "icon": "circle" },
                { "id": "in-progress", "label": "In progress", "color": "blue", "icon": "circle-dot" },
                { "id": "in-review", "label": "In review", "color": "violet", "icon": "git-pull-request" },
                { "id": "completed", "label": "Completed", "color": "emerald", "icon": "circle-check" }
            ]),
            WorkspaceStatusNormalizeOptions { migrate_legacy_default_status_visuals: true, ..Default::default() },
        );
        assert_eq!(ids(&statuses), ["todo", "in-progress", "in-review", "completed"]);
        assert_eq!(
            statuses.iter().map(|s| s.color.as_str()).collect::<Vec<_>>(),
            ["neutral", "conductor-progress", "conductor-review", "conductor-done"]
        );
    }

    #[test]
    fn preserves_explicit_status_order_while_migrating_default_visuals() {
        let statuses = normalize_persisted_workspace_statuses(
            &json!([
                { "id": "completed", "label": "Completed", "color": "emerald", "icon": "circle-check" },
                { "id": "in-review", "label": "In review", "color": "violet", "icon": "git-pull-request" },
                { "id": "in-progress", "label": "In progress", "color": "blue", "icon": "circle-dot" },
                { "id": "todo", "label": "Todo", "color": "neutral", "icon": "circle" }
            ]),
            WorkspaceStatusNormalizeOptions { migrate_legacy_default_status_visuals: true, ..Default::default() },
        );
        assert_eq!(ids(&statuses), ["completed", "in-review", "in-progress", "todo"]);
        assert_eq!((statuses[0].color.as_str(), statuses[0].icon.as_str()), ("conductor-done", "conductor-done"));
    }

    #[test]
    fn preserves_default_label_reordered_statuses_unless_migration_requested() {
        let statuses = normalize_persisted_workspace_statuses(
            &json!([
                { "id": "completed", "label": "Completed", "color": "conductor-done", "icon": "conductor-done" },
                { "id": "in-review", "label": "In review", "color": "conductor-review", "icon": "conductor-review" },
                { "id": "in-progress", "label": "In progress", "color": "conductor-progress", "icon": "conductor-progress" },
                { "id": "todo", "label": "Todo", "color": "neutral", "icon": "circle" }
            ]),
            WorkspaceStatusNormalizeOptions::default(),
        );
        assert_eq!(ids(&statuses), ["completed", "in-review", "in-progress", "todo"]);
    }

    #[test]
    fn migrates_exact_reordered_default_statuses_when_requested() {
        let statuses = normalize_persisted_workspace_statuses(
            &json!([
                { "id": "completed", "label": "Completed", "color": "conductor-done", "icon": "conductor-done" },
                { "id": "in-review", "label": "In review", "color": "conductor-review", "icon": "conductor-review" },
                { "id": "in-progress", "label": "In progress", "color": "conductor-progress", "icon": "conductor-progress" },
                { "id": "todo", "label": "Todo", "color": "neutral", "icon": "circle" }
            ]),
            WorkspaceStatusNormalizeOptions { migrate_default_workflow_statuses: true, ..Default::default() },
        );
        assert_eq!(statuses, clone_default_workspace_statuses());
    }

    #[test]
    fn repairs_the_exact_pr_introduced_default_reorder_when_gated() {
        let statuses = normalize_persisted_workspace_statuses(
            &json!([
                { "id": "completed", "label": "Completed", "color": "conductor-done", "icon": "conductor-done" },
                { "id": "in-review", "label": "In review", "color": "conductor-review", "icon": "conductor-review" },
                { "id": "in-progress", "label": "In progress", "color": "conductor-progress", "icon": "conductor-progress" },
                { "id": "todo", "label": "Todo", "color": "neutral", "icon": "circle" }
            ]),
            WorkspaceStatusNormalizeOptions { repair_reordered_default_statuses: true, ..Default::default() },
        );
        assert_eq!(statuses, clone_default_workspace_statuses());
    }

    #[test]
    fn does_not_repair_reordered_statuses_with_a_different_raw_shape() {
        let statuses = normalize_persisted_workspace_statuses(
            &json!([
                { "id": "completed", "label": "Completed", "color": "emerald", "icon": "circle-check" },
                { "id": "in-review", "label": "In review", "color": "violet", "icon": "git-pull-request" },
                { "id": "in-progress", "label": "In progress", "color": "blue", "icon": "circle-dot" },
                { "id": "todo", "label": "Todo", "color": "neutral", "icon": "circle" }
            ]),
            WorkspaceStatusNormalizeOptions { repair_reordered_default_statuses: true, ..Default::default() },
        );
        assert_eq!(ids(&statuses), ["completed", "in-review", "in-progress", "todo"]);
    }

    #[test]
    fn leaves_custom_persisted_status_layouts_in_their_saved_order() {
        let statuses = normalize_persisted_workspace_statuses(
            &json!([
                { "id": "completed", "label": "Shipped", "color": "conductor-done", "icon": "conductor-done" },
                { "id": "todo", "label": "Todo", "color": "neutral", "icon": "circle" }
            ]),
            WorkspaceStatusNormalizeOptions::default(),
        );
        assert_eq!(ids(&statuses), ["completed", "todo"]);
    }

    #[test]
    fn uses_conductor_style_visuals_for_the_default_status_icons() {
        let statuses = clone_default_workspace_statuses();
        let find = |id: &str| statuses.iter().find(|s| s.id == id).unwrap();
        assert_eq!((find("in-progress").color.as_str(), find("in-progress").icon.as_str()), ("conductor-progress", "conductor-progress"));
        assert_eq!((find("in-review").color.as_str(), find("in-review").icon.as_str()), ("conductor-review", "conductor-review"));
        assert_eq!((find("completed").color.as_str(), find("completed").icon.as_str()), ("conductor-done", "conductor-done"));
    }

    #[test]
    fn migrates_the_old_in_progress_blue_dot_default_only_when_requested() {
        let statuses = normalize_persisted_workspace_statuses(
            &json!([{ "id": "in-progress", "label": "In progress", "color": "blue", "icon": "circle-dot" }]),
            WorkspaceStatusNormalizeOptions { migrate_legacy_default_status_visuals: true, ..Default::default() },
        );
        assert_eq!((statuses[0].color.as_str(), statuses[0].icon.as_str()), ("conductor-progress", "conductor-progress"));
    }

    #[test]
    fn preserves_valid_legacy_visuals_for_default_label_statuses_at_runtime() {
        let statuses = normalize_workspace_statuses(&json!([{ "id": "in-progress", "label": "In progress", "color": "blue", "icon": "circle-dot" }]));
        assert_eq!((statuses[0].color.as_str(), statuses[0].icon.as_str()), ("blue", "circle-dot"));
    }

    #[test]
    fn keeps_intentional_custom_in_progress_visuals() {
        let statuses = normalize_workspace_statuses(&json!([{ "id": "in-progress", "label": "Doing", "color": "blue", "icon": "circle-dot" }]));
        assert_eq!((statuses[0].color.as_str(), statuses[0].icon.as_str()), ("blue", "circle-dot"));
    }

    #[test]
    fn clamps_workspace_board_column_widths_to_resizable_bounds() {
        assert_eq!(clamp_workspace_board_column_width(None), WORKSPACE_BOARD_COLUMN_WIDTH_DEFAULT);
        assert_eq!(clamp_workspace_board_column_width(Some(100.0)), WORKSPACE_BOARD_COLUMN_WIDTH_MIN);
        assert_eq!(clamp_workspace_board_column_width(Some(321.6)), 322);
        assert_eq!(clamp_workspace_board_column_width(Some(900.0)), WORKSPACE_BOARD_COLUMN_WIDTH_MAX);
    }
}
