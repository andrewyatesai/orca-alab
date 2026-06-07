//! Explicit publish-target status, ported from
//! `src/shared/git-publish-target-status.ts`. Used when source-control targets
//! a specific `remote/branch` rather than the configured upstream.

use crate::effective_upstream::parse_rev_list_counts;
use crate::push_target::{publish_target_display_name, publish_target_remote_ref, GitPushTarget};
use crate::runner::{GitError, GitRunner};
use orca_core::git_upstream_status::GitUpstreamStatus;

/// A bare `git exited with 1` (empty stderr) means the remote-tracking ref just
/// hasn't been fetched yet — treated as "no upstream", not an error.
fn is_missing_remote_tracking_ref_error(error: &GitError) -> bool {
    if !error.stderr.trim().is_empty() {
        return false;
    }
    error.code == Some(1) || message_indicates_exit_1(&error.message)
}

/// Matches `(?:exited with|exit code) 1\b` case-insensitively, for runners that
/// don't surface a numeric exit code.
fn message_indicates_exit_1(message: &str) -> bool {
    let lower = message.to_lowercase();
    for pat in ["exited with 1", "exit code 1"] {
        if let Some(idx) = lower.find(pat) {
            match lower[idx + pat.len()..].chars().next() {
                None => return true,
                Some(c) if !(c.is_ascii_alphanumeric() || c == '_') => return true,
                _ => {}
            }
        }
    }
    false
}

pub fn get_publish_target_status<R: GitRunner>(
    runner: &R,
    target: &GitPushTarget,
    behind_equiv: Option<&dyn Fn(&str) -> bool>,
) -> Result<GitUpstreamStatus, GitError> {
    let upstream_name = publish_target_display_name(target);
    let remote_ref = publish_target_remote_ref(target);

    if let Err(error) = runner.run(&["rev-parse", "--verify", "--quiet", &remote_ref]) {
        if !is_missing_remote_tracking_ref_error(&error) {
            return Err(error);
        }
        return Ok(GitUpstreamStatus {
            has_upstream: false,
            upstream_name: Some(upstream_name),
            ahead: 0,
            behind: 0,
            behind_commits_are_patch_equivalent: None,
        });
    }

    let out = runner.run(&["rev-list", "--left-right", "--count", &format!("HEAD...{remote_ref}")])?;
    let (ahead, behind) = parse_rev_list_counts(&out.stdout)?;

    let behind_commits_are_patch_equivalent = if ahead > 0 && behind > 0 {
        behind_equiv.map(|f| f(&remote_ref))
    } else {
        None
    };

    Ok(GitUpstreamStatus {
        has_upstream: true,
        upstream_name: Some(upstream_name),
        ahead,
        behind,
        behind_commits_are_patch_equivalent,
    })
}
