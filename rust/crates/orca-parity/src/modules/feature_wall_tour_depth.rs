//! Parity dispatch for `orca_core::feature_wall_tour_depth` vs
//! `src/shared/feature-wall-tour-depth.ts`.

use orca_core::feature_wall_tour_depth::{
    build_feature_wall_tour_depth_summary, get_feature_wall_tour_depth_step,
    FeatureWallTourDepthInput, FeatureWallTourDepthSummary,
};
use serde_json::{json, Map, Value};
use std::collections::HashSet;

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "getFeatureWallTourDepthStep" => Value::String(get_feature_wall_tour_depth_step(
            input.get("workflowId").and_then(Value::as_str).unwrap_or_default(),
            input.get("agentStepId").and_then(Value::as_str),
            input.get("workbenchStepId").and_then(Value::as_str),
            input.get("reviewStepId").and_then(Value::as_str),
        )),
        "buildFeatureWallTourDepthSummary" => {
            // Visited sets arrive as arrays; done records arrive as objects whose
            // values are the booleans (only the true-count matters in the port).
            let visited_workflows = str_vec(input, "visitedWorkflows");
            let visited_agent_steps = str_vec(input, "visitedAgentSteps");
            let visited_workbench_steps = str_vec(input, "visitedWorkbenchSteps");
            let visited_review_steps = str_vec(input, "visitedReviewSteps");
            let workflow_done = bool_values(input, "workflowDone");
            let agent_step_done = bool_values(input, "agentStepDone");
            let workbench_step_done = bool_values(input, "workbenchStepDone");
            let review_step_done = bool_values(input, "reviewStepDone");

            let visited_workflows_set: HashSet<&str> =
                visited_workflows.iter().map(String::as_str).collect();
            let visited_agent_steps_set: HashSet<&str> =
                visited_agent_steps.iter().map(String::as_str).collect();
            let visited_workbench_steps_set: HashSet<&str> =
                visited_workbench_steps.iter().map(String::as_str).collect();
            let visited_review_steps_set: HashSet<&str> =
                visited_review_steps.iter().map(String::as_str).collect();

            let summary = build_feature_wall_tour_depth_summary(&FeatureWallTourDepthInput {
                visited_workflows: &visited_workflows_set,
                visited_agent_steps: &visited_agent_steps_set,
                visited_workbench_steps: &visited_workbench_steps_set,
                visited_review_steps: &visited_review_steps_set,
                workflow_done_values: &workflow_done,
                agent_step_done_values: &agent_step_done,
                workbench_step_done_values: &workbench_step_done,
                review_step_done_values: &review_step_done,
                last_group_id: input.get("lastGroupId").and_then(Value::as_str),
            });
            summary_to_json(&summary)
        }
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

/// Collect a JSON string array into owned strings (non-strings dropped).
fn str_vec(input: &Value, key: &str) -> Vec<String> {
    input
        .get(key)
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

/// Collect the boolean values of a JSON `Record<id, boolean>` (the TS done map).
fn bool_values(input: &Value, key: &str) -> Vec<bool> {
    input
        .get(key)
        .and_then(Value::as_object)
        .map(|obj| obj.values().map(|v| v.as_bool().unwrap_or(false)).collect())
        .unwrap_or_default()
}

/// Match `JSON.stringify` of the TS `FeatureWallTourDepthSummary`: omit absent
/// optionals (None → no key, not null).
fn summary_to_json(summary: &FeatureWallTourDepthSummary) -> Value {
    let mut map = Map::new();
    if let Some(step) = &summary.furthest_step {
        map.insert("furthest_step".to_string(), Value::String(step.clone()));
    }
    if let Some(id) = &summary.last_group_id {
        map.insert("last_group_id".to_string(), Value::String(id.clone()));
    }
    map.insert("visited_workflow_count".to_string(), json!(summary.visited_workflow_count));
    map.insert("visited_substep_count".to_string(), json!(summary.visited_substep_count));
    map.insert("completed_workflow_count".to_string(), json!(summary.completed_workflow_count));
    map.insert("completed_substep_count".to_string(), json!(summary.completed_substep_count));
    Value::Object(map)
}
