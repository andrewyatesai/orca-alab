//! Branch-cleanup safety checks, ported from `src/shared/git-branch-cleanup.ts`.
//!
//! Decides whether a local branch is safe to delete on worktree removal:
//! gathers candidate base refs, refreshes the relevant remotes (non-fatal), and
//! checks whether the branch has any *unmerged* changes against those bases
//! (tree-equal merge, merge-only commits, or patch-equivalent commits). All
//! over the `GitRunner` boundary, so it's testable against a mock.

use crate::runner::GitRunner;
use std::collections::HashSet;

/// Cap on the target-ancestry commits scanned for a squash match (matches the TS
/// `SQUASH_PATCH_SCAN_LIMIT`).
const SQUASH_PATCH_SCAN_LIMIT: usize = 200;

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

/// Raw (untrimmed) stdout, or `None` on error/empty — patch text for `git
/// patch-id` must not be trimmed.
fn read_optional_raw<R: GitRunner>(runner: &R, argv: &[&str]) -> Option<String> {
    match runner.run(argv) {
        Ok(out) if !out.stdout.is_empty() => Some(out.stdout),
        _ => None,
    }
}

/// The patch-id: first whitespace token of the first non-empty line.
fn parse_patch_id(stdout: Option<&str>) -> Option<String> {
    let line = stdout?.split('\n').map(str::trim).find(|line| !line.is_empty())?;
    let patch_id = line.split_whitespace().next().unwrap_or("");
    if patch_id.is_empty() {
        None
    } else {
        Some(patch_id.to_string())
    }
}

/// Stable patch-id for the given patch text (`git patch-id --stable`, piped stdin).
fn compute_stable_patch_id<R: GitRunner>(runner: &R, patch_text: Option<&str>) -> Option<String> {
    let patch_text = patch_text?;
    if patch_text.is_empty() {
        return None;
    }
    let out = runner.run_with_stdin(&["patch-id", "--stable"], patch_text).ok()?;
    parse_patch_id(Some(&out.stdout))
}

/// A branch whose only extra commits are merges can still be squash-merged into
/// the target: scan the target's ancestry for a commit whose patch-id matches the
/// branch's net diff AND that the branch merges into without tree changes.
fn branch_net_patch_matches_target_squash_commit<R: GitRunner>(
    runner: &R,
    target_oid: &str,
    branch_ref: &str,
) -> bool {
    let Some(merge_base) = read_optional(runner, &["merge-base", target_oid, branch_ref]) else {
        return false;
    };
    let branch_patch_id = compute_stable_patch_id(
        runner,
        read_optional_raw(runner, &["diff", &merge_base, branch_ref]).as_deref(),
    );
    let Some(branch_patch_id) = branch_patch_id else {
        return false;
    };
    let commits: Vec<String> = match read_optional(
        runner,
        &[
            "rev-list",
            "--ancestry-path",
            &format!("--max-count={}", SQUASH_PATCH_SCAN_LIMIT + 1),
            &format!("{merge_base}..{target_oid}"),
        ],
    ) {
        None => return false,
        Some(stdout) => stdout
            .split('\n')
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect(),
    };
    if commits.is_empty() || commits.len() > SQUASH_PATCH_SCAN_LIMIT {
        return false;
    }
    for commit_oid in &commits {
        let commit_patch_id = compute_stable_patch_id(
            runner,
            read_optional_raw(runner, &["show", "--format=", commit_oid]).as_deref(),
        );
        // A matching patch-id flags a possible squash; the tree merge proves the
        // branch adds no further changes there.
        if commit_patch_id.as_deref() == Some(branch_patch_id.as_str())
            && branch_merges_without_tree_changes(runner, commit_oid, branch_ref)
        {
            return true;
        }
    }
    false
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
            // The branch's only extra commits are merges — but it may still have
            // been squash-merged into the target.
            if branch_net_patch_matches_target_squash_commit(runner, &target_oid, &branch_ref) {
                return true;
            }
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

    /// A struct runner (unlike the bare `Fn` mocks) so it can implement
    /// `run_with_stdin` — needed to exercise the `git patch-id --stable` squash path.
    struct StdinAwareRunner<F: Fn(&[&str], Option<&str>) -> Result<GitOutput, GitError>> {
        respond: F,
        stdin_calls: RefCell<Vec<(Vec<String>, String)>>,
    }
    impl<F: Fn(&[&str], Option<&str>) -> Result<GitOutput, GitError>> GitRunner for StdinAwareRunner<F> {
        fn run(&self, args: &[&str]) -> Result<GitOutput, GitError> {
            (self.respond)(args, None)
        }
        fn run_with_stdin(&self, args: &[&str], stdin: &str) -> Result<GitOutput, GitError> {
            self.stdin_calls
                .borrow_mut()
                .push((args.iter().map(|s| s.to_string()).collect(), stdin.to_string()));
            (self.respond)(args, Some(stdin))
        }
    }

    #[test]
    fn squash_merged_branch_with_merge_commits_is_safe_to_delete() {
        // A branch whose only extra commits are merges, but whose net patch matches a
        // squash commit on the target — TS deletes it; the ported squash path must too.
        let runner = StdinAwareRunner {
            stdin_calls: RefCell::new(Vec::new()),
            respond: |args: &[&str], stdin: Option<&str>| match args {
                ["rev-parse", "--verify", "--quiet", r] if *r == "origin/main^{commit}" => ok("TOID"),
                // not tree-equal directly against the target
                ["merge-tree", "--write-tree", "TOID", "refs/heads/feature"] => ok("OTHERTREE\n"),
                ["rev-parse", "--verify", "--quiet", "TOID^{tree}"] => ok("TTREE"),
                // branch has a merge commit -> take the squash path
                ["rev-list", "--right-only", "--merges", "--count", "TOID...refs/heads/feature"] => ok("1"),
                ["merge-base", "TOID", "refs/heads/feature"] => ok("MBASE"),
                ["diff", "MBASE", "refs/heads/feature"] => ok("BRANCH_PATCH_TEXT"),
                ["rev-list", "--ancestry-path", _, range] if *range == "MBASE..TOID" => ok("SQUASH\n"),
                ["show", "--format=", "SQUASH"] => ok("SQUASH_PATCH_TEXT"),
                // the squash commit is tree-equal when the branch merges into it
                ["merge-tree", "--write-tree", "SQUASH", "refs/heads/feature"] => ok("STREE\n"),
                ["rev-parse", "--verify", "--quiet", "SQUASH^{tree}"] => ok("STREE"),
                ["patch-id", "--stable"] => match stdin {
                    // both diffs hash to the same stable patch-id
                    Some("BRANCH_PATCH_TEXT") => ok("PID 111\n"),
                    Some("SQUASH_PATCH_TEXT") => ok("PID 222\n"),
                    _ => ok(""),
                },
                other => Err(GitError::from_message(format!("unexpected git args: {other:?}"))),
            },
        };
        assert!(branch_has_no_unmerged_changes_on_any_target(&runner, "feature", &["origin/main"]));
        // proves patch text was piped to `git patch-id --stable` via run_with_stdin
        let stdin_calls = runner.stdin_calls.borrow();
        assert_eq!(stdin_calls.len(), 2);
        assert!(stdin_calls.iter().all(|(args, _)| args == &["patch-id", "--stable"]));
        assert_eq!(stdin_calls[0].1, "BRANCH_PATCH_TEXT");
        assert_eq!(stdin_calls[1].1, "SQUASH_PATCH_TEXT");
    }

    #[test]
    fn merge_commit_branch_without_squash_match_is_preserved() {
        // Same shape, but the patch-ids differ -> no squash match -> preserve (false).
        let runner = StdinAwareRunner {
            stdin_calls: RefCell::new(Vec::new()),
            respond: |args: &[&str], stdin: Option<&str>| match args {
                ["rev-parse", "--verify", "--quiet", r] if *r == "origin/main^{commit}" => ok("TOID"),
                ["merge-tree", "--write-tree", "TOID", "refs/heads/feature"] => ok("OTHERTREE\n"),
                ["rev-parse", "--verify", "--quiet", "TOID^{tree}"] => ok("TTREE"),
                ["rev-list", "--right-only", "--merges", "--count", "TOID...refs/heads/feature"] => ok("1"),
                ["merge-base", "TOID", "refs/heads/feature"] => ok("MBASE"),
                ["diff", "MBASE", "refs/heads/feature"] => ok("BRANCH_PATCH_TEXT"),
                ["rev-list", "--ancestry-path", _, range] if *range == "MBASE..TOID" => ok("SQUASH\n"),
                ["show", "--format=", "SQUASH"] => ok("DIFFERENT_PATCH_TEXT"),
                ["patch-id", "--stable"] => match stdin {
                    Some("BRANCH_PATCH_TEXT") => ok("PID 111\n"),
                    Some("DIFFERENT_PATCH_TEXT") => ok("PID 999\n"),
                    _ => ok(""),
                },
                other => Err(GitError::from_message(format!("unexpected git args: {other:?}"))),
            },
        };
        assert!(!branch_has_no_unmerged_changes_on_any_target(&runner, "feature", &["origin/main"]));
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
