//! Feature-wall walkthrough depth telemetry.
//!
//! Maps lifecycle chapters and steps to one canonical order and summarizes the
//! current explicit walkthrough session. Pure; identifiers remain path-free.

use std::collections::HashSet;

pub const FEATURE_WALL_TOUR_DEPTH_STEPS: [&str; 14] = [
    "terminal",
    "add-project",
    "tasks",
    "workspaces",
    "agents",
    "workbench",
    "browser-design",
    "review-ship",
    "cli-skills",
    "orchestration",
    "automations",
    "remote-mobile",
    "mobile-emulators",
    "computer-use",
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

pub struct FeatureWallTourDepthInput<'a> {
    pub visited_workflows: &'a HashSet<&'a str>,
    pub visited_steps: &'a HashSet<&'a str>,
    pub workflow_done_values: &'a [bool],
    pub step_done_values: &'a [bool],
    pub last_group_id: Option<&'a str>,
}

fn default_step_for_workflow(workflow_id: &str) -> &str {
    match workflow_id {
        "start" => "terminal",
        "plan" => "tasks",
        "build" => "agents",
        "ship" => "review-ship",
        "scale" => "cli-skills",
        "anywhere" => "remote-mobile",
        _ => "terminal",
    }
}

pub fn get_feature_wall_tour_depth_step(workflow_id: &str, step_id: Option<&str>) -> String {
    step_id
        .filter(|step| FEATURE_WALL_TOUR_DEPTH_STEPS.contains(step))
        .unwrap_or_else(|| default_step_for_workflow(workflow_id))
        .to_string()
}

fn furthest_depth_step(steps: &HashSet<&str>) -> Option<String> {
    FEATURE_WALL_TOUR_DEPTH_STEPS
        .iter()
        .rev()
        .find(|step| steps.contains(**step))
        .map(|step| (*step).to_string())
}

pub fn build_feature_wall_tour_depth_summary(
    input: &FeatureWallTourDepthInput,
) -> FeatureWallTourDepthSummary {
    let count_true = |values: &[bool]| values.iter().filter(|&&done| done).count();
    FeatureWallTourDepthSummary {
        furthest_step: furthest_depth_step(input.visited_steps),
        last_group_id: input
            .last_group_id
            .filter(|id| !id.is_empty())
            .map(str::to_string),
        visited_workflow_count: input.visited_workflows.len(),
        visited_substep_count: input.visited_steps.len(),
        completed_workflow_count: count_true(input.workflow_done_values),
        completed_substep_count: count_true(input.step_done_values),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(items: &[&'static str]) -> HashSet<&'static str> {
        items.iter().copied().collect()
    }

    #[test]
    fn maps_chapters_to_their_first_step() {
        assert_eq!(get_feature_wall_tour_depth_step("start", None), "terminal");
        assert_eq!(get_feature_wall_tour_depth_step("plan", None), "tasks");
        assert_eq!(
            get_feature_wall_tour_depth_step("anywhere", Some("computer-use")),
            "computer-use"
        );
    }

    #[test]
    fn builds_session_counts_and_furthest_step() {
        let visited_workflows = set(&["start", "plan"]);
        let visited_steps = set(&["terminal", "add-project", "tasks"]);
        let summary = build_feature_wall_tour_depth_summary(&FeatureWallTourDepthInput {
            visited_workflows: &visited_workflows,
            visited_steps: &visited_steps,
            workflow_done_values: &[true, false, false, false, false, false],
            step_done_values: &[
                true, true, true, false, false, false, false, false, false, false, false, false,
                false, false,
            ],
            last_group_id: Some("plan"),
        });
        assert_eq!(
            summary,
            FeatureWallTourDepthSummary {
                furthest_step: Some("tasks".to_string()),
                last_group_id: Some("plan".to_string()),
                visited_workflow_count: 2,
                visited_substep_count: 3,
                completed_workflow_count: 1,
                completed_substep_count: 3,
            }
        );
    }
}
