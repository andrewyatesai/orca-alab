//! Push / pull / fast-forward / fetch, ported from `src/main/git/remote.ts`.
//!
//! `gitPullRebaseFromBase` is deferred until `git-rebase-source` is ported. All
//! errors are normalised (credential-scrubbed, tail line) before surfacing,
//! matching the TS IPC-boundary contract.

use crate::effective_upstream::{
    find_remote_name_for_url, find_remote_name_for_url_async, git_config_value,
    git_config_value_async, git_ref_targets_branch_on_remote, is_url_valued_remote,
    resolve_effective_git_upstream,
};
use crate::push_target::{validate_git_push_target, validate_git_push_target_async, GitPushTarget};
use crate::rebase_source::resolve_git_remote_rebase_source;
use crate::runner::{AsyncGitRunner, GitError, GitRunner};
use orca_text::git_remote_error::{normalize_git_error_message, GitRemoteOperation};

fn normalize(error: GitError, op: GitRemoteOperation) -> GitError {
    GitError::from_message(normalize_git_error_message(Some(&error.message), Some(op)))
}

/// The push remote resolved from config, plus the (resolved) `branch.<b>.remote`.
struct ConfiguredPushRemote {
    remote: String,
    branch_remote: Option<String>,
}

/// Resolve a URL-valued remote to its configured name (else return it as-is).
fn normalize_push_remote<R: GitRunner>(runner: &R, remote: &str) -> String {
    if !is_url_valued_remote(remote) {
        return remote.to_string();
    }
    find_remote_name_for_url(runner, remote).unwrap_or_else(|| remote.to_string())
}

/// Async twin of [`normalize_push_remote`].
async fn normalize_push_remote_async<R: AsyncGitRunner>(runner: &R, remote: &str) -> String {
    if !is_url_valued_remote(remote) {
        return remote.to_string();
    }
    find_remote_name_for_url_async(runner, remote).await.unwrap_or_else(|| remote.to_string())
}

/// Port of `getConfiguredPushRemote`: `branch.<b>.pushRemote ?? remote.pushDefault
/// ?? branch.<b>.remote`, with URL-valued remotes resolved to names.
fn get_configured_push_remote<R: GitRunner>(runner: &R, branch: &str) -> Option<ConfiguredPushRemote> {
    let branch_remote = git_config_value(runner, &format!("branch.{branch}.remote"));
    let remote = git_config_value(runner, &format!("branch.{branch}.pushRemote"))
        .or_else(|| git_config_value(runner, "remote.pushDefault"))
        .or_else(|| branch_remote.clone())?;
    Some(ConfiguredPushRemote {
        remote: normalize_push_remote(runner, &remote),
        branch_remote: branch_remote.as_deref().map(|br| normalize_push_remote(runner, br)),
    })
}

/// Async twin of [`get_configured_push_remote`]. Preserves the sync call order +
/// laziness: `branch.<b>.remote`, then `branch.<b>.pushRemote`, then
/// `remote.pushDefault` ONLY when pushRemote is absent.
async fn get_configured_push_remote_async<R: AsyncGitRunner>(
    runner: &R,
    branch: &str,
) -> Option<ConfiguredPushRemote> {
    let branch_remote = git_config_value_async(runner, &format!("branch.{branch}.remote")).await;
    let mut remote = git_config_value_async(runner, &format!("branch.{branch}.pushRemote")).await;
    if remote.is_none() {
        remote = git_config_value_async(runner, "remote.pushDefault").await;
    }
    let remote = remote.or_else(|| branch_remote.clone())?;
    // Normalise `remote` BEFORE `branch_remote` to match the sync struct-literal
    // evaluation order (both may issue `git remote` + `get-url` when URL-valued).
    let remote_name = normalize_push_remote_async(runner, &remote).await;
    let branch_remote_name = match branch_remote.as_deref() {
        Some(br) => Some(normalize_push_remote_async(runner, br).await),
        None => None,
    };
    Some(ConfiguredPushRemote { remote: remote_name, branch_remote: branch_remote_name })
}

/// Port of `branchMergeTargetsConfiguredBase`: does `branch.<b>.base` point at the
/// merge branch on the push remote (a fork-review base pinned to origin)?
fn branch_merge_targets_configured_base<R: GitRunner>(
    runner: &R,
    branch: &str,
    remote: &str,
    branch_ref: &str,
) -> bool {
    git_ref_targets_branch_on_remote(
        git_config_value(runner, &format!("branch.{branch}.base")).as_deref(),
        remote,
        branch_ref,
    )
}

/// Async twin of [`branch_merge_targets_configured_base`].
async fn branch_merge_targets_configured_base_async<R: AsyncGitRunner>(
    runner: &R,
    branch: &str,
    remote: &str,
    branch_ref: &str,
) -> bool {
    git_ref_targets_branch_on_remote(
        git_config_value_async(runner, &format!("branch.{branch}.base")).await.as_deref(),
        remote,
        branch_ref,
    )
}

/// Port of `canPushConfiguredMergeBranch`: `branch.merge` belongs to `branch.remote`,
/// so a `pushDefault` fork must not inherit an `origin/main` merge target.
fn can_push_configured_merge_branch(
    push_remote: &ConfiguredPushRemote,
    branch: &str,
    branch_ref: &str,
) -> bool {
    if branch_ref == branch {
        return true;
    }
    push_remote.remote != "origin"
        && push_remote.branch_remote.as_deref() == Some(push_remote.remote.as_str())
}

/// The branch's configured push target (`remote`, `HEAD:<ref>`), or `None` if
/// not configured / not safe to infer. Swallows all git errors (→ `None`).
/// Ported faithfully from `getConfiguredPushTarget` (remote.ts): the push remote is
/// pushRemote/pushDefault/branch.remote (URL-resolved), and a merge branch pointing
/// at the configured base or a mismatched fork is rejected.
fn get_configured_push_target<R: GitRunner>(runner: &R) -> Option<(String, String)> {
    let branch = runner.run(&["symbolic-ref", "--quiet", "--short", "HEAD"]).ok()?.stdout.trim().to_string();
    if branch.is_empty() {
        return None;
    }
    let push_remote = get_configured_push_remote(runner, &branch)?;
    let merge_ref = runner.run(&["config", "--get", &format!("branch.{branch}.merge")]).ok()?.stdout.trim().to_string();
    let branch_ref = merge_ref.strip_prefix("refs/heads/").unwrap_or(&merge_ref).to_string();
    let remote = push_remote.remote.clone();
    if branch_ref.is_empty() || remote == "." || branch_ref == merge_ref {
        return None;
    }
    if branch_merge_targets_configured_base(runner, &branch, &remote, &branch_ref) {
        return None;
    }
    if !can_push_configured_merge_branch(&push_remote, &branch, &branch_ref) {
        return None;
    }
    Some((remote, format!("HEAD:{branch_ref}")))
}

/// Async twin of [`get_configured_push_target`]: same call sequence
/// (`symbolic-ref` → push-remote config → `branch.<b>.merge` → base guard), same
/// fork/merge-base rejections, awaited. Swallows git errors to `None`.
async fn get_configured_push_target_async<R: AsyncGitRunner>(
    runner: &R,
) -> Option<(String, String)> {
    let branch = runner
        .run(&["symbolic-ref", "--quiet", "--short", "HEAD"], None)
        .await
        .ok()?
        .stdout
        .trim()
        .to_string();
    if branch.is_empty() {
        return None;
    }
    let push_remote = get_configured_push_remote_async(runner, &branch).await?;
    let merge_ref = runner
        .run(&["config", "--get", &format!("branch.{branch}.merge")], None)
        .await
        .ok()?
        .stdout
        .trim()
        .to_string();
    let branch_ref = merge_ref.strip_prefix("refs/heads/").unwrap_or(&merge_ref).to_string();
    let remote = push_remote.remote.clone();
    if branch_ref.is_empty() || remote == "." || branch_ref == merge_ref {
        return None;
    }
    if branch_merge_targets_configured_base_async(runner, &branch, &remote, &branch_ref).await {
        return None;
    }
    if !can_push_configured_merge_branch(&push_remote, &branch, &branch_ref) {
        return None;
    }
    Some((remote, format!("HEAD:{branch_ref}")))
}

fn explicit_push_target(target: &GitPushTarget) -> (String, String) {
    (target.remote_name.clone(), format!("HEAD:{}", target.branch_name))
}

/// Build `git push [--force-with-lease] --set-upstream <remote> <refspec>`, falling
/// back to first-publish `origin HEAD` when no target resolves. Pure, so the sync
/// + async pushers emit byte-identical argv.
fn build_push_args(target: &Option<(String, String)>, force_with_lease: bool) -> Vec<String> {
    let mut args: Vec<String> = vec!["push".to_string()];
    if force_with_lease {
        args.push("--force-with-lease".to_string());
    }
    args.push("--set-upstream".to_string());
    match target {
        Some((remote, refspec)) => {
            args.push(remote.clone());
            args.push(refspec.clone());
        }
        None => {
            args.push("origin".to_string());
            args.push("HEAD".to_string());
        }
    }
    args
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
        let args = build_push_args(&target, force_with_lease);
        let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
        runner.run(&arg_refs).map(|_| ())
    };
    inner().map_err(|e| normalize(e, GitRemoteOperation::Push))
}

/// Async twin of [`git_push`] for the wasm relay: validate an explicit target,
/// resolve the refspec (explicit; else the branch's configured push remote; else
/// first-publish `origin HEAD`), then run the mutating push — the SAME resolution
/// the sync path runs, awaited. Errors normalise identically.
pub async fn git_push_async<R: AsyncGitRunner>(
    runner: &R,
    push_target: Option<&GitPushTarget>,
    force_with_lease: bool,
) -> Result<(), GitError> {
    let result = git_push_inner_async(runner, push_target, force_with_lease).await;
    result.map_err(|e| normalize(e, GitRemoteOperation::Push))
}

async fn git_push_inner_async<R: AsyncGitRunner>(
    runner: &R,
    push_target: Option<&GitPushTarget>,
    force_with_lease: bool,
) -> Result<(), GitError> {
    if let Some(target) = push_target {
        validate_git_push_target_async(runner, target).await?;
    }
    let target = match push_target {
        Some(t) => Some(explicit_push_target(t)),
        None => get_configured_push_target_async(runner).await,
    };
    let args = build_push_args(&target, force_with_lease);
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    runner.run(&arg_refs, None).await.map(|_| ())
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

/// Async twin of [`git_fetch`] for the wasm relay: validate an explicit target
/// (`check-ref-format`) then `fetch --prune [<remote>]`, awaited. No
/// effective-upstream resolution, so — unlike pull/fast-forward — it needs no
/// async upstream resolver. Errors normalise identically.
pub async fn git_fetch_async<R: AsyncGitRunner>(
    runner: &R,
    push_target: Option<&GitPushTarget>,
) -> Result<(), GitError> {
    let result = git_fetch_inner_async(runner, push_target).await;
    result.map_err(|e| normalize(e, GitRemoteOperation::Fetch))
}

async fn git_fetch_inner_async<R: AsyncGitRunner>(
    runner: &R,
    push_target: Option<&GitPushTarget>,
) -> Result<(), GitError> {
    if let Some(target) = push_target {
        validate_git_push_target_async(runner, target).await?;
        return runner.run(&["fetch", "--prune", &target.remote_name], None).await.map(|_| ());
    }
    runner.run(&["fetch", "--prune"], None).await.map(|_| ())
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
    // The same queued runner as an AsyncGitRunner, so the async push twin is driven
    // by the identical response scripts (it never suspends — the queue is ready).
    impl AsyncGitRunner for SeqRunner {
        async fn run(&self, args: &[&str], _stdin: Option<&str>) -> Result<GitOutput, GitError> {
            self.calls.borrow_mut().push(args.iter().map(|s| s.to_string()).collect());
            self.queue.borrow_mut().pop_front().expect("unexpected extra git call")
        }
    }
    /// Poll an immediately-ready future (the async SeqRunner never pends). No
    /// executor dep, keeps the crate's `forbid(unsafe_code)` intact.
    fn block_on_ready<F: std::future::Future>(fut: F) -> F::Output {
        use std::pin::pin;
        use std::task::{Context, Poll, Waker};
        let mut cx = Context::from_waker(Waker::noop());
        let mut fut = pin!(fut);
        match fut.as_mut().poll(&mut cx) {
            Poll::Ready(value) => value,
            Poll::Pending => panic!("mock future should be immediately ready"),
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
            ok("review/pr-1738\n"),                               // symbolic-ref
            ok("pr-prateek-orca\n"),                              // branch.<b>.remote
            err("no pushRemote"),                                 // branch.<b>.pushRemote
            err("no pushDefault"),                                // remote.pushDefault
            ok("refs/heads/prateek/fix-sidebar-agents-toggle\n"), // branch.<b>.merge
            err("no base"),                                       // branch.<b>.base
            ok(""),                                               // push
        ]);
        git_push(&r, None, false).unwrap();
        assert_eq!(
            r.calls(),
            vec![
                vec!["symbolic-ref", "--quiet", "--short", "HEAD"],
                vec!["config", "--get", "branch.review/pr-1738.remote"],
                vec!["config", "--get", "branch.review/pr-1738.pushRemote"],
                vec!["config", "--get", "remote.pushDefault"],
                vec!["config", "--get", "branch.review/pr-1738.merge"],
                vec!["config", "--get", "branch.review/pr-1738.base"],
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
        let r = SeqRunner::new(vec![
            ok("feature\n"),            // symbolic-ref
            ok("origin\n"),             // branch.feature.remote
            err("no pushRemote"),       // branch.feature.pushRemote
            err("no pushDefault"),      // remote.pushDefault
            ok("refs/heads/feature\n"), // branch.feature.merge
            err("no base"),             // branch.feature.base
            ok(""),                     // push
        ]);
        git_push(&r, None, true).unwrap();
        assert_eq!(
            r.calls().last().unwrap(),
            &vec!["push", "--force-with-lease", "--set-upstream", "origin", "HEAD:feature"]
        );
    }

    #[test]
    fn pushes_to_configured_push_remote_over_branch_remote() {
        // branch.pushRemote overrides branch.remote — the incomplete port ignored it
        // and would have pushed to origin instead of the fork.
        let r = SeqRunner::new(vec![
            ok("feature\n"),            // symbolic-ref
            ok("origin\n"),             // branch.feature.remote
            ok("myfork\n"),             // branch.feature.pushRemote  <- wins
            ok("refs/heads/feature\n"), // branch.feature.merge
            err("no base"),             // branch.feature.base
            ok(""),                     // push
        ]);
        git_push(&r, None, false).unwrap();
        assert_eq!(
            r.calls().last().unwrap(),
            &vec!["push", "--set-upstream", "myfork", "HEAD:feature"]
        );
    }

    #[test]
    fn skips_push_target_when_merge_targets_the_configured_base() {
        // branch.base points at origin/main and merge is main -> a fork-review base
        // pinned to origin -> don't infer a push target; fall back to origin HEAD.
        let r = SeqRunner::new(vec![
            ok("review/x\n"),        // symbolic-ref
            ok("origin\n"),          // branch.review/x.remote
            err("no pushRemote"),    // branch.review/x.pushRemote
            err("no pushDefault"),   // remote.pushDefault
            ok("refs/heads/main\n"), // branch.review/x.merge -> branchRef=main
            ok("origin/main\n"),     // branch.review/x.base -> targets origin/main
            ok(""),                  // push (origin HEAD)
        ]);
        git_push(&r, None, false).unwrap();
        assert_eq!(
            r.calls().last().unwrap(),
            &vec!["push", "--set-upstream", "origin", "HEAD"]
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
    fn async_push_matches_sync_for_configured_push_remote_over_branch_remote() {
        // branch.pushRemote overrides branch.remote — the async twin must resolve the
        // same configured target and emit the same argv + call order as the sync path.
        let responses = || {
            vec![
                ok("feature\n"),            // symbolic-ref
                ok("origin\n"),             // branch.feature.remote
                ok("myfork\n"),             // branch.feature.pushRemote <- wins
                ok("refs/heads/feature\n"), // branch.feature.merge
                err("no base"),             // branch.feature.base
                ok(""),                     // push
            ]
        };
        let sync_runner = SeqRunner::new(responses());
        git_push(&sync_runner, None, false).unwrap();

        let async_runner = SeqRunner::new(responses());
        block_on_ready(git_push_async(&async_runner, None, false)).unwrap();

        assert_eq!(async_runner.calls(), sync_runner.calls());
        assert_eq!(
            async_runner.calls().last().unwrap(),
            &vec!["push", "--set-upstream", "myfork", "HEAD:feature"]
        );
    }

    #[test]
    fn async_push_uses_explicit_target_and_force_with_lease() {
        let r = SeqRunner::new(vec![ok(""), ok("")]);
        block_on_ready(git_push_async(&r, Some(&target("origin", "contributor/fix")), true)).unwrap();
        assert_eq!(
            r.calls(),
            vec![
                vec!["check-ref-format", "--branch", "contributor/fix"],
                vec!["push", "--force-with-lease", "--set-upstream", "origin", "HEAD:contributor/fix"],
            ]
        );
    }

    #[test]
    fn async_push_falls_back_to_origin_head_and_normalizes_errors() {
        let r = SeqRunner::new(vec![err("no branch"), err("remote rejected: non-fast-forward")]);
        let error = block_on_ready(git_push_async(&r, None, false)).unwrap_err();
        assert_eq!(
            error.message,
            "Push rejected: remote has newer commits (non-fast-forward). Please pull or sync first."
        );
        assert_eq!(
            r.calls().last().unwrap(),
            &vec!["push", "--set-upstream", "origin", "HEAD"]
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

    #[test]
    fn async_fetch_matches_sync_for_both_target_shapes() {
        // The relay drives the SAME fetch through the async twin; prove identical
        // argv + call order for the no-target and explicit-target shapes.
        let no_target = SeqRunner::new(vec![ok("")]);
        block_on_ready(git_fetch_async(&no_target, None)).unwrap();
        assert_eq!(no_target.calls(), vec![vec!["fetch", "--prune"]]);

        let explicit = SeqRunner::new(vec![ok(""), ok("")]);
        block_on_ready(git_fetch_async(&explicit, Some(&target("fork", "feature/fix")))).unwrap();
        assert_eq!(
            explicit.calls(),
            vec![
                vec!["check-ref-format", "--branch", "feature/fix"],
                vec!["fetch", "--prune", "fork"],
            ]
        );
    }
}
