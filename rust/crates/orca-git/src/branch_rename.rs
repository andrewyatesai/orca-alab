//! Branch-rename collision resolution, ported from
//! `src/main/git/branch-rename.ts`.

use crate::effective_upstream::resolve_effective_git_upstream;
use crate::runner::{GitError, GitRunner};
use orca_text::git_remote_error::is_no_upstream_error;

/// True when the branch has a configured/effective upstream (pushed or
/// tracking). Auto-rename refuses to touch such a branch. On an unexpected
/// failure, conservatively reports `true` so a published branch is never
/// renamed out from under its remote.
pub fn branch_has_upstream<R: GitRunner>(runner: &R) -> bool {
    match resolve_effective_git_upstream(runner) {
        Ok(upstream) => upstream.is_some(),
        Err(error) => !is_no_upstream_error(Some(&error.message)),
    }
}

/// Default collision-suffix attempt cap (matches the TS default).
pub const DEFAULT_MAX_BRANCH_ATTEMPTS: usize = 100;

fn local_branch_exists<R: GitRunner>(runner: &R, branch: &str) -> bool {
    runner
        .run(&["show-ref", "--verify", "--quiet", &format!("refs/heads/{branch}")])
        .is_ok()
}

/// Resolve a branch name that doesn't collide with an existing local branch by
/// appending `-2`, `-3`, … to the leaf. `compute` applies any configured prefix
/// to a leaf. The branch being renamed away from is never a collision.
pub fn resolve_unique_branch_name<R, F>(
    runner: &R,
    leaf: &str,
    compute: F,
    current_branch: &str,
    max_attempts: usize,
) -> Option<String>
where
    R: GitRunner,
    F: Fn(&str) -> String,
{
    let is_available =
        |candidate: &str| candidate == current_branch || !local_branch_exists(runner, candidate);

    let first = compute(leaf);
    if is_available(&first) {
        return Some(first);
    }
    for suffix in 2..=max_attempts {
        let candidate = compute(&format!("{leaf}-{suffix}"));
        if is_available(&candidate) {
            return Some(candidate);
        }
    }
    None
}

/// Rename the currently checked-out branch (`git branch -m <newBranch>`).
pub fn rename_current_branch<R: GitRunner>(runner: &R, new_branch: &str) -> Result<(), GitError> {
    runner.run(&["branch", "-m", new_branch]).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::GitOutput;
    use std::cell::RefCell;

    fn missing(_: &[&str]) -> Result<GitOutput, GitError> {
        Err(GitError { code: None, stdout: String::new(), stderr: String::new(), message: "not found".to_string() })
    }
    fn compute(leaf: &str) -> String {
        format!("you/{leaf}")
    }

    const NO_UPSTREAM: &str = "fatal: no upstream configured for branch 'feature'\nTo push the current branch and set the remote as upstream, use\n    git push --set-upstream origin feature";
    fn ok(stdout: &str) -> Result<GitOutput, GitError> {
        Ok(GitOutput { stdout: stdout.to_string(), stderr: String::new() })
    }

    #[test]
    fn branch_has_upstream_true_when_u_resolves() {
        let runner = |args: &[&str]| {
            if args[0] == "symbolic-ref" {
                ok("feature\n")
            } else if args[0] == "rev-parse" && args.contains(&"HEAD@{u}") {
                ok("origin/feature\n")
            } else {
                missing(args)
            }
        };
        assert!(branch_has_upstream(&runner));
    }

    #[test]
    fn branch_has_upstream_false_when_no_upstream() {
        let runner = |args: &[&str]| {
            if args[0] == "symbolic-ref" {
                ok("feature\n")
            } else if args[0] == "rev-parse" && args.contains(&"HEAD@{u}") {
                Err(GitError::from_message(NO_UPSTREAM))
            } else {
                missing(args) // refs/remotes/origin/feature → not found
            }
        };
        assert!(!branch_has_upstream(&runner));
    }

    #[test]
    fn branch_has_upstream_true_for_same_name_origin_ref() {
        let runner = |args: &[&str]| {
            if args[0] == "symbolic-ref" {
                ok("feature\n")
            } else if args[0] == "rev-parse" && args.contains(&"HEAD@{u}") {
                Err(GitError::from_message(NO_UPSTREAM))
            } else if args[0] == "rev-parse" && args.iter().any(|a| a.contains("refs/remotes/origin/feature")) {
                ok("")
            } else {
                missing(args)
            }
        };
        assert!(branch_has_upstream(&runner));
    }

    #[test]
    fn branch_has_upstream_conservatively_true_on_unexpected_failure() {
        let runner = |_: &[&str]| Err(GitError::from_message("fatal: not a git repository"));
        assert!(branch_has_upstream(&runner));
    }

    #[test]
    fn returns_first_candidate_when_no_collision() {
        let result = resolve_unique_branch_name(&missing, "fix-auth", compute, "you/Nautilus", DEFAULT_MAX_BRANCH_ATTEMPTS);
        assert_eq!(result.as_deref(), Some("you/fix-auth"));
    }

    #[test]
    fn suffixes_when_first_candidate_exists() {
        let runner = |args: &[&str]| {
            if args.last() == Some(&"refs/heads/you/fix-auth") {
                Ok(GitOutput::default()) // exists
            } else {
                Err(GitError { code: None, stdout: String::new(), stderr: String::new(), message: "not found".to_string() })
            }
        };
        let result = resolve_unique_branch_name(&runner, "fix-auth", compute, "you/Nautilus", DEFAULT_MAX_BRANCH_ATTEMPTS);
        assert_eq!(result.as_deref(), Some("you/fix-auth-2"));
    }

    #[test]
    fn does_not_treat_current_branch_as_collision() {
        // Reports every ref as existing; only the current-branch shortcut passes.
        let runner = |_: &[&str]| Ok(GitOutput::default());
        let result = resolve_unique_branch_name(&runner, "octopus", compute, "you/octopus", DEFAULT_MAX_BRANCH_ATTEMPTS);
        assert_eq!(result.as_deref(), Some("you/octopus"));
    }

    #[test]
    fn rename_runs_git_branch_m() {
        let calls: RefCell<Vec<Vec<String>>> = RefCell::new(Vec::new());
        let runner = |args: &[&str]| {
            calls.borrow_mut().push(args.iter().map(|s| s.to_string()).collect());
            Ok(GitOutput::default())
        };
        rename_current_branch(&runner, "you/fix-auth").unwrap();
        assert_eq!(calls.borrow()[0], vec!["branch", "-m", "you/fix-auth"]);
    }
}
