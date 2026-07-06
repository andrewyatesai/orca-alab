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

/// Port of `getGitConfigValue`: `git config --get <key>`, trimmed; `None` on empty
/// value or a config miss (git exits non-zero).
pub(crate) fn git_config_value<R: GitRunner>(runner: &R, key: &str) -> Option<String> {
    let out = runner.run(&["config", "--get", key]).ok()?;
    let value = out.stdout.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

/// Port of `isUrlValuedRemote`: a scheme URL (`scheme://…`) or scp-like SSH
/// (`user@host:path`). Mirrors `^[A-Za-z][A-Za-z0-9+.-]*://` and `^[^@/:]+@[^:]+:.+`.
pub(crate) fn is_url_valued_remote(remote: &str) -> bool {
    let has_scheme = {
        let bytes = remote.as_bytes();
        !bytes.is_empty()
            && bytes[0].is_ascii_alphabetic()
            && {
                let mut i = 1;
                while i < bytes.len()
                    && (bytes[i].is_ascii_alphanumeric() || matches!(bytes[i], b'+' | b'.' | b'-'))
                {
                    i += 1;
                }
                remote[i..].starts_with("://")
            }
    };
    if has_scheme {
        return true;
    }
    // scp-like: [^@/:]+ @ [^:]+ : .+
    match remote.find('@') {
        Some(at) if at > 0 && !remote[..at].contains(['/', ':']) => {
            let rest = &remote[at + 1..];
            match rest.find(':') {
                Some(colon) if colon > 0 => !rest[colon + 1..].is_empty(),
                _ => false,
            }
        }
        _ => false,
    }
}

/// Port of `findRemoteNameForUrl`: the configured remote whose `get-url` matches.
pub(crate) fn find_remote_name_for_url<R: GitRunner>(runner: &R, remote_url: &str) -> Option<String> {
    let out = runner.run(&["remote"]).ok()?;
    for remote_name in out.stdout.split(['\r', '\n']).map(str::trim).filter(|l| !l.is_empty()) {
        if let Ok(url_out) = runner.run(&["remote", "get-url", remote_name]) {
            if url_out.stdout.trim() == remote_url {
                return Some(remote_name.to_string());
            }
        }
    }
    None
}

/// Port of `gitRefTargetsBranchOnRemote` (git-remote-branch-name.ts): does a saved
/// base ref point at `<remote>/<branch>` (or a plain `<branch>` on the current remote)?
pub(crate) fn git_ref_targets_branch_on_remote(
    ref_name: Option<&str>,
    remote_name: &str,
    branch_name: &str,
) -> bool {
    let trimmed = ref_name.map(str::trim).unwrap_or("");
    if trimmed.is_empty() || remote_name.is_empty() || branch_name.is_empty() {
        return false;
    }
    if trimmed == format!("{remote_name}/{branch_name}")
        || trimmed == format!("remotes/{remote_name}/{branch_name}")
        || trimmed == format!("refs/remotes/{remote_name}/{branch_name}")
    {
        return true;
    }
    if trimmed.starts_with("refs/remotes/") || trimmed.starts_with("remotes/") {
        return false;
    }
    if let Some(rest) = trimmed.strip_prefix("refs/heads/") {
        return rest == branch_name;
    }
    trimmed == branch_name
}

/// Port of `getConfiguredBranchRemoteUpstream`: an older fork-review worktree can
/// carry a usable `branch.<name>.{remote,merge}` even when git can't resolve
/// `HEAD@{u}` (URL-valued `branch.remote`). Returns the resolved upstream or `None`.
fn get_configured_branch_remote_upstream<R: GitRunner>(
    runner: &R,
    current_branch: &str,
) -> Option<EffectiveGitUpstream> {
    // Fetch all three up front (the TS source does Promise.all), then decide — so the
    // git-call count matches regardless of which value is missing.
    let remote = git_config_value(runner, &format!("branch.{current_branch}.remote"));
    let merge_ref = git_config_value(runner, &format!("branch.{current_branch}.merge"));
    let base_ref = git_config_value(runner, &format!("branch.{current_branch}.base"));
    let remote = remote?;
    let branch_name = merge_ref
        .as_deref()
        .map(|m| m.strip_prefix("refs/heads/").unwrap_or(m).to_string())
        .unwrap_or_default();
    if branch_name.is_empty() || Some(&branch_name) == merge_ref.as_ref() || remote == "." {
        return None;
    }
    let remote_name = if is_url_valued_remote(&remote) {
        find_remote_name_for_url(runner, &remote)?
    } else {
        remote
    };
    if git_ref_targets_branch_on_remote(base_ref.as_deref(), &remote_name, &branch_name)
        || !remote_tracking_ref_exists(runner, &remote_name, &branch_name)
    {
        return None;
    }
    Some(EffectiveGitUpstream {
        upstream_name: format!("{remote_name}/{branch_name}"),
        remote_name: Some(remote_name),
        branch_name,
        is_configured_upstream: false,
    })
}

/// Port of `hasConfiguredBranchPushTarget`: is there a configured remote+branch the
/// current branch can be published to, even without a tracking upstream?
fn has_configured_branch_push_target<R: GitRunner>(runner: &R, current_branch: &str) -> bool {
    let push_remote = git_config_value(runner, &format!("branch.{current_branch}.pushRemote"));
    let push_default = git_config_value(runner, "remote.pushDefault");
    let branch_remote = git_config_value(runner, &format!("branch.{current_branch}.remote"));
    let merge_ref = git_config_value(runner, &format!("branch.{current_branch}.merge"));
    let base_ref = git_config_value(runner, &format!("branch.{current_branch}.base"));
    let Some(remote) = push_remote.or(push_default).or_else(|| branch_remote.clone()) else {
        return false;
    };
    let branch_name = merge_ref
        .as_deref()
        .map(|m| m.strip_prefix("refs/heads/").unwrap_or(m).to_string())
        .unwrap_or_default();
    if remote == "." || branch_name.is_empty() || Some(&branch_name) == merge_ref.as_ref() {
        return false;
    }
    let resolve_name = |value: &str| -> String {
        if is_url_valued_remote(value) {
            find_remote_name_for_url(runner, value).unwrap_or_else(|| value.to_string())
        } else {
            value.to_string()
        }
    };
    let push_remote_name = resolve_name(&remote);
    let branch_remote_name = branch_remote.as_deref().map(resolve_name);
    if git_ref_targets_branch_on_remote(base_ref.as_deref(), &push_remote_name, &branch_name) {
        return false;
    }
    // branch.merge belongs to branch.remote: don't combine a pushDefault fork with an
    // origin/main merge target and call it pushable.
    if branch_name != current_branch
        && (push_remote_name == "origin" || branch_remote_name.as_deref() != Some(push_remote_name.as_str()))
    {
        return false;
    }
    true
}

pub fn resolve_effective_git_upstream<R: GitRunner>(
    runner: &R,
) -> Result<Option<EffectiveGitUpstream>, GitError> {
    let current = current_branch_name(runner);
    resolve_effective_git_upstream_for_branch(runner, current.as_deref())
}

/// Resolve the effective upstream for an already-known current branch — split out
/// so `effective_git_upstream_status` computes the current branch exactly once
/// (matching the TS `resolveEffectiveGitUpstreamForBranch`).
fn resolve_effective_git_upstream_for_branch<R: GitRunner>(
    runner: &R,
    current: Option<&str>,
) -> Result<Option<EffectiveGitUpstream>, GitError> {
    if let Some(mut configured) = configured_upstream(runner)? {
        // Legacy `origin/<base>` upstream whose name has multiple segments: try
        // to re-split against the known remotes so the publish branch wins.
        if let Some(cur) = current {
            if configured.remote_name.as_deref() == Some("origin")
                && configured.branch_name.as_str() != cur
                && has_multiple_slash_segments(&configured.upstream_name)
            {
                if let Some((remote, branch)) = split_by_known_remote(runner, &configured.upstream_name) {
                    configured.remote_name = Some(remote);
                    configured.branch_name = branch;
                }
            }
        }

        match current {
            None => return Ok(Some(configured)),
            Some(cur) if configured.branch_name.as_str() == cur => return Ok(Some(configured)),
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

    if let Some(cur) = current {
        // Git can't resolve HEAD@{u} when branch.<name>.remote is URL-valued, but an
        // older fork-review worktree still carries the usable merge target.
        if let Some(branch_remote_upstream) = get_configured_branch_remote_upstream(runner, cur) {
            return Ok(Some(branch_remote_upstream));
        }
    }

    if let Some(cur) = current {
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
    let current = current_branch_name(runner);
    let Some(upstream) = resolve_effective_git_upstream_for_branch(runner, current.as_deref())? else {
        // No upstream, but the branch may still be publishable to a configured
        // remote — flag it (Some(true)) so the UI keeps the publish action.
        let has_configured_push_target = current
            .as_deref()
            .map(|cur| has_configured_branch_push_target(runner, cur))
            .filter(|&pushable| pushable)
            .map(|_| true);
        return Ok(GitUpstreamStatus {
            has_upstream: false,
            upstream_name: None,
            ahead: 0,
            behind: 0,
            has_configured_push_target,
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
        // The effective-upstream (no explicit target) path doesn't compute this;
        // the pushTarget path owns the "can still publish" flag.
        has_configured_push_target: None,
        behind_commits_are_patch_equivalent,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_url_valued_remote_matches_ts_regex() {
        // scheme URLs
        assert!(is_url_valued_remote("https://github.com/o/r.git"));
        assert!(is_url_valued_remote("ssh://git@host/o/r.git"));
        assert!(is_url_valued_remote("git+ssh://host/x"));
        // scp-like ssh
        assert!(is_url_valued_remote("git@github.com:owner/repo.git"));
        assert!(is_url_valued_remote("user@host:path"));
        // plain remote names are NOT url-valued
        assert!(!is_url_valued_remote("origin"));
        assert!(!is_url_valued_remote("fork"));
        assert!(!is_url_valued_remote("my-remote"));
        // no path after the colon → not scp-like
        assert!(!is_url_valued_remote("user@host:"));
        // slash/colon before @ disqualifies the scp form
        assert!(!is_url_valued_remote("a/b@host:path"));
    }

    #[test]
    fn git_ref_targets_branch_on_remote_matches_ts() {
        assert!(git_ref_targets_branch_on_remote(Some("fork/main"), "fork", "main"));
        assert!(git_ref_targets_branch_on_remote(Some("remotes/fork/main"), "fork", "main"));
        assert!(git_ref_targets_branch_on_remote(Some("refs/remotes/fork/main"), "fork", "main"));
        assert!(git_ref_targets_branch_on_remote(Some("refs/heads/main"), "fork", "main"));
        assert!(git_ref_targets_branch_on_remote(Some("main"), "fork", "main"));
        // a remote-qualified ref on a DIFFERENT remote must not match
        assert!(!git_ref_targets_branch_on_remote(Some("origin/main"), "fork", "main"));
        assert!(!git_ref_targets_branch_on_remote(Some("refs/remotes/origin/main"), "fork", "main"));
        // empties
        assert!(!git_ref_targets_branch_on_remote(None, "fork", "main"));
        assert!(!git_ref_targets_branch_on_remote(Some("  "), "fork", "main"));
    }

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
