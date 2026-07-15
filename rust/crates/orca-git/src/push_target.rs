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

#[cfg(test)]
mod tests {
    use super::*;

    // Golden pins for the pure target-naming helpers. These used to live in the
    // TS↔Rust parity corpus, but the TS `getPublishTargetRemoteRef` twin was
    // retired when publish-target status moved fully to Rust — so this is now the
    // sole home for the golden (co-located with the code it pins).
    fn target(remote: &str, branch: &str, url: Option<&str>) -> GitPushTarget {
        GitPushTarget {
            remote_name: remote.to_string(),
            branch_name: branch.to_string(),
            remote_url: url.map(str::to_string),
        }
    }

    #[test]
    fn display_name_joins_remote_and_branch_with_a_slash() {
        assert_eq!(publish_target_display_name(&target("origin", "main", None)), "origin/main");
        // Slashed branch is preserved verbatim; remote_url is ignored.
        assert_eq!(
            publish_target_display_name(&target(
                "upstream",
                "feature/foo",
                Some("git@github.com:me/repo.git"),
            )),
            "upstream/feature/foo",
        );
    }

    #[test]
    fn remote_ref_qualifies_the_target_under_refs_remotes() {
        assert_eq!(
            publish_target_remote_ref(&target("origin", "main", None)),
            "refs/remotes/origin/main",
        );
        // Slashed branch + present remote_url does not affect the ref.
        assert_eq!(
            publish_target_remote_ref(&target(
                "fork",
                "wip/x",
                Some("https://example.com/me/repo.git"),
            )),
            "refs/remotes/fork/wip/x",
        );
    }
}
