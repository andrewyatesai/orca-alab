//! Remote rebase-source resolution, ported from
//! `src/shared/git-rebase-source.ts`. Maps a base ref (possibly
//! `refs/remotes/...`) to the `remote`/`branch` pair `git pull --rebase` needs,
//! choosing the longest matching configured remote.

use crate::runner::{AsyncGitRunner, GitError, GitRunner};

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

/// Pure: pick the longest configured remote that prefixes the normalized base
/// ref and split off its branch. Shared by the sync + async resolvers so the two
/// are byte-identical — only the git-call sequencing differs between them.
fn choose_rebase_remote(
    remote_stdout: &str,
    normalized: &str,
) -> Result<(String, String), GitError> {
    let mut remotes: Vec<&str> =
        remote_stdout.split('\n').map(str::trim).filter(|l| !l.is_empty()).collect();
    remotes.sort_by_key(|s| std::cmp::Reverse(s.len()));

    let remote_name = remotes
        .iter()
        .copied()
        .find(|remote| normalized != *remote && normalized.starts_with(&format!("{remote}/")))
        .ok_or_else(choose_remote_base)?;
    let branch_name = normalized[remote_name.len() + 1..].to_string();
    Ok((remote_name.to_string(), branch_name))
}

fn rebase_source(remote_name: String, branch_name: String) -> GitRemoteRebaseSource {
    let display_name = format!("{remote_name}/{branch_name}");
    GitRemoteRebaseSource { remote_name, branch_name, display_name }
}

pub fn resolve_git_remote_rebase_source<R: GitRunner>(
    runner: &R,
    base_ref: &str,
) -> Result<GitRemoteRebaseSource, GitError> {
    let normalized = normalize_base_ref(base_ref)?;
    let out = runner.run(&["remote"])?;
    let (remote_name, branch_name) = choose_rebase_remote(&out.stdout, &normalized)?;
    runner.run(&["check-ref-format", "--branch", &branch_name])?;
    Ok(rebase_source(remote_name, branch_name))
}

/// Async twin of [`resolve_git_remote_rebase_source`] for the wasm relay: same
/// pure decision (`choose_rebase_remote`), same two git calls, awaited through
/// an [`AsyncGitRunner`] instead of a blocking one.
pub async fn resolve_git_remote_rebase_source_async<R: AsyncGitRunner>(
    runner: &R,
    base_ref: &str,
) -> Result<GitRemoteRebaseSource, GitError> {
    let normalized = normalize_base_ref(base_ref)?;
    let out = runner.run(&["remote"], None).await?;
    let (remote_name, branch_name) = choose_rebase_remote(&out.stdout, &normalized)?;
    runner.run(&["check-ref-format", "--branch", &branch_name], None).await?;
    Ok(rebase_source(remote_name, branch_name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::GitOutput;
    use std::future::Future;

    fn ok(stdout: &str) -> Result<GitOutput, GitError> {
        Ok(GitOutput { stdout: stdout.to_string(), stderr: String::new() })
    }

    /// Drive an immediately-ready future to completion without an executor dep —
    /// the mock async runner below never actually suspends, so a no-op waker is
    /// enough (and keeps this crate's `forbid(unsafe_code)` intact).
    fn block_on_ready<F: Future>(fut: F) -> F::Output {
        use std::pin::pin;
        use std::task::{Context, Poll, Waker};
        let mut cx = Context::from_waker(Waker::noop());
        let mut fut = pin!(fut);
        match fut.as_mut().poll(&mut cx) {
            Poll::Ready(value) => value,
            Poll::Pending => panic!("mock future should be immediately ready"),
        }
    }

    /// A mock [`AsyncGitRunner`] backed by a sync closure (resolves instantly).
    struct MockAsyncRunner<F>(F);
    impl<F: Fn(&[&str]) -> Result<GitOutput, GitError>> AsyncGitRunner for MockAsyncRunner<F> {
        async fn run(&self, args: &[&str], _stdin: Option<&str>) -> Result<GitOutput, GitError> {
            (self.0)(args)
        }
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

    #[test]
    fn async_resolver_matches_the_sync_resolver() {
        // The relay drives the SAME decision through the async trait; prove the two
        // paths agree on both the happy path and the "no matching remote" error.
        let runner = MockAsyncRunner(|args: &[&str]| {
            if args[0] == "remote" {
                ok("origin\nupstream\n")
            } else {
                ok("")
            }
        });
        let source = block_on_ready(resolve_git_remote_rebase_source_async(
            &runner,
            "refs/remotes/upstream/main",
        ))
        .unwrap();
        assert_eq!(
            source,
            GitRemoteRebaseSource {
                remote_name: "upstream".to_string(),
                branch_name: "main".to_string(),
                display_name: "upstream/main".to_string(),
            }
        );

        let no_match = MockAsyncRunner(|_: &[&str]| ok("origin\n"));
        assert!(block_on_ready(resolve_git_remote_rebase_source_async(&no_match, "local-branch"))
            .is_err());
    }
}
