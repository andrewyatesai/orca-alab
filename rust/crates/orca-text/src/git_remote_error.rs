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

fn submodule_named_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"(?i)Unable to push submodule ['"](.+?)['"]"#).unwrap())
}

fn submodule_sentinel_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)failed to push all needed submodules|Unable to push submodule").unwrap()
    })
}

fn submodule_remote_changed_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)non-fast-forward|fetch first|updates were rejected|remote contains work that you do not have",
        )
        .unwrap()
    })
}

fn normalized_submodule_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)(?:^|:\s)((?:Submodule '[^'\n]+'|A submodule) (?:has remote changes\. Pull inside the submodule, then try again\.|could not be pushed\. Resolve the submodule push error, then try again\.))(?:$|\s)",
        )
        .unwrap()
    })
}

fn divergent_pull_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)Need to specify how to reconcile divergent branches|divergent branches and need to specify how to reconcile them",
        )
        .unwrap()
    })
}

/// Port of `formatSubmodulePushFailureDetail`: recursive push can hide the
/// actionable nested submodule rejection behind a top-level "failed to push all
/// needed submodules" line. Returns the user-facing detail, or `None`.
/// Public: the renderer consumes it directly (via wasm) for push-failure toasts.
pub fn format_submodule_push_failure_detail(message: &str) -> Option<String> {
    let raw = strip_credentials_from_message(message);
    let trimmed = raw.trim();
    if let Some(caps) = normalized_submodule_re().captures(trimmed) {
        return caps.get(1).map(|m| m.as_str().to_string());
    }
    if !submodule_sentinel_re().is_match(trimmed) {
        return None;
    }
    let submodule_name = submodule_named_re()
        .captures(trimmed)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim().to_string())
        .filter(|n| !n.is_empty());
    let subject = match submodule_name {
        Some(name) => format!("Submodule '{name}'"),
        None => "A submodule".to_string(),
    };
    if submodule_remote_changed_re().is_match(trimmed) {
        Some(format!("{subject} has remote changes. Pull inside the submodule, then try again."))
    } else {
        Some(format!(
            "{subject} could not be pushed. Resolve the submodule push error, then try again."
        ))
    }
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

    // A submodule push failure carries the actionable detail — check it BEFORE the
    // generic non-fast-forward branch (the recursive push stderr contains both).
    if operation == Some(GitRemoteOperation::Push) || operation.is_none() {
        if let Some(detail) = format_submodule_push_failure_detail(&raw) {
            return detail;
        }
    }

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
    if operation == Some(GitRemoteOperation::Pull) && divergent_pull_re().is_match(&raw) {
        return "Pull needs a Git pull policy for divergent branches. Configure one for this repository or host, then try again: git config pull.rebase false (merge), git config pull.rebase true (rebase), or git config pull.ff only (fast-forward only).".to_string();
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
    fn submodule_push_failure_wins_over_non_fast_forward() {
        // A recursive push failure carries BOTH the submodule sentinel and a
        // non-fast-forward hint; the submodule detail must win.
        let stderr = "Command failed: git push\nPushing submodule 'find-cmux-followers'\n ! [rejected]        master -> master (fetch first)\nUnable to push submodule 'find-cmux-followers'\nfatal: failed to push all needed submodules";
        assert_eq!(
            normalize_git_error_message(Some(stderr), Some(GitRemoteOperation::Push)),
            "Submodule 'find-cmux-followers' has remote changes. Pull inside the submodule, then try again."
        );
        // An already-normalized detail round-trips (idempotent).
        assert_eq!(
            normalize_git_error_message(
                Some("Submodule 'x' has remote changes. Pull inside the submodule, then try again."),
                Some(GitRemoteOperation::Push)
            ),
            "Submodule 'x' has remote changes. Pull inside the submodule, then try again."
        );
        // Sentinel without a remote-changed hint -> generic push-error guidance.
        assert_eq!(
            normalize_git_error_message(
                Some("fatal: failed to push all needed submodules"),
                Some(GitRemoteOperation::Push)
            ),
            "A submodule could not be pushed. Resolve the submodule push error, then try again."
        );
    }

    #[test]
    fn divergent_pull_guidance_only_for_pull() {
        let stderr = "hint: You have divergent branches and need to specify how to reconcile them.\nfatal: Need to specify how to reconcile divergent branches";
        assert!(normalize_git_error_message(Some(stderr), Some(GitRemoteOperation::Pull))
            .starts_with("Pull needs a Git pull policy for divergent branches."));
        // Not pull -> falls through to the tail line.
        assert_eq!(
            normalize_git_error_message(Some(stderr), Some(GitRemoteOperation::Push)),
            "fatal: Need to specify how to reconcile divergent branches"
        );
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
