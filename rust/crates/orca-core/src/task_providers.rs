//! Task-provider settings normalization, ported from `src/shared/task-providers.ts`.
//!
//! Provider-neutral (GitHub / GitLab / Linear / Jira). Normalizes the
//! visible-provider list and default source from possibly-drifted saved
//! settings, and filters by runtime availability — always leaving at least one
//! valid source so the Tasks surface can select something.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskProvider {
    GitHub,
    GitLab,
    Linear,
    Jira,
}

/// All providers, in the canonical order used for fallback/restore.
pub const TASK_PROVIDERS: [TaskProvider; 4] =
    [TaskProvider::GitHub, TaskProvider::GitLab, TaskProvider::Linear, TaskProvider::Jira];

impl TaskProvider {
    pub fn from_id(value: &str) -> Option<TaskProvider> {
        match value {
            "github" => Some(TaskProvider::GitHub),
            "gitlab" => Some(TaskProvider::GitLab),
            "linear" => Some(TaskProvider::Linear),
            "jira" => Some(TaskProvider::Jira),
            _ => None,
        }
    }

    pub fn as_id(self) -> &'static str {
        match self {
            TaskProvider::GitHub => "github",
            TaskProvider::GitLab => "gitlab",
            TaskProvider::Linear => "linear",
            TaskProvider::Jira => "jira",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct TaskProviderAvailability {
    pub gitlab_installed: bool,
    pub linear_connected: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NormalizedTaskProviderSettings {
    pub visible_task_providers: Vec<TaskProvider>,
    pub default_task_source: TaskProvider,
}

/// Normalize a candidate provider-id list: keep supported ids in order, dedupe,
/// and fall back to all providers when `None` (not a list) or empty — at least
/// one provider must stay visible.
pub fn normalize_visible_task_providers(value: Option<&[&str]>) -> Vec<TaskProvider> {
    let Some(list) = value else {
        return TASK_PROVIDERS.to_vec();
    };
    let mut normalized: Vec<TaskProvider> = Vec::new();
    for candidate in list {
        if let Some(provider) = TaskProvider::from_id(candidate) {
            if !normalized.contains(&provider) {
                normalized.push(provider);
            }
        }
    }
    if normalized.is_empty() {
        TASK_PROVIDERS.to_vec()
    } else {
        normalized
    }
}

pub fn normalize_task_provider_settings(
    visible_task_providers: Option<&[&str]>,
    default_task_source: Option<&str>,
) -> NormalizedTaskProviderSettings {
    let visible_task_providers = normalize_visible_task_providers(visible_task_providers);
    let default_task_source = default_task_source.and_then(TaskProvider::from_id).unwrap_or_else(|| {
        resolve_visible_task_provider(Some(TaskProvider::GitHub), &visible_task_providers)
    });

    if visible_task_providers.contains(&default_task_source) {
        return NormalizedTaskProviderSettings { visible_task_providers, default_task_source };
    }

    // Older profiles can keep a saved default while the visible list drifted;
    // persist the default back into the list so every surface reads the same
    // settings contract.
    let visible_task_providers = TASK_PROVIDERS
        .iter()
        .copied()
        .filter(|provider| *provider == default_task_source || visible_task_providers.contains(provider))
        .collect();
    NormalizedTaskProviderSettings { visible_task_providers, default_task_source }
}

pub fn filter_available_task_providers(
    visible_providers: &[TaskProvider],
    availability: &TaskProviderAvailability,
) -> Vec<TaskProvider> {
    let available: Vec<TaskProvider> = visible_providers
        .iter()
        .copied()
        .filter(|provider| is_task_provider_available(*provider, availability))
        .collect();
    if available.is_empty() {
        vec![TaskProvider::GitHub]
    } else {
        available
    }
}

pub fn restore_available_default_task_provider(
    visible_providers: &[TaskProvider],
    availability: &TaskProviderAvailability,
    preferred_provider: Option<&str>,
) -> Vec<TaskProvider> {
    let available = filter_available_task_providers(visible_providers, availability);

    // Drifted settings can hide a saved default while another provider becomes
    // available; keep that default reachable after hydration.
    if let Some(preferred) = preferred_provider.and_then(TaskProvider::from_id) {
        if is_task_provider_available(preferred, availability) && !available.contains(&preferred) {
            return TASK_PROVIDERS
                .iter()
                .copied()
                .filter(|provider| *provider == preferred || available.contains(provider))
                .collect();
        }
    }
    available
}

fn is_task_provider_available(provider: TaskProvider, availability: &TaskProviderAvailability) -> bool {
    match provider {
        TaskProvider::GitHub => true,
        TaskProvider::GitLab => availability.gitlab_installed,
        // Jira can be connected from the Tasks surface itself, so hiding it when
        // disconnected would remove the first-time-setup entry point.
        TaskProvider::Jira => true,
        TaskProvider::Linear => availability.linear_connected,
    }
}

pub fn resolve_visible_task_provider(
    preferred: Option<TaskProvider>,
    visible_providers: &[TaskProvider],
) -> TaskProvider {
    if let Some(preferred) = preferred {
        if visible_providers.contains(&preferred) {
            return preferred;
        }
    }
    visible_providers.first().copied().unwrap_or(TaskProvider::GitHub)
}

#[cfg(test)]
mod tests {
    use super::*;
    use TaskProvider::{GitHub, GitLab, Jira, Linear};

    fn availability(gitlab_installed: bool, linear_connected: bool) -> TaskProviderAvailability {
        TaskProviderAvailability { gitlab_installed, linear_connected }
    }

    #[test]
    fn normalizes_provider_lists_while_preserving_supported_order() {
        assert_eq!(
            normalize_visible_task_providers(Some(&["gitlab", "unknown", "gitlab", "linear"])),
            vec![GitLab, Linear]
        );
    }

    #[test]
    fn falls_back_to_all_providers_when_none_are_visible() {
        assert_eq!(normalize_visible_task_providers(Some(&[])), vec![GitHub, GitLab, Linear, Jira]);
    }

    #[test]
    fn restores_a_valid_saved_default_when_provider_settings_drifted() {
        assert_eq!(
            normalize_task_provider_settings(Some(&["linear"]), Some("github")),
            NormalizedTaskProviderSettings {
                default_task_source: GitHub,
                visible_task_providers: vec![GitHub, Linear],
            }
        );
    }

    #[test]
    fn normalizes_invalid_saved_defaults_to_the_first_visible_provider() {
        assert_eq!(
            normalize_task_provider_settings(Some(&["gitlab"]), Some("bitbucket")),
            NormalizedTaskProviderSettings {
                default_task_source: GitLab,
                visible_task_providers: vec![GitLab],
            }
        );
    }

    #[test]
    fn resolves_hidden_preferred_providers_to_the_first_visible_provider() {
        assert_eq!(resolve_visible_task_provider(Some(GitHub), &[Linear]), Linear);
    }

    #[test]
    fn filters_runtime_unavailable_providers() {
        assert_eq!(
            filter_available_task_providers(&[GitHub, GitLab, Linear], &availability(false, true)),
            vec![GitHub, Linear]
        );
    }

    #[test]
    fn keeps_an_available_saved_default_visible_when_provider_visibility_drifted() {
        assert_eq!(
            restore_available_default_task_provider(&[Linear], &availability(false, true), Some("github")),
            vec![GitHub, Linear]
        );
    }

    #[test]
    fn preserves_intentionally_narrowed_providers_when_saved_default_matches() {
        assert_eq!(
            restore_available_default_task_provider(&[Linear], &availability(false, true), Some("linear")),
            vec![Linear]
        );
    }

    #[test]
    fn does_not_restore_an_unavailable_saved_default() {
        assert_eq!(
            restore_available_default_task_provider(&[Linear], &availability(false, true), Some("gitlab")),
            vec![Linear]
        );
    }

    #[test]
    fn ignores_invalid_saved_defaults_while_restoring_visible_providers() {
        assert_eq!(
            restore_available_default_task_provider(&[GitLab], &availability(false, true), Some("bitbucket")),
            vec![GitHub]
        );
    }

    #[test]
    fn falls_back_to_github_when_every_preferred_provider_is_unavailable() {
        assert_eq!(
            filter_available_task_providers(&[GitLab, Linear], &availability(false, false)),
            vec![GitHub]
        );
    }
}
