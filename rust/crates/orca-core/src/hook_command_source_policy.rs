//! Hook-command source policy resolution, ported from
//! `src/shared/hook-command-source-policy.ts`.
//!
//! Decides whether a workspace runs local hook scripts, the committed shared
//! config, or both. Unknown/legacy persisted values fall back to the
//! authoritative committed (`shared-only`) policy. `None` distinguishes an
//! *absent* setting (which can default to local when a local script exists)
//! from a *present-but-invalid* one (which never does).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookCommandSourcePolicy {
    LocalOnly,
    RunBoth,
    SharedOnly,
}

impl HookCommandSourcePolicy {
    pub fn as_wire(self) -> &'static str {
        match self {
            HookCommandSourcePolicy::LocalOnly => "local-only",
            HookCommandSourcePolicy::RunBoth => "run-both",
            HookCommandSourcePolicy::SharedOnly => "shared-only",
        }
    }

    fn parse(value: &str) -> Option<HookCommandSourcePolicy> {
        match value {
            "local-only" => Some(HookCommandSourcePolicy::LocalOnly),
            "run-both" => Some(HookCommandSourcePolicy::RunBoth),
            "shared-only" => Some(HookCommandSourcePolicy::SharedOnly),
            _ => None,
        }
    }
}

/// Normalize a persisted value; unknown/legacy (e.g. the removed `shared-first`)
/// falls back to the committed `shared-only` policy.
pub fn normalize_hook_command_source_policy(policy: Option<&str>) -> HookCommandSourcePolicy {
    policy.and_then(HookCommandSourcePolicy::parse).unwrap_or(HookCommandSourcePolicy::SharedOnly)
}

/// Resolve the effective policy: an explicit valid choice wins; an *absent*
/// setting defaults to `local-only` when a local script is configured;
/// otherwise `shared-only`.
pub fn resolve_hook_command_source_policy(policy: Option<&str>, has_local_script: bool) -> HookCommandSourcePolicy {
    if let Some(valid) = policy.and_then(HookCommandSourcePolicy::parse) {
        return valid;
    }
    if policy.is_none() && has_local_script {
        return HookCommandSourcePolicy::LocalOnly;
    }
    HookCommandSourcePolicy::SharedOnly
}

#[cfg(test)]
mod tests {
    use super::*;
    use HookCommandSourcePolicy::{LocalOnly, RunBoth, SharedOnly};

    #[test]
    fn normalizes_unknown_persisted_policies_to_shared_only() {
        assert_eq!(normalize_hook_command_source_policy(Some("shared-first")), SharedOnly);
    }

    #[test]
    fn uses_local_commands_by_default_when_a_local_script_is_configured() {
        assert_eq!(resolve_hook_command_source_policy(None, true), LocalOnly);
    }

    #[test]
    fn uses_shared_commands_by_default_when_no_local_script_is_configured() {
        assert_eq!(resolve_hook_command_source_policy(None, false), SharedOnly);
    }

    #[test]
    fn preserves_explicit_command_source_choices() {
        assert_eq!(resolve_hook_command_source_policy(Some("shared-only"), true), SharedOnly);
        assert_eq!(resolve_hook_command_source_policy(Some("run-both"), true), RunBoth);
    }
}
