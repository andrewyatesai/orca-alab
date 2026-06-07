//! Effective upstream resolution, ported from
//! `src/shared/git-effective-upstream.ts`. Resolves the branch source-control
//! pull/sync should follow: the configured `@{u}`, with a legacy fix-up for old
//! worktrees that inherited `origin/<base>` while pushing `origin/<branch>`.

use crate::runner::{GitError, GitRunner};
use orca_core::git_upstream_status::GitUpstreamStatus;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffectiveGitUpstream {
    pub upstream_name: String,
    pub remote_name: Option<String>,
    pub branch_name: String,
    pub is_configured_upstream: bool,
}

pub fn split_remote_branch_name(ref_name: &str) -> Option<(String, String)> {
    let slash = ref_name.find('/')?;
    if slash == 0 || slash == ref_name.len() - 1 {
        return None;
    }
    Some((ref_name[..slash].to_string(), ref_name[slash + 1..].to_string()))
}

fn has_multiple_slash_segments(ref_name: &str) -> bool {
    ref_name.contains('/') && ref_name.find('/') != ref_name.rfind('/')
}

/// Parse `git rev-list --left-right --count` output (`"<ahead>\t<behind>"`).
pub(crate) fn parse_rev_list_counts(stdout: &str) -> Result<(i64, i64), GitError> {
    let tokens: Vec<&str> = stdout.split_whitespace().collect();
    if tokens.len() != 2 {
        return Err(GitError::from_message(format!(
            "Unexpected git rev-list output: {stdout:?}"
        )));
    }
    let ahead = tokens[0].parse::<i64>().ok();
    let behind = tokens[1].parse::<i64>().ok();
    match (ahead, behind) {
        (Some(a), Some(b)) if a >= 0 && b >= 0 => Ok((a, b)),
        _ => Err(GitError::from_message(format!(
            "Unparseable git rev-list counts: {stdout:?}"
        ))),
    }
}

fn current_branch_name<R: GitRunner>(runner: &R) -> Option<String> {
    let out = runner.run(&["symbolic-ref", "--quiet", "--short", "HEAD"]).ok()?;
    let branch = out.stdout.trim();
    (!branch.is_empty()).then(|| branch.to_string())
}

fn configured_upstream<R: GitRunner>(runner: &R) -> Result<Option<EffectiveGitUpstream>, GitError> {
    match runner.run(&["rev-parse", "--abbrev-ref", "HEAD@{u}"]) {
        Ok(out) => {
            let upstream_name = out.stdout.trim().to_string();
            if upstream_name.is_empty() {
                return Ok(None);
            }
            Ok(Some(match split_remote_branch_name(&upstream_name) {
                Some((remote, branch)) => EffectiveGitUpstream {
                    upstream_name,
                    remote_name: Some(remote),
                    branch_name: branch,
                    is_configured_upstream: true,
                },
                None => EffectiveGitUpstream {
                    branch_name: upstream_name.clone(),
                    upstream_name,
                    remote_name: None,
                    is_configured_upstream: true,
                },
            }))
        }
        Err(error) => {
            if orca_text::git_remote_error::is_no_upstream_error(Some(&error.message)) {
                Ok(None)
            } else {
                Err(error)
            }
        }
    }
}

fn remote_tracking_ref_exists<R: GitRunner>(runner: &R, remote: &str, branch: &str) -> bool {
    runner
        .run(&["rev-parse", "--verify", "--quiet", &format!("refs/remotes/{remote}/{branch}")])
        .is_ok()
}

fn split_by_known_remote<R: GitRunner>(runner: &R, ref_name: &str) -> Option<(String, String)> {
    let out = runner.run(&["remote"]).ok()?;
    let mut remotes: Vec<&str> =
        out.stdout.split('\n').map(str::trim).filter(|l| !l.is_empty()).collect();
    // Longest remote name first, so `origin/team` beats `origin`.
    remotes.sort_by_key(|s| std::cmp::Reverse(s.len()));
    for remote in remotes {
        if ref_name == remote || !ref_name.starts_with(&format!("{remote}/")) {
            continue;
        }
        let branch = &ref_name[remote.len() + 1..];
        if !branch.is_empty() {
            return Some((remote.to_string(), branch.to_string()));
        }
    }
    None
}

fn same_name_origin(branch: &str) -> EffectiveGitUpstream {
    EffectiveGitUpstream {
        upstream_name: format!("origin/{branch}"),
        remote_name: Some("origin".to_string()),
        branch_name: branch.to_string(),
        is_configured_upstream: false,
    }
}

pub fn resolve_effective_git_upstream<R: GitRunner>(
    runner: &R,
) -> Result<Option<EffectiveGitUpstream>, GitError> {
    let current = current_branch_name(runner);

    if let Some(mut configured) = configured_upstream(runner)? {
        // Legacy `origin/<base>` upstream whose name has multiple segments: try
        // to re-split against the known remotes so the publish branch wins.
        if let Some(cur) = &current {
            if configured.remote_name.as_deref() == Some("origin")
                && configured.branch_name != *cur
                && has_multiple_slash_segments(&configured.upstream_name)
            {
                if let Some((remote, branch)) = split_by_known_remote(runner, &configured.upstream_name) {
                    configured.remote_name = Some(remote);
                    configured.branch_name = branch;
                }
            }
        }

        match &current {
            None => return Ok(Some(configured)),
            Some(cur) if configured.branch_name == *cur => return Ok(Some(configured)),
            Some(cur) => {
                // Old worktrees inherited origin/<base>; if a same-name origin
                // tracking ref exists, follow the publish branch instead.
                if configured.remote_name.as_deref() == Some("origin")
                    && remote_tracking_ref_exists(runner, "origin", cur)
                {
                    return Ok(Some(same_name_origin(cur)));
                }
                return Ok(Some(configured));
            }
        }
    }

    if let Some(cur) = &current {
        if remote_tracking_ref_exists(runner, "origin", cur) {
            return Ok(Some(same_name_origin(cur)));
        }
    }
    Ok(None)
}

/// Resolve the effective upstream and compute ahead/behind counts.
pub fn effective_git_upstream_status<R: GitRunner>(
    runner: &R,
    behind_equiv: Option<&dyn Fn(&str) -> bool>,
) -> Result<GitUpstreamStatus, GitError> {
    let Some(upstream) = resolve_effective_git_upstream(runner)? else {
        return Ok(GitUpstreamStatus {
            has_upstream: false,
            upstream_name: None,
            ahead: 0,
            behind: 0,
            behind_commits_are_patch_equivalent: None,
        });
    };

    let out = runner.run(&[
        "rev-list",
        "--left-right",
        "--count",
        &format!("HEAD...{}", upstream.upstream_name),
    ])?;
    let (ahead, behind) = parse_rev_list_counts(&out.stdout)?;

    let behind_commits_are_patch_equivalent = if ahead > 0 && behind > 0 {
        behind_equiv.map(|f| f(&upstream.upstream_name))
    } else {
        None
    };

    Ok(GitUpstreamStatus {
        has_upstream: true,
        upstream_name: Some(upstream.upstream_name),
        ahead,
        behind,
        behind_commits_are_patch_equivalent,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_remote_branch_name_cases() {
        assert_eq!(
            split_remote_branch_name("origin/main"),
            Some(("origin".to_string(), "main".to_string()))
        );
        assert_eq!(
            split_remote_branch_name("origin/feature/x"),
            Some(("origin".to_string(), "feature/x".to_string()))
        );
        assert_eq!(split_remote_branch_name("main"), None);
        assert_eq!(split_remote_branch_name("/leading"), None);
        assert_eq!(split_remote_branch_name("trailing/"), None);
    }

    #[test]
    fn parse_counts_validates() {
        assert_eq!(parse_rev_list_counts("2\t3\n").unwrap(), (2, 3));
        assert!(parse_rev_list_counts("oops").is_err());
        assert!(parse_rev_list_counts("1\t-2").is_err());
    }
}
