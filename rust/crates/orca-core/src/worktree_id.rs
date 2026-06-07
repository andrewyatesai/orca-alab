//! Worktree-id parsing, ported from `src/shared/worktree-id.ts`.
//!
//! A worktree id is `"<repoId>::<worktreePath>"`. Folder projects can back
//! several workspace sessions with the same directory, so their ids carry a
//! `::workspace:<uuid>` suffix that filesystem callers must strip to recover the
//! real folder path.

/// The literal `"::"` separator between repo id and worktree path.
pub const WORKTREE_ID_SEPARATOR: &str = "::";

/// Separator introducing a per-session folder-workspace instance suffix.
pub const FOLDER_WORKSPACE_INSTANCE_SEPARATOR: &str = "::workspace:";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedWorktreeId {
    pub repo_id: String,
    pub worktree_path: String,
}

pub fn get_repo_id_from_worktree_id(worktree_id: &str) -> String {
    match worktree_id.find(WORKTREE_ID_SEPARATOR) {
        Some(i) => worktree_id[..i].to_string(),
        None => worktree_id.to_string(),
    }
}

pub fn split_worktree_id(worktree_id: &str) -> Option<ParsedWorktreeId> {
    let i = worktree_id.find(WORKTREE_ID_SEPARATOR)?;
    Some(ParsedWorktreeId {
        repo_id: worktree_id[..i].to_string(),
        worktree_path: worktree_id[i + WORKTREE_ID_SEPARATOR.len()..].to_string(),
    })
}

/// `::workspace:` followed by exactly 36 `[0-9a-f-]` chars at end of string.
fn is_folder_instance_uuid(after: &str) -> bool {
    after.len() == 36
        && after
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f' | b'-'))
}

fn strip_folder_workspace_instance_suffix(path: &str) -> String {
    let sep = FOLDER_WORKSPACE_INSTANCE_SEPARATOR;
    let mut search_start = 0;
    while let Some(rel) = path[search_start..].find(sep) {
        let pos = search_start + rel;
        let after = &path[pos + sep.len()..];
        if is_folder_instance_uuid(after) {
            return path[..pos].to_string();
        }
        search_start = pos + 1;
    }
    path.to_string()
}

pub fn split_worktree_id_for_filesystem(worktree_id: &str) -> Option<ParsedWorktreeId> {
    let parsed = split_worktree_id(worktree_id)?;
    Some(ParsedWorktreeId {
        repo_id: parsed.repo_id,
        worktree_path: strip_folder_workspace_instance_suffix(&parsed.worktree_path),
    })
}

pub fn get_worktree_path_basename_from_id(worktree_id: &str) -> Option<String> {
    let worktree_path = split_worktree_id_for_filesystem(worktree_id)
        .map(|p| p.worktree_path)
        .unwrap_or_default();
    let normalized_path = worktree_path.trim().trim_end_matches(['\\', '/']);
    if normalized_path.is_empty() {
        return None;
    }
    let basename = normalized_path
        .split(['\\', '/'])
        .rfind(|s| !s.is_empty())
        .map(str::trim)
        .unwrap_or("");
    if basename.is_empty() {
        None
    } else {
        Some(basename.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(repo_id: &str, worktree_path: &str) -> ParsedWorktreeId {
        ParsedWorktreeId {
            repo_id: repo_id.to_string(),
            worktree_path: worktree_path.to_string(),
        }
    }

    #[test]
    fn separator_is_double_colon() {
        assert_eq!(WORKTREE_ID_SEPARATOR, "::");
    }

    #[test]
    fn get_repo_id_cases() {
        assert_eq!(get_repo_id_from_worktree_id("repo-123::/abs/path"), "repo-123");
        assert_eq!(get_repo_id_from_worktree_id("just-a-repo-id"), "just-a-repo-id");
        assert_eq!(get_repo_id_from_worktree_id(""), "");
        assert_eq!(get_repo_id_from_worktree_id("::"), "");
        assert_eq!(get_repo_id_from_worktree_id("::path"), "");
        assert_eq!(get_repo_id_from_worktree_id("repo::"), "repo");
        assert_eq!(get_repo_id_from_worktree_id("repo::a::b"), "repo");
    }

    #[test]
    fn split_worktree_id_cases() {
        assert_eq!(
            split_worktree_id("repo-123::/abs/path"),
            Some(parsed("repo-123", "/abs/path"))
        );
        assert_eq!(split_worktree_id("just-a-repo-id"), None);
        assert_eq!(split_worktree_id(""), None);
        assert_eq!(split_worktree_id("::"), Some(parsed("", "")));
        assert_eq!(split_worktree_id("::path"), Some(parsed("", "path")));
        assert_eq!(split_worktree_id("repo::"), Some(parsed("repo", "")));
        assert_eq!(split_worktree_id("repo::a::b"), Some(parsed("repo", "a::b")));
        assert_eq!(
            split_worktree_id("repo::/folder::workspace:123e4567-e89b-12d3-a456-426614174000"),
            Some(parsed(
                "repo",
                "/folder::workspace:123e4567-e89b-12d3-a456-426614174000"
            ))
        );
    }

    #[test]
    fn split_for_filesystem_strips_instance_suffix() {
        assert_eq!(
            split_worktree_id_for_filesystem(
                "repo::/folder::workspace:123e4567-e89b-12d3-a456-426614174000"
            ),
            Some(parsed("repo", "/folder"))
        );
    }

    #[test]
    fn basename_cases() {
        assert_eq!(
            get_worktree_path_basename_from_id("repo-123::/abs/path/nightly-checks"),
            Some("nightly-checks".to_string())
        );
        assert_eq!(
            get_worktree_path_basename_from_id("repo-123::C:\\workspaces\\nightly-checks"),
            Some("nightly-checks".to_string())
        );
        assert_eq!(
            get_worktree_path_basename_from_id(
                "repo-123::/abs/project::workspace:123e4567-e89b-12d3-a456-426614174000"
            ),
            Some("project".to_string())
        );
        assert_eq!(get_worktree_path_basename_from_id("repo-123"), None);
        assert_eq!(get_worktree_path_basename_from_id("repo-123::"), None);
    }
}
