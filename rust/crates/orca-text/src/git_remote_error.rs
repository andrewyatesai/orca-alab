//! Git remote error normalisation + credential scrubbing, ported from
//! `src/shared/git-remote-error.ts`.
//!
//! git's stderr often embeds the full remote URL, which can carry a credential.
//! Scrub carefully: `user:password@` on any scheme, plus token-only `user@` on
//! HTTP(S); keep `ssh://git@host` user-info intact (it's required by the
//! remote). Then collapse multi-line stderr to its meaningful tail line.

use regex::Regex;
use std::sync::OnceLock;

fn userpass_url_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // `([a-z][a-z0-9+.-]*://)[^\s/@:]+:[^\s/@]+@` (case-insensitive)
    RE.get_or_init(|| Regex::new(r"(?i)([a-z][a-z0-9+.\-]*://)[^\s/@:]+:[^\s/@]+@").unwrap())
}

fn https_token_url_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)(https?://)[^\s/@:]+@").unwrap())
}

fn no_upstream_phrase_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)no upstream configured|no tracking information|HEAD does not point|Needed a single revision|ambiguous argument 'HEAD@\{u\}'",
        )
        .unwrap()
    })
}

fn fatal_prefix_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // `(^|\n)fatal:` case-insensitive → any line beginning with `fatal:`.
    RE.get_or_init(|| Regex::new(r"(?im)^fatal:").unwrap())
}

pub fn strip_credentials_from_message(message: &str) -> String {
    let once = userpass_url_re().replace_all(message, "${1}");
    https_token_url_re().replace_all(&once, "${1}").into_owned()
}

fn extract_tail_line(message: &str) -> String {
    // The meaningful diagnostic is typically the last non-empty line; the full
    // blob risks leaking local paths / environment details to the UI.
    message
        .split('\n')
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .unwrap_or(message)
        .to_string()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GitRemoteOperation {
    Push,
    Pull,
    Fetch,
    Upstream,
}

/// `message` is `None` when the failure was not an `Error` (TS `instanceof`).
pub fn normalize_git_error_message(
    message: Option<&str>,
    operation: Option<GitRemoteOperation>,
) -> String {
    let Some(message) = message else {
        return "Git remote operation failed.".to_string();
    };
    // Scrub up-front so every branch operates on already-redacted text.
    let raw = strip_credentials_from_message(message);

    // non-fast-forward guidance only makes sense when pushing (or for legacy
    // callers that pass no operation).
    if (operation == Some(GitRemoteOperation::Push) || operation.is_none())
        && (raw.contains("non-fast-forward") || raw.contains("fetch first"))
    {
        return "Push rejected: remote has newer commits (non-fast-forward). Please pull or sync first.".to_string();
    }
    if raw.contains("could not read Username") || raw.contains("Authentication failed") {
        return "Authentication failed. Check your remote credentials.".to_string();
    }
    if raw.contains("Could not resolve host") || raw.contains("Network is unreachable") {
        return "Network error. Check your connection.".to_string();
    }
    if raw.contains("no tracking information") || raw.contains("no upstream") {
        return "Branch has no upstream. Publish the branch first.".to_string();
    }
    if raw.contains("Your local changes to the following files would be overwritten")
        || raw.contains("Your local changes would be overwritten")
    {
        return "Pull would overwrite local changes. Commit, stash, or discard them before pulling.".to_string();
    }
    if raw.contains("untracked working tree files would be overwritten") {
        return "Pull would overwrite untracked files. Move, remove, or add them before pulling.".to_string();
    }
    extract_tail_line(&raw)
}

/// True only for clearly-no-upstream signals (an expected state). Gated on a
/// `fatal:` prefix so unrelated output cannot spuriously look like no-upstream.
pub fn is_no_upstream_error(message: Option<&str>) -> bool {
    match message {
        None => false,
        Some(message) => {
            fatal_prefix_re().is_match(message) && no_upstream_phrase_re().is_match(message)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn treats_missing_head_u_tracking_ref_as_no_upstream() {
        let error = "fatal: ambiguous argument 'HEAD@{u}': unknown revision or path not in the working tree.\nUse '--' to separate paths from revisions, like this:\n'git <command> [<revision>...] -- [<file>...]'";
        assert!(is_no_upstream_error(Some(error)));
    }

    #[test]
    fn does_not_treat_unrelated_ambiguous_refs_as_no_upstream() {
        let error = "fatal: ambiguous argument 'feature': unknown revision or path not in the working tree.";
        assert!(!is_no_upstream_error(Some(error)));
    }

    #[test]
    fn scrubs_userpass_on_any_scheme_but_keeps_ssh_user_info() {
        assert_eq!(
            strip_credentials_from_message("remote: https://user:ghp_secret@github.com/o/r.git failed"),
            "remote: https://github.com/o/r.git failed"
        );
        // ssh://git@host user-info is required by the remote — keep it.
        assert_eq!(
            strip_credentials_from_message("ssh://git@github.com/o/r.git"),
            "ssh://git@github.com/o/r.git"
        );
        // HTTPS token-only form is a credential — scrub it.
        assert_eq!(
            strip_credentials_from_message("https://ghp_token@github.com/o/r.git"),
            "https://github.com/o/r.git"
        );
    }

    #[test]
    fn maps_known_failures_and_falls_back_to_tail_line() {
        assert_eq!(
            normalize_git_error_message(Some("error: failed to push\nhint: non-fast-forward"), Some(GitRemoteOperation::Push)),
            "Push rejected: remote has newer commits (non-fast-forward). Please pull or sync first."
        );
        assert_eq!(
            normalize_git_error_message(Some("fatal: Authentication failed for 'x'"), None),
            "Authentication failed. Check your remote credentials."
        );
        assert_eq!(
            normalize_git_error_message(Some("Command failed: git push\nfatal: something specific went wrong"), Some(GitRemoteOperation::Push)),
            "fatal: something specific went wrong"
        );
        assert_eq!(normalize_git_error_message(None, None), "Git remote operation failed.");
    }

    #[test]
    fn non_fast_forward_guidance_only_for_push_or_unspecified() {
        // On a fetch, non-fast-forward should fall through to the tail line.
        assert_eq!(
            normalize_git_error_message(Some("hint: non-fast-forward update rejected"), Some(GitRemoteOperation::Fetch)),
            "hint: non-fast-forward update rejected"
        );
    }
}
