//! Upstream/ahead-behind status, ported from `src/main/git/upstream.ts`.
//!
//! Composes effective-upstream resolution (or an explicit publish target),
//! patch-equivalence probing, and the no-upstream/error normalisation policy.

use crate::effective_upstream::effective_git_upstream_status;
use crate::publish_target_status::{get_publish_target_status, get_publish_target_status_async};
use crate::push_target::{validate_git_push_target, validate_git_push_target_async, GitPushTarget};
use crate::runner::{AsyncGitRunner, GitError, GitRunner};
use orca_core::git_upstream_status::{upstream_only_commits_are_patch_equivalent, GitUpstreamStatus};
use orca_text::git_remote_error::{
    is_no_upstream_error, normalize_git_error_message, GitRemoteOperation,
};

/// Probe whether the upstream-only commits are patch-equivalent (rebased
/// copies). On any failure, stay conservative (`false`) — it's an optimisation.
fn behind_commits_are_patch_equivalent<R: GitRunner>(runner: &R, upstream_name: &str) -> bool {
    match runner.run(&[
        "log",
        "--oneline",
        "--cherry-mark",
        "--right-only",
        &format!("HEAD...{upstream_name}"),
        "--",
    ]) {
        Ok(out) => upstream_only_commits_are_patch_equivalent(&out.stdout),
        Err(_) => false,
    }
}

pub fn get_upstream_status<R: GitRunner>(
    runner: &R,
    push_target: Option<&GitPushTarget>,
) -> Result<GitUpstreamStatus, GitError> {
    let behind = |name: &str| behind_commits_are_patch_equivalent(runner, name);
    let behind_ref: &dyn Fn(&str) -> bool = &behind;

    let result = match push_target {
        Some(target) => validate_git_push_target(runner, target)
            .and_then(|()| get_publish_target_status(runner, target, Some(behind_ref))),
        None => effective_git_upstream_status(runner, Some(behind_ref)),
    };

    finalize_upstream_status(result)
}

/// Async twin of [`get_upstream_status`] for the EXPLICIT publish-target path
/// (the relay's `upstreamStatus` with a `pushTarget`): validate the target
/// (`check-ref-format`), resolve its publish-target status, then apply the SAME
/// no-upstream swallow + error normalization. The effective (no-target) path
/// stays sync-only for now — its async twin lands with a later milestone.
pub async fn get_publish_target_upstream_status_async<R: AsyncGitRunner>(
    runner: &R,
    target: &GitPushTarget,
) -> Result<GitUpstreamStatus, GitError> {
    let result = async {
        validate_git_push_target_async(runner, target).await?;
        get_publish_target_status_async(runner, target).await
    }
    .await;

    finalize_upstream_status(result)
}

/// The shared no-upstream/normalize policy tail, run identically by the sync and
/// async resolvers so their error handling can't drift.
fn finalize_upstream_status(
    result: Result<GitUpstreamStatus, GitError>,
) -> Result<GitUpstreamStatus, GitError> {
    match result {
        Ok(status) => Ok(status),
        // Only swallow clearly-no-upstream signals — an expected state. Other
        // errors normalise (scrub credentials, tail line) before surfacing.
        Err(error) if is_no_upstream_error(Some(&error.message)) => Ok(GitUpstreamStatus {
            has_upstream: false,
            upstream_name: None,
            ahead: 0,
            behind: 0,
            has_configured_push_target: None,
            behind_commits_are_patch_equivalent: None,
        }),
        Err(error) => Err(GitError::from_message(normalize_git_error_message(
            Some(&error.message),
            Some(GitRemoteOperation::Upstream),
        ))),
    }
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
    }
    impl GitRunner for SeqRunner {
        fn run(&self, args: &[&str]) -> Result<GitOutput, GitError> {
            self.calls.borrow_mut().push(args.iter().map(|s| s.to_string()).collect());
            self.queue.borrow_mut().pop_front().expect("unexpected extra git call")
        }
    }
    // The same queued runner as an AsyncGitRunner, so the async twin can be driven
    // by the identical response scripts (it never suspends — the queue is ready).
    impl AsyncGitRunner for SeqRunner {
        async fn run(
            &self,
            args: &[&str],
            _stdin: Option<&str>,
        ) -> Result<GitOutput, GitError> {
            self.calls.borrow_mut().push(args.iter().map(|s| s.to_string()).collect());
            self.queue.borrow_mut().pop_front().expect("unexpected extra git call")
        }
    }
    /// Poll an immediately-ready future to completion (the async SeqRunner never
    /// pends). No executor dep, and keeps the crate's `forbid(unsafe_code)` intact.
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
    fn err_full(code: Option<i32>, stderr: &str, message: &str) -> Result<GitOutput, GitError> {
        Err(GitError { code, stdout: String::new(), stderr: stderr.to_string(), message: message.to_string() })
    }
    fn status(has: bool, name: Option<&str>, ahead: i64, behind: i64, bce: Option<bool>) -> GitUpstreamStatus {
        GitUpstreamStatus {
            has_upstream: has,
            upstream_name: name.map(str::to_string),
            ahead,
            behind,
            has_configured_push_target: None,
            behind_commits_are_patch_equivalent: bce,
        }
    }

    const MISSING_TRACKING_REF: &str = "fatal: ambiguous argument 'HEAD@{u}': unknown revision or path not in the working tree.\nUse '--' to separate paths from revisions, like this:\n'git <command> [<revision>...] -- [<file>...]'";

    #[test]
    fn returns_upstream_and_ahead_behind_counts() {
        let r = SeqRunner::new(vec![ok("main\n"), ok("origin/main\n"), ok("2\t3\n"), ok("+ abc123 remote work\n")]);
        assert_eq!(get_upstream_status(&r, None).unwrap(), status(true, Some("origin/main"), 2, 3, Some(false)));
    }

    #[test]
    fn marks_diverged_commits_patch_equivalent_after_rebase() {
        let r = SeqRunner::new(vec![
            ok("feature\n"),
            ok("origin/feature\n"),
            ok("14\t3\n"),
            ok("= ac503deae Stabilize pull request creation flow\n= 7dc0fc1a6 Clean up fork PR remotes\n"),
        ]);
        assert_eq!(get_upstream_status(&r, None).unwrap(), status(true, Some("origin/feature"), 14, 3, Some(true)));
    }

    #[test]
    fn keeps_configured_local_branch_upstreams() {
        let r = SeqRunner::new(vec![ok("feature\n"), ok("main\n"), ok("1\t0\n")]);
        assert_eq!(get_upstream_status(&r, None).unwrap(), status(true, Some("main"), 1, 0, None));
    }

    // After HEAD@{u} yields no upstream, the effective path probes the configured
    // branch remote (branch.<b>.{remote,merge,base}), the origin tracking ref, then
    // the push-target config (pushRemote, pushDefault, branch remote, merge, base) —
    // nine more git calls that all miss in these "truly no upstream" cases.
    fn no_config_misses() -> Vec<Result<GitOutput, GitError>> {
        (0..9).map(|_| err("missing config")).collect()
    }
    fn no_upstream_responses(head_u: Result<GitOutput, GitError>) -> Vec<Result<GitOutput, GitError>> {
        let mut responses = vec![ok("feature\n"), head_u];
        responses.extend(no_config_misses());
        responses
    }

    #[test]
    fn returns_no_upstream_when_output_empty() {
        let r = SeqRunner::new(no_upstream_responses(ok("\n")));
        assert_eq!(get_upstream_status(&r, None).unwrap(), status(false, None, 0, 0, None));
    }

    #[test]
    fn returns_no_upstream_when_missing() {
        let r = SeqRunner::new(no_upstream_responses(err("fatal: no upstream configured")));
        assert_eq!(get_upstream_status(&r, None).unwrap(), status(false, None, 0, 0, None));
    }

    #[test]
    fn returns_no_upstream_when_tracking_ref_missing() {
        let r = SeqRunner::new(no_upstream_responses(err(MISSING_TRACKING_REF)));
        assert_eq!(get_upstream_status(&r, None).unwrap(), status(false, None, 0, 0, None));
    }

    #[test]
    fn uses_same_name_origin_branch_for_legacy_worktree() {
        let r = SeqRunner::new(vec![
            ok("feature\n"),
            ok("origin/main\n"),
            ok("abc123\n"),
            ok("3\t1\n"),
            ok("+ def456 remote work\n"),
        ]);
        assert_eq!(get_upstream_status(&r, None).unwrap(), status(true, Some("origin/feature"), 3, 1, Some(false)));
    }

    #[test]
    fn keeps_upstream_whose_remote_name_contains_a_slash() {
        let runner = |args: &[&str]| -> Result<GitOutput, GitError> {
            if args[0] == "symbolic-ref" {
                ok("feature\n")
            } else if args[0] == "rev-parse" && args.contains(&"HEAD@{u}") {
                ok("origin/team/feature\n")
            } else if args[0] == "remote" {
                ok("origin\norigin/team\n")
            } else if args[0] == "rev-list" && args.iter().any(|a| a.contains("HEAD...origin/team/feature")) {
                ok("2\t0\n")
            } else {
                err(&format!("unexpected git args: {}", args.join(" ")))
            }
        };
        assert_eq!(get_upstream_status(&runner, None).unwrap(), status(true, Some("origin/team/feature"), 2, 0, None));
    }

    #[test]
    fn uses_explicit_publish_target_with_expected_call_order() {
        let r = SeqRunner::new(vec![ok(""), ok("abc123\n"), ok("1\t2\n"), ok("+ def456 remote work\n")]);
        let target = GitPushTarget { remote_name: "fork".into(), branch_name: "feature/fix".into(), remote_url: None };
        assert_eq!(get_upstream_status(&r, Some(&target)).unwrap(), status(true, Some("fork/feature/fix"), 1, 2, Some(false)));
        assert_eq!(
            *r.calls.borrow(),
            vec![
                vec!["check-ref-format", "--branch", "feature/fix"],
                vec!["rev-parse", "--verify", "--quiet", "refs/remotes/fork/feature/fix"],
                vec!["rev-list", "--left-right", "--count", "HEAD...refs/remotes/fork/feature/fix"],
                vec!["log", "--oneline", "--cherry-mark", "--right-only", "HEAD...refs/remotes/fork/feature/fix", "--"],
            ]
        );
    }

    #[test]
    fn async_explicit_target_matches_sync_including_call_order() {
        // The relay drives the SAME explicit-target sequence through the async
        // twin; prove it agrees with the sync path on both result and call order.
        let responses =
            || vec![ok(""), ok("abc123\n"), ok("1\t2\n"), ok("+ def456 remote work\n")];
        let target =
            GitPushTarget { remote_name: "fork".into(), branch_name: "feature/fix".into(), remote_url: None };

        let sync_runner = SeqRunner::new(responses());
        let sync = get_upstream_status(&sync_runner, Some(&target)).unwrap();

        let async_runner = SeqRunner::new(responses());
        let asynced =
            block_on_ready(get_publish_target_upstream_status_async(&async_runner, &target)).unwrap();

        assert_eq!(asynced, sync);
        assert_eq!(asynced, status(true, Some("fork/feature/fix"), 1, 2, Some(false)));
        assert_eq!(*async_runner.calls.borrow(), *sync_runner.calls.borrow());
    }

    #[test]
    fn async_explicit_target_swallows_missing_tracking_ref() {
        // The not-fetched (bare "git exited with 1", empty stderr) case must
        // resolve to the publishable no-upstream status, not reject — matching sync.
        let target =
            GitPushTarget { remote_name: "fork".into(), branch_name: "feature/fix".into(), remote_url: None };
        let runner = SeqRunner::new(vec![ok(""), err_full(None, "", "git exited with 1.")]);
        let status = block_on_ready(get_publish_target_upstream_status_async(&runner, &target)).unwrap();
        assert_eq!(
            status,
            GitUpstreamStatus {
                has_configured_push_target: Some(true),
                ..self::status(false, Some("fork/feature/fix"), 0, 0, None)
            }
        );
    }

    #[test]
    fn reports_no_upstream_when_publish_target_not_fetched() {
        let r = SeqRunner::new(vec![ok(""), err_full(None, "", "git exited with 1.")]);
        let target = GitPushTarget { remote_name: "fork".into(), branch_name: "feature/fix".into(), remote_url: None };
        // The not-fetched (missing-tracking-ref) branch flags that the branch can
        // still be published, mirroring TS getPublishTargetStatus.
        assert_eq!(
            get_upstream_status(&r, Some(&target)).unwrap(),
            GitUpstreamStatus {
                has_configured_push_target: Some(true),
                ..status(false, Some("fork/feature/fix"), 0, 0, None)
            }
        );
    }

    #[test]
    fn does_not_hide_failures_while_checking_publish_target() {
        let r = SeqRunner::new(vec![ok(""), err_full(None, "fatal: not a git repository", "fatal: not a git repository")]);
        let target = GitPushTarget { remote_name: "fork".into(), branch_name: "feature/fix".into(), remote_url: None };
        let error = get_upstream_status(&r, Some(&target)).unwrap_err();
        assert!(error.message.contains("fatal: not a git repository"), "{}", error.message);
    }
}
