//! Push-target type + validation, ported from
//! `src/main/git/push-target-validation.ts` (shape validation lives in
//! `orca-core::git_push_target`).

use crate::runner::{AsyncGitRunner, GitError, GitRunner};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitPushTarget {
    pub remote_name: String,
    pub branch_name: String,
    pub remote_url: Option<String>,
}

/// Validate a push target's shape, then confirm the branch name is a legal git
/// ref via `git check-ref-format`.
pub fn validate_git_push_target<R: GitRunner>(
    runner: &R,
    target: &GitPushTarget,
) -> Result<(), GitError> {
    validate_push_target_shape(target)?;
    runner.run(&["check-ref-format", "--branch", &target.branch_name])?;
    Ok(())
}

/// Async twin of [`validate_git_push_target`] for the wasm relay: same pure shape
/// validation, same `check-ref-format` call, awaited.
pub async fn validate_git_push_target_async<R: AsyncGitRunner>(
    runner: &R,
    target: &GitPushTarget,
) -> Result<(), GitError> {
    validate_push_target_shape(target)?;
    runner.run(&["check-ref-format", "--branch", &target.branch_name], None).await?;
    Ok(())
}

/// Pure shape/path-traversal validation shared by the sync + async validators.
fn validate_push_target_shape(target: &GitPushTarget) -> Result<(), GitError> {
    orca_core::git_push_target::validate_git_push_target(
        &target.remote_name,
        &target.branch_name,
        target.remote_url.as_deref(),
    )
    .map_err(GitError::from_message)
}

pub fn publish_target_display_name(target: &GitPushTarget) -> String {
    format!("{}/{}", target.remote_name, target.branch_name)
}

pub fn publish_target_remote_ref(target: &GitPushTarget) -> String {
    format!("refs/remotes/{}/{}", target.remote_name, target.branch_name)
}
