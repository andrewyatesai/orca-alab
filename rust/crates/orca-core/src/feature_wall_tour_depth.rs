//! Feature-wall tour depth telemetry, ported from `src/shared/feature-wall-tour-depth.ts`.
//!
//! Maps a workflow + nested substep to a canonical ordered depth step, and
//! summarizes how far through the onboarding tour a session reached (furthest
//! step, visited/completed counts). Pure; ids are referenced by string.

use std::collections::HashSet;

/// Depth steps in tour order — index is the depth rank.
pub const FEATURE_WALL_TOUR_DEPTH_STEPS: [&str; 11] = [
    "workspaces",
    "tasks",
    "agents_statuses",
    "agents_usage",
    "agents_orchestration",
    "workbench_terminal",
    "workbench_editor",
    "workbench_browser",
    "review_notes",
    "review_pr_view",
    "review_ship",
];

pub const FEATURE_WALL_EXIT_ACTIONS: [&str; 3] = ["done", "dismissed", "onboarding_continue"];

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FeatureWallTourDepthSummary {
    pub furthest_step: Option<String>,
    pub last_group_id: Option<String>,
    pub visited_workflow_count: usize,
    pub visited_substep_count: usize,
    pub completed_workflow_count: usize,
    pub completed_substep_count: usize,
}

/// Visited sets + per-group "done" flag values + the last group opened. The
/// `*_done_values` are the booleans of the TS `Record`s (only the true-count
/// matters).
pub struct FeatureWallTourDepthInput<'a> {
    pub visited_workflows: &'a HashSet<&'a str>,
    pub visited_agent_steps: &'a HashSet<&'a str>,
    pub visited_workbench_steps: &'a HashSet<&'a str>,
    pub visited_review_steps: &'a HashSet<&'a str>,
    pub workflow_done_values: &'a [bool],
    pub agent_step_done_values: &'a [bool],
    pub workbench_step_done_values: &'a [bool],
    pub review_step_done_values: &'a [bool],
    pub last_group_id: Option<&'a str>,
}

fn agent_depth_step(step: &str) -> String {
    match step {
        "statuses" => "agents_statuses",
        "usage" => "agents_usage",
        "orchestration" => "agents_orchestration",
        other => other,
    }
    .to_string()
}

fn workbench_depth_step(step: &str) -> String {
    match step {
        "terminal" => "workbench_terminal",
        "editor" => "workbench_editor",
        "browser" => "workbench_browser",
        other => other,
    }
    .to_string()
}

fn review_depth_step(step: &str) -> String {
    match step {
        "notes" => "review_notes",
        "pr-view" => "review_pr_view",
        "ship" => "review_ship",
        other => other,
    }
    .to_string()
}

/// Furthest (highest-rank) known depth step, ignoring unranked steps.
fn furthest_depth_step(steps: &[String]) -> Option<String> {
    steps
        .iter()
        .filter_map(|step| FEATURE_WALL_TOUR_DEPTH_STEPS.iter().position(|d| d == step).map(|rank| (rank, step)))
        .max_by_key(|(rank, _)| *rank)
        .map(|(_, step)| step.clone())
}

pub fn get_feature_wall_tour_depth_step(
    workflow_id: &str,
    agent_step_id: Option<&str>,
    workbench_step_id: Option<&str>,
    review_step_id: Option<&str>,
) -> String {
    match workflow_id {
        "agents-orchestration" => agent_depth_step(agent_step_id.unwrap_or("statuses")),
        "workbench" => workbench_depth_step(workbench_step_id.unwrap_or("terminal")),
        "review" => review_depth_step(review_step_id.unwrap_or("notes")),
        other => other.to_string(),
    }
}

pub fn build_feature_wall_tour_depth_summary(input: &FeatureWallTourDepthInput) -> FeatureWallTourDepthSummary {
    let mut visited_depth_steps: Vec<String> = Vec::new();
    if input.visited_workflows.contains("workspaces") {
        visited_depth_steps.push("workspaces".to_string());
    }
    if input.visited_workflows.contains("tasks") {
        visited_depth_steps.push("tasks".to_string());
    }
    visited_depth_steps.extend(input.visited_agent_steps.iter().map(|s| agent_depth_step(s)));
    visited_depth_steps.extend(input.visited_workbench_steps.iter().map(|s| workbench_depth_step(s)));
    visited_depth_steps.extend(input.visited_review_steps.iter().map(|s| review_depth_step(s)));

    let count_true = |values: &[bool]| values.iter().filter(|&&done| done).count();
    FeatureWallTourDepthSummary {
        furthest_step: furthest_depth_step(&visited_depth_steps),
        last_group_id: input.last_group_id.filter(|id| !id.is_empty()).map(str::to_string),
        visited_workflow_count: input.visited_workflows.len(),
        visited_substep_count: input.visited_agent_steps.len()
            + input.visited_workbench_steps.len()
            + input.visited_review_steps.len(),
        completed_workflow_count: count_true(input.workflow_done_values),
        completed_substep_count: count_true(input.agent_step_done_values)
            + count_true(input.workbench_step_done_values)
            + count_true(input.review_step_done_values),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(items: &[&'static str]) -> HashSet<&'static str> {
        items.iter().copied().collect()
    }

    #[test]
    fn maps_workflow_and_nested_steps_to_canonical_depth_values() {
        assert_eq!(get_feature_wall_tour_depth_step("workspaces", None, None, None), "workspaces");
        assert_eq!(
            get_feature_wall_tour_depth_step("agents-orchestration", Some("usage"), None, None),
            "agents_usage"
        );
        assert_eq!(
            get_feature_wall_tour_depth_step("workbench", None, Some("browser"), None),
            "workbench_browser"
        );
        assert_eq!(get_feature_wall_tour_depth_step("review", None, None, Some("ship")), "review_ship");
    }

    #[test]
    fn builds_session_local_counts_and_furthest_step_from_visited_sets() {
        let visited_workflows = set(&["workspaces", "workbench"]);
        let visited_agent_steps = set(&[]);
        let visited_workbench_steps = set(&["terminal", "editor"]);
        let visited_review_steps = set(&[]);
        let summary = build_feature_wall_tour_depth_summary(&FeatureWallTourDepthInput {
            visited_workflows: &visited_workflows,
            visited_agent_steps: &visited_agent_steps,
            visited_workbench_steps: &visited_workbench_steps,
            visited_review_steps: &visited_review_steps,
            // workspaces=true, rest false
            workflow_done_values: &[true, false, false, false, false],
            agent_step_done_values: &[false, false, false],
            // terminal=true, editor=true, browser=false
            workbench_step_done_values: &[true, true, false],
            review_step_done_values: &[false, false, false],
            last_group_id: Some("workbench"),
        });
        assert_eq!(
            summary,
            FeatureWallTourDepthSummary {
                furthest_step: Some("workbench_editor".to_string()),
                last_group_id: Some("workbench".to_string()),
                visited_workflow_count: 2,
                visited_substep_count: 2,
                completed_workflow_count: 1,
                completed_substep_count: 2,
            }
        );
    }
}
