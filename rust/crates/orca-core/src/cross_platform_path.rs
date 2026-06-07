//! Cross-platform path containment and resolution, ported from
//! `src/shared/cross-platform-path.ts`.
//!
//! These helpers deliberately operate on path *strings* with an explicit POSIX
//! or Windows flavour rather than the host `std::path`, because Orca resolves
//! paths for *remote* hosts (SSH/WSL) whose separator and drive semantics differ
//! from the machine running the code. The behaviour is byte-for-byte matched to
//! the TypeScript source so local and remote runtimes agree on containment.

/// Whether a path string should be treated with Windows semantics, regardless of
/// the host platform.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathFlavor {
    Posix,
    Windows,
}

/// `^[A-Za-z]:[\\/]` — a drive-letter prefix followed by a separator.
fn starts_with_windows_drive(value: &str) -> bool {
    let b = value.as_bytes();
    b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/')
}

/// `/^[A-Za-z]:\/$/` — exactly `X:/`, a bare drive root.
fn is_drive_root(value: &str) -> bool {
    let b = value.as_bytes();
    b.len() == 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'/'
}

pub fn is_windows_absolute_path_like(value: &str) -> bool {
    starts_with_windows_drive(value) || value.starts_with("\\\\") || value.starts_with("//")
}

/// Collapse runs of `/` into a single `/`.
fn collapse_slashes(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut prev_slash = false;
    for ch in value.chars() {
        if ch == '/' {
            if !prev_slash {
                out.push(ch);
            }
            prev_slash = true;
        } else {
            out.push(ch);
            prev_slash = false;
        }
    }
    out
}

pub fn normalize_runtime_path_separators(value: &str) -> String {
    let normalized = collapse_slashes(&value.replace('\\', "/"));
    if value.starts_with("\\\\") || value.starts_with("//") {
        format!("//{}", normalized.trim_start_matches('/'))
    } else {
        normalized
    }
}

fn trim_runtime_path_trailing_slash(value: &str) -> String {
    if value == "/" || is_drive_root(value) {
        value.to_string()
    } else {
        value.trim_end_matches('/').to_string()
    }
}

pub fn normalize_runtime_path_for_comparison(value: &str) -> String {
    let normalized = trim_runtime_path_trailing_slash(&normalize_runtime_path_separators(value));
    if is_windows_absolute_path_like(value) {
        normalized.to_lowercase()
    } else {
        normalized
    }
}

fn is_windows_path_flavor(value: &str) -> bool {
    starts_with_windows_drive(value) || value.contains('\\') || value.starts_with("//")
}

fn flavor_of(value: &str) -> PathFlavor {
    if is_windows_path_flavor(value) {
        PathFlavor::Windows
    } else {
        PathFlavor::Posix
    }
}

/// Whether `value` is absolute under the given flavour. When `flavor` is `None`
/// it is inferred from `value`, matching the TS default parameter.
pub fn is_runtime_path_absolute(value: &str, flavor: Option<PathFlavor>) -> bool {
    let flavor = flavor.unwrap_or_else(|| flavor_of(value));
    match flavor {
        PathFlavor::Windows => {
            starts_with_windows_drive(value) || value.starts_with('\\') || value.starts_with('/')
        }
        PathFlavor::Posix => value.starts_with('/'),
    }
}

pub fn resolve_runtime_path(base_path: &str, target_path: &str) -> String {
    let flavor = if is_windows_path_flavor(base_path) || is_windows_path_flavor(target_path) {
        PathFlavor::Windows
    } else {
        PathFlavor::Posix
    };
    if is_runtime_path_absolute(target_path, Some(flavor)) {
        return normalize_runtime_path_dots(target_path, flavor);
    }
    let base = trim_runtime_path_trailing_slash(&normalize_runtime_path_separators(base_path));
    normalize_runtime_path_dots(&format!("{base}/{target_path}"), flavor)
}

pub fn get_runtime_path_basename(value: &str) -> String {
    let trimmed = value.trim_end_matches(['\\', '/']);
    if trimmed.is_empty() {
        return String::new();
    }
    trimmed
        .split(['\\', '/'])
        .rfind(|s| !s.is_empty())
        .unwrap_or("")
        .to_string()
}

pub fn is_path_inside_or_equal(root_path: &str, candidate_path: &str) -> bool {
    let root = normalize_runtime_path_for_comparison(root_path);
    let candidate = normalize_runtime_path_for_comparison(candidate_path);
    if candidate == root {
        return true;
    }
    let root_with_boundary = if root == "/" || is_drive_root(&root) {
        root
    } else {
        format!("{}/", root.trim_end_matches('/'))
    };
    candidate.starts_with(&root_with_boundary)
}

/// Returns the path of `candidate_path` relative to `root_path`, or `None` if it
/// is not contained. An exact match returns `Some("")`.
pub fn relative_path_inside_root(root_path: &str, candidate_path: &str) -> Option<String> {
    let normalized_root =
        trim_runtime_path_trailing_slash(&normalize_runtime_path_separators(root_path));
    let normalized_candidate =
        trim_runtime_path_trailing_slash(&normalize_runtime_path_separators(candidate_path));
    let windows = is_windows_absolute_path_like(root_path);
    let comparison_root = if windows {
        normalized_root.to_lowercase()
    } else {
        normalized_root.clone()
    };
    let comparison_candidate = if windows {
        normalized_candidate.to_lowercase()
    } else {
        normalized_candidate.clone()
    };
    if comparison_candidate == comparison_root {
        return Some(String::new());
    }
    let is_root = comparison_root == "/" || is_drive_root(&comparison_root);
    let comparison_prefix = if is_root {
        comparison_root.clone()
    } else {
        format!("{comparison_root}/")
    };
    if !comparison_candidate.starts_with(&comparison_prefix) {
        return None;
    }
    // Slice the original-cased candidate by the prefix length. Path prefixes are
    // ASCII separators/drives here, so char-count slicing matches the TS
    // UTF-16-unit `.slice` for the cases we support.
    let skip = comparison_prefix.chars().count();
    Some(normalized_candidate.chars().skip(skip).collect())
}

fn normalize_runtime_path_dots(value: &str, flavor: PathFlavor) -> String {
    let normalized = normalize_runtime_path_separators(value);
    let (root, rest) = split_runtime_path_root(&normalized, flavor);
    let mut segments: Vec<&str> = Vec::new();
    for segment in rest.split('/') {
        if segment.is_empty() || segment == "." {
            continue;
        }
        if segment == ".." {
            // Panic-free (no unwrap) so Trust can verify panic-safety: an empty
            // stack or a top-of-stack `..` both fall through to the else branch.
            if segments.last().is_some_and(|last| *last != "..") {
                segments.pop();
            } else if root.is_empty() {
                segments.push("..");
            }
            continue;
        }
        segments.push(segment);
    }
    let suffix = segments.join("/");
    if root.is_empty() {
        if suffix.is_empty() {
            ".".to_string()
        } else {
            suffix
        }
    } else if !suffix.is_empty() {
        format!("{root}{suffix}")
    } else {
        trim_runtime_path_trailing_slash(&root)
    }
}

fn split_runtime_path_root(value: &str, flavor: PathFlavor) -> (String, String) {
    if flavor == PathFlavor::Windows {
        let b = value.as_bytes();
        // `^([A-Za-z]:)(?:\/|$)`
        if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
            if b.len() == 2 {
                return (format!("{}:/", &value[0..1]), String::new());
            }
            if b[2] == b'/' {
                return (format!("{}:/", &value[0..1]), value[3..].to_string());
            }
        }
        if let Some(stripped) = value.strip_prefix("//") {
            let parts: Vec<&str> = stripped.split('/').collect();
            if parts.len() >= 2 && !parts[0].is_empty() && !parts[1].is_empty() {
                let root = format!("//{}/{}/", parts[0], parts[1]);
                return (root, parts[2..].join("/"));
            }
            return ("//".to_string(), stripped.to_string());
        }
        if let Some(stripped) = value.strip_prefix('/') {
            return ("/".to_string(), stripped.to_string());
        }
    }
    if let Some(stripped) = value.strip_prefix('/') {
        return ("/".to_string(), stripped.to_string());
    }
    (String::new(), value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Ported verbatim from src/shared/cross-platform-path.test.ts.

    #[test]
    fn keeps_posix_sibling_prefixes_outside_the_root() {
        assert!(is_path_inside_or_equal("/repo/app", "/repo/app"));
        assert!(is_path_inside_or_equal("/repo/app", "/repo/app/src/index.ts"));
        assert!(!is_path_inside_or_equal(
            "/repo/app",
            "/repo/application/src/index.ts"
        ));
        assert_eq!(
            relative_path_inside_root("/repo/app/", "/repo/app/src/index.ts"),
            Some("src/index.ts".to_string())
        );
    }

    #[test]
    fn handles_windows_drive_roots_and_sibling_drives_case_insensitively() {
        assert!(is_path_inside_or_equal("C:\\Repo", "c:\\repo\\src\\index.ts"));
        assert_eq!(
            relative_path_inside_root("C:\\Repo", "c:\\repo\\src\\index.ts"),
            Some("src/index.ts".to_string())
        );
        assert!(!is_path_inside_or_equal(
            "C:\\Repo",
            "D:\\Repo\\src\\index.ts"
        ));
        assert_eq!(
            relative_path_inside_root("C:\\", "c:\\repo\\src\\index.ts"),
            Some("repo/src/index.ts".to_string())
        );
    }

    #[test]
    fn handles_unc_roots_trailing_slashes_mixed_separators_and_case() {
        assert!(is_path_inside_or_equal(
            "\\\\Server\\Share\\Repo\\",
            "//server/share/repo/src"
        ));
        assert_eq!(
            relative_path_inside_root("\\\\Server\\Share\\Repo\\", "//server/share/repo/src"),
            Some("src".to_string())
        );
        assert!(!is_path_inside_or_equal(
            "\\\\Server\\Share\\Repo",
            "\\\\server\\share\\repo2"
        ));
    }

    #[test]
    fn resolves_posix_relative_paths_without_using_the_process_cwd() {
        assert_eq!(
            resolve_runtime_path("/repos/app/repo", "../worktrees/feature"),
            "/repos/app/worktrees/feature"
        );
        assert_eq!(
            resolve_runtime_path("/repos/app/repo", "/custom/worktrees"),
            "/custom/worktrees"
        );
        assert!(!is_runtime_path_absolute("../worktrees", None));
    }

    #[test]
    fn resolves_windows_relative_paths_with_windows_semantics() {
        assert_eq!(
            resolve_runtime_path("C:\\Repos\\app\\repo", "..\\worktrees\\feature"),
            "C:/Repos/app/worktrees/feature"
        );
        assert_eq!(
            resolve_runtime_path("C:\\Repos\\app\\repo", "D:\\worktrees"),
            "D:/worktrees"
        );
        assert!(is_runtime_path_absolute(
            "/remote/worktrees",
            Some(PathFlavor::Windows)
        ));
    }

    #[test]
    fn basename_strips_trailing_separators() {
        assert_eq!(get_runtime_path_basename("/repo/app/"), "app");
        assert_eq!(get_runtime_path_basename("C:\\repo\\app\\\\"), "app");
        assert_eq!(get_runtime_path_basename(""), "");
    }
}
