//! Legacy base-ref search results, ported from
//! `src/shared/base-ref-search-result.ts`.
//!
//! Mixed-version runtimes only return display refs; this keeps common remote
//! refs from reintroducing `origin/feature/foo` as the local branch name.

const LEGACY_REMOTE_REF_PREFIXES: [&str; 2] = ["origin/", "upstream/"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BaseRefSearchResult {
    pub ref_name: String,
    pub local_branch_name: String,
}

pub fn derive_legacy_local_branch_name(ref_name: &str) -> String {
    for prefix in LEGACY_REMOTE_REF_PREFIXES {
        if ref_name.starts_with(prefix) && ref_name.len() > prefix.len() {
            return ref_name[prefix.len()..].to_string();
        }
    }
    ref_name.to_string()
}

pub fn legacy_base_ref_search_result(ref_name: &str) -> BaseRefSearchResult {
    BaseRefSearchResult {
        ref_name: ref_name.to_string(),
        local_branch_name: derive_legacy_local_branch_name(ref_name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_local_branch_names_for_common_remote_refs() {
        assert_eq!(
            derive_legacy_local_branch_name("origin/feature/something"),
            "feature/something"
        );
        assert_eq!(
            derive_legacy_local_branch_name("upstream/release/1.2"),
            "release/1.2"
        );
    }

    #[test]
    fn keeps_local_branch_refs_unchanged_when_prefix_unknown() {
        assert_eq!(
            legacy_base_ref_search_result("feature/something"),
            BaseRefSearchResult {
                ref_name: "feature/something".to_string(),
                local_branch_name: "feature/something".to_string(),
            }
        );
    }
}
