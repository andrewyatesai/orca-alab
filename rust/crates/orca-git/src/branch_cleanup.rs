//! Branch-cleanup safety checks, ported from `src/shared/git-branch-cleanup.ts`.
//!
//! Decides whether a local branch is safe to delete on worktree removal:
//! gathers candidate base refs, refreshes the relevant remotes (non-fatal), and
//! checks whether the branch has any *unmerged* changes against those bases
//! (tree-equal merge, merge-only commits, or patch-equivalent commits). All
//! over the `GitRunner` boundary, so it's testable against a mock.

use crate::runner::GitRunner;
use std::collections::HashSet;

/// Run git, returning trimmed non-empty stdout, or `None` on error/empty.
fn read_optional<R: GitRunner>(runner: &R, argv: &[&str]) -> Option<String> {
    match runner.run(argv) {
        Ok(out) => {
            let trimmed = out.stdout.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        Err(_) => None,
    }
}

fn add_candidate(candidates: &mut Vec<String>, candidate: Option<String>) {
    if let Some(candidate) = candidate {
        let trimmed = candidate.trim();
        if !trimmed.is_empty()
            && !trimmed.starts_with('-')
            && !candidates.iter().any(|existing| existing == trimmed)
        {
            candidates.push(trimmed.to_string());
        }
    }
}

/// Candidate base refs to check a branch against: configured base, origin's
/// default branch, then `HEAD`.
pub fn get_branch_cleanup_target_refs<R: GitRunner>(runner: &R, branch_name: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    add_candidate(
        &mut candidates,
        read_optional(runner, &["config", "--get", &format!("branch.{branch_name}.base")]),
    );
    add_candidate(
        &mut candidates,
        read_optional(runner, &["symbolic-ref", "--quiet", "refs/remotes/origin/HEAD"]),
    );
    add_candidate(&mut candidates, Some("HEAD".to_string()));
    candidates
}

/// Fetch each remote referenced by the target refs once (longest remote name
/// wins), so a just-merged base is current. Network failures are non-fatal.
pub fn refresh_branch_cleanup_target_refs<R: GitRunner>(runner: &R, target_refs: &[&str]) {
    let remotes_stdout = read_optional(runner, &["remote"]).unwrap_or_default();
    let mut remotes: Vec<&str> = remotes_stdout
        .split('\n')
        .map(str::trim)
        .filter(|remote| !remote.is_empty() && !remote.starts_with('-'))
        .collect();
    remotes.sort_by_key(|remote| std::cmp::Reverse(remote.len()));

    let mut fetched: HashSet<&str> = HashSet::new();
    for target_ref in target_refs {
        let remote = remotes
            .iter()
            .copied()
            .find(|candidate| target_ref.starts_with(&format!("refs/remotes/{candidate}/")));
        if let Some(remote) = remote {
            if fetched.insert(remote) {
                let _ = read_optional(runner, &["fetch", "--prune", remote]);
            }
        }
    }
}

fn resolve_commit_oid<R: GitRunner>(runner: &R, reference: &str) -> Option<String> {
    read_optional(runner, &["rev-parse", "--verify", "--quiet", &format!("{reference}^{{commit}}")])
}

fn has_branch_only_merge_commits<R: GitRunner>(runner: &R, target_oid: &str, branch_ref: &str) -> bool {
    let stdout = read_optional(
        runner,
        &["rev-list", "--right-only", "--merges", "--count", &format!("{target_oid}...{branch_ref}")],
    );
    stdout.and_then(|s| s.parse::<i64>().ok()).unwrap_or(0) > 0
}

fn branch_merges_without_tree_changes<R: GitRunner>(runner: &R, target_oid: &str, branch_ref: &str) -> bool {
    let merged_tree = read_optional(runner, &["merge-tree", "--write-tree", target_oid, branch_ref]);
    let target_tree =
        read_optional(runner, &["rev-parse", "--verify", "--quiet", &format!("{target_oid}^{{tree}}")]);
    match (merged_tree, target_tree) {
        (Some(merged), Some(target)) => merged.split('\n').next().unwrap_or("") == target,
        _ => false,
    }
}

fn branch_only_commits_are_patch_equivalent<R: GitRunner>(runner: &R, target_oid: &str, branch_ref: &str) -> bool {
    match read_optional(runner, &["cherry", "-v", target_oid, branch_ref]) {
        None => false,
        Some(stdout) => stdout
            .split('\n')
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .all(|line| line.starts_with('-')),
    }
}

/// True when the branch has no unmerged changes against any candidate target —
/// i.e. it's safe to delete.
pub fn branch_has_no_unmerged_changes_on_any_target<R: GitRunner>(
    runner: &R,
    branch_name: &str,
    target_refs: &[&str],
) -> bool {
    let branch_ref = format!("refs/heads/{branch_name}");
    for target_ref in target_refs {
        let Some(target_oid) = resolve_commit_oid(runner, target_ref) else {
            continue;
        };
        if branch_merges_without_tree_changes(runner, &target_oid, &branch_ref) {
            return true;
        }
        if has_branch_only_merge_commits(runner, &target_oid, &branch_ref) {
            continue;
        }
        if branch_only_commits_are_patch_equivalent(runner, &target_oid, &branch_ref) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::{GitError, GitOutput};
    use std::cell::RefCell;

    fn ok(stdout: &str) -> Result<GitOutput, GitError> {
        Ok(GitOutput { stdout: stdout.to_string(), stderr: String::new() })
    }

    #[test]
    fn refresh_fetches_each_remote_once_preferring_slashed_names() {
        let calls: RefCell<Vec<Vec<String>>> = RefCell::new(Vec::new());
        let runner = |args: &[&str]| {
            calls.borrow_mut().push(args.iter().map(|s| s.to_string()).collect());
            if args.first() == Some(&"remote") {
                ok("origin\nfoo\nfoo/bar\n")
            } else {
                ok("")
            }
        };
        refresh_branch_cleanup_target_refs(
            &runner,
            &["refs/remotes/origin/main", "refs/remotes/foo/bar/feature", "refs/remotes/foo/bar/another", "HEAD"],
        );
        assert_eq!(
            *calls.borrow(),
            vec![
                vec!["remote"],
                vec!["fetch", "--prune", "origin"],
                vec!["fetch", "--prune", "foo/bar"],
            ]
        );
    }

    #[test]
    fn refresh_is_non_fatal_when_remote_listing_or_fetch_fails() {
        let always_fails = |_: &[&str]| Err(GitError::from_message("offline"));
        refresh_branch_cleanup_target_refs(&always_fails, &["refs/remotes/origin/main"]); // must not panic

        let fetch_fails = |args: &[&str]| {
            if args.first() == Some(&"remote") {
                ok("origin\n")
            } else {
                Err(GitError::from_message("offline"))
            }
        };
        refresh_branch_cleanup_target_refs(&fetch_fails, &["refs/remotes/origin/main"]); // must not panic
    }

    #[test]
    fn target_refs_gather_base_origin_head() {
        let runner = |args: &[&str]| {
            if args.first() == Some(&"config") {
                ok("origin/main")
            } else if args.first() == Some(&"symbolic-ref") {
                ok("refs/remotes/origin/HEAD")
            } else {
                ok("")
            }
        };
        assert_eq!(
            get_branch_cleanup_target_refs(&runner, "feature"),
            vec!["origin/main".to_string(), "refs/remotes/origin/HEAD".to_string(), "HEAD".to_string()]
        );
    }

    #[test]
    fn safe_to_delete_when_merge_is_tree_equal() {
        // merge-tree result's first line equals the target tree → no changes.
        let runner = |args: &[&str]| match args.first().copied() {
            Some("rev-parse") if args.iter().any(|a| a.contains("^{commit}")) => ok("targetoid"),
            Some("merge-tree") => ok("sametree\n"),
            Some("rev-parse") if args.iter().any(|a| a.contains("^{tree}")) => ok("sametree"),
            _ => ok(""),
        };
        assert!(branch_has_no_unmerged_changes_on_any_target(&runner, "feature", &["refs/remotes/origin/main"]));
    }

    #[test]
    fn unsafe_when_branch_has_distinct_commits() {
        let runner = |args: &[&str]| match args.first().copied() {
            Some("rev-parse") if args.iter().any(|a| a.contains("^{commit}")) => ok("targetoid"),
            Some("merge-tree") => ok("differenttree"),
            Some("rev-parse") if args.iter().any(|a| a.contains("^{tree}")) => ok("targettree"),
            Some("rev-list") => ok("0"),           // no merge-only commits
            Some("cherry") => ok("+ abc new work"), // a non-equivalent commit
            _ => ok(""),
        };
        assert!(!branch_has_no_unmerged_changes_on_any_target(&runner, "feature", &["refs/remotes/origin/main"]));
    }
}
