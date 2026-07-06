//! Pure `git status --porcelain=v2` field parsers, ported from the parsing
//! helpers in `src/main/git/status.ts`. The full `getStatus` (which also does
//! fs existence checks for conflict-compat classification) builds on these.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GitFileStatus {
    Modified,
    Added,
    Deleted,
    Renamed,
    Untracked,
    Copied,
}

/// Submodule dirtiness flags from a porcelain-v2 `<sub>` field (the `S<c><m><u>`
/// form). Mirrors `GitSubmoduleStatus` in `src/shared/git-status-types.ts`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GitSubmoduleStatus {
    pub commit_changed: bool,
    pub tracked_changes: bool,
    pub untracked_changes: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GitConflictKind {
    BothModified,
    BothAdded,
    BothDeleted,
    AddedByUs,
    AddedByThem,
    DeletedByUs,
    DeletedByThem,
}

/// Map a porcelain-v2 status char (XY field) to a file status; unknown → modified.
/// (Branch-change records use the same mapping in the TS source.)
pub fn parse_status_char(char: char) -> GitFileStatus {
    match char {
        'M' => GitFileStatus::Modified,
        'A' => GitFileStatus::Added,
        'D' => GitFileStatus::Deleted,
        'R' => GitFileStatus::Renamed,
        'C' => GitFileStatus::Copied,
        _ => GitFileStatus::Modified,
    }
}

/// Map a porcelain-v2 unmerged `XY` pair to a conflict kind, or `None` for
/// records we don't model (e.g. submodule states).
pub fn parse_conflict_kind(xy: &str) -> Option<GitConflictKind> {
    match xy {
        "UU" => Some(GitConflictKind::BothModified),
        "AA" => Some(GitConflictKind::BothAdded),
        "DD" => Some(GitConflictKind::BothDeleted),
        "AU" => Some(GitConflictKind::AddedByUs),
        "UA" => Some(GitConflictKind::AddedByThem),
        "DU" => Some(GitConflictKind::DeletedByUs),
        "UD" => Some(GitConflictKind::DeletedByThem),
        _ => None,
    }
}

/// Parse a porcelain-v2 `<sub>` field: `None` unless it starts with `S`, then
/// the three dirtiness flags by fixed position. The field is ASCII, so byte
/// indexing is exact. `status_char` is this entry's XY char for its area (index
/// for staged, worktree for unstaged) — a bare `S...` gitlink that git reports as
/// modified is a commit change, matching the TS parsers (the flag is area-
/// sensitive, so this must be computed per-area, not once per line).
pub fn parse_submodule_status(field: Option<&str>, status_char: char) -> Option<GitSubmoduleStatus> {
    let field = field?;
    if !field.starts_with('S') {
        return None;
    }
    let b = field.as_bytes();
    Some(GitSubmoduleStatus {
        commit_changed: b.get(1) == Some(&b'C') || (field == "S..." && status_char == 'M'),
        tracked_changes: b.get(2) == Some(&b'M'),
        untracked_changes: b.get(3) == Some(&b'U'),
    })
}

/// Parse a `# branch.ab +<ahead> -<behind>` header line.
pub fn parse_branch_ahead_behind(line: &str) -> Option<(i64, i64)> {
    let rest = line.strip_prefix("# branch.ab ")?;
    let mut parts = rest.split(' ');
    let ahead = parts.next()?.strip_prefix('+')?.parse::<i64>().ok()?;
    let behind = parts.next()?.strip_prefix('-')?.parse::<i64>().ok()?;
    if parts.next().is_some() {
        return None; // the TS regex is anchored ($) — no trailing fields
    }
    Some((ahead, behind))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_char_mapping_with_modified_default() {
        assert_eq!(parse_status_char('M'), GitFileStatus::Modified);
        assert_eq!(parse_status_char('A'), GitFileStatus::Added);
        assert_eq!(parse_status_char('D'), GitFileStatus::Deleted);
        assert_eq!(parse_status_char('R'), GitFileStatus::Renamed);
        assert_eq!(parse_status_char('C'), GitFileStatus::Copied);
        assert_eq!(parse_status_char('.'), GitFileStatus::Modified);
        assert_eq!(parse_status_char('?'), GitFileStatus::Modified);
    }

    #[test]
    fn conflict_kind_mapping() {
        assert_eq!(parse_conflict_kind("UU"), Some(GitConflictKind::BothModified));
        assert_eq!(parse_conflict_kind("AA"), Some(GitConflictKind::BothAdded));
        assert_eq!(parse_conflict_kind("DD"), Some(GitConflictKind::BothDeleted));
        assert_eq!(parse_conflict_kind("AU"), Some(GitConflictKind::AddedByUs));
        assert_eq!(parse_conflict_kind("UA"), Some(GitConflictKind::AddedByThem));
        assert_eq!(parse_conflict_kind("DU"), Some(GitConflictKind::DeletedByUs));
        assert_eq!(parse_conflict_kind("UD"), Some(GitConflictKind::DeletedByThem));
        assert_eq!(parse_conflict_kind("M."), None);
        assert_eq!(parse_conflict_kind(""), None);
    }

    #[test]
    fn submodule_status_parsing() {
        assert_eq!(parse_submodule_status(None, '.'), None);
        assert_eq!(parse_submodule_status(Some("N..."), '.'), None);
        assert_eq!(parse_submodule_status(Some("...."), '.'), None);
        assert_eq!(
            parse_submodule_status(Some("S..U"), '.'),
            Some(GitSubmoduleStatus {
                commit_changed: false,
                tracked_changes: false,
                untracked_changes: true
            })
        );
        assert_eq!(
            parse_submodule_status(Some("SCMU"), '.'),
            Some(GitSubmoduleStatus {
                commit_changed: true,
                tracked_changes: true,
                untracked_changes: true
            })
        );
    }

    #[test]
    fn bare_gitlink_reported_modified_is_a_commit_change() {
        // A `S...` gitlink (all sub-flags dot) that git reports as modified in
        // this area is a commit change — matches the TS `submoduleField === 'S...'
        // && statusChar === 'M'` special case. Only for the exact `M` char.
        assert_eq!(
            parse_submodule_status(Some("S..."), 'M'),
            Some(GitSubmoduleStatus {
                commit_changed: true,
                tracked_changes: false,
                untracked_changes: false
            })
        );
        // Not modified (e.g. the other area's `.`) -> no commit change.
        assert_eq!(
            parse_submodule_status(Some("S..."), '.').unwrap().commit_changed,
            false
        );
    }

    #[test]
    fn branch_ahead_behind_parsing() {
        assert_eq!(parse_branch_ahead_behind("# branch.ab +2 -3"), Some((2, 3)));
        assert_eq!(parse_branch_ahead_behind("# branch.ab +0 -0"), Some((0, 0)));
        assert_eq!(parse_branch_ahead_behind("# branch.oid abcdef"), None);
        assert_eq!(parse_branch_ahead_behind("# branch.ab +2 -3 extra"), None);
        assert_eq!(parse_branch_ahead_behind("# branch.ab 2 -3"), None);
    }
}
