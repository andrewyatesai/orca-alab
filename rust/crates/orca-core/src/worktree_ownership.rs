//! Worktree ownership classification + external-visibility policy, ported from
//! `src/shared/worktree-ownership.ts`.
//!
//! Decides whether a discovered git worktree is Orca-managed, an unknown legacy
//! row, or external — and whether it should be shown — by matching its path
//! against the known Orca workspace layouts. Composes `cross_platform_path` and
//! `wsl_paths`. Input structs are the lean projections the logic reads.

use crate::cross_platform_path::{
    get_runtime_path_basename, is_runtime_path_absolute, is_windows_absolute_path_like,
    normalize_runtime_path_for_comparison, normalize_runtime_path_separators,
    relative_path_inside_root, resolve_runtime_path, PathFlavor,
};
use crate::wsl_paths::parse_wsl_unc_path;
use std::collections::HashSet;

/// `Date.UTC(2026, 4, 23)` — 2026-05-23 UTC, in epoch milliseconds.
pub const EXTERNAL_WORKTREE_VISIBILITY_ROLLOUT_AT: i64 = 1_779_494_400_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExternalWorktreeVisibility {
    Show,
    Hide,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorktreeOwnership {
    OrcaManaged,
    UnknownLegacy,
    External,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrcaWorkspaceLayout {
    pub path: String,
    pub nest_workspaces: bool,
}

/// The repo fields the ownership/visibility logic reads.
#[derive(Clone, Debug, Default)]
pub struct Repo {
    pub path: String,
    pub external_worktree_visibility: Option<ExternalWorktreeVisibility>,
    pub external_worktree_visibility_legacy: Option<bool>,
    pub added_at: Option<f64>,
    pub connection_id: Option<String>,
    pub worktree_base_path: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Worktree {
    pub path: String,
    pub is_main_worktree: bool,
}

/// Strong-ownership signals: any present marker means Orca created the worktree.
#[derive(Clone, Debug, Default)]
pub struct WorktreeMeta {
    pub orca_created_at: Option<f64>,
    pub created_at: Option<f64>,
    pub created_with_agent: bool,
    pub push_target: bool,
    pub sparse_base_ref: Option<String>,
    pub sparse_preset_id: Option<String>,
    pub preserve_branch_on_delete: bool,
}

/// `Pick<GlobalSettings, 'workspaceDir' | 'nestWorkspaces' | 'workspaceDirHistory'>`.
#[derive(Clone, Debug, Default)]
pub struct WorkspaceLayoutSettings {
    pub workspace_dir: Option<String>,
    pub nest_workspaces: bool,
    pub workspace_dir_history: Vec<OrcaWorkspaceLayout>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DetectedWorktree {
    pub path: String,
    pub is_main_worktree: bool,
    pub ownership: WorktreeOwnership,
    pub selected_checkout: bool,
    pub visible: bool,
}

pub fn is_legacy_repo_for_external_worktree_visibility(repo: &Repo) -> bool {
    if let Some(legacy) = repo.external_worktree_visibility_legacy {
        return legacy;
    }
    if repo.external_worktree_visibility.is_none() {
        return true;
    }
    match repo.added_at {
        Some(added) if added.is_finite() => added < EXTERNAL_WORKTREE_VISIBILITY_ROLLOUT_AT as f64,
        _ => true,
    }
}

pub fn effective_external_worktree_visibility(
    visibility: Option<ExternalWorktreeVisibility>,
    is_legacy_repo_for_visibility: bool,
) -> ExternalWorktreeVisibility {
    visibility.unwrap_or(if is_legacy_repo_for_visibility {
        ExternalWorktreeVisibility::Show
    } else {
        ExternalWorktreeVisibility::Hide
    })
}

pub fn build_known_orca_workspace_layouts(
    settings: &WorkspaceLayoutSettings,
    repo: Option<&Repo>,
) -> Vec<OrcaWorkspaceLayout> {
    let mut layouts: Vec<OrcaWorkspaceLayout> = Vec::new();

    if let (Some(repo), Some(base)) = (repo, repo.and_then(get_repo_worktree_base_path)) {
        layouts.push(OrcaWorkspaceLayout {
            path: resolve_workspace_layout_path(&repo.path, &base),
            nest_workspaces: settings.nest_workspaces,
        });
    }

    if let Some(workspace_dir) = settings.workspace_dir.as_deref().filter(|dir| !dir.is_empty()) {
        if should_include_workspace_layout(repo, workspace_dir) {
            layouts.push(OrcaWorkspaceLayout {
                path: match repo {
                    Some(repo) => resolve_workspace_layout_path(&repo.path, workspace_dir),
                    None => workspace_dir.to_string(),
                },
                nest_workspaces: settings.nest_workspaces,
            });
            for layout in &settings.workspace_dir_history {
                if should_include_workspace_layout(repo, &layout.path) {
                    layouts.push(OrcaWorkspaceLayout {
                        path: match repo {
                            Some(repo) => resolve_workspace_layout_path(&repo.path, &layout.path),
                            None => layout.path.clone(),
                        },
                        nest_workspaces: layout.nest_workspaces,
                    });
                }
            }
        }
    }

    if let Some(repo) = repo {
        layouts.extend(build_wsl_workspace_layouts(&repo.path, settings));
    }

    let mut seen: HashSet<String> = HashSet::new();
    layouts
        .into_iter()
        .filter(|layout| {
            let key = format!("{}:{}", normalize_runtime_path_for_comparison(&layout.path), layout.nest_workspaces);
            seen.insert(key) && !layout.path.is_empty()
        })
        .collect()
}

fn get_repo_worktree_base_path(repo: &Repo) -> Option<String> {
    repo.worktree_base_path.as_deref().map(str::trim).filter(|trimmed| !trimmed.is_empty()).map(str::to_string)
}

fn resolve_workspace_layout_path(repo_path: &str, layout_path: &str) -> String {
    if is_runtime_path_absolute_for_repo(repo_path, layout_path) {
        normalize_runtime_path_separators(layout_path)
    } else {
        resolve_runtime_path(repo_path, layout_path)
    }
}

fn is_runtime_path_absolute_for_repo(repo_path: &str, layout_path: &str) -> bool {
    let flavor = if is_windows_absolute_path_like(repo_path) || is_windows_absolute_path_like(layout_path) {
        PathFlavor::Windows
    } else {
        PathFlavor::Posix
    };
    is_runtime_path_absolute(layout_path, Some(flavor))
}

fn should_include_workspace_layout(repo: Option<&Repo>, layout_path: &str) -> bool {
    match repo {
        Some(repo) if repo.connection_id.as_deref().is_some_and(|id| !id.is_empty()) => {
            !is_runtime_path_absolute_for_repo(&repo.path, layout_path)
        }
        _ => true,
    }
}

fn build_wsl_workspace_layouts(repo_path: &str, settings: &WorkspaceLayoutSettings) -> Vec<OrcaWorkspaceLayout> {
    let Some(parsed) = parse_wsl_unc_path(repo_path) else {
        return Vec::new();
    };
    // The Linux home is `/home/<user>` (the first segment under /home).
    let Some(rest) = parsed.linux_path.strip_prefix("/home/") else {
        return Vec::new();
    };
    let user = rest.split('/').next().unwrap_or("");
    if user.is_empty() {
        return Vec::new();
    }
    let root = format!("//wsl.localhost/{}/home/{}/orca/workspaces", parsed.distro, user);

    let mut modes = vec![settings.nest_workspaces];
    modes.extend(settings.workspace_dir_history.iter().map(|layout| layout.nest_workspaces));
    let mut seen = HashSet::new();
    modes
        .into_iter()
        .filter(|mode| seen.insert(*mode))
        .map(|nest_workspaces| OrcaWorkspaceLayout { path: root.clone(), nest_workspaces })
        .collect()
}

pub fn classify_worktree_ownership(
    repo: &Repo,
    worktree: &Worktree,
    meta: Option<&WorktreeMeta>,
    known_orca_layouts: &[OrcaWorkspaceLayout],
) -> WorktreeOwnership {
    if has_strong_orca_metadata(meta) {
        return WorktreeOwnership::OrcaManaged;
    }
    if matches_strong_orca_create_path(&worktree.path, known_orca_layouts, &repo.path) {
        return WorktreeOwnership::OrcaManaged;
    }
    if is_under_flat_or_untrusted_orca_root(&worktree.path, known_orca_layouts) {
        return WorktreeOwnership::UnknownLegacy;
    }
    if can_classify_as_external(&worktree.path, known_orca_layouts) {
        return WorktreeOwnership::External;
    }
    WorktreeOwnership::UnknownLegacy
}

pub fn to_detected_worktree(
    repo: &Repo,
    worktree: &Worktree,
    meta: Option<&WorktreeMeta>,
    known_orca_layouts: &[OrcaWorkspaceLayout],
    is_legacy_repo_for_visibility: Option<bool>,
) -> DetectedWorktree {
    let ownership = classify_worktree_ownership(repo, worktree, meta, known_orca_layouts);
    let selected_checkout = are_runtime_paths_equal(&worktree.path, &repo.path);
    let is_legacy =
        is_legacy_repo_for_visibility.unwrap_or_else(|| is_legacy_repo_for_external_worktree_visibility(repo));
    let visible = should_show_worktree(ownership, repo, is_legacy, selected_checkout);
    DetectedWorktree {
        path: worktree.path.clone(),
        is_main_worktree: worktree.is_main_worktree,
        ownership,
        selected_checkout,
        visible,
    }
}

pub fn should_show_worktree(
    ownership: WorktreeOwnership,
    repo: &Repo,
    is_legacy_repo_for_visibility: bool,
    is_selected_checkout: bool,
) -> bool {
    if is_selected_checkout {
        return true;
    }
    if ownership == WorktreeOwnership::OrcaManaged {
        return true;
    }
    if ownership == WorktreeOwnership::UnknownLegacy && is_legacy_repo_for_visibility {
        return true;
    }
    effective_external_worktree_visibility(repo.external_worktree_visibility, is_legacy_repo_for_visibility)
        == ExternalWorktreeVisibility::Show
}

pub fn are_runtime_paths_equal(left_path: &str, right_path: &str) -> bool {
    normalize_runtime_path_for_comparison(left_path) == normalize_runtime_path_for_comparison(right_path)
}

fn has_strong_orca_metadata(meta: Option<&WorktreeMeta>) -> bool {
    let Some(meta) = meta else {
        return false;
    };
    meta.orca_created_at.is_some_and(|value| value != 0.0)
        || meta.created_at.is_some_and(|value| value != 0.0)
        || meta.created_with_agent
        || meta.push_target
        || meta.sparse_base_ref.as_deref().is_some_and(|value| !value.is_empty())
        || meta.sparse_preset_id.as_deref().is_some_and(|value| !value.is_empty())
        || meta.preserve_branch_on_delete
}

pub fn matches_strong_orca_create_path(
    worktree_path: &str,
    known_orca_layouts: &[OrcaWorkspaceLayout],
    repo_path: &str,
) -> bool {
    let repo_name = strip_git_suffix(&get_runtime_path_basename(repo_path));
    if repo_name.is_empty() {
        return false;
    }
    for layout in known_orca_layouts {
        if !layout.nest_workspaces {
            continue;
        }
        let Some(relative) = relative_path_inside_root(&layout.path, worktree_path) else {
            continue;
        };
        let segments = split_normalized_path(&relative);
        let case_insensitive =
            is_windows_absolute_path_like(&layout.path) || is_windows_absolute_path_like(worktree_path);
        if segments.len() == 2
            && normalize_path_segment(&segments[0], case_insensitive)
                == normalize_path_segment(&repo_name, case_insensitive)
            && !segments[1].is_empty()
        {
            return true;
        }
    }
    false
}

fn is_under_flat_or_untrusted_orca_root(worktree_path: &str, known_orca_layouts: &[OrcaWorkspaceLayout]) -> bool {
    for layout in known_orca_layouts {
        if relative_path_inside_root(&layout.path, worktree_path).is_none() {
            continue;
        }
        if !layout.nest_workspaces {
            return true;
        }
    }
    false
}

fn can_classify_as_external(worktree_path: &str, known_orca_layouts: &[OrcaWorkspaceLayout]) -> bool {
    if known_orca_layouts.is_empty() {
        return false;
    }
    for layout in known_orca_layouts {
        if relative_path_inside_root(&layout.path, worktree_path).is_none() {
            continue;
        }
        return layout.nest_workspaces;
    }
    true
}

fn strip_git_suffix(name: &str) -> String {
    match name.to_ascii_lowercase().strip_suffix(".git") {
        Some(stripped) => name[..stripped.len()].to_string(),
        None => name.to_string(),
    }
}

fn split_normalized_path(value: &str) -> Vec<String> {
    normalize_runtime_path_separators(value).split('/').filter(|segment| !segment.is_empty()).map(str::to_string).collect()
}

fn normalize_path_segment(value: &str, case_insensitive: bool) -> String {
    let normalized = normalize_runtime_path_separators(value);
    if case_insensitive {
        normalized.to_lowercase()
    } else {
        normalized
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ExternalWorktreeVisibility::{Hide, Show};
    use WorktreeOwnership::{External, OrcaManaged, UnknownLegacy};

    fn make_repo() -> Repo {
        Repo {
            path: "/repos/app".to_string(),
            added_at: Some((EXTERNAL_WORKTREE_VISIBILITY_ROLLOUT_AT + 1) as f64),
            ..Default::default()
        }
    }

    fn make_settings() -> WorkspaceLayoutSettings {
        WorkspaceLayoutSettings {
            workspace_dir: Some("/orca/workspaces".to_string()),
            nest_workspaces: true,
            workspace_dir_history: Vec::new(),
        }
    }

    fn worktree(path: &str) -> Worktree {
        Worktree { path: path.to_string(), is_main_worktree: true }
    }

    #[test]
    fn treats_explicit_orca_metadata_as_managed_even_outside_workspace_root() {
        let repo = make_repo();
        let settings = make_settings();
        let layouts = build_known_orca_workspace_layouts(&settings, Some(&repo));
        let meta = WorktreeMeta { orca_created_at: Some(1.0), ..Default::default() };
        assert_eq!(
            classify_worktree_ownership(&repo, &worktree("/tmp/outside"), Some(&meta), &layouts),
            OrcaManaged
        );
    }

    #[test]
    fn requires_the_nested_repo_specific_path_shape_for_path_only_ownership() {
        let repo = make_repo();
        let settings = make_settings();
        let layouts = build_known_orca_workspace_layouts(&settings, Some(&repo));
        assert_eq!(
            classify_worktree_ownership(&repo, &worktree("/orca/workspaces/app/feature"), None, &layouts),
            OrcaManaged
        );
        assert_eq!(
            classify_worktree_ownership(&repo, &worktree("/orca/workspaces/other/feature"), None, &layouts),
            External
        );
    }

    #[test]
    fn treats_flat_workspace_root_descendants_as_unknown_legacy_without_strong_metadata() {
        let repo = make_repo();
        let settings = WorkspaceLayoutSettings { nest_workspaces: false, ..make_settings() };
        let layouts = build_known_orca_workspace_layouts(&settings, Some(&repo));
        assert_eq!(
            classify_worktree_ownership(&repo, &worktree("/orca/workspaces/feature"), None, &layouts),
            UnknownLegacy
        );
    }

    #[test]
    fn keeps_flat_layout_history_weak_after_switching_same_root_to_nested() {
        let repo = make_repo();
        let settings = WorkspaceLayoutSettings {
            workspace_dir_history: vec![OrcaWorkspaceLayout { path: "/orca/workspaces".to_string(), nest_workspaces: false }],
            ..make_settings()
        };
        let layouts = build_known_orca_workspace_layouts(&settings, Some(&repo));
        assert_eq!(
            classify_worktree_ownership(&repo, &worktree("/orca/workspaces/feature"), None, &layouts),
            UnknownLegacy
        );
    }

    #[test]
    fn uses_each_historical_layout_nest_mode_when_matching_old_roots() {
        let repo = make_repo();
        let settings = WorkspaceLayoutSettings {
            workspace_dir: Some("/new/workspaces".to_string()),
            workspace_dir_history: vec![OrcaWorkspaceLayout { path: "/old/workspaces".to_string(), nest_workspaces: true }],
            ..make_settings()
        };
        let layouts = build_known_orca_workspace_layouts(&settings, Some(&repo));
        assert_eq!(
            classify_worktree_ownership(&repo, &worktree("/old/workspaces/app/feature"), None, &layouts),
            OrcaManaged
        );
    }

    #[test]
    fn builds_known_layouts_from_large_workspace_history_lists() {
        const COUNT: usize = 150_000;
        let repo = make_repo();
        let history: Vec<OrcaWorkspaceLayout> = (0..COUNT)
            .map(|index| OrcaWorkspaceLayout { path: format!("/history/workspaces-{index}"), nest_workspaces: index % 2 == 0 })
            .collect();
        let settings = WorkspaceLayoutSettings {
            workspace_dir: Some("/new/workspaces".to_string()),
            workspace_dir_history: history,
            ..make_settings()
        };
        let layouts = build_known_orca_workspace_layouts(&settings, Some(&repo));
        assert_eq!(layouts.len(), COUNT + 1);
        assert_eq!(layouts[0], OrcaWorkspaceLayout { path: "/new/workspaces".to_string(), nest_workspaces: true });
        assert_eq!(layouts[1], OrcaWorkspaceLayout { path: "/history/workspaces-0".to_string(), nest_workspaces: true });
        assert_eq!(
            layouts.last().unwrap(),
            &OrcaWorkspaceLayout { path: format!("/history/workspaces-{}", COUNT - 1), nest_workspaces: false }
        );
    }

    #[test]
    fn handles_windows_drive_casing_and_separators() {
        let repo = Repo { path: "C:\\repos\\App".to_string(), ..make_repo() };
        let settings = WorkspaceLayoutSettings { workspace_dir: Some("C:\\Orca\\Workspaces".to_string()), ..make_settings() };
        let layouts = build_known_orca_workspace_layouts(&settings, Some(&repo));
        let worktree = Worktree { path: "C:\\ORCA\\WORKSPACES\\App\\Feature".to_string(), is_main_worktree: false };
        assert_eq!(classify_worktree_ownership(&repo, &worktree, None, &layouts), OrcaManaged);
    }

    #[test]
    fn keeps_selected_linked_checkouts_visible_without_trusting_git_main_worktree() {
        let repo = Repo {
            path: "/repos/app-linked".to_string(),
            external_worktree_visibility: Some(Hide),
            ..make_repo()
        };
        let settings = make_settings();
        let layouts = build_known_orca_workspace_layouts(&settings, Some(&repo));
        let selected = to_detected_worktree(
            &repo,
            &Worktree { path: "/repos/app-linked".to_string(), is_main_worktree: false },
            None,
            &layouts,
            None,
        );
        let git_main = to_detected_worktree(
            &repo,
            &Worktree { path: "/repos/app-main".to_string(), is_main_worktree: true },
            None,
            &layouts,
            None,
        );
        assert!(selected.visible);
        assert!(!git_main.visible);
        assert_eq!(git_main.ownership, External);
    }

    #[test]
    fn defaults_undefined_visibility_to_hide_for_new_and_show_for_legacy() {
        assert_eq!(effective_external_worktree_visibility(None, false), Hide);
        assert_eq!(effective_external_worktree_visibility(None, true), Show);
    }

    #[test]
    fn treats_persisted_repos_without_explicit_visibility_as_legacy() {
        assert!(is_legacy_repo_for_external_worktree_visibility(&make_repo()));
    }

    #[test]
    fn computes_legacy_status_from_rollout_timing_not_stored_visibility() {
        let repo = Repo {
            added_at: Some((EXTERNAL_WORKTREE_VISIBILITY_ROLLOUT_AT - 1) as f64),
            external_worktree_visibility: Some(Hide),
            ..make_repo()
        };
        assert!(is_legacy_repo_for_external_worktree_visibility(&repo));
    }

    #[test]
    fn honors_an_explicit_legacy_marker_after_visibility_changes() {
        let legacy = Repo {
            external_worktree_visibility: Some(Hide),
            external_worktree_visibility_legacy: Some(true),
            ..make_repo()
        };
        let not_legacy = Repo {
            external_worktree_visibility: Some(Hide),
            external_worktree_visibility_legacy: Some(false),
            ..make_repo()
        };
        assert!(is_legacy_repo_for_external_worktree_visibility(&legacy));
        assert!(!is_legacy_repo_for_external_worktree_visibility(&not_legacy));
    }

    #[test]
    fn treats_repos_without_a_valid_added_at_as_legacy() {
        assert!(is_legacy_repo_for_external_worktree_visibility(&Repo { added_at: None, ..make_repo() }));
        assert!(is_legacy_repo_for_external_worktree_visibility(&Repo { added_at: Some(f64::NAN), ..make_repo() }));
    }

    #[test]
    fn keeps_unknown_legacy_rows_visible_for_legacy_repos_after_hiding_external_rows() {
        let repo = Repo {
            added_at: Some((EXTERNAL_WORKTREE_VISIBILITY_ROLLOUT_AT - 1) as f64),
            external_worktree_visibility: Some(Hide),
            ..make_repo()
        };
        assert!(should_show_worktree(UnknownLegacy, &repo, true, false));
    }
}
