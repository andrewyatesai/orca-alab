//! Hosted-review ref normalization, ported from `src/shared/hosted-review-refs.ts`.
//!
//! Reduces a git ref to a branch name: strip `refs/heads/` and
//! `refs/remotes/<remote>/` for head refs, and additionally a leading
//! `origin/`/`upstream/` for base refs. Provider-neutral.

/// Strip a leading `refs/remotes/<remote>/` (requires a non-empty remote
/// segment and the trailing slash), else return unchanged.
fn strip_refs_remotes(reference: &str) -> &str {
    let Some(rest) = reference.strip_prefix("refs/remotes/") else {
        return reference;
    };
    match rest.find('/') {
        Some(slash) if slash > 0 => &rest[slash + 1..],
        _ => reference,
    }
}

pub fn normalize_hosted_review_head_ref(reference: &str) -> String {
    let trimmed = reference.trim();
    let after_heads = trimmed.strip_prefix("refs/heads/").unwrap_or(trimmed);
    strip_refs_remotes(after_heads).to_string()
}

pub fn normalize_hosted_review_base_ref(reference: &str) -> String {
    let normalized = normalize_hosted_review_head_ref(reference);
    for prefix in ["origin/", "upstream/"] {
        if let Some(rest) = normalized.strip_prefix(prefix) {
            return rest.to_string();
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_local_and_remote_head_refs_to_branch_names() {
        assert_eq!(normalize_hosted_review_head_ref(" refs/heads/feature/create-pr "), "feature/create-pr");
        assert_eq!(normalize_hosted_review_head_ref("refs/remotes/origin/feature/create-pr"), "feature/create-pr");
    }

    #[test]
    fn strips_common_remote_prefixes_from_base_refs() {
        assert_eq!(normalize_hosted_review_base_ref("origin/main"), "main");
        assert_eq!(normalize_hosted_review_base_ref("refs/remotes/upstream/release/1.0"), "release/1.0");
    }
}
