//! Clone-path derivation + validation, ported from the pure parts of
//! `src/main/git/repo-clone-path.ts`. Security-relevant: derives the repo
//! folder name from a URL and refuses any path that would escape the
//! destination directory. Platform is a parameter (the TS reads
//! `process.platform`) so it's host-independent and testable. The fs claim/
//! cleanup helpers stay in the IO layer.

use orca_core::cross_platform_path::{
    is_runtime_path_absolute, is_windows_absolute_path_like, normalize_runtime_path_for_comparison,
    normalize_runtime_path_separators, relative_path_inside_root, PathFlavor,
};

fn starts_with_windows_drive(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/')
}

/// Strip a trailing `.git` or `.git/` (the TS `/\.git\/?$/`).
fn strip_git_suffix(url: &str) -> &str {
    if let Some(stripped) = url.strip_suffix(".git/") {
        stripped
    } else if let Some(stripped) = url.strip_suffix(".git") {
        stripped
    } else {
        url
    }
}

fn basename(source: &str, windows_local: bool) -> &str {
    let separators: &[char] = if windows_local { &['\\', '/'] } else { &['/'] };
    source.trim_end_matches(separators).rsplit(separators).next().unwrap_or("")
}

/// Derive `<destination>/<repoName>` for `git clone`, validating that the
/// destination is absolute (in the given flavour) and the result stays inside
/// it. Returns the clone path or an error message.
pub fn derive_validated_clone_path(
    url: &str,
    destination: &str,
    platform: PathFlavor,
) -> Result<String, String> {
    if destination.is_empty()
        || !is_runtime_path_absolute(destination, Some(platform))
        || (platform == PathFlavor::Posix && is_windows_absolute_path_like(destination))
    {
        return Err("Clone destination must be an absolute path".to_string());
    }

    let source = strip_git_suffix(url);
    let windows_local = starts_with_windows_drive(source) || source.starts_with("\\\\");
    let repo_name = basename(source, windows_local);
    if repo_name.is_empty()
        || repo_name == "."
        || repo_name == ".."
        || repo_name.contains('/')
        || repo_name.contains('\\')
    {
        return Err("Invalid repository name derived from URL".to_string());
    }

    let separator = if platform == PathFlavor::Windows { '\\' } else { '/' };
    let clone_path = format!("{}{separator}{repo_name}", destination.trim_end_matches(['/', '\\']));

    // The repo name has no separators and isn't `.`/`..`, so this is always
    // strictly inside — but verify, as a backstop against traversal.
    match relative_path_inside_root(destination, &clone_path) {
        Some(rel) if !rel.is_empty() => Ok(clone_path),
        _ => Err("Clone path must be inside the destination directory".to_string()),
    }
}

/// `^//(?:wsl\.localhost|wsl\$)/([^/]+)(/.*)?$` → (distro, linux-path-or-"").
fn match_wsl_unc(normalized: &str) -> Option<(String, String)> {
    let rest = normalized.strip_prefix("//")?;
    let lower = rest.to_lowercase();
    let prefix_len = if lower.starts_with("wsl.localhost/") {
        "wsl.localhost".len()
    } else if lower.starts_with("wsl$/") {
        "wsl$".len()
    } else {
        return None;
    };
    let after = &rest[prefix_len + 1..];
    let (distro, linux) = match after.find('/') {
        Some(i) => (&after[..i], &after[i..]),
        None => (after, ""),
    };
    if distro.is_empty() {
        return None;
    }
    Some((distro.to_string(), linux.to_string()))
}

/// Stable key for comparing clone paths. WSL UNC paths case-fold only the
/// Windows server alias + distro (the Linux path is case-sensitive); other
/// paths use the cross-platform comparison normalisation.
pub fn get_clone_path_comparison_key(clone_path: &str) -> String {
    // (For non-Windows-absolute relative paths the TS resolves against cwd
    // first; we compare the path as given — callers pass absolute paths.)
    let normalized = normalize_runtime_path_separators(clone_path);
    if let Some((distro, linux)) = match_wsl_unc(&normalized) {
        return format!("//wsl/{}{}", distro.to_lowercase(), linux.trim_end_matches('/'));
    }
    normalize_runtime_path_for_comparison(clone_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_repo_names_starting_with_two_dots() {
        assert_eq!(
            derive_validated_clone_path("https://example.com/..repo.git", "/tmp/orca-test", PathFlavor::Posix),
            Ok("/tmp/orca-test/..repo".to_string())
        );
    }

    #[test]
    fn derives_normal_repo_name() {
        assert_eq!(
            derive_validated_clone_path("https://github.com/owner/repo.git", "/home/me/src", PathFlavor::Posix),
            Ok("/home/me/src/repo".to_string())
        );
    }

    #[test]
    fn rejects_windows_looking_destinations_on_posix() {
        for destination in ["C:\\Users\\me\\src", "\\\\server\\share", "//server/share", "//wsl.localhost/Ubuntu/home/me"] {
            assert_eq!(
                derive_validated_clone_path("https://example.com/orca.git", destination, PathFlavor::Posix),
                Err("Clone destination must be an absolute path".to_string()),
                "destination {destination:?}"
            );
        }
    }

    #[test]
    fn rejects_relative_destination() {
        assert!(derive_validated_clone_path("https://x/r.git", "relative/path", PathFlavor::Posix).is_err());
    }

    #[test]
    fn wsl_comparison_key_folds_alias_not_linux_path() {
        let a = get_clone_path_comparison_key("\\\\wsl.localhost\\Ubuntu\\home\\User\\repo");
        let b = get_clone_path_comparison_key("\\\\wsl$\\ubuntu\\home\\User\\repo");
        assert_eq!(a, b);
        // trailing slash is ignored
        assert_eq!(get_clone_path_comparison_key("\\\\wsl.localhost\\Ubuntu\\home\\User\\repo\\"), b);
        // Linux path casing is significant
        assert_ne!(get_clone_path_comparison_key("\\\\wsl$\\ubuntu\\home\\user\\repo"), b);
    }
}
