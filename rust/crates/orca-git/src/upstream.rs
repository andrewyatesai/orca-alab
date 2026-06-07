//! Upstream/ahead-behind status, ported from `src/main/git/upstream.ts`.
//!
//! Composes effective-upstream resolution (or an explicit publish target),
//! patch-equivalence probing, and the no-upstream/error normalisation policy.

use crate::effective_upstream::effective_git_upstream_status;
use crate::publish_target_status::get_publish_target_status;
use crate::push_target::{validate_git_push_target, GitPushTarget};
use crate::runner::{GitError, GitRunner};
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

    match result {
        Ok(status) => Ok(status),
        // Only swallow clearly-no-upstream signals — an expected state. Other
        // errors normalise (scrub credentials, tail line) before surfacing.
        Err(error) if is_no_upstream_error(Some(&error.message)) => Ok(GitUpstreamStatus {
            has_upstream: false,
            upstream_name: None,
            ahead: 0,
            behind: 0,
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

    #[test]
    fn returns_no_upstream_when_output_empty() {
        let r = SeqRunner::new(vec![ok("feature\n"), ok("\n"), err("missing remote branch")]);
        assert_eq!(get_upstream_status(&r, None).unwrap(), status(false, None, 0, 0, None));
    }

    #[test]
    fn returns_no_upstream_when_missing() {
        let r = SeqRunner::new(vec![ok("feature\n"), err("fatal: no upstream configured"), err("missing remote branch")]);
        assert_eq!(get_upstream_status(&r, None).unwrap(), status(false, None, 0, 0, None));
    }

    #[test]
    fn returns_no_upstream_when_tracking_ref_missing() {
        let r = SeqRunner::new(vec![ok("feature\n"), err(MISSING_TRACKING_REF), err("missing remote branch")]);
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
    fn reports_no_upstream_when_publish_target_not_fetched() {
        let r = SeqRunner::new(vec![ok(""), err_full(None, "", "git exited with 1.")]);
        let target = GitPushTarget { remote_name: "fork".into(), branch_name: "feature/fix".into(), remote_url: None };
        assert_eq!(get_upstream_status(&r, Some(&target)).unwrap(), status(false, Some("fork/feature/fix"), 0, 0, None));
    }

    #[test]
    fn does_not_hide_failures_while_checking_publish_target() {
        let r = SeqRunner::new(vec![ok(""), err_full(None, "fatal: not a git repository", "fatal: not a git repository")]);
        let target = GitPushTarget { remote_name: "fork".into(), branch_name: "feature/fix".into(), remote_url: None };
        let error = get_upstream_status(&r, Some(&target)).unwrap_err();
        assert!(error.message.contains("fatal: not a git repository"), "{}", error.message);
    }
}
