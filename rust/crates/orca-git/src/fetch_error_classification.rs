//! Fetch error classification, ported from
//! `src/main/git/fetch-error-classification.ts`.

/// True for "missing remote ref" fetch failures (not auth/network errors).
pub fn is_missing_remote_ref_git_error(message: &str) -> bool {
    let normalized = message.to_lowercase();
    normalized.contains("could not find remote ref")
        || normalized.contains("couldn't find remote ref")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_missing_remote_ref_messages() {
        assert!(is_missing_remote_ref_git_error(
            "fatal: could not find remote ref refs/heads/feature/test"
        ));
        assert!(is_missing_remote_ref_git_error(
            "fatal: couldn't find remote ref refs/heads/feature/test"
        ));
    }

    #[test]
    fn does_not_match_auth_or_network_failures() {
        assert!(!is_missing_remote_ref_git_error("fatal: Authentication failed"));
        assert!(!is_missing_remote_ref_git_error(
            "fatal: unable to access repo: Could not resolve host"
        ));
    }
}
