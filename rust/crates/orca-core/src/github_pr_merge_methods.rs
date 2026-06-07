//! GitHub PR merge-method resolution, ported from
//! `src/shared/github-pr-merge-methods.ts`.
//!
//! Maps GitHub's repository merge settings to the ordered, labelled options the
//! UI offers, preserving the historical squash-first fallback when repository
//! metadata is missing.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GitHubPrMergeMethod {
    Squash,
    Merge,
    Rebase,
}

impl GitHubPrMergeMethod {
    pub fn label(self) -> &'static str {
        match self {
            GitHubPrMergeMethod::Squash => "Squash and merge",
            GitHubPrMergeMethod::Merge => "Create merge commit",
            GitHubPrMergeMethod::Rebase => "Rebase and merge",
        }
    }
}

/// Display/priority order: squash, merge, rebase.
pub const GITHUB_PR_MERGE_METHODS: [GitHubPrMergeMethod; 3] = [
    GitHubPrMergeMethod::Squash,
    GitHubPrMergeMethod::Merge,
    GitHubPrMergeMethod::Rebase,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AllowedMethods {
    pub squash: bool,
    pub merge: bool,
    pub rebase: bool,
}

impl AllowedMethods {
    fn all() -> Self {
        Self { squash: true, merge: true, rebase: true }
    }
    fn get(self, method: GitHubPrMergeMethod) -> bool {
        match method {
            GitHubPrMergeMethod::Squash => self.squash,
            GitHubPrMergeMethod::Merge => self.merge,
            GitHubPrMergeMethod::Rebase => self.rebase,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GitHubPrMergeMethodSettings {
    pub default_method: GitHubPrMergeMethod,
    pub allowed_methods: AllowedMethods,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GitHubPrMergeMethodOption {
    pub method: GitHubPrMergeMethod,
    pub label: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitHubPrMergeMethodPresentation {
    pub default_method: GitHubPrMergeMethod,
    pub default_label: &'static str,
    pub methods: Vec<GitHubPrMergeMethodOption>,
}

pub fn map_github_default_merge_method(value: Option<&str>) -> Option<GitHubPrMergeMethod> {
    match value.map(str::to_uppercase).as_deref() {
        Some("MERGE") => Some(GitHubPrMergeMethod::Merge),
        Some("SQUASH") => Some(GitHubPrMergeMethod::Squash),
        Some("REBASE") => Some(GitHubPrMergeMethod::Rebase),
        _ => None,
    }
}

pub fn normalize_github_pr_merge_method_settings(
    default_method: Option<&str>,
    merge_commit_allowed: bool,
    rebase_merge_allowed: bool,
    squash_merge_allowed: bool,
) -> Option<GitHubPrMergeMethodSettings> {
    let allowed = AllowedMethods {
        squash: squash_merge_allowed,
        merge: merge_commit_allowed,
        rebase: rebase_merge_allowed,
    };
    let default = map_github_default_merge_method(default_method);
    let first_allowed = GITHUB_PR_MERGE_METHODS.iter().copied().find(|m| allowed.get(*m));
    let resolved = match default {
        Some(d) if allowed.get(d) => Some(d),
        // `firstAllowed ?? defaultMethod`
        _ => first_allowed.or(default),
    };
    resolved.map(|default_method| GitHubPrMergeMethodSettings {
        default_method,
        allowed_methods: allowed,
    })
}

pub fn resolve_github_pr_merge_methods(
    settings: Option<&GitHubPrMergeMethodSettings>,
) -> GitHubPrMergeMethodPresentation {
    let allowed = settings.map(|s| s.allowed_methods).unwrap_or_else(AllowedMethods::all);
    let first_allowed = GITHUB_PR_MERGE_METHODS.iter().copied().find(|m| allowed.get(*m));
    let default_method = match settings.map(|s| s.default_method) {
        Some(d) if allowed.get(d) => d,
        _ => first_allowed.unwrap_or(GitHubPrMergeMethod::Squash),
    };
    let ordered: Vec<GitHubPrMergeMethod> = std::iter::once(default_method)
        .chain(GITHUB_PR_MERGE_METHODS.iter().copied().filter(|m| *m != default_method))
        .filter(|m| allowed.get(*m))
        .collect();
    let source = if ordered.is_empty() {
        GITHUB_PR_MERGE_METHODS.to_vec()
    } else {
        ordered
    };
    let methods = source
        .into_iter()
        .map(|method| GitHubPrMergeMethodOption { method, label: method.label() })
        .collect();
    GitHubPrMergeMethodPresentation {
        default_method,
        default_label: default_method.label(),
        methods,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use GitHubPrMergeMethod::{Merge, Rebase, Squash};

    fn opt(method: GitHubPrMergeMethod) -> GitHubPrMergeMethodOption {
        GitHubPrMergeMethodOption { method, label: method.label() }
    }

    #[test]
    fn keeps_squash_first_fallback_when_metadata_missing() {
        let p = resolve_github_pr_merge_methods(None);
        assert_eq!(p.default_method, Squash);
        assert_eq!(p.default_label, "Squash and merge");
        assert_eq!(p.methods, vec![opt(Squash), opt(Merge), opt(Rebase)]);
    }

    #[test]
    fn uses_viewer_default_and_hides_disabled_methods() {
        let settings = normalize_github_pr_merge_method_settings(Some("REBASE"), false, true, true)
            .expect("settings");
        let p = resolve_github_pr_merge_methods(Some(&settings));
        assert_eq!(p.default_method, Rebase);
        assert_eq!(p.default_label, "Rebase and merge");
        assert_eq!(p.methods, vec![opt(Rebase), opt(Squash)]);
    }

    #[test]
    fn falls_back_to_allowed_method_when_default_disabled() {
        let settings = normalize_github_pr_merge_method_settings(Some("SQUASH"), true, false, false)
            .expect("settings");
        assert_eq!(settings.default_method, Merge);
        let p = resolve_github_pr_merge_methods(Some(&settings));
        assert_eq!(p.methods, vec![opt(Merge)]);
    }
}
