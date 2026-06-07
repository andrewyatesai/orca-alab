//! Parity dispatch for `orca_core::worktree_ownership` vs
//! `src/shared/worktree-ownership.ts`.
//!
//! Inputs carry the lean field projections the logic reads (camelCase, matching
//! the TS vectors). Outputs are shaped by hand to match `JSON.stringify` of the
//! TS return: enums become their TS string ids (`show`/`hide`,
//! `orca-managed`/`unknown-legacy`/`external`), `DetectedWorktree` keeps only the
//! fields the TS spread carries from the lean `{ path, isMainWorktree }` worktree.

use orca_core::worktree_ownership::{
    are_runtime_paths_equal, build_known_orca_workspace_layouts, classify_worktree_ownership,
    effective_external_worktree_visibility, is_legacy_repo_for_external_worktree_visibility,
    matches_strong_orca_create_path, should_show_worktree, to_detected_worktree,
    DetectedWorktree, ExternalWorktreeVisibility, OrcaWorkspaceLayout, Repo, WorkspaceLayoutSettings,
    Worktree, WorktreeMeta, WorktreeOwnership,
};
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "isLegacyRepoForExternalWorktreeVisibility" => {
            Value::Bool(is_legacy_repo_for_external_worktree_visibility(&parse_repo(input)))
        }
        "effectiveExternalWorktreeVisibility" => {
            let repo = parse_repo(field(input, "repo"));
            let is_legacy = bool_field(input, "isLegacyRepoForVisibility");
            visibility_to_value(effective_external_worktree_visibility(
                repo.external_worktree_visibility,
                is_legacy,
            ))
        }
        "buildKnownOrcaWorkspaceLayouts" => {
            let settings = parse_settings(field(input, "settings"));
            let repo = input.get("repo").filter(|value| value.is_object()).map(parse_repo);
            let layouts = build_known_orca_workspace_layouts(&settings, repo.as_ref());
            Value::Array(layouts.iter().map(layout_to_value).collect())
        }
        "classifyWorktreeOwnership" => {
            let repo = parse_repo(field(input, "repo"));
            let worktree = parse_worktree(field(input, "worktree"));
            let meta = parse_meta(input.get("meta"));
            let layouts = parse_layouts(input.get("knownOrcaLayouts"));
            ownership_to_value(classify_worktree_ownership(&repo, &worktree, meta.as_ref(), &layouts))
        }
        "toDetectedWorktree" => {
            let repo = parse_repo(field(input, "repo"));
            let worktree = parse_worktree(field(input, "worktree"));
            let meta = parse_meta(input.get("meta"));
            let layouts = parse_layouts(input.get("knownOrcaLayouts"));
            let is_legacy = input.get("isLegacyRepoForVisibility").and_then(Value::as_bool);
            detected_to_value(&to_detected_worktree(&repo, &worktree, meta.as_ref(), &layouts, is_legacy))
        }
        "shouldShowWorktree" => {
            let repo = parse_repo(field(input, "repo"));
            let ownership = parse_ownership(input.get("ownership"));
            let is_legacy = bool_field(input, "isLegacyRepoForVisibility");
            let is_selected = bool_field(input, "isSelectedCheckout");
            Value::Bool(should_show_worktree(ownership, &repo, is_legacy, is_selected))
        }
        "areRuntimePathsEqual" => {
            Value::Bool(are_runtime_paths_equal(&str_field(input, "leftPath"), &str_field(input, "rightPath")))
        }
        "matchesStrongOrcaCreatePath" => {
            let worktree_path = str_field(input, "worktreePath");
            let layouts = parse_layouts(input.get("knownOrcaLayouts"));
            let repo_path = str_field(field(input, "repo"), "path");
            Value::Bool(matches_strong_orca_create_path(&worktree_path, &layouts, &repo_path))
        }
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

/// Borrow a nested object field, falling back to `null` so parsers see absent keys.
fn field<'a>(value: &'a Value, key: &str) -> &'a Value {
    value.get(key).unwrap_or(&Value::Null)
}

fn str_field(value: &Value, key: &str) -> String {
    value.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

fn opt_str_field(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_string)
}

fn bool_field(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn parse_repo(value: &Value) -> Repo {
    Repo {
        path: str_field(value, "path"),
        // `null` (non-finite addedAt persisted by JSON.stringify) parses to None,
        // which the port treats as legacy — matching the TS Number.isFinite guard.
        external_worktree_visibility: parse_visibility(value.get("externalWorktreeVisibility")),
        external_worktree_visibility_legacy: value
            .get("externalWorktreeVisibilityLegacy")
            .and_then(Value::as_bool),
        added_at: value.get("addedAt").and_then(Value::as_f64),
        connection_id: opt_str_field(value, "connectionId"),
        worktree_base_path: opt_str_field(value, "worktreeBasePath"),
    }
}

fn parse_worktree(value: &Value) -> Worktree {
    Worktree {
        path: str_field(value, "path"),
        is_main_worktree: bool_field(value, "isMainWorktree"),
    }
}

fn parse_meta(value: Option<&Value>) -> Option<WorktreeMeta> {
    let value = value.filter(|value| value.is_object())?;
    Some(WorktreeMeta {
        orca_created_at: value.get("orcaCreatedAt").and_then(Value::as_f64),
        created_at: value.get("createdAt").and_then(Value::as_f64),
        created_with_agent: bool_field(value, "createdWithAgent"),
        push_target: bool_field(value, "pushTarget"),
        sparse_base_ref: opt_str_field(value, "sparseBaseRef"),
        sparse_preset_id: opt_str_field(value, "sparsePresetId"),
        preserve_branch_on_delete: bool_field(value, "preserveBranchOnDelete"),
    })
}

fn parse_settings(value: &Value) -> WorkspaceLayoutSettings {
    WorkspaceLayoutSettings {
        workspace_dir: opt_str_field(value, "workspaceDir"),
        nest_workspaces: bool_field(value, "nestWorkspaces"),
        workspace_dir_history: parse_layouts(value.get("workspaceDirHistory")),
    }
}

fn parse_layouts(value: Option<&Value>) -> Vec<OrcaWorkspaceLayout> {
    value
        .and_then(Value::as_array)
        .map(|items| items.iter().map(parse_layout).collect())
        .unwrap_or_default()
}

fn parse_layout(value: &Value) -> OrcaWorkspaceLayout {
    OrcaWorkspaceLayout {
        path: str_field(value, "path"),
        nest_workspaces: bool_field(value, "nestWorkspaces"),
    }
}

fn parse_visibility(value: Option<&Value>) -> Option<ExternalWorktreeVisibility> {
    match value.and_then(Value::as_str) {
        Some("show") => Some(ExternalWorktreeVisibility::Show),
        Some("hide") => Some(ExternalWorktreeVisibility::Hide),
        _ => None,
    }
}

fn parse_ownership(value: Option<&Value>) -> WorktreeOwnership {
    match value.and_then(Value::as_str) {
        Some("orca-managed") => WorktreeOwnership::OrcaManaged,
        Some("external") => WorktreeOwnership::External,
        _ => WorktreeOwnership::UnknownLegacy,
    }
}

fn visibility_to_value(visibility: ExternalWorktreeVisibility) -> Value {
    Value::String(
        match visibility {
            ExternalWorktreeVisibility::Show => "show",
            ExternalWorktreeVisibility::Hide => "hide",
        }
        .to_string(),
    )
}

fn ownership_to_value(ownership: WorktreeOwnership) -> Value {
    Value::String(
        match ownership {
            WorktreeOwnership::OrcaManaged => "orca-managed",
            WorktreeOwnership::UnknownLegacy => "unknown-legacy",
            WorktreeOwnership::External => "external",
        }
        .to_string(),
    )
}

fn layout_to_value(layout: &OrcaWorkspaceLayout) -> Value {
    json!({ "path": layout.path, "nestWorkspaces": layout.nest_workspaces })
}

/// Match `JSON.stringify` of `{ ...worktree, ownership, selectedCheckout, visible }`
/// where the vector worktree only carries `{ path, isMainWorktree }`.
fn detected_to_value(detected: &DetectedWorktree) -> Value {
    json!({
        "path": detected.path,
        "isMainWorktree": detected.is_main_worktree,
        "ownership": ownership_to_value(detected.ownership),
        "selectedCheckout": detected.selected_checkout,
        "visible": detected.visible,
    })
}
