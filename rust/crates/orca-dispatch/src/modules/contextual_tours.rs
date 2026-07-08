//! Parity dispatch for `orca_config::contextual_tours` vs
//! `src/shared/contextual-tours.ts`.

use orca_config::contextual_tours::{
    get_contextual_tour, is_contextual_tour_id, normalize_contextual_tour_ids, ContextualTour,
    ContextualTourId, ContextualTourStep, ContextualTourStepAction, ContextualTourStepControl,
};
use serde_json::{json, Map, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "isContextualTourId" => Value::Bool(is_contextual_tour_id(input)),
        "normalizeContextualTourIds" => {
            Value::Array(
                normalize_contextual_tour_ids(input)
                    .iter()
                    .map(|id| Value::String(id.as_str().to_string()))
                    .collect(),
            )
        }
        "getContextualTour" => match input.as_str().and_then(ContextualTourId::from_id) {
            Some(id) => match get_contextual_tour(id) {
                Some(tour) => tour_to_json(&tour),
                None => json!({ "__parity_error__": "contextual tour not found" }),
            },
            // Vectors only carry known ids; an unknown one is a vector bug.
            None => json!({ "__parity_error__": "unknown ContextualTourId in input" }),
        },
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

/// Match `JSON.stringify` of a TS `ContextualTour` (object-literal key order;
/// optional fields are emitted only when present).
fn tour_to_json(tour: &ContextualTour) -> Value {
    let mut object = Map::new();
    object.insert("id".to_string(), Value::String(tour.id.as_str().to_string()));
    if !tour.allowed_active_modals.is_empty() {
        object.insert(
            "allowedActiveModals".to_string(),
            Value::Array(tour.allowed_active_modals.iter().map(|m| Value::String(m.to_string())).collect()),
        );
    }
    object.insert("steps".to_string(), Value::Array(tour.steps.iter().map(step_to_json).collect()));
    Value::Object(object)
}

fn step_to_json(step: &ContextualTourStep) -> Value {
    let mut object = Map::new();
    object.insert("title".to_string(), Value::String(step.title.to_string()));
    object.insert("body".to_string(), Value::String(step.body.to_string()));
    object.insert("targetSelector".to_string(), Value::String(step.target_selector.to_string()));
    if let Some(value) = step.required_for_start {
        object.insert("requiredForStart".to_string(), Value::Bool(value));
    }
    if let Some(value) = step.fallback_copy {
        object.insert("fallbackCopy".to_string(), Value::String(value.to_string()));
    }
    if let Some(value) = step.preferred_placement {
        object.insert("preferredPlacement".to_string(), Value::String(value.as_str().to_string()));
    }
    if let Some(value) = step.target_pulse {
        object.insert("targetPulse".to_string(), Value::Bool(value));
    }
    if let Some(value) = step.hide_primary_action {
        object.insert("hidePrimaryAction".to_string(), Value::Bool(value));
    }
    if let Some(control) = step.control {
        object.insert("control".to_string(), control_to_json(&control));
    }
    if let Some(action) = step.primary_action {
        object.insert("primaryAction".to_string(), action_to_json(&action));
    }
    if let Some(action) = step.secondary_action {
        object.insert("secondaryAction".to_string(), action_to_json(&action));
    }
    if let Some(id) = step.advance_on_feature_interaction {
        object.insert("advanceOnFeatureInteraction".to_string(), Value::String(id.as_str().to_string()));
    }
    Value::Object(object)
}

fn control_to_json(control: &ContextualTourStepControl) -> Value {
    json!({ "kind": control.kind.as_str() })
}

fn action_to_json(action: &ContextualTourStepAction) -> Value {
    json!({ "kind": action.kind.as_str(), "label": action.label })
}
