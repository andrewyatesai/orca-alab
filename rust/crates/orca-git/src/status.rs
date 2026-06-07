//! `git status --porcelain=v2` parsing, ported from the parse loop in
//! `src/main/git/status.ts`. Pure over the status output + an injected
//! `exists` predicate (the conflict-compat fs check), so it is fully testable;
//! the production `getStatus` binds `exists` to the worktree filesystem.

use crate::status_parse::{
    parse_branch_ahead_behind, parse_conflict_kind, parse_status_char, GitConflictKind,
    GitFileStatus,
};
use orca_core::git_cquoted_path::decode_git_cquoted_path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GitStagingArea {
    Staged,
    Unstaged,
    Untracked,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitStatusEntry {
    pub path: String,
    pub status: GitFileStatus,
    pub area: GitStagingArea,
    pub old_path: Option<String>,
    pub conflict_kind: Option<GitConflictKind>,
    pub conflict_status: Option<&'static str>,
}

impl GitStatusEntry {
    fn change(path: String, status: GitFileStatus, area: GitStagingArea, old_path: Option<String>) -> Self {
        Self { path, status, area, old_path, conflict_kind: None, conflict_status: None }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParsedStatus {
    pub head: Option<String>,
    pub branch: Option<String>,
    pub upstream_name: Option<String>,
    pub ahead_behind: Option<(i64, i64)>,
    pub entries: Vec<GitStatusEntry>,
    pub ignored_paths: Vec<String>,
}

/// Rendering-compatibility status for a conflict entry (not a semantic claim) —
/// `modified` when a working-tree file exists, `deleted` otherwise.
fn conflict_compat_status(
    path: &str,
    kind: GitConflictKind,
    exists: &dyn Fn(&str) -> bool,
) -> GitFileStatus {
    match kind {
        GitConflictKind::BothModified | GitConflictKind::BothAdded => GitFileStatus::Modified,
        GitConflictKind::BothDeleted => GitFileStatus::Deleted,
        _ => {
            if exists(path) {
                GitFileStatus::Modified
            } else {
                GitFileStatus::Deleted
            }
        }
    }
}

fn parse_unmerged_entry(line: &str, exists: &dyn Fn(&str) -> bool) -> Option<GitStatusEntry> {
    // `u <XY> <sub> <m1> <m2> <m3> <mW> <h1> <h2> <h3> <path>` (space-separated;
    // path may contain spaces, so it starts at field index 10).
    let parts: Vec<&str> = line.split(' ').collect();
    let xy = *parts.get(1)?;
    let mode1 = parts.get(3).copied().unwrap_or("");
    let mode2 = parts.get(4).copied().unwrap_or("");
    let mode3 = parts.get(5).copied().unwrap_or("");
    if parts.len() <= 10 {
        return None;
    }
    let file_path = decode_git_cquoted_path(&parts[10..].join(" "));
    if file_path.is_empty() {
        return None;
    }
    // Submodule conflicts (mode 160000) are out of scope.
    if [mode1, mode2, mode3].contains(&"160000") {
        return None;
    }
    let conflict_kind = parse_conflict_kind(xy)?;
    Some(GitStatusEntry {
        status: conflict_compat_status(&file_path, conflict_kind, exists),
        path: file_path,
        area: GitStagingArea::Unstaged,
        old_path: None,
        conflict_kind: Some(conflict_kind),
        conflict_status: Some("unresolved"),
    })
}

pub fn parse_porcelain_v2_status(stdout: &str, exists: &dyn Fn(&str) -> bool) -> ParsedStatus {
    let mut result = ParsedStatus::default();

    for raw in stdout.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if line.is_empty() {
            continue;
        }

        if let Some(oid) = line.strip_prefix("# branch.oid ") {
            result.head = Some(oid.trim().to_string());
        } else if let Some(head) = line.strip_prefix("# branch.head ") {
            let head = head.trim();
            result.branch = if !head.is_empty() && head != "(detached)" {
                Some(format!("refs/heads/{head}"))
            } else {
                None
            };
        } else if let Some(upstream) = line.strip_prefix("# branch.upstream ") {
            let upstream = upstream.trim();
            result.upstream_name = (!upstream.is_empty()).then(|| upstream.to_string());
        } else if line.starts_with("# branch.ab ") {
            result.ahead_behind = parse_branch_ahead_behind(line);
        } else if line.starts_with("1 ") || line.starts_with("2 ") {
            parse_change_entry(line, &mut result.entries);
        } else if let Some(path) = line.strip_prefix("? ") {
            result.entries.push(GitStatusEntry::change(
                decode_git_cquoted_path(path),
                GitFileStatus::Untracked,
                GitStagingArea::Untracked,
                None,
            ));
        } else if let Some(path) = line.strip_prefix("! ") {
            result.ignored_paths.push(decode_git_cquoted_path(path));
        } else if line.starts_with("u ") {
            if let Some(entry) = parse_unmerged_entry(line, exists) {
                result.entries.push(entry);
            }
        }
    }

    result
}

fn parse_change_entry(line: &str, entries: &mut Vec<GitStatusEntry>) {
    let parts: Vec<&str> = line.split(' ').collect();
    let Some(xy) = parts.get(1) else { return };
    let mut chars = xy.chars();
    let index_status = chars.next().unwrap_or('.');
    let worktree_status = chars.next().unwrap_or('.');

    let (path, old_path) = if line.starts_with("2 ") {
        // type-2 (rename/copy): new path after 9 space fields, old path after the tab.
        let tab_parts: Vec<&str> = line.split('\t').collect();
        let before_tab: Vec<&str> = tab_parts[0].split(' ').collect();
        let new_path = decode_git_cquoted_path(&before_tab[9.min(before_tab.len())..].join(" "));
        let old = decode_git_cquoted_path(&tab_parts[1..].join("\t"));
        (new_path, Some(old))
    } else {
        (decode_git_cquoted_path(&parts[8.min(parts.len())..].join(" ")), None)
    };

    if index_status != '.' {
        entries.push(GitStatusEntry::change(
            path.clone(),
            parse_status_char(index_status),
            GitStagingArea::Staged,
            old_path.clone(),
        ));
    }
    if worktree_status != '.' {
        entries.push(GitStatusEntry::change(
            path,
            parse_status_char(worktree_status),
            GitStagingArea::Unstaged,
            old_path,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(
        path: &str,
        status: GitFileStatus,
        area: GitStagingArea,
        old_path: Option<&str>,
    ) -> GitStatusEntry {
        GitStatusEntry {
            path: path.to_string(),
            status,
            area,
            old_path: old_path.map(str::to_string),
            conflict_kind: None,
            conflict_status: None,
        }
    }

    fn no_files(_: &str) -> bool {
        false
    }

    #[test]
    fn parses_branch_headers() {
        let out = "# branch.oid abc123\n# branch.head feature/x\n# branch.upstream origin/feature/x\n# branch.ab +2 -1\n";
        let parsed = parse_porcelain_v2_status(out, &no_files);
        assert_eq!(parsed.head.as_deref(), Some("abc123"));
        assert_eq!(parsed.branch.as_deref(), Some("refs/heads/feature/x"));
        assert_eq!(parsed.upstream_name.as_deref(), Some("origin/feature/x"));
        assert_eq!(parsed.ahead_behind, Some((2, 1)));
    }

    #[test]
    fn detached_head_clears_branch() {
        let parsed = parse_porcelain_v2_status("# branch.head (detached)\n", &no_files);
        assert_eq!(parsed.branch, None);
    }

    #[test]
    fn parses_type1_staged_unstaged_and_both() {
        let out = "1 M. N... 100644 100644 100644 aaa bbb staged.rs\n\
                   1 .M N... 100644 100644 100644 aaa bbb work.rs\n\
                   1 MM N... 100644 100644 100644 aaa bbb both.rs\n";
        let parsed = parse_porcelain_v2_status(out, &no_files);
        assert_eq!(
            parsed.entries,
            vec![
                entry("staged.rs", GitFileStatus::Modified, GitStagingArea::Staged, None),
                entry("work.rs", GitFileStatus::Modified, GitStagingArea::Unstaged, None),
                entry("both.rs", GitFileStatus::Modified, GitStagingArea::Staged, None),
                entry("both.rs", GitFileStatus::Modified, GitStagingArea::Unstaged, None),
            ]
        );
    }

    #[test]
    fn parses_type2_rename_with_old_path() {
        let out = "2 R. N... 100644 100644 100644 aaa bbb R100 new name.rs\told.rs\n";
        let parsed = parse_porcelain_v2_status(out, &no_files);
        assert_eq!(
            parsed.entries,
            vec![entry("new name.rs", GitFileStatus::Renamed, GitStagingArea::Staged, Some("old.rs"))]
        );
    }

    #[test]
    fn parses_untracked_and_ignored() {
        let out = "? untracked.txt\n! build/out.o\n";
        let parsed = parse_porcelain_v2_status(out, &no_files);
        assert_eq!(
            parsed.entries,
            vec![entry("untracked.txt", GitFileStatus::Untracked, GitStagingArea::Untracked, None)]
        );
        assert_eq!(parsed.ignored_paths, vec!["build/out.o".to_string()]);
    }

    #[test]
    fn parses_unmerged_conflict_entries() {
        let out = "u UU N... 100644 100644 100644 100644 h1 h2 h3 conflict.rs\n";
        let parsed = parse_porcelain_v2_status(out, &no_files);
        assert_eq!(parsed.entries.len(), 1);
        let e = &parsed.entries[0];
        assert_eq!(e.path, "conflict.rs");
        assert_eq!(e.conflict_kind, Some(GitConflictKind::BothModified));
        assert_eq!(e.status, GitFileStatus::Modified); // both_modified → modified regardless of fs
        assert_eq!(e.conflict_status, Some("unresolved"));
    }

    #[test]
    fn deleted_by_us_conflict_uses_fs_existence() {
        let out = "u DU N... 100644 100644 100644 100644 h1 h2 h3 gone.rs\n";
        // exists=false → deleted
        let parsed = parse_porcelain_v2_status(out, &no_files);
        assert_eq!(parsed.entries[0].status, GitFileStatus::Deleted);
        // exists=true → modified
        let present = |_: &str| true;
        let parsed = parse_porcelain_v2_status(out, &present);
        assert_eq!(parsed.entries[0].status, GitFileStatus::Modified);
    }

    #[test]
    fn skips_submodule_unmerged_entries() {
        let out = "u UU N... 160000 160000 160000 100644 h1 h2 h3 submodule\n";
        let parsed = parse_porcelain_v2_status(out, &no_files);
        assert!(parsed.entries.is_empty());
    }

    #[test]
    fn decodes_cquoted_utf8_paths() {
        // git c-quotes a UTF-8 "é" (0xC3 0xA9) as \303\251 when quotePath is on.
        let out = "1 .M N... 100644 100644 100644 aaa bbb \"caf\\303\\251.txt\"\n";
        let parsed = parse_porcelain_v2_status(out, &no_files);
        assert_eq!(parsed.entries[0].path, "caf\u{00c3}\u{00a9}.txt");
    }
}
