//! `git worktree list --porcelain` parsing, ported from `parseWorktreeList`
//! in `src/main/git/worktree.ts` (the rest of that file is IO/orchestration
//! built on this pure parser).

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GitWorktreeInfo {
    pub path: String,
    pub head: String,
    pub branch: String,
    pub is_bare: bool,
    pub is_sparse: bool,
    /// True for the repo's main working tree (the first entry git emits).
    pub is_main_worktree: bool,
}

fn split_line_worktree_list(output: &str) -> Vec<Vec<String>> {
    let normalized = output.replace("\r\n", "\n");
    let trimmed = normalized.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    trimmed
        .split("\n\n")
        .map(|block| block.trim().split('\n').map(str::to_string).collect())
        .collect()
}

fn split_nul_worktree_list(output: &str) -> Vec<Vec<String>> {
    if !output.contains('\0') {
        return split_line_worktree_list(output);
    }
    let mut blocks: Vec<Vec<String>> = Vec::new();
    let mut current: Vec<String> = Vec::new();
    for field in output.split('\0') {
        if !field.is_empty() {
            current.push(field.to_string());
        } else if !current.is_empty() {
            blocks.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        blocks.push(current);
    }
    blocks
}

/// Parse `git worktree list --porcelain` output. With `nul_delimited`, the
/// `-z` form is parsed (paths may contain newlines).
pub fn parse_worktree_list(output: &str, nul_delimited: bool) -> Vec<GitWorktreeInfo> {
    let blocks = if nul_delimited {
        split_nul_worktree_list(output)
    } else {
        split_line_worktree_list(output)
    };

    let mut worktrees: Vec<GitWorktreeInfo> = Vec::new();
    for lines in blocks {
        if lines.is_empty() {
            continue;
        }
        let mut info = GitWorktreeInfo::default();
        for line in &lines {
            if let Some(path) = line.strip_prefix("worktree ") {
                info.path = path.to_string();
            } else if let Some(head) = line.strip_prefix("HEAD ") {
                info.head = head.to_string();
            } else if let Some(branch) = line.strip_prefix("branch ") {
                info.branch = branch.to_string();
            } else if line == "bare" {
                info.is_bare = true;
            } else if line == "sparse" {
                info.is_sparse = true;
            }
        }
        if !info.path.is_empty() {
            // git always emits the main working tree first.
            info.is_main_worktree = worktrees.is_empty();
            worktrees.push(info);
        }
    }
    worktrees
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(path: &str, head: &str, branch: &str, is_bare: bool, is_main: bool) -> GitWorktreeInfo {
        GitWorktreeInfo {
            path: path.to_string(),
            head: head.to_string(),
            branch: branch.to_string(),
            is_bare,
            is_sparse: false,
            is_main_worktree: is_main,
        }
    }

    #[test]
    fn parses_regular_and_bare_blocks() {
        let output = "\nworktree /repo\nHEAD abc123\nbranch refs/heads/main\n\nworktree /repo-feature\nHEAD def456\nbranch refs/heads/feature/test\n\nworktree /repo-bare\nHEAD 0000000\nbare\n";
        assert_eq!(
            parse_worktree_list(output, false),
            vec![
                info("/repo", "abc123", "refs/heads/main", false, true),
                info("/repo-feature", "def456", "refs/heads/feature/test", false, false),
                info("/repo-bare", "0000000", "", true, false),
            ]
        );
    }

    #[test]
    fn empty_and_whitespace_input() {
        assert_eq!(parse_worktree_list("", false), Vec::new());
        assert_eq!(parse_worktree_list("   \n\n  \n  ", false), Vec::new());
    }

    #[test]
    fn single_block_is_main() {
        let output = "worktree /single-repo\nHEAD aaa111\nbranch refs/heads/main\n";
        assert_eq!(
            parse_worktree_list(output, false),
            vec![info("/single-repo", "aaa111", "refs/heads/main", false, true)]
        );
    }

    #[test]
    fn detached_head_has_no_branch() {
        let output = "worktree /repo-detached\nHEAD abc123\ndetached\n";
        assert_eq!(
            parse_worktree_list(output, false),
            vec![info("/repo-detached", "abc123", "", false, true)]
        );
    }

    #[test]
    fn captures_path_with_spaces() {
        let output = "worktree /path/to/my worktree\nHEAD ccc333\nbranch refs/heads/main\n";
        assert_eq!(
            parse_worktree_list(output, false),
            vec![info("/path/to/my worktree", "ccc333", "refs/heads/main", false, true)]
        );
    }

    #[test]
    fn handles_extra_blank_lines_between_blocks() {
        let output = "worktree /repo-a\nHEAD aaa111\nbranch refs/heads/main\n\n\nworktree /repo-b\nHEAD bbb222\nbranch refs/heads/dev\n";
        assert_eq!(
            parse_worktree_list(output, false),
            vec![
                info("/repo-a", "aaa111", "refs/heads/main", false, true),
                info("/repo-b", "bbb222", "refs/heads/dev", false, false),
            ]
        );
    }

    #[test]
    fn parses_nul_delimited_with_newline_paths() {
        let output = [
            "worktree /repo",
            "HEAD abc123",
            "branch refs/heads/main",
            "",
            "worktree /repo/linked\nworktree",
            "HEAD def456",
            "branch refs/heads/feature/newline",
            "",
        ]
        .join("\0");
        assert_eq!(
            parse_worktree_list(&output, true),
            vec![
                info("/repo", "abc123", "refs/heads/main", false, true),
                info("/repo/linked\nworktree", "def456", "refs/heads/feature/newline", false, false),
            ]
        );
    }

    #[test]
    fn sparse_flag_is_captured() {
        let output = "worktree /repo\nHEAD abc\nbranch refs/heads/main\nsparse\n";
        let result = parse_worktree_list(output, false);
        assert!(result[0].is_sparse);
    }
}
