//! Upstream-status reconciliation helpers, ported from
//! `src/shared/git-upstream-status.ts`.
//!
//! Decides whether upstream-only commits are patch-equivalent (rebased copies)
//! and whether a lease-protected force push is the correct reconciliation.

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GitUpstreamStatus {
    pub has_upstream: bool,
    pub upstream_name: Option<String>,
    pub ahead: i64,
    pub behind: i64,
    /// When a branch was rebased, upstream-only commits can be older
    /// patch-equivalent copies; pulling them reintroduces stale history.
    pub behind_commits_are_patch_equivalent: Option<bool>,
}

/// True when `git cherry`-style `-`/`=` marks all indicate patch-equivalence
/// (`=`) and there is at least one commit to judge.
pub fn upstream_only_commits_are_patch_equivalent(cherry_mark_output: &str) -> bool {
    let lines: Vec<&str> = cherry_mark_output
        .split('\n')
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    !lines.is_empty() && lines.iter().all(|line| line.starts_with('='))
}

pub fn should_force_push_with_lease_for_upstream(status: Option<&GitUpstreamStatus>) -> bool {
    match status {
        Some(s) => {
            s.has_upstream
                && s.ahead > 0
                && s.behind > 0
                && s.behind_commits_are_patch_equivalent == Some(true)
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_equivalent_requires_all_equals_marks_and_at_least_one_line() {
        assert!(upstream_only_commits_are_patch_equivalent("= abc\n= def\r\n"));
        assert!(!upstream_only_commits_are_patch_equivalent("= abc\n- def"));
        assert!(!upstream_only_commits_are_patch_equivalent("   \n  "));
        assert!(!upstream_only_commits_are_patch_equivalent(""));
    }

    #[test]
    fn force_push_only_when_diverged_and_patch_equivalent() {
        let diverged = GitUpstreamStatus {
            has_upstream: true,
            ahead: 2,
            behind: 3,
            behind_commits_are_patch_equivalent: Some(true),
            ..Default::default()
        };
        assert!(should_force_push_with_lease_for_upstream(Some(&diverged)));

        let not_equivalent = GitUpstreamStatus {
            behind_commits_are_patch_equivalent: Some(false),
            ..diverged.clone()
        };
        assert!(!should_force_push_with_lease_for_upstream(Some(&not_equivalent)));

        let only_ahead = GitUpstreamStatus { behind: 0, ..diverged.clone() };
        assert!(!should_force_push_with_lease_for_upstream(Some(&only_ahead)));

        assert!(!should_force_push_with_lease_for_upstream(None));
    }
}
