//! Push / pull / fast-forward / fetch, ported from `src/main/git/remote.ts`.
//!
//! `gitPullRebaseFromBase` is deferred until `git-rebase-source` is ported. All
//! errors are normalised (credential-scrubbed, tail line) before surfacing,
//! matching the TS IPC-boundary contract.

use crate::effective_upstream::resolve_effective_git_upstream;
use crate::push_target::{validate_git_push_target, GitPushTarget};
use crate::rebase_source::resolve_git_remote_rebase_source;
use crate::runner::{GitError, GitRunner};
use orca_text::git_remote_error::{normalize_git_error_message, GitRemoteOperation};

fn normalize(error: GitError, op: GitRemoteOperation) -> GitError {
    GitError::from_message(normalize_git_error_message(Some(&error.message), Some(op)))
}

/// The branch's configured push target (`remote`, `HEAD:<ref>`), or `None` if
/// not configured / not safe to infer. Swallows all git errors (→ `None`).
fn get_configured_push_target<R: GitRunner>(runner: &R) -> Option<(String, String)> {
    let branch = runner.run(&["symbolic-ref", "--quiet", "--short", "HEAD"]).ok()?.stdout.trim().to_string();
    if branch.is_empty() {
        return None;
    }
    let remote = runner.run(&["config", "--get", &format!("branch.{branch}.remote")]).ok()?.stdout.trim().to_string();
    let merge_ref = runner.run(&["config", "--get", &format!("branch.{branch}.merge")]).ok()?.stdout.trim().to_string();
    let branch_ref = merge_ref.strip_prefix("refs/heads/").unwrap_or(&merge_ref).to_string();
    if remote.is_empty() || branch_ref.is_empty() || remote == "." || branch_ref == merge_ref {
        return None;
    }
    if remote == "origin" && branch_ref != branch {
        return None;
    }
    Some((remote, format!("HEAD:{branch_ref}")))
}

fn explicit_push_target(target: &GitPushTarget) -> (String, String) {
    (target.remote_name.clone(), format!("HEAD:{}", target.branch_name))
}

pub fn git_push<R: GitRunner>(
    runner: &R,
    push_target: Option<&GitPushTarget>,
    force_with_lease: bool,
) -> Result<(), GitError> {
    let inner = || -> Result<(), GitError> {
        if let Some(target) = push_target {
            validate_git_push_target(runner, target)?;
        }
        let target = match push_target {
            Some(t) => Some(explicit_push_target(t)),
            None => get_configured_push_target(runner),
        };
        let mut args: Vec<String> = vec!["push".to_string()];
        if force_with_lease {
            args.push("--force-with-lease".to_string());
        }
        args.push("--set-upstream".to_string());
        match &target {
            Some((remote, refspec)) => {
                args.push(remote.clone());
                args.push(refspec.clone());
            }
            None => {
                args.push("origin".to_string());
                args.push("HEAD".to_string());
            }
        }
        let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
        runner.run(&arg_refs).map(|_| ())
    };
    inner().map_err(|e| normalize(e, GitRemoteOperation::Push))
}

fn git_pull_with_args<R: GitRunner>(
    runner: &R,
    pull_args: &[&str],
    push_target: Option<&GitPushTarget>,
) -> Result<(), GitError> {
    let inner = || -> Result<(), GitError> {
        if let Some(target) = push_target {
            validate_git_push_target(runner, target)?;
            let mut args = vec!["pull"];
            args.extend_from_slice(pull_args);
            args.push(&target.remote_name);
            args.push(&target.branch_name);
            return runner.run(&args).map(|_| ());
        }
        if let Some(upstream) = resolve_effective_git_upstream(runner)? {
            if !upstream.is_configured_upstream {
                // Legacy worktrees track origin/<base> while pushing
                // origin/<branch>; pull the effective branch the UI reports.
                let remote = upstream.remote_name.as_deref().unwrap_or("origin");
                let mut args = vec!["pull"];
                args.extend_from_slice(pull_args);
                args.push(remote);
                args.push(&upstream.branch_name);
                return runner.run(&args).map(|_| ());
            }
        }
        let mut args = vec!["pull"];
        args.extend_from_slice(pull_args);
        runner.run(&args).map(|_| ())
    };
    inner().map_err(|e| normalize(e, GitRemoteOperation::Pull))
}

pub fn git_pull<R: GitRunner>(runner: &R, push_target: Option<&GitPushTarget>) -> Result<(), GitError> {
    git_pull_with_args(runner, &[], push_target)
}

pub fn git_fast_forward<R: GitRunner>(runner: &R, push_target: Option<&GitPushTarget>) -> Result<(), GitError> {
    git_pull_with_args(runner, &["--ff-only"], push_target)
}

pub fn git_pull_rebase_from_base<R: GitRunner>(runner: &R, base_ref: &str) -> Result<(), GitError> {
    let inner = || -> Result<(), GitError> {
        let source = resolve_git_remote_rebase_source(runner, base_ref)?;
        runner.run(&["pull", "--rebase", &source.remote_name, &source.branch_name]).map(|_| ())
    };
    inner().map_err(|e| normalize(e, GitRemoteOperation::Pull))
}

pub fn git_fetch<R: GitRunner>(runner: &R, push_target: Option<&GitPushTarget>) -> Result<(), GitError> {
    let inner = || -> Result<(), GitError> {
        if let Some(target) = push_target {
            validate_git_push_target(runner, target)?;
            return runner.run(&["fetch", "--prune", &target.remote_name]).map(|_| ());
        }
        runner.run(&["fetch", "--prune"]).map(|_| ())
    };
    inner().map_err(|e| normalize(e, GitRemoteOperation::Fetch))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::GitOutput;
    use std::cell::RefCell;
    use std::collections::VecDeque;

    struct SeqRunner {
        queue: RefCell<VecDeque<Result<GitOutput, GitError>>>,
        calls: RefCell<Vec<Vec<String>>>,
    }
    impl SeqRunner {
        fn new(responses: Vec<Result<GitOutput, GitError>>) -> Self {
            Self { queue: RefCell::new(responses.into()), calls: RefCell::new(Vec::new()) }
        }
        fn calls(&self) -> Vec<Vec<String>> {
            self.calls.borrow().clone()
        }
    }
    impl GitRunner for SeqRunner {
        fn run(&self, args: &[&str]) -> Result<GitOutput, GitError> {
            self.calls.borrow_mut().push(args.iter().map(|s| s.to_string()).collect());
            self.queue.borrow_mut().pop_front().expect("unexpected extra git call")
        }
    }
    fn ok(stdout: &str) -> Result<GitOutput, GitError> {
        Ok(GitOutput { stdout: stdout.to_string(), stderr: String::new() })
    }
    fn err(message: &str) -> Result<GitOutput, GitError> {
        Err(GitError::from_message(message))
    }
    fn target(remote: &str, branch: &str) -> GitPushTarget {
        GitPushTarget { remote_name: remote.to_string(), branch_name: branch.to_string(), remote_url: None }
    }

    #[test]
    fn pushes_to_origin_when_no_upstream_configured() {
        let r = SeqRunner::new(vec![err("no branch"), ok("")]);
        git_push(&r, None, false).unwrap();
        assert_eq!(r.calls().last().unwrap(), &vec!["push", "--set-upstream", "origin", "HEAD"]);
    }

    #[test]
    fn pushes_to_configured_upstream_remote_and_branch() {
        let r = SeqRunner::new(vec![
            ok("review/pr-1738\n"),
            ok("pr-prateek-orca\n"),
            ok("refs/heads/prateek/fix-sidebar-agents-toggle\n"),
            ok(""),
        ]);
        git_push(&r, None, false).unwrap();
        assert_eq!(
            r.calls(),
            vec![
                vec!["symbolic-ref", "--quiet", "--short", "HEAD"],
                vec!["config", "--get", "branch.review/pr-1738.remote"],
                vec!["config", "--get", "branch.review/pr-1738.merge"],
                vec!["push", "--set-upstream", "pr-prateek-orca", "HEAD:prateek/fix-sidebar-agents-toggle"],
            ]
        );
    }

    #[test]
    fn uses_explicit_push_target_differing_from_local_branch() {
        let r = SeqRunner::new(vec![ok(""), ok("")]);
        git_push(&r, Some(&target("origin", "contributor/fix-sidebar")), false).unwrap();
        assert_eq!(
            r.calls(),
            vec![
                vec!["check-ref-format", "--branch", "contributor/fix-sidebar"],
                vec!["push", "--set-upstream", "origin", "HEAD:contributor/fix-sidebar"],
            ]
        );
    }

    #[test]
    fn passes_force_with_lease() {
        let r = SeqRunner::new(vec![ok("feature\n"), ok("origin\n"), ok("refs/heads/feature\n"), ok("")]);
        git_push(&r, None, true).unwrap();
        assert_eq!(
            r.calls().last().unwrap(),
            &vec!["push", "--force-with-lease", "--set-upstream", "origin", "HEAD:feature"]
        );
    }

    #[test]
    fn maps_non_fast_forward_push_failures() {
        let r = SeqRunner::new(vec![err("no branch"), err("remote rejected: non-fast-forward")]);
        let error = git_push(&r, None, false).unwrap_err();
        assert_eq!(
            error.message,
            "Push rejected: remote has newer commits (non-fast-forward). Please pull or sync first."
        );
    }

    #[test]
    fn passes_through_clean_tail_line_for_unknown_push_errors() {
        let r = SeqRunner::new(vec![err("no branch"), err("Command failed: git push\nfatal: something obscure happened")]);
        let error = git_push(&r, None, false).unwrap_err();
        assert_eq!(error.message, "fatal: something obscure happened");
    }

    #[test]
    fn strips_embedded_credentials_from_push_errors() {
        let r = SeqRunner::new(vec![
            err("no branch"),
            err("Command failed: git push\nhttps://x-access-token:ghp_abc@github.com/foo/bar.git\nfatal: remote error"),
        ]);
        let error = git_push(&r, None, false).unwrap_err();
        assert!(!error.message.contains("ghp_abc"), "{}", error.message);
        assert!(!error.message.contains("x-access-token"), "{}", error.message);
    }

    #[test]
    fn pull_uses_configured_strategy() {
        let r = SeqRunner::new(vec![ok("feature\n"), ok("origin/feature\n"), ok("")]);
        git_pull(&r, None).unwrap();
        assert_eq!(
            r.calls(),
            vec![
                vec!["symbolic-ref", "--quiet", "--short", "HEAD"],
                vec!["rev-parse", "--abbrev-ref", "HEAD@{u}"],
                vec!["pull"],
            ]
        );
    }

    #[test]
    fn pull_uses_same_name_origin_for_legacy_worktrees() {
        let r = SeqRunner::new(vec![ok("feature\n"), ok("origin/main\n"), ok("abc123\n"), ok("")]);
        git_pull(&r, None).unwrap();
        assert_eq!(
            r.calls(),
            vec![
                vec!["symbolic-ref", "--quiet", "--short", "HEAD"],
                vec!["rev-parse", "--abbrev-ref", "HEAD@{u}"],
                vec!["rev-parse", "--verify", "--quiet", "refs/remotes/origin/feature"],
                vec!["pull", "origin", "feature"],
            ]
        );
    }

    #[test]
    fn pull_uses_explicit_publish_target() {
        let r = SeqRunner::new(vec![ok(""), ok("")]);
        git_pull(&r, Some(&target("fork", "feature/fix"))).unwrap();
        assert_eq!(
            r.calls(),
            vec![
                vec!["check-ref-format", "--branch", "feature/fix"],
                vec!["pull", "fork", "feature/fix"],
            ]
        );
    }

    #[test]
    fn fast_forward_uses_ff_only() {
        let r = SeqRunner::new(vec![ok("feature\n"), ok("origin/feature\n"), ok("")]);
        git_fast_forward(&r, None).unwrap();
        assert_eq!(r.calls().last().unwrap(), &vec!["pull", "--ff-only"]);
    }

    #[test]
    fn fast_forward_from_explicit_publish_target() {
        let r = SeqRunner::new(vec![ok(""), ok("")]);
        git_fast_forward(&r, Some(&target("fork", "feature/fix"))).unwrap();
        assert_eq!(
            r.calls(),
            vec![
                vec!["check-ref-format", "--branch", "feature/fix"],
                vec!["pull", "--ff-only", "fork", "feature/fix"],
            ]
        );
    }

    #[test]
    fn rebases_from_selected_remote_base_ref() {
        let r = SeqRunner::new(vec![ok("origin\nupstream\n"), ok(""), ok("")]);
        git_pull_rebase_from_base(&r, "upstream/main").unwrap();
        assert_eq!(
            r.calls(),
            vec![
                vec!["remote"],
                vec!["check-ref-format", "--branch", "main"],
                vec!["pull", "--rebase", "upstream", "main"],
            ]
        );
    }

    #[test]
    fn rebase_uses_longest_configured_remote_name() {
        let r = SeqRunner::new(vec![ok("fork\nfork/team\n"), ok(""), ok("")]);
        git_pull_rebase_from_base(&r, "fork/team/feature/base").unwrap();
        assert_eq!(
            r.calls().last().unwrap(),
            &vec!["pull", "--rebase", "fork/team", "feature/base"]
        );
    }

    #[test]
    fn fetch_prunes_and_supports_explicit_target() {
        let r = SeqRunner::new(vec![ok("")]);
        git_fetch(&r, None).unwrap();
        assert_eq!(r.calls(), vec![vec!["fetch", "--prune"]]);

        let r = SeqRunner::new(vec![ok(""), ok("")]);
        git_fetch(&r, Some(&target("fork", "feature/fix"))).unwrap();
        assert_eq!(
            r.calls(),
            vec![
                vec!["check-ref-format", "--branch", "feature/fix"],
                vec!["fetch", "--prune", "fork"],
            ]
        );
    }
}
