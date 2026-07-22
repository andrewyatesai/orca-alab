//! JSON dispatch for `orca_core::feature_wall_tour_depth`.

use orca_core::feature_wall_tour_depth::{
    build_feature_wall_tour_depth_summary, get_feature_wall_tour_depth_step,
    FeatureWallTourDepthInput, FeatureWallTourDepthSummary,
};
use serde_json::{json, Map, Value};
use std::collections::HashSet;

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "getFeatureWallTourDepthStep" => Value::String(get_feature_wall_tour_depth_step(
            input
                .get("workflowId")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            input.get("stepId").and_then(Value::as_str),
        )),
        "buildFeatureWallTourDepthSummary" => {
            let visited_workflows = str_vec(input, "visitedWorkflows");
            let visited_steps = str_vec(input, "visitedSteps");
            let workflow_done = bool_values(input, "workflowDone");
            let step_done = bool_values(input, "stepDone");
            let visited_workflows_set: HashSet<&str> =
                visited_workflows.iter().map(String::as_str).collect();
            let visited_steps_set: HashSet<&str> =
                visited_steps.iter().map(String::as_str).collect();

            let summary = build_feature_wall_tour_depth_summary(&FeatureWallTourDepthInput {
                visited_workflows: &visited_workflows_set,
                visited_steps: &visited_steps_set,
                workflow_done_values: &workflow_done,
                step_done_values: &step_done,
                last_group_id: input.get("lastGroupId").and_then(Value::as_str),
            });
            summary_to_json(&summary)
        }
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

fn str_vec(input: &Value, key: &str) -> Vec<String> {
    input
        .get(key)
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn bool_values(input: &Value, key: &str) -> Vec<bool> {
    input
        .get(key)
        .and_then(Value::as_object)
        .map(|values| {
            values
                .values()
                .map(|value| value.as_bool().unwrap_or(false))
                .collect()
        })
        .unwrap_or_default()
}

fn summary_to_json(summary: &FeatureWallTourDepthSummary) -> Value {
    let mut map = Map::new();
    if let Some(step) = &summary.furthest_step {
        map.insert("furthest_step".to_string(), Value::String(step.clone()));
    }
    if let Some(id) = &summary.last_group_id {
        map.insert("last_group_id".to_string(), Value::String(id.clone()));
    }
    map.insert(
        "visited_workflow_count".to_string(),
        json!(summary.visited_workflow_count),
    );
    map.insert(
        "visited_substep_count".to_string(),
        json!(summary.visited_substep_count),
    );
    map.insert(
        "completed_workflow_count".to_string(),
        json!(summary.completed_workflow_count),
    );
    map.insert(
        "completed_substep_count".to_string(),
        json!(summary.completed_substep_count),
    );
    Value::Object(map)
}
