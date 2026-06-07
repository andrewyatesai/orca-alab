//! Parity dispatch for `orca_core::workspace_cleanup` vs
//! `src/shared/workspace-cleanup.ts`.

use orca_core::workspace_cleanup::{
    apply_workspace_cleanup_policy, can_queue_workspace_cleanup_candidate,
    can_select_workspace_cleanup_candidate, create_workspace_cleanup_fingerprint,
    get_workspace_cleanup_inactivity_reasons, is_workspace_cleanup_hard_blocker,
    should_force_workspace_cleanup_removal, should_hide_workspace_cleanup_candidate,
    WorkspaceCleanupBlocker, WorkspaceCleanupCandidate, WorkspaceCleanupDismissal,
    WorkspaceCleanupGit, WorkspaceCleanupInactivityInput, WorkspaceCleanupReason,
    WorkspaceCleanupTier,
};
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "isWorkspaceCleanupHardBlocker" => match input.as_str().and_then(parse_blocker) {
            Some(blocker) => Value::Bool(is_workspace_cleanup_hard_blocker(blocker)),
            // Vectors only carry known blocker ids; an unknown one is a vector bug.
            None => json!({ "__parity_error__": "unknown WorkspaceCleanupBlocker in input" }),
        },
        "canQueueWorkspaceCleanupCandidate" => {
            Value::Bool(can_queue_workspace_cleanup_candidate(&parse_candidate(input)))
        }
        "shouldForceWorkspaceCleanupRemoval" => {
            Value::Bool(should_force_workspace_cleanup_removal(&parse_candidate(input)))
        }
        "canSelectWorkspaceCleanupCandidate" => {
            Value::Bool(can_select_workspace_cleanup_candidate(&parse_candidate(input)))
        }
        "applyWorkspaceCleanupPolicy" => apply_policy_to_json(input),
        "createWorkspaceCleanupFingerprint" => fingerprint_to_json(input),
        "getWorkspaceCleanupInactivityReasons" => inactivity_reasons_to_json(input),
        "shouldHideWorkspaceCleanupCandidate" => should_hide_to_json(input),
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

/// Mirror TS `{ ...candidate, tier, selectedByDefault }`: every input field
/// passes through unchanged; only the two fields the policy recomputes are
/// overwritten (preserve_order keeps their original key positions).
fn apply_policy_to_json(input: &Value) -> Value {
    let result = apply_workspace_cleanup_policy(&parse_candidate(input));
    let mut obj = input.as_object().cloned().unwrap_or_default();
    obj.insert("tier".to_string(), Value::String(tier_id(result.tier).to_string()));
    obj.insert("selectedByDefault".to_string(), Value::Bool(result.selected_by_default));
    Value::Object(obj)
}

fn fingerprint_to_json(input: &Value) -> Value {
    let branch = input.get("branch").and_then(Value::as_str).unwrap_or("");
    let head = input.get("head").and_then(Value::as_str).unwrap_or("");
    // `gitClean` is `boolean | null`; null/absent -> None ("unknown").
    let git_clean = input.get("gitClean").and_then(Value::as_bool);
    let last_activity_at = input.get("lastActivityAt").and_then(Value::as_i64).unwrap_or(0);
    let classifier_version = input.get("classifierVersion").and_then(Value::as_i64);
    Value::String(create_workspace_cleanup_fingerprint(
        branch,
        head,
        git_clean,
        last_activity_at,
        classifier_version,
    ))
}

fn inactivity_reasons_to_json(input: &Value) -> Value {
    let workspace = input.get("workspace");
    let is_archived = workspace
        .and_then(|w| w.get("isArchived"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let last_activity_at = workspace
        .and_then(|w| w.get("lastActivityAt"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let scanned_at = input.get("scannedAt").and_then(Value::as_i64).unwrap_or(0);
    let reasons = get_workspace_cleanup_inactivity_reasons(
        WorkspaceCleanupInactivityInput { is_archived, last_activity_at },
        scanned_at,
    );
    Value::Array(
        reasons
            .into_iter()
            .map(|reason| Value::String(reason_id(reason).to_string()))
            .collect(),
    )
}

fn should_hide_to_json(input: &Value) -> Value {
    let candidate = parse_candidate(input.get("candidate").unwrap_or(&Value::Null));
    let dismissal = input.get("dismissal").and_then(parse_dismissal);
    Value::Bool(should_hide_workspace_cleanup_candidate(&candidate, dismissal.as_ref()))
}

/// Parse the lean candidate fields the policy reads. Pick-typed callers only
/// carry a subset of keys; missing keys fall back to inert defaults.
fn parse_candidate(input: &Value) -> WorkspaceCleanupCandidate {
    WorkspaceCleanupCandidate {
        worktree_id: input.get("worktreeId").and_then(Value::as_str).unwrap_or("").to_string(),
        fingerprint: input.get("fingerprint").and_then(Value::as_str).unwrap_or("").to_string(),
        // Recomputed by the policy; an inert default for the predicate callers.
        tier: WorkspaceCleanupTier::Review,
        selected_by_default: input
            .get("selectedByDefault")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        reasons: parse_reasons(input.get("reasons")),
        blockers: parse_blockers(input.get("blockers")),
        git: parse_git(input.get("git")),
    }
}

fn parse_dismissal(value: &Value) -> Option<WorkspaceCleanupDismissal> {
    let obj = value.as_object()?;
    Some(WorkspaceCleanupDismissal {
        worktree_id: obj.get("worktreeId").and_then(Value::as_str).unwrap_or("").to_string(),
        dismissed_at: obj.get("dismissedAt").and_then(Value::as_i64).unwrap_or(0),
        fingerprint: obj.get("fingerprint").and_then(Value::as_str).unwrap_or("").to_string(),
        classifier_version: obj.get("classifierVersion").and_then(Value::as_i64).unwrap_or(0),
    })
}

fn parse_git(value: Option<&Value>) -> WorkspaceCleanupGit {
    WorkspaceCleanupGit {
        // null/absent -> None ("status not yet known"); true/false map directly.
        clean: value.and_then(|git| git.get("clean")).and_then(Value::as_bool),
        checked_at: value.and_then(|git| git.get("checkedAt")).and_then(Value::as_i64),
    }
}

fn parse_reasons(value: Option<&Value>) -> Vec<WorkspaceCleanupReason> {
    value
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(Value::as_str).filter_map(parse_reason).collect())
        .unwrap_or_default()
}

fn parse_blockers(value: Option<&Value>) -> Vec<WorkspaceCleanupBlocker> {
    value
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(Value::as_str).filter_map(parse_blocker).collect())
        .unwrap_or_default()
}

fn parse_reason(id: &str) -> Option<WorkspaceCleanupReason> {
    Some(match id {
        "archived" => WorkspaceCleanupReason::Archived,
        "idle-clean" => WorkspaceCleanupReason::IdleClean,
        _ => return None,
    })
}

fn reason_id(reason: WorkspaceCleanupReason) -> &'static str {
    match reason {
        WorkspaceCleanupReason::Archived => "archived",
        WorkspaceCleanupReason::IdleClean => "idle-clean",
    }
}

fn tier_id(tier: WorkspaceCleanupTier) -> &'static str {
    match tier {
        WorkspaceCleanupTier::Ready => "ready",
        WorkspaceCleanupTier::Review => "review",
        WorkspaceCleanupTier::Protected => "protected",
    }
}

fn parse_blocker(id: &str) -> Option<WorkspaceCleanupBlocker> {
    use WorkspaceCleanupBlocker as B;
    Some(match id {
        "main-worktree" => B::MainWorktree,
        "folder-repo" => B::FolderRepo,
        "pinned" => B::Pinned,
        "active-workspace" => B::ActiveWorkspace,
        "running-terminal" => B::RunningTerminal,
        "terminal-liveness-unknown" => B::TerminalLivenessUnknown,
        "dirty-editor-buffer" => B::DirtyEditorBuffer,
        "volatile-local-context" => B::VolatileLocalContext,
        "recent-visible-context" => B::RecentVisibleContext,
        "live-agent" => B::LiveAgent,
        "ssh-disconnected" => B::SshDisconnected,
        "git-status-error" => B::GitStatusError,
        "dirty-files" => B::DirtyFiles,
        "unpushed-commits" => B::UnpushedCommits,
        "unknown-base" => B::UnknownBase,
        "dismissed" => B::Dismissed,
        _ => return None,
    })
}
