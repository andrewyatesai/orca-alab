//! Workspace cleanup classification, ported from `src/shared/workspace-cleanup.ts`.
//!
//! Decides whether an inactive worktree is ready/review/protected for cleanup,
//! whether it can be queued/selected/force-removed, and computes the dismissal
//! fingerprint. Pure; lean structs model the fields the policy reads.

pub const WORKSPACE_CLEANUP_CLASSIFIER_VERSION: i64 = 2;
pub const WORKSPACE_CLEANUP_ARCHIVED_IDLE_MS: i64 = 7 * 24 * 60 * 60 * 1000;
pub const WORKSPACE_CLEANUP_IDLE_MS: i64 = 30 * 24 * 60 * 60 * 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkspaceCleanupTier {
    Ready,
    Review,
    Protected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkspaceCleanupReason {
    Archived,
    IdleClean,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkspaceCleanupBlocker {
    MainWorktree,
    FolderRepo,
    Pinned,
    ActiveWorkspace,
    RunningTerminal,
    TerminalLivenessUnknown,
    DirtyEditorBuffer,
    VolatileLocalContext,
    RecentVisibleContext,
    LiveAgent,
    SshDisconnected,
    GitStatusError,
    DirtyFiles,
    UnpushedCommits,
    UnknownBase,
    Dismissed,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WorkspaceCleanupGit {
    /// `None` = git status not yet known.
    pub clean: Option<bool>,
    /// Epoch-ms of the last git status check; `None` if never checked.
    pub checked_at: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceCleanupCandidate {
    pub worktree_id: String,
    pub fingerprint: String,
    pub tier: WorkspaceCleanupTier,
    pub selected_by_default: bool,
    pub reasons: Vec<WorkspaceCleanupReason>,
    pub blockers: Vec<WorkspaceCleanupBlocker>,
    pub git: WorkspaceCleanupGit,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceCleanupDismissal {
    pub worktree_id: String,
    pub dismissed_at: i64,
    pub fingerprint: String,
    pub classifier_version: i64,
}

#[derive(Clone, Copy, Debug)]
pub struct WorkspaceCleanupInactivityInput {
    pub is_archived: bool,
    pub last_activity_at: i64,
}

use WorkspaceCleanupBlocker as B;

/// Every blocker currently hard-blocks selection.
const HARD_BLOCKERS: [WorkspaceCleanupBlocker; 16] = [
    B::MainWorktree, B::FolderRepo, B::Pinned, B::ActiveWorkspace, B::RunningTerminal,
    B::TerminalLivenessUnknown, B::DirtyEditorBuffer, B::VolatileLocalContext, B::RecentVisibleContext,
    B::LiveAgent, B::SshDisconnected, B::GitStatusError, B::DirtyFiles, B::UnpushedCommits, B::UnknownBase,
    B::Dismissed,
];

const QUEUE_BLOCKERS: [WorkspaceCleanupBlocker; 3] = [B::MainWorktree, B::FolderRepo, B::Dismissed];

const FORCE_REMOVE_BLOCKERS: [WorkspaceCleanupBlocker; 4] =
    [B::DirtyFiles, B::UnpushedCommits, B::UnknownBase, B::GitStatusError];

pub fn is_workspace_cleanup_hard_blocker(blocker: WorkspaceCleanupBlocker) -> bool {
    HARD_BLOCKERS.contains(&blocker)
}

pub fn can_queue_workspace_cleanup_candidate(candidate: &WorkspaceCleanupCandidate) -> bool {
    !candidate.reasons.is_empty() && !candidate.blockers.iter().any(|blocker| QUEUE_BLOCKERS.contains(blocker))
}

pub fn should_force_workspace_cleanup_removal(candidate: &WorkspaceCleanupCandidate) -> bool {
    candidate.git.clean != Some(true)
        || candidate.git.checked_at.is_none()
        || candidate.blockers.iter().any(|blocker| FORCE_REMOVE_BLOCKERS.contains(blocker))
}

pub fn can_select_workspace_cleanup_candidate(candidate: &WorkspaceCleanupCandidate) -> bool {
    !candidate.reasons.is_empty()
        && candidate.git.clean == Some(true)
        && candidate.git.checked_at.is_some()
        && !candidate.blockers.iter().copied().any(is_workspace_cleanup_hard_blocker)
}

pub fn apply_workspace_cleanup_policy(candidate: &WorkspaceCleanupCandidate) -> WorkspaceCleanupCandidate {
    let can_select = can_select_workspace_cleanup_candidate(candidate);
    let has_hard_blocker = candidate.blockers.iter().copied().any(is_workspace_cleanup_hard_blocker);
    let tier = if has_hard_blocker {
        WorkspaceCleanupTier::Protected
    } else if can_select {
        WorkspaceCleanupTier::Ready
    } else {
        WorkspaceCleanupTier::Review
    };
    WorkspaceCleanupCandidate {
        tier,
        selected_by_default: tier == WorkspaceCleanupTier::Ready && can_select,
        ..candidate.clone()
    }
}

pub fn create_workspace_cleanup_fingerprint(
    branch: &str,
    head: &str,
    git_clean: Option<bool>,
    last_activity_at: i64,
    classifier_version: Option<i64>,
) -> String {
    let version = classifier_version.unwrap_or(WORKSPACE_CLEANUP_CLASSIFIER_VERSION);
    let last_activity_bucket = last_activity_at.max(0) / (24 * 60 * 60 * 1000);
    let clean = match git_clean {
        None => "unknown",
        Some(true) => "clean",
        Some(false) => "dirty",
    };
    format!("{version}|{branch}|{head}|{clean}|{last_activity_bucket}")
}

pub fn get_workspace_cleanup_inactivity_reasons(
    workspace: WorkspaceCleanupInactivityInput,
    scanned_at: i64,
) -> Vec<WorkspaceCleanupReason> {
    let mut reasons = Vec::new();
    if workspace.is_archived && scanned_at - workspace.last_activity_at >= WORKSPACE_CLEANUP_ARCHIVED_IDLE_MS {
        reasons.push(WorkspaceCleanupReason::Archived);
    }
    if scanned_at - workspace.last_activity_at >= WORKSPACE_CLEANUP_IDLE_MS {
        reasons.push(WorkspaceCleanupReason::IdleClean);
    }
    reasons
}

pub fn is_workspace_old_for_cleanup(workspace: WorkspaceCleanupInactivityInput, scanned_at: i64) -> bool {
    !get_workspace_cleanup_inactivity_reasons(workspace, scanned_at).is_empty()
}

pub fn should_hide_workspace_cleanup_candidate(
    candidate: &WorkspaceCleanupCandidate,
    dismissal: Option<&WorkspaceCleanupDismissal>,
) -> bool {
    matches!(dismissal, Some(dismissal)
        if dismissal.worktree_id == candidate.worktree_id
            && dismissal.fingerprint == candidate.fingerprint
            && dismissal.classifier_version == WORKSPACE_CLEANUP_CLASSIFIER_VERSION)
}

#[cfg(test)]
mod tests {
    use super::*;
    use WorkspaceCleanupReason::IdleClean;
    use WorkspaceCleanupTier::{Protected, Ready, Review};

    fn candidate(reasons: Vec<WorkspaceCleanupReason>, blockers: Vec<WorkspaceCleanupBlocker>, git: WorkspaceCleanupGit) -> WorkspaceCleanupCandidate {
        WorkspaceCleanupCandidate {
            worktree_id: "repo-1::/tmp/feature".to_string(),
            fingerprint: "fingerprint".to_string(),
            tier: Review,
            selected_by_default: false,
            reasons,
            blockers,
            git,
        }
    }

    fn clean_git() -> WorkspaceCleanupGit {
        WorkspaceCleanupGit { clean: Some(true), checked_at: Some(1_700_000_000_000) }
    }

    #[test]
    fn marks_clean_inactive_workspaces_as_ready_and_selected() {
        let result = apply_workspace_cleanup_policy(&candidate(vec![IdleClean], Vec::new(), clean_git()));
        assert_eq!(result.tier, Ready);
        assert!(result.selected_by_default);
        assert!(can_select_workspace_cleanup_candidate(&result));
    }

    #[test]
    fn requires_an_inactivity_reason_before_selecting() {
        let result = apply_workspace_cleanup_policy(&candidate(Vec::new(), Vec::new(), clean_git()));
        assert!(!can_select_workspace_cleanup_candidate(&result));
        assert_eq!(result.tier, Review);
        assert!(!result.selected_by_default);
    }

    #[test]
    fn keeps_not_suggested_candidates_queueable_when_git_is_clean() {
        let result = apply_workspace_cleanup_policy(&candidate(vec![IdleClean], vec![B::UnpushedCommits], clean_git()));
        assert_eq!(result.tier, Protected);
        assert!(!result.selected_by_default);
        assert!(!can_select_workspace_cleanup_candidate(&result));
        assert!(can_queue_workspace_cleanup_candidate(&result));
        assert!(should_force_workspace_cleanup_removal(&result));
    }

    #[test]
    fn does_not_queue_main_worktrees_or_folder_projects() {
        let main = apply_workspace_cleanup_policy(&candidate(vec![IdleClean], vec![B::MainWorktree], clean_git()));
        let folder = apply_workspace_cleanup_policy(&candidate(vec![IdleClean], vec![B::FolderRepo], clean_git()));
        assert!(!can_queue_workspace_cleanup_candidate(&main));
        assert!(!can_queue_workspace_cleanup_candidate(&folder));
    }

    #[test]
    fn requires_current_git_status_before_selecting() {
        let result = apply_workspace_cleanup_policy(&candidate(
            vec![IdleClean],
            Vec::new(),
            WorkspaceCleanupGit { clean: None, checked_at: None },
        ));
        assert_eq!(result.tier, Review);
        assert!(!can_select_workspace_cleanup_candidate(&result));
    }

    #[test]
    fn matches_dismissals_only_for_the_current_classifier_fingerprint() {
        let fingerprint = create_workspace_cleanup_fingerprint("feature", "abc123", Some(true), 1_700_000_000_000, None);
        let candidate = candidate(vec![IdleClean], Vec::new(), clean_git());
        let candidate = WorkspaceCleanupCandidate { fingerprint: fingerprint.clone(), ..candidate };
        assert!(should_hide_workspace_cleanup_candidate(
            &candidate,
            Some(&WorkspaceCleanupDismissal {
                worktree_id: candidate.worktree_id.clone(),
                dismissed_at: 1_700_000_000_000,
                fingerprint: fingerprint.clone(),
                classifier_version: WORKSPACE_CLEANUP_CLASSIFIER_VERSION,
            })
        ));
        assert!(!should_hide_workspace_cleanup_candidate(
            &candidate,
            Some(&WorkspaceCleanupDismissal {
                worktree_id: candidate.worktree_id.clone(),
                dismissed_at: 1_700_000_000_000,
                fingerprint: format!("{fingerprint}|changed"),
                classifier_version: WORKSPACE_CLEANUP_CLASSIFIER_VERSION,
            })
        ));
    }

    #[test]
    fn inactivity_reasons_track_archived_and_idle_thresholds() {
        let archived = WorkspaceCleanupInactivityInput { is_archived: true, last_activity_at: 0 };
        assert_eq!(
            get_workspace_cleanup_inactivity_reasons(archived, WORKSPACE_CLEANUP_IDLE_MS),
            [WorkspaceCleanupReason::Archived, WorkspaceCleanupReason::IdleClean]
        );
        let fresh = WorkspaceCleanupInactivityInput { is_archived: false, last_activity_at: 0 };
        assert!(get_workspace_cleanup_inactivity_reasons(fresh, 0).is_empty());
        assert!(!is_workspace_old_for_cleanup(fresh, 0));
    }
}
