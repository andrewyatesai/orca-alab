//! Git-history loader (IO edge), ported from `src/shared/git-history.ts`.
//!
//! Orchestrates one bounded `topo-order` log query plus the ref/merge-base
//! lookups that frame it, all through an injected [`GitHistoryExecutor`] so the
//! same logic runs against a real `git`, an SSH worktree, or a test mock. Every
//! lookup that can fail is a soft fallback (mirroring the TS `try/catch`); only
//! the final `git log` propagates its error.

use crate::git_history_log_parser::{
    git_history_ref_from_full_name, parse_git_history_log, short_git_hash, GIT_HISTORY_COMMIT_FORMAT,
};
use crate::git_history_types::{
    GitHistoryExecError, GitHistoryExecutor, GitHistoryItem, GitHistoryItemRef, GitHistoryOptions,
    GitHistoryRefCategory, GitHistoryResult, GIT_HISTORY_DEFAULT_LIMIT, GIT_HISTORY_MAX_LIMIT,
};

/// Clamp a requested limit into `[1, GIT_HISTORY_MAX_LIMIT]`, defaulting when
/// absent. (TS clamps `Math.min(MAX, Math.max(1, trunc(limit ?? DEFAULT)))`.)
#[cfg_attr(trust_verify, trust::ensures(|out: &i64| *out >= 1 && *out <= GIT_HISTORY_MAX_LIMIT))]
fn clamp_history_limit(limit: Option<i64>) -> i64 {
    match limit {
        None => GIT_HISTORY_DEFAULT_LIMIT,
        Some(value) => GIT_HISTORY_MAX_LIMIT.min(1.max(value)),
    }
}

fn resolve_commit<E: GitHistoryExecutor>(git: &E, cwd: &str, reference: &str) -> Option<String> {
    if reference.is_empty() || reference.starts_with('-') {
        return None;
    }
    let arg = format!("{reference}^{{commit}}");
    match git.run(&["rev-parse", "--verify", "--end-of-options", &arg], cwd) {
        Ok(output) => {
            let oid = output.stdout.trim();
            (!oid.is_empty()).then(|| oid.to_string())
        }
        Err(_) => None,
    }
}

fn resolve_symbolic_full_name<E: GitHistoryExecutor>(
    git: &E,
    cwd: &str,
    reference: &str,
) -> Option<String> {
    if reference.is_empty() || reference.starts_with('-') {
        return None;
    }
    match git.run(&["rev-parse", "--symbolic-full-name", "--end-of-options", reference], cwd) {
        Ok(output) => output.stdout.trim().lines().find(|line| !line.is_empty()).map(str::to_string),
        Err(_) => None,
    }
}

fn resolve_current_ref<E: GitHistoryExecutor>(
    git: &E,
    cwd: &str,
    head_oid: &str,
) -> (GitHistoryItemRef, Option<String>) {
    if let Ok(output) = git.run(&["symbolic-ref", "--quiet", "--short", "HEAD"], cwd) {
        let branch_name = output.stdout.trim();
        if !branch_name.is_empty() {
            return (
                GitHistoryItemRef {
                    id: format!("refs/heads/{branch_name}"),
                    name: branch_name.to_string(),
                    revision: Some(head_oid.to_string()),
                    category: Some(GitHistoryRefCategory::Branches),
                    ..Default::default()
                },
                Some(branch_name.to_string()),
            );
        }
    }

    (
        GitHistoryItemRef {
            id: head_oid.to_string(),
            name: short_git_hash(head_oid),
            revision: Some(head_oid.to_string()),
            category: Some(GitHistoryRefCategory::Commits),
            ..Default::default()
        },
        None,
    )
}

fn resolve_upstream_ref<E: GitHistoryExecutor>(
    git: &E,
    cwd: &str,
    branch_name: Option<&str>,
) -> Option<GitHistoryItemRef> {
    let branch_name = branch_name?;
    let format_arg = "--format=%(upstream)%00%(upstream:short)";
    let ref_arg = format!("refs/heads/{branch_name}");
    let output = git.run(&["for-each-ref", format_arg, &ref_arg], cwd).ok()?;

    let mut parts = output.stdout.split('\0');
    let full_name = parts.next().map(str::trim).filter(|name| !name.is_empty())?;
    let short_name = parts.next().map(str::trim).filter(|name| !name.is_empty())?;
    // Why: %(upstream:objectname) is not portable; resolve the name, then ask
    // rev-parse for the commit object.
    let oid = resolve_commit(git, cwd, full_name)?;
    Some(git_history_ref_from_full_name(Some(full_name), short_name, &oid))
}

fn resolve_named_ref<E: GitHistoryExecutor>(
    git: &E,
    cwd: &str,
    reference: Option<&str>,
) -> Option<GitHistoryItemRef> {
    let normalized = reference.map(str::trim).filter(|name| !name.is_empty())?;
    if normalized.starts_with('-') {
        return None;
    }
    let revision = resolve_commit(git, cwd, normalized);
    let full_name = resolve_symbolic_full_name(git, cwd, normalized);
    let revision = revision?;
    Some(git_history_ref_from_full_name(full_name.as_deref(), normalized, &revision))
}

pub fn load_git_history_from_executor<E: GitHistoryExecutor>(
    git: &E,
    cwd: &str,
    options: &GitHistoryOptions,
) -> Result<GitHistoryResult, GitHistoryExecError> {
    let limit = clamp_history_limit(options.limit);
    let head_oid = match resolve_commit(git, cwd, "HEAD") {
        Some(oid) => oid,
        None => {
            return Ok(GitHistoryResult {
                items: Vec::new(),
                has_incoming_changes: false,
                has_outgoing_changes: false,
                has_more: false,
                limit,
                ..Default::default()
            });
        }
    };

    let (current_ref, branch_name) = resolve_current_ref(git, cwd, &head_oid);
    let remote_ref = resolve_upstream_ref(git, cwd, branch_name.as_deref());
    let raw_base_ref = resolve_named_ref(git, cwd, options.base_ref.as_deref());

    let base_ref = match raw_base_ref {
        Some(base)
            if Some(&base.id) != remote_ref.as_ref().map(|r| &r.id) && base.id != current_ref.id =>
        {
            Some(base)
        }
        _ => None,
    };

    // Why: this panel is scoped to the active workspace; upstream/base stay as
    // comparison metadata so old workspaces do not list newly fetched commits.
    let history_revisions = vec![head_oid];

    let mut merge_base: Option<String> = None;
    let remote_revision = remote_ref.as_ref().and_then(|r| r.revision.as_deref());
    let current_revision = current_ref.revision.as_deref();
    if let (Some(remote_revision), Some(current_revision)) = (remote_revision, current_revision) {
        if remote_revision != current_revision {
            if let Ok(output) = git.run(&["merge-base", current_revision, remote_revision], cwd) {
                let trimmed = output.stdout.trim();
                merge_base = (!trimmed.is_empty()).then(|| trimmed.to_string());
            }
        }
    }

    let format_arg = format!("--format={GIT_HISTORY_COMMIT_FORMAT}");
    let limit_arg = format!("-n{}", limit + 1);
    let mut log_args: Vec<&str> = vec![
        "log",
        format_arg.as_str(),
        "-z",
        "--topo-order",
        "--decorate=full",
        limit_arg.as_str(),
    ];
    for revision in &history_revisions {
        log_args.push(revision.as_str());
    }
    let stdout = git.run(&log_args, cwd)?.stdout;

    let parsed = parse_git_history_log(&stdout);
    let items: Vec<GitHistoryItem> = parsed.iter().take(limit.max(0) as usize).cloned().collect();

    let remote_revision = remote_ref.as_ref().and_then(|r| r.revision.as_deref());
    let current_revision = current_ref.revision.as_deref();
    let merge_base_present = merge_base.as_deref().is_some_and(|value| !value.is_empty());
    let has_incoming_changes = remote_revision.is_some_and(|value| !value.is_empty())
        && merge_base_present
        && remote_revision != merge_base.as_deref();
    let has_outgoing_changes = current_revision.is_some_and(|value| !value.is_empty())
        && remote_revision.is_some_and(|value| !value.is_empty())
        && merge_base_present
        && current_revision != merge_base.as_deref();
    let has_more = (parsed.len() as i64) > limit;

    Ok(GitHistoryResult {
        items,
        current_ref: Some(current_ref),
        remote_ref,
        base_ref,
        merge_base,
        has_incoming_changes,
        has_outgoing_changes,
        has_more,
        limit,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git_history_types::GitHistoryExecOutput;
    use std::cell::RefCell;

    fn log_record(hash: &str, parents: &[&str], decorations: &str, message: &str) -> String {
        let timestamp = "1700000000";
        let fields = [
            hash,
            "Ada Lovelace",
            "ada@example.com",
            timestamp,
            timestamp,
            &parents.join(" "),
            decorations,
            message,
        ];
        format!("{}\0", fields.join("\n"))
    }

    fn includes(args: &[&str], needle: &str) -> bool {
        args.contains(&needle)
    }

    fn ok(stdout: impl Into<String>) -> Result<GitHistoryExecOutput, GitHistoryExecError> {
        Ok(GitHistoryExecOutput { stdout: stdout.into() })
    }

    #[test]
    fn uses_one_bounded_topo_order_log_query_for_the_graph_data() {
        let head_oid = "a".repeat(40);
        let remote_oid = "b".repeat(40);
        let base_oid = "c".repeat(40);
        let limit_records = 2usize;
        let calls: RefCell<Vec<Vec<String>>> = RefCell::new(Vec::new());

        let executor = |args: &[&str], cwd: &str| -> Result<GitHistoryExecOutput, GitHistoryExecError> {
            assert_eq!(cwd, "/repo");
            calls.borrow_mut().push(args.iter().map(|a| a.to_string()).collect());
            let command = args[0];
            if command == "rev-parse" && includes(args, "HEAD^{commit}") {
                return ok(format!("{head_oid}\n"));
            }
            if command == "rev-parse"
                && includes(args, "refs/remotes/origin/feature^{commit}")
            {
                return ok(format!("{remote_oid}\n"));
            }
            if command == "symbolic-ref" {
                return ok("feature\n");
            }
            if command == "for-each-ref" {
                return ok("refs/remotes/origin/feature\0origin/feature\n");
            }
            if command == "merge-base" {
                return ok(format!("{base_oid}\n"));
            }
            if command == "log" {
                let includes_remote_root = includes(args, &remote_oid);
                let mut stdout = String::new();
                for index in 0..limit_records {
                    let hash = if index == 0 {
                        head_oid.clone()
                    } else if includes_remote_root && index == 1 {
                        remote_oid.clone()
                    } else {
                        format!("{:x}", index % 16).repeat(40)
                    };
                    let decorations = if index == 0 { "HEAD -> refs/heads/feature" } else { "" };
                    stdout.push_str(&log_record(&hash, &[&base_oid], decorations, &format!("commit {index}")));
                }
                return ok(stdout);
            }
            Err(GitHistoryExecError::new(format!("unexpected git command: {}", args.join(" "))))
        };

        let result = load_git_history_from_executor(
            &executor,
            "/repo",
            &GitHistoryOptions { limit: Some(50), base_ref: None },
        )
        .unwrap();

        let recorded = calls.borrow();
        let log_call = recorded.iter().find(|args| args[0] == "log").unwrap();
        for expected in [
            format!("--format={GIT_HISTORY_COMMIT_FORMAT}"),
            "-z".to_string(),
            "--topo-order".to_string(),
            "--decorate=full".to_string(),
            "-n51".to_string(),
            head_oid.clone(),
        ] {
            assert!(log_call.contains(&expected), "log call missing {expected}");
        }
        assert!(!log_call.contains(&remote_oid));
        assert_eq!(recorded.iter().filter(|args| args[0] == "log").count(), 1);
        assert_eq!(result.items.len(), 2);
        assert!(!result.items.iter().any(|item| item.id == remote_oid));
        assert!(result.has_incoming_changes);
        assert!(result.has_outgoing_changes);
        assert_eq!(result.merge_base.as_deref(), Some(base_oid.as_str()));
    }

    #[test]
    fn does_not_list_newly_fetched_upstream_commits_in_old_workspace_history() {
        let head_oid = "a".repeat(40);
        let remote_oid = "b".repeat(40);
        let upstream_only_oid = "d".repeat(40);
        let calls: RefCell<Vec<Vec<String>>> = RefCell::new(Vec::new());

        let executor = |args: &[&str], cwd: &str| -> Result<GitHistoryExecOutput, GitHistoryExecError> {
            assert_eq!(cwd, "/repo");
            calls.borrow_mut().push(args.iter().map(|a| a.to_string()).collect());
            let command = args[0];
            if command == "rev-parse" && includes(args, "HEAD^{commit}") {
                return ok(format!("{head_oid}\n"));
            }
            if command == "rev-parse" && includes(args, "refs/remotes/origin/main^{commit}") {
                return ok(format!("{remote_oid}\n"));
            }
            if command == "rev-parse" && includes(args, "origin/main^{commit}") {
                return ok(format!("{remote_oid}\n"));
            }
            if command == "rev-parse" && includes(args, "origin/main") {
                return ok("refs/remotes/origin/main\n");
            }
            if command == "symbolic-ref" {
                return ok("old-workspace\n");
            }
            if command == "for-each-ref" {
                return ok("refs/remotes/origin/main\0origin/main\n");
            }
            if command == "merge-base" {
                return ok(format!("{head_oid}\n"));
            }
            if command == "log" {
                let includes_remote_root = includes(args, &remote_oid);
                let stdout = if includes_remote_root {
                    format!(
                        "{}{}",
                        log_record(&upstream_only_oid, &[&remote_oid], "", "new upstream commit"),
                        log_record(&head_oid, &[], "HEAD -> refs/heads/old-workspace", "old workspace base"),
                    )
                } else {
                    log_record(&head_oid, &[], "HEAD -> refs/heads/old-workspace", "old workspace base")
                };
                return ok(stdout);
            }
            Err(GitHistoryExecError::new(format!("unexpected git command: {}", args.join(" "))))
        };

        let result = load_git_history_from_executor(
            &executor,
            "/repo",
            &GitHistoryOptions { limit: Some(50), base_ref: Some("origin/main".to_string()) },
        )
        .unwrap();

        let recorded = calls.borrow();
        let log_call = recorded.iter().find(|args| args[0] == "log").unwrap();
        assert!(!log_call.contains(&remote_oid));
        assert_eq!(result.remote_ref.as_ref().and_then(|r| r.revision.as_deref()), Some(remote_oid.as_str()));
        assert!(result.base_ref.is_none());
        assert!(result.has_incoming_changes);
        assert!(!result.has_outgoing_changes);
        assert_eq!(result.items.iter().map(|item| item.id.clone()).collect::<Vec<_>>(), vec![head_oid]);
    }

    #[test]
    fn clamps_oversized_limits_before_shelling_out_to_git_log() {
        let head_oid = "a".repeat(40);
        let remote_oid = "b".repeat(40);
        let base_oid = "c".repeat(40);
        let limit_records = (GIT_HISTORY_MAX_LIMIT + 1) as usize;
        let calls: RefCell<Vec<Vec<String>>> = RefCell::new(Vec::new());

        let executor = |args: &[&str], cwd: &str| -> Result<GitHistoryExecOutput, GitHistoryExecError> {
            assert_eq!(cwd, "/repo");
            calls.borrow_mut().push(args.iter().map(|a| a.to_string()).collect());
            let command = args[0];
            if command == "rev-parse" && includes(args, "HEAD^{commit}") {
                return ok(format!("{head_oid}\n"));
            }
            if command == "rev-parse" && includes(args, "refs/remotes/origin/feature^{commit}") {
                return ok(format!("{remote_oid}\n"));
            }
            if command == "symbolic-ref" {
                return ok("feature\n");
            }
            if command == "for-each-ref" {
                return ok("refs/remotes/origin/feature\0origin/feature\n");
            }
            if command == "merge-base" {
                return ok(format!("{base_oid}\n"));
            }
            if command == "log" {
                let includes_remote_root = includes(args, &remote_oid);
                let mut stdout = String::new();
                for index in 0..limit_records {
                    let hash = if index == 0 {
                        head_oid.clone()
                    } else if includes_remote_root && index == 1 {
                        remote_oid.clone()
                    } else {
                        format!("{:x}", index % 16).repeat(40)
                    };
                    let decorations = if index == 0 { "HEAD -> refs/heads/feature" } else { "" };
                    stdout.push_str(&log_record(&hash, &[&base_oid], decorations, &format!("commit {index}")));
                }
                return ok(stdout);
            }
            Err(GitHistoryExecError::new(format!("unexpected git command: {}", args.join(" "))))
        };

        let result = load_git_history_from_executor(
            &executor,
            "/repo",
            &GitHistoryOptions { limit: Some(500), base_ref: None },
        )
        .unwrap();

        let recorded = calls.borrow();
        let log_call = recorded.iter().find(|args| args[0] == "log").unwrap();
        assert!(log_call.contains(&format!("-n{}", GIT_HISTORY_MAX_LIMIT + 1)));
        assert_eq!(result.items.len() as i64, GIT_HISTORY_MAX_LIMIT);
        assert_eq!(result.limit, GIT_HISTORY_MAX_LIMIT);
        assert!(result.has_more);
    }

    #[test]
    fn returns_an_empty_result_for_unborn_repositories_without_running_git_log() {
        let call_count: RefCell<usize> = RefCell::new(0);

        let executor = |args: &[&str], _cwd: &str| -> Result<GitHistoryExecOutput, GitHistoryExecError> {
            *call_count.borrow_mut() += 1;
            if args[0] == "rev-parse" {
                return Err(GitHistoryExecError::new("ambiguous argument HEAD"));
            }
            Err(GitHistoryExecError::new(format!("unexpected git command: {}", args.join(" "))))
        };

        let result =
            load_git_history_from_executor(&executor, "/repo", &GitHistoryOptions::default()).unwrap();

        assert!(result.items.is_empty());
        assert!(!result.has_incoming_changes);
        assert!(!result.has_outgoing_changes);
        assert!(!result.has_more);
        assert_eq!(*call_count.borrow(), 1);
    }

    #[test]
    fn parse_git_history_log_is_reachable_from_loader_module() {
        // Mirrors the TS re-export surface (`git-history.ts` re-exports the parser).
        assert!(parse_git_history_log("").is_empty());
    }
}
