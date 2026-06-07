//! Parity dispatch for `orca_config::feature_interactions` vs
//! `src/shared/feature-interactions.ts`.

use orca_config::{
    has_feature_interaction, is_feature_interaction_id, normalize_feature_interactions,
    FeatureInteractionId, FeatureInteractionState,
};
use serde_json::{json, Map, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "normalizeFeatureInteractions" => state_to_json(&normalize_feature_interactions(input)),
        "hasFeatureInteraction" => match input.get("id").and_then(Value::as_str).and_then(FeatureInteractionId::from_id) {
            Some(id) => Value::Bool(has_feature_interaction(input.get("state"), id)),
            // Vectors only carry known ids; an unknown one is a vector bug, not a port divergence.
            None => json!({ "__parity_error__": "unknown FeatureInteractionId in input.id" }),
        },
        "isFeatureInteractionId" => Value::Bool(is_feature_interaction_id(input)),
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

/// Match `JSON.stringify` of the TS `FeatureInteractionState` record map.
fn state_to_json(state: &FeatureInteractionState) -> Value {
    let mut map = Map::new();
    for (id, record) in state {
        map.insert(
            id.as_str().to_string(),
            json!({
                "firstInteractedAt": record.first_interacted_at,
                "interactionCount": record.interaction_count,
            }),
        );
    }
    Value::Object(map)
}
