//! Remote rebase-source resolution, ported from
//! `src/shared/git-rebase-source.ts`. Maps a base ref (possibly
//! `refs/remotes/...`) to the `remote`/`branch` pair `git pull --rebase` needs,
//! choosing the longest matching configured remote.

use crate::runner::{GitError, GitRunner};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitRemoteRebaseSource {
    pub remote_name: String,
    pub branch_name: String,
    pub display_name: String,
}

fn choose_remote_base() -> GitError {
    GitError::from_message("Choose a remote base branch to rebase from.")
}

fn normalize_base_ref(base_ref: &str) -> Result<String, GitError> {
    let trimmed = base_ref.trim();
    if trimmed.is_empty() || trimmed.starts_with('-') {
        return Err(choose_remote_base());
    }
    if let Some(rest) = trimmed.strip_prefix("refs/remotes/") {
        return Ok(rest.to_string());
    }
    if let Some(rest) = trimmed.strip_prefix("remotes/") {
        return Ok(rest.to_string());
    }
    Ok(trimmed.to_string())
}

pub fn resolve_git_remote_rebase_source<R: GitRunner>(
    runner: &R,
    base_ref: &str,
) -> Result<GitRemoteRebaseSource, GitError> {
    let normalized = normalize_base_ref(base_ref)?;
    let out = runner.run(&["remote"])?;
    let mut remotes: Vec<&str> =
        out.stdout.split('\n').map(str::trim).filter(|l| !l.is_empty()).collect();
    remotes.sort_by_key(|s| std::cmp::Reverse(s.len()));

    let remote_name = remotes
        .iter()
        .copied()
        .find(|remote| normalized != *remote && normalized.starts_with(&format!("{remote}/")));
    let Some(remote_name) = remote_name else {
        return Err(choose_remote_base());
    };

    let branch_name = normalized[remote_name.len() + 1..].to_string();
    runner.run(&["check-ref-format", "--branch", &branch_name])?;

    Ok(GitRemoteRebaseSource {
        remote_name: remote_name.to_string(),
        branch_name: branch_name.clone(),
        display_name: format!("{remote_name}/{branch_name}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::GitOutput;

    fn ok(stdout: &str) -> Result<GitOutput, GitError> {
        Ok(GitOutput { stdout: stdout.to_string(), stderr: String::new() })
    }

    #[test]
    fn rejects_empty_or_flag_like_base_refs() {
        let runner = |_: &[&str]| ok("origin\n");
        assert!(resolve_git_remote_rebase_source(&runner, "   ").is_err());
        assert!(resolve_git_remote_rebase_source(&runner, "-rf").is_err());
    }

    #[test]
    fn rejects_base_refs_without_a_matching_remote() {
        let runner = |_: &[&str]| ok("origin\n");
        assert!(resolve_git_remote_rebase_source(&runner, "local-branch").is_err());
    }

    #[test]
    fn resolves_and_strips_refs_remotes_prefix() {
        let runner = |args: &[&str]| {
            if args[0] == "remote" {
                ok("origin\nupstream\n")
            } else {
                ok("")
            }
        };
        let source = resolve_git_remote_rebase_source(&runner, "refs/remotes/upstream/main").unwrap();
        assert_eq!(
            source,
            GitRemoteRebaseSource {
                remote_name: "upstream".to_string(),
                branch_name: "main".to_string(),
                display_name: "upstream/main".to_string(),
            }
        );
    }
}
