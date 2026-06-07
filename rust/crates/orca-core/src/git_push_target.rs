//! Git push-target validation, ported from
//! `src/shared/git-push-target-validation.ts`.
//!
//! Persisted PR push targets are replayed into `git push`, so the remote name,
//! branch name, and optional remote URL must be validated to keep a stored
//! target from smuggling path traversal or an arbitrary remote. The TS source
//! throws; here we return `Result<(), String>` with the same messages.

const MAX_REMOTE_NAME_LEN: usize = 100;

fn is_safe_remote_segment(segment: &str) -> bool {
    // `^[A-Za-z0-9][A-Za-z0-9._-]*$`
    let mut chars = segment.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphanumeric() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

fn is_safe_remote_name(remote_name: &str) -> bool {
    if remote_name.is_empty() || remote_name.len() > MAX_REMOTE_NAME_LEN {
        return false;
    }
    // Git accepts slash-separated remote names; each segment must still be a
    // concrete name so a persisted target cannot smuggle `.`/`..` traversal.
    remote_name
        .split('/')
        .all(|seg| seg != "." && seg != ".." && is_safe_remote_segment(seg))
}

fn is_owner_repo(mid: &str) -> bool {
    let parts: Vec<&str> = mid.split('/').collect();
    parts.len() == 2
        && parts.iter().all(|p| {
            !p.is_empty()
                && p.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
        })
}

fn is_github_remote_url(url: &str) -> bool {
    // `^https://github\.com/<owner>/<repo>\.git$` or `^git@github\.com:<owner>/<repo>\.git$`
    let clone = url
        .strip_prefix("https://github.com/")
        .and_then(|r| r.strip_suffix(".git"))
        .is_some_and(is_owner_repo);
    let ssh = url
        .strip_prefix("git@github.com:")
        .and_then(|r| r.strip_suffix(".git"))
        .is_some_and(is_owner_repo);
    clone || ssh
}

/// Validate a push target's fields. `remote_url` is the optional remote URL.
pub fn validate_git_push_target(
    remote_name: &str,
    branch_name: &str,
    remote_url: Option<&str>,
) -> Result<(), String> {
    if !is_safe_remote_name(remote_name) {
        return Err(format!("Invalid git remote name: {remote_name}"));
    }
    if branch_name.is_empty() || branch_name.starts_with('-') {
        return Err(format!("Invalid git branch name: {branch_name}"));
    }
    if let Some(url) = remote_url {
        if !is_github_remote_url(url) {
            return Err("Invalid PR push target remote URL.".to_string());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_slash_separated_git_remote_names() {
        assert!(validate_git_push_target("foo/bar", "feature/fix", None).is_ok());
    }

    #[test]
    fn rejects_remote_names_with_empty_or_parent_segments() {
        let err = validate_git_push_target("foo//bar", "feature/fix", None).unwrap_err();
        assert!(err.contains("Invalid git remote name"), "{err}");
        let err = validate_git_push_target("foo/../bar", "feature/fix", None).unwrap_err();
        assert!(err.contains("Invalid git remote name"), "{err}");
    }

    #[test]
    fn rejects_dash_leading_and_empty_branch_names() {
        assert!(validate_git_push_target("origin", "-rf", None)
            .unwrap_err()
            .contains("Invalid git branch name"));
        assert!(validate_git_push_target("origin", "", None)
            .unwrap_err()
            .contains("Invalid git branch name"));
    }

    #[test]
    fn validates_optional_github_remote_url() {
        assert!(validate_git_push_target(
            "origin",
            "main",
            Some("https://github.com/owner/repo.git")
        )
        .is_ok());
        assert!(validate_git_push_target("origin", "main", Some("git@github.com:owner/repo.git")).is_ok());
        assert!(validate_git_push_target("origin", "main", Some("https://evil.com/owner/repo.git"))
            .unwrap_err()
            .contains("remote URL"));
    }
}
