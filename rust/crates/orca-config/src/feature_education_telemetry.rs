//! Feature-education telemetry tables + membership-fallback normalizers, ported
//! from `src/shared/feature-education-telemetry.ts`.
//!
//! These tables bound the dimension values emitted to analytics. The normalizers
//! map any off-table string (or null/undefined) to a safe in-table fallback so a
//! caller can never leak an arbitrary string (e.g. a URL) into telemetry.

use crate::contextual_tours::ContextualTourId;

/// Mirrors `contextual_tours::CONTEXTUAL_TOUR_IDS`; the unit test pins the two
/// lists together so telemetry stays aligned with the tour catalog.
pub const FEATURE_EDUCATION_CONTEXTUAL_TOUR_IDS: [ContextualTourId; 6] = [
    ContextualTourId::WorkspaceBoard,
    ContextualTourId::WorkspaceAgentSessions,
    ContextualTourId::Browser,
    ContextualTourId::Tasks,
    ContextualTourId::Automations,
    ContextualTourId::WorkspaceCreation,
];

pub const FEATURE_EDUCATION_SOURCES: [&str; 9] = [
    "workspace_board_visible",
    "workspace_agent_sessions_visible",
    "browser_visible",
    "tasks_open",
    "automations_open",
    "workspace_creation_visible",
    "workspace_creation_modal",
    "setup_guide_parallel_work",
    "unknown",
];

pub const CONTEXTUAL_TOUR_OUTCOMES: [&str; 3] = ["completed", "skipped", "cancelled"];

pub const SETUP_GUIDE_SOURCES: [&str; 6] =
    ["sidebar", "contextual_tour", "settings", "feature_wall", "help_menu", "unknown"];

pub const SETUP_GUIDE_CLOSE_OUTCOMES: [&str; 3] = ["completed", "dismissed", "interrupted"];

pub const TERMINAL_PANE_SPLIT_SOURCES: [&str; 5] =
    ["contextual_tour", "keyboard", "context_menu", "command", "unknown"];

pub fn normalize_feature_education_source(value: Option<&str>) -> &'static str {
    value
        .and_then(|v| FEATURE_EDUCATION_SOURCES.iter().copied().find(|&source| source == v))
        .unwrap_or("unknown")
}

pub fn normalize_setup_guide_source(value: Option<&str>) -> &'static str {
    value
        .and_then(|v| SETUP_GUIDE_SOURCES.iter().copied().find(|&source| source == v))
        .unwrap_or("unknown")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contextual_tours::CONTEXTUAL_TOUR_IDS;

    #[test]
    fn keeps_contextual_tour_telemetry_ids_aligned_with_tour_definitions() {
        assert_eq!(FEATURE_EDUCATION_CONTEXTUAL_TOUR_IDS, CONTEXTUAL_TOUR_IDS);
    }

    #[test]
    fn normalizes_unknown_telemetry_sources_to_a_bounded_fallback() {
        assert_eq!(normalize_feature_education_source(Some("tasks_open")), "tasks_open");
        assert_eq!(
            normalize_feature_education_source(Some("workspace_agent_sessions_visible")),
            "workspace_agent_sessions_visible"
        );
        assert_eq!(
            normalize_feature_education_source(Some("setup_guide_parallel_work")),
            "setup_guide_parallel_work"
        );
        assert_eq!(normalize_feature_education_source(Some("https://example.com/private")), "unknown");
        assert_eq!(normalize_feature_education_source(None), "unknown");
    }

    #[test]
    fn normalizes_setup_guide_telemetry_sources_to_a_bounded_fallback() {
        assert_eq!(normalize_setup_guide_source(Some("sidebar")), "sidebar");
        assert_eq!(normalize_setup_guide_source(Some("settings")), "settings");
        assert_eq!(normalize_setup_guide_source(Some("help_menu")), "help_menu");
        assert_eq!(normalize_setup_guide_source(Some("private-source")), "unknown");
    }
}
