//! Parity dispatch for `orca_config::feature_tips` vs
//! `src/shared/feature-tips.ts`.

use orca_config::feature_tips::{
    get_completed_feature_tip_ids, get_ordered_unseen_feature_tips, is_feature_tip_id,
    normalize_feature_tip_ids, CompletedFeatureTipState, FeatureTip, FeatureTipId,
};
use serde_json::{json, Map, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "isFeatureTipId" => Value::Bool(is_feature_tip_id(input)),
        "normalizeFeatureTipIds" => ids_to_json(&normalize_feature_tip_ids(input)),
        "getCompletedFeatureTipIds" => {
            let state = CompletedFeatureTipState {
                cli_installed: input.get("cliInstalled").and_then(Value::as_bool).unwrap_or(false),
                voice_dictation_enabled: input
                    .get("voiceDictationEnabled")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                feature_interactions: input.get("featureInteractions").cloned(),
            };
            // TS returns a `Set`; the dispatch spreads it to an array.
            ids_to_json(&get_completed_feature_tip_ids(&state))
        }
        "getOrderedUnseenFeatureTips" => {
            let seen = parse_ids(input.get("seenTipIds"));
            let completed = parse_ids(input.get("completedTipIds"));
            Value::Array(get_ordered_unseen_feature_tips(&seen, &completed).iter().map(tip_to_json).collect())
        }
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

fn parse_ids(value: Option<&Value>) -> Vec<FeatureTipId> {
    value
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(|item| item.as_str().and_then(FeatureTipId::from_id)).collect())
        .unwrap_or_default()
}

fn ids_to_json(ids: &[FeatureTipId]) -> Value {
    Value::Array(ids.iter().map(|id| Value::String(id.as_str().to_string())).collect())
}

/// Match `JSON.stringify` of a TS `FeatureTip` (object-literal key order).
fn tip_to_json(tip: &FeatureTip) -> Value {
    let mut object = Map::new();
    object.insert("id".to_string(), Value::String(tip.id.as_str().to_string()));
    object.insert("priority".to_string(), Value::String(tip.priority.as_str().to_string()));
    object.insert("eyebrow".to_string(), Value::String(tip.eyebrow.to_string()));
    object.insert("title".to_string(), Value::String(tip.title.to_string()));
    object.insert("description".to_string(), Value::String(tip.description.to_string()));
    object.insert("action".to_string(), Value::String(tip.action.as_str().to_string()));
    object.insert("ctaLabel".to_string(), Value::String(tip.cta_label.to_string()));
    object.insert(
        "completedByFeatureInteractions".to_string(),
        Value::Array(
            tip.completed_by_feature_interactions
                .iter()
                .map(|id| Value::String(id.as_str().to_string()))
                .collect(),
        ),
    );
    Value::Object(object)
}
