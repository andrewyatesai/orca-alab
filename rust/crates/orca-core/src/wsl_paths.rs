//! WSL UNC path parsing, ported from `src/shared/wsl-paths.ts`.
//!
//! Recognises `\\wsl.localhost\<distro>\...` and the legacy `\\wsl$\<distro>\...`
//! forms and extracts the distro + Linux path. Deliberately platform-check-free
//! so the same logic runs when resolving paths for a remote Windows host.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WslUncPathInfo {
    pub distro: String,
    pub linux_path: String,
}

pub fn parse_wsl_unc_path(path: &str) -> Option<WslUncPathInfo> {
    // `^//(wsl\.localhost|wsl\$)/([^/]+)(/.*)?$` (case-insensitive) over the
    // backslash-normalised input.
    let normalized = path.replace('\\', "/");
    let rest = normalized.strip_prefix("//")?;
    let rest_lower = rest.to_lowercase();
    let prefix_len = if rest_lower.starts_with("wsl.localhost/") {
        "wsl.localhost".len()
    } else if rest_lower.starts_with("wsl$/") {
        "wsl$".len()
    } else {
        return None;
    };

    // Skip the matched prefix and its trailing '/'.
    let after_prefix = &rest[prefix_len + 1..];
    let (distro, remainder) = match after_prefix.find('/') {
        Some(i) => (&after_prefix[..i], &after_prefix[i..]),
        None => (after_prefix, ""),
    };
    if distro.is_empty() {
        return None;
    }

    Some(WslUncPathInfo {
        distro: distro.to_string(),
        linux_path: if remainder.is_empty() {
            "/".to_string()
        } else {
            remainder.to_string()
        },
    })
}

pub fn is_wsl_unc_path(path: &str) -> bool {
    parse_wsl_unc_path(path).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_modern_and_legacy_wsl_unc_paths_without_platform_checks() {
        assert_eq!(
            parse_wsl_unc_path("\\\\wsl.localhost\\Ubuntu\\home\\jin\\repo"),
            Some(WslUncPathInfo {
                distro: "Ubuntu".to_string(),
                linux_path: "/home/jin/repo".to_string(),
            })
        );
        assert_eq!(
            parse_wsl_unc_path("\\\\wsl$\\Debian\\home\\jin"),
            Some(WslUncPathInfo {
                distro: "Debian".to_string(),
                linux_path: "/home/jin".to_string(),
            })
        );
    }

    #[test]
    fn rejects_ordinary_windows_and_posix_paths() {
        assert!(!is_wsl_unc_path("C:\\Users\\jin\\repo"));
        assert!(!is_wsl_unc_path("/home/jin/repo"));
    }
}
