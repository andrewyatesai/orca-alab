//! Contextual-tour definitions + id validation/dedup, ported from
//! `src/shared/contextual-tours.ts`.
//!
//! The catalog drives the in-app guided tours (board, agent sessions, browser,
//! tasks, automations, workspace creation). `normalize_contextual_tour_ids`
//! defends persisted "seen tour" lists against unknown/duplicate ids.

use crate::feature_interactions::FeatureInteractionId;
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextualTourId {
    WorkspaceBoard,
    WorkspaceAgentSessions,
    Browser,
    Tasks,
    Automations,
    WorkspaceCreation,
}

impl ContextualTourId {
    pub fn as_str(self) -> &'static str {
        match self {
            ContextualTourId::WorkspaceBoard => "workspace-board",
            ContextualTourId::WorkspaceAgentSessions => "workspace-agent-sessions",
            ContextualTourId::Browser => "browser",
            ContextualTourId::Tasks => "tasks",
            ContextualTourId::Automations => "automations",
            ContextualTourId::WorkspaceCreation => "workspace-creation",
        }
    }

    pub fn from_id(value: &str) -> Option<ContextualTourId> {
        match value {
            "workspace-board" => Some(ContextualTourId::WorkspaceBoard),
            "workspace-agent-sessions" => Some(ContextualTourId::WorkspaceAgentSessions),
            "browser" => Some(ContextualTourId::Browser),
            "tasks" => Some(ContextualTourId::Tasks),
            "automations" => Some(ContextualTourId::Automations),
            "workspace-creation" => Some(ContextualTourId::WorkspaceCreation),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextualTourStepControlKind {
    AutoRenameBranchFromWork,
}

impl ContextualTourStepControlKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ContextualTourStepControlKind::AutoRenameBranchFromWork => "auto-rename-branch-from-work",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContextualTourStepControl {
    pub kind: ContextualTourStepControlKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextualTourStepActionKind {
    Next,
    Complete,
    SplitTerminalPane,
    CreateWorktree,
    ShowWorktrees,
    OpenTasks,
    OpenGettingStarted,
}

impl ContextualTourStepActionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ContextualTourStepActionKind::Next => "next",
            ContextualTourStepActionKind::Complete => "complete",
            ContextualTourStepActionKind::SplitTerminalPane => "split-terminal-pane",
            ContextualTourStepActionKind::CreateWorktree => "create-worktree",
            ContextualTourStepActionKind::ShowWorktrees => "show-worktrees",
            ContextualTourStepActionKind::OpenTasks => "open-tasks",
            ContextualTourStepActionKind::OpenGettingStarted => "open-getting-started",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContextualTourStepAction {
    pub kind: ContextualTourStepActionKind,
    pub label: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextualTourStepPlacement {
    Top,
    Right,
    Bottom,
    Left,
}

impl ContextualTourStepPlacement {
    pub fn as_str(self) -> &'static str {
        match self {
            ContextualTourStepPlacement::Top => "top",
            ContextualTourStepPlacement::Right => "right",
            ContextualTourStepPlacement::Bottom => "bottom",
            ContextualTourStepPlacement::Left => "left",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContextualTourStep {
    pub title: &'static str,
    pub body: &'static str,
    pub target_selector: &'static str,
    pub required_for_start: Option<bool>,
    pub fallback_copy: Option<&'static str>,
    pub preferred_placement: Option<ContextualTourStepPlacement>,
    pub target_pulse: Option<bool>,
    pub hide_primary_action: Option<bool>,
    pub control: Option<ContextualTourStepControl>,
    pub primary_action: Option<ContextualTourStepAction>,
    pub secondary_action: Option<ContextualTourStepAction>,
    pub advance_on_feature_interaction: Option<FeatureInteractionId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContextualTour {
    pub id: ContextualTourId,
    /// Modals over which this tour is still allowed to run; empty means none
    /// (the optional TS field is absent).
    pub allowed_active_modals: &'static [&'static str],
    pub steps: &'static [ContextualTourStep],
}

/// Builds a step with all optional fields cleared; literals below override only
/// the fields actually present in the TS catalog.
const fn step(
    title: &'static str,
    body: &'static str,
    target_selector: &'static str,
) -> ContextualTourStep {
    ContextualTourStep {
        title,
        body,
        target_selector,
        required_for_start: None,
        fallback_copy: None,
        preferred_placement: None,
        target_pulse: None,
        hide_primary_action: None,
        control: None,
        primary_action: None,
        secondary_action: None,
        advance_on_feature_interaction: None,
    }
}

pub const CONTEXTUAL_TOURS: [ContextualTour; 6] = [
    ContextualTour {
        id: ContextualTourId::WorkspaceBoard,
        allowed_active_modals: &[],
        steps: &[
            ContextualTourStep {
                required_for_start: Some(true),
                preferred_placement: Some(ContextualTourStepPlacement::Bottom),
                ..step(
                    "Plan work on the board",
                    "Use the board when you want to see workspaces by status instead of by project.",
                    "[data-contextual-tour-target=\"workspace-board-center\"]",
                )
            },
            step(
                "Move work through lanes",
                "Drag workspaces between lanes as their status changes.",
                "[data-contextual-tour-target=\"workspace-board-done-lane\"], [data-contextual-tour-target=\"workspace-board-lanes\"]",
            ),
            step(
                "Tune density",
                "Use board settings to switch between detailed and compact cards.",
                "[data-contextual-tour-target=\"workspace-board-settings\"], [data-contextual-tour-target=\"workspace-board-lanes\"]",
            ),
        ],
    },
    ContextualTour {
        id: ContextualTourId::WorkspaceAgentSessions,
        allowed_active_modals: &[],
        steps: &[
            ContextualTourStep {
                required_for_start: Some(true),
                preferred_placement: Some(ContextualTourStepPlacement::Bottom),
                primary_action: Some(ContextualTourStepAction {
                    kind: ContextualTourStepActionKind::SplitTerminalPane,
                    label: "Split terminal",
                }),
                advance_on_feature_interaction: Some(FeatureInteractionId::TerminalPaneSplit),
                ..step(
                    "Split a terminal pane",
                    "Open a second terminal pane with {terminal.splitRight}, or right-click the pane for split options.",
                    "[data-contextual-tour-target=\"terminal-pane-split-target\"], [data-contextual-tour-target=\"workspace-agent-terminal-tip\"]",
                )
            },
            ContextualTourStep {
                preferred_placement: Some(ContextualTourStepPlacement::Right),
                target_pulse: Some(true),
                hide_primary_action: Some(true),
                ..step(
                    "Start another task in parallel",
                    "Each worktree gets its own branch, so parallel work stays separate.",
                    "[data-contextual-tour-target=\"workspace-create-control\"]",
                )
            },
        ],
    },
    ContextualTour {
        id: ContextualTourId::Browser,
        allowed_active_modals: &[],
        steps: &[
            ContextualTourStep {
                required_for_start: Some(true),
                ..step(
                    "Grab page context for agents",
                    "Grab controls can copy elements or hand page context to an agent.",
                    "[data-contextual-tour-target=\"browser-grab-control\"]",
                )
            },
            step(
                "Mark design feedback in place",
                "Annotate elements and send those notes to an agent.",
                "[data-contextual-tour-target=\"browser-annotation-control\"]",
            ),
        ],
    },
    ContextualTour {
        id: ContextualTourId::Tasks,
        allowed_active_modals: &[],
        steps: &[
            ContextualTourStep {
                required_for_start: Some(true),
                ..step(
                    "Choose the work source",
                    "Switch between connected providers and project filters without changing pages.",
                    "[data-contextual-tour-target=\"tasks-source-filters\"]",
                )
            },
            step(
                "Filter to the work you need",
                "Use presets and search to narrow issues, reviews, merge requests, or tasks.",
                "[data-contextual-tour-target=\"tasks-search-presets\"]",
            ),
            step(
                "Start from work items",
                "Use Start or Open on a task, issue, review, or merge request to bring its context into a workspace.",
                "[data-contextual-tour-target=\"tasks-start-workspace\"], [data-contextual-tour-target=\"tasks-actions\"], [data-contextual-tour-target=\"tasks-search-presets\"]",
            ),
        ],
    },
    ContextualTour {
        id: ContextualTourId::Automations,
        allowed_active_modals: &[],
        steps: &[
            ContextualTourStep {
                required_for_start: Some(true),
                ..step(
                    "What is an automation?",
                    "Automations run agent work on a schedule. Add an automation by clicking this button.",
                    "[data-contextual-tour-target=\"automations-create\"]",
                )
            },
            step(
                "Find the results",
                "Runs show when automations executed, what happened, and where to inspect their output.",
                "[data-contextual-tour-target=\"automations-runs\"]",
            ),
        ],
    },
    ContextualTour {
        id: ContextualTourId::WorkspaceCreation,
        allowed_active_modals: &["new-workspace-composer"],
        steps: &[
            ContextualTourStep {
                required_for_start: Some(true),
                ..step(
                    "Pick a project",
                    "Orca isolates each task in its own worktree, branched off your base.",
                    "[data-contextual-tour-target=\"workspace-creation-project\"]",
                )
            },
            ContextualTourStep {
                control: Some(ContextualTourStepControl {
                    kind: ContextualTourStepControlKind::AutoRenameBranchFromWork,
                }),
                ..step(
                    "Name it, or start from existing work",
                    "Start from a linked task for a short issue or PR name. Or leave it blank to auto-name it from your first agent message.",
                    "[data-contextual-tour-target=\"workspace-creation-name\"]",
                )
            },
            step(
                "Choose what agent starts the work",
                "Pick the agent that should be opened when this worktree is created.",
                "[data-contextual-tour-target=\"workspace-creation-agent\"]",
            ),
        ],
    },
];

pub const CONTEXTUAL_TOUR_IDS: [ContextualTourId; 6] = [
    ContextualTourId::WorkspaceBoard,
    ContextualTourId::WorkspaceAgentSessions,
    ContextualTourId::Browser,
    ContextualTourId::Tasks,
    ContextualTourId::Automations,
    ContextualTourId::WorkspaceCreation,
];

pub fn is_contextual_tour_id(value: &Value) -> bool {
    value.as_str().is_some_and(|s| ContextualTourId::from_id(s).is_some())
}

/// TS returns `ContextualTour` via a non-null assertion; the enum makes the
/// catalog total, but we stay panic-free by returning `Option`. The tour is
/// `Copy` (its `steps`/`allowed_active_modals` are `'static`), so we hand back
/// an owned value rather than a borrow into the inlined const array.
pub fn get_contextual_tour(id: ContextualTourId) -> Option<ContextualTour> {
    CONTEXTUAL_TOURS.iter().find(|tour| tour.id == id).copied()
}

// Result is the unique valid ids, bounded by the catalog size.
#[cfg_attr(trust_verify, trust::ensures(|out: &Vec<ContextualTourId>| out.len() <= CONTEXTUAL_TOURS.len()))]
pub fn normalize_contextual_tour_ids(value: &Value) -> Vec<ContextualTourId> {
    let Some(items) = value.as_array() else {
        return Vec::new();
    };
    // First-seen order, deduped — matches the TS `Set` insertion semantics.
    let mut seen: Vec<ContextualTourId> = Vec::new();
    for item in items {
        if let Some(id) = item.as_str().and_then(ContextualTourId::from_id) {
            if !seen.contains(&id) {
                seen.push(id);
            }
        }
    }
    seen
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn find(id: ContextualTourId) -> ContextualTour {
        CONTEXTUAL_TOURS.iter().find(|tour| tour.id == id).copied().unwrap()
    }

    #[test]
    fn defines_the_required_tours_with_concise_visible_steps() {
        let expected_ids = [
            ContextualTourId::WorkspaceBoard,
            ContextualTourId::WorkspaceAgentSessions,
            ContextualTourId::Browser,
            ContextualTourId::Tasks,
            ContextualTourId::Automations,
            ContextualTourId::WorkspaceCreation,
        ];

        assert_eq!(CONTEXTUAL_TOURS.iter().map(|tour| tour.id).collect::<Vec<_>>(), expected_ids.to_vec());
        for tour in CONTEXTUAL_TOURS {
            assert_eq!(tour.steps[0].required_for_start, Some(true));
            let step_count = tour.steps.len();
            if step_count == 1 {
                assert!(tour.steps[0].advance_on_feature_interaction.is_some());
            } else {
                assert!(step_count >= 2);
            }
            let max = if tour.id == ContextualTourId::WorkspaceAgentSessions { 5 } else { 3 };
            assert!(step_count <= max);
            for step in tour.steps {
                assert!(!step.title.is_empty());
                assert!(!step.body.is_empty());
                assert!(step.body.encode_utf16().count() <= 140);
                assert!(step.target_selector.contains("data-contextual-tour-target"));
            }
        }
    }

    #[test]
    fn defines_the_workspace_agent_sessions_value_tour_as_split_then_create_worktree() {
        let tour = find(ContextualTourId::WorkspaceAgentSessions);

        // Two steps only: tasks and orchestration education lives in their own
        // page tours, so the in-app tour ends after the worktree CTA.
        assert_eq!(
            tour.steps.iter().map(|step| step.title).collect::<Vec<_>>(),
            vec!["Split a terminal pane", "Start another task in parallel"]
        );
        // The opening step teaches the split gesture and offers the convenience button.
        assert_eq!(tour.steps[0].required_for_start, Some(true));
        assert_eq!(
            tour.steps[0].primary_action,
            Some(ContextualTourStepAction {
                kind: ContextualTourStepActionKind::SplitTerminalPane,
                label: "Split terminal"
            })
        );
        assert_eq!(
            tour.steps[0].advance_on_feature_interaction,
            Some(FeatureInteractionId::TerminalPaneSplit)
        );
        assert!(tour.steps[0].body.contains("{terminal.splitRight}"));
        assert!(tour.steps[0].target_selector.contains("terminal-pane-split-target"));
        assert!(!tour.steps[0].target_selector.contains("terminal-split-control"));
        assert_eq!(tour.steps[0].secondary_action, None);
        // The closing step anchors on the real new-worktree button; the pulse makes
        // that button the CTA instead of duplicating it inside the panel.
        assert_eq!(tour.steps[1].target_pulse, Some(true));
        assert_eq!(tour.steps[1].hide_primary_action, Some(true));
        assert!(tour.steps[1].target_selector.contains("workspace-create-control"));
        assert_eq!(tour.steps[1].primary_action, None);
        assert_eq!(tour.steps[1].secondary_action, None);
    }

    #[test]
    fn points_the_workspace_board_tour_at_the_board_center_done_lane_and_settings() {
        let tour = find(ContextualTourId::WorkspaceBoard);

        assert_eq!(
            tour.steps.iter().map(|step| step.title).collect::<Vec<_>>(),
            vec!["Plan work on the board", "Move work through lanes", "Tune density"]
        );
        assert_eq!(
            tour.steps[0].target_selector,
            "[data-contextual-tour-target=\"workspace-board-center\"]"
        );
        assert_eq!(tour.steps[0].required_for_start, Some(true));
        assert_eq!(tour.steps[0].preferred_placement, Some(ContextualTourStepPlacement::Bottom));
        assert_eq!(tour.steps[1].body, "Drag workspaces between lanes as their status changes.");
        assert_eq!(
            tour.steps[1].target_selector,
            "[data-contextual-tour-target=\"workspace-board-done-lane\"], [data-contextual-tour-target=\"workspace-board-lanes\"]"
        );
        assert_eq!(tour.steps[2].body, "Use board settings to switch between detailed and compact cards.");
        assert_eq!(
            tour.steps[2].target_selector,
            "[data-contextual-tour-target=\"workspace-board-settings\"], [data-contextual-tour-target=\"workspace-board-lanes\"]"
        );
    }

    #[test]
    fn points_the_tasks_tour_at_the_row_workspace_action_before_toolbar_fallbacks() {
        let tour = find(ContextualTourId::Tasks);
        let step = tour.steps[2];

        assert_eq!(step.title, "Start from work items");
        assert_eq!(
            step.body,
            "Use Start or Open on a task, issue, review, or merge request to bring its context into a workspace."
        );
        assert_eq!(
            step.target_selector.split(", ").collect::<Vec<_>>(),
            vec![
                "[data-contextual-tour-target=\"tasks-start-workspace\"]",
                "[data-contextual-tour-target=\"tasks-actions\"]",
                "[data-contextual-tour-target=\"tasks-search-presets\"]",
            ]
        );
    }

    #[test]
    fn orders_the_automations_tour_as_create_then_results() {
        let tour = find(ContextualTourId::Automations);

        assert_eq!(
            tour.steps.iter().map(|step| step.title).collect::<Vec<_>>(),
            vec!["What is an automation?", "Find the results"]
        );
        assert_eq!(
            tour.steps[0].body,
            "Automations run agent work on a schedule. Add an automation by clicking this button."
        );
        assert_eq!(tour.steps[0].required_for_start, Some(true));
        assert_eq!(
            tour.steps.iter().map(|step| step.target_selector).collect::<Vec<_>>(),
            vec![
                "[data-contextual-tour-target=\"automations-create\"]",
                "[data-contextual-tour-target=\"automations-runs\"]",
            ]
        );
    }

    #[test]
    fn allows_only_workspace_creation_over_its_workspace_composer_modal() {
        let modal_tours: Vec<ContextualTour> = CONTEXTUAL_TOURS
            .iter()
            .copied()
            .filter(|tour| !tour.allowed_active_modals.is_empty())
            .collect();

        assert_eq!(
            modal_tours.iter().map(|tour| tour.id).collect::<Vec<_>>(),
            vec![ContextualTourId::WorkspaceCreation]
        );
        assert_eq!(modal_tours[0].allowed_active_modals.to_vec(), vec!["new-workspace-composer"]);
    }

    #[test]
    fn normalizes_persisted_ids_by_removing_unknowns_and_duplicates() {
        assert_eq!(
            normalize_contextual_tour_ids(&json!([
                "tasks",
                "unknown",
                "workspace-agent-sessions",
                "browser",
                "tasks",
                null,
                "workspace-creation"
            ])),
            vec![
                ContextualTourId::Tasks,
                ContextualTourId::WorkspaceAgentSessions,
                ContextualTourId::Browser,
                ContextualTourId::WorkspaceCreation,
            ]
        );
    }
}
