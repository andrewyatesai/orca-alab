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
    fn branch_ahead_behind_parsing() {
        assert_eq!(parse_branch_ahead_behind("# branch.ab +2 -3"), Some((2, 3)));
        assert_eq!(parse_branch_ahead_behind("# branch.ab +0 -0"), Some((0, 0)));
        assert_eq!(parse_branch_ahead_behind("# branch.oid abcdef"), None);
        assert_eq!(parse_branch_ahead_behind("# branch.ab +2 -3 extra"), None);
        assert_eq!(parse_branch_ahead_behind("# branch.ab 2 -3"), None);
    }
}
