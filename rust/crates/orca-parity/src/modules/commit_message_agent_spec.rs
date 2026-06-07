//! Parity dispatch for `orca_agents::commit_message_agent_spec` vs the spec/lookup
//! half of `src/shared/commit-message-agent-spec.ts`.

use orca_agents::{
    get_commit_message_agent_capability, get_commit_message_model,
    get_commit_message_model_capability, is_custom_agent_id, list_commit_message_agent_capabilities,
    list_commit_message_agent_ids, resolve_commit_message_agent_choice,
    CommitMessageAgentCapability, CommitMessageModel, CommitMessageModelCapability, ModelSource,
    ThinkingLevel,
};
use serde_json::{json, Map, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        // Single raw arg; JSON null -> None -> false (TS null/undefined both false).
        "isCustomAgentId" => Value::Bool(is_custom_agent_id(input.as_str())),
        "resolveCommitMessageAgentChoice" => {
            let configured = input.get("configuredAgentId").and_then(Value::as_str);
            let default = input.get("defaultTuiAgent").and_then(Value::as_str);
            let disabled: Vec<&str> = input
                .get("disabledTuiAgents")
                .and_then(Value::as_array)
                .map(|items| items.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            // TS returns `... | null` (a literal null), so None maps to Value::Null.
            match resolve_commit_message_agent_choice(configured, default, &disabled) {
                Some(choice) => Value::String(choice),
                None => Value::Null,
            }
        }
        // Vectors only exercise defined returns: TS returns `undefined` for a miss,
        // which JSON cannot represent and the comparator never equates with null.
        "getCommitMessageModel" => match get_commit_message_model(arg(input, "agentId"), arg(input, "modelId")) {
            Some(model) => model_to_value(&model),
            None => Value::Null,
        },
        "getCommitMessageAgentCapability" => {
            match get_commit_message_agent_capability(arg(input, "agentId")) {
                Some(capability) => capability_to_value(&capability),
                None => Value::Null,
            }
        }
        "getCommitMessageModelCapability" => {
            match get_commit_message_model_capability(arg(input, "agentId"), arg(input, "modelId")) {
                Some(model) => capability_model_to_value(&model),
                None => Value::Null,
            }
        }
        "listCommitMessageAgentIds" => Value::Array(
            list_commit_message_agent_ids().into_iter().map(|id| Value::String(id.to_string())).collect(),
        ),
        "listCommitMessageAgentCapabilities" => Value::Array(
            list_commit_message_agent_capabilities().iter().map(capability_to_value).collect(),
        ),
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

fn arg<'a>(input: &'a Value, key: &str) -> &'a str {
    input.get(key).and_then(Value::as_str).unwrap_or("")
}

/// Shape a `{ id, label, thinkingLevels?, defaultThinkingLevel? }` record exactly
/// like `JSON.stringify` of the TS `CommitMessageModel`: absent optionals are
/// omitted, not serialized as null.
fn model_value(
    id: &str,
    label: &str,
    thinking_levels: &Option<Vec<ThinkingLevel>>,
    default_thinking_level: &Option<String>,
) -> Value {
    let mut map = Map::new();
    map.insert("id".to_string(), Value::String(id.to_string()));
    map.insert("label".to_string(), Value::String(label.to_string()));
    if let Some(levels) = thinking_levels {
        map.insert("thinkingLevels".to_string(), levels_value(levels));
    }
    if let Some(default) = default_thinking_level {
        map.insert("defaultThinkingLevel".to_string(), Value::String(default.clone()));
    }
    Value::Object(map)
}

fn levels_value(levels: &[ThinkingLevel]) -> Value {
    Value::Array(levels.iter().map(|level| json!({ "id": level.id, "label": level.label })).collect())
}

fn model_to_value(model: &CommitMessageModel) -> Value {
    model_value(&model.id, &model.label, &model.thinking_levels, &model.default_thinking_level)
}

fn capability_model_to_value(model: &CommitMessageModelCapability) -> Value {
    model_value(&model.id, &model.label, &model.thinking_levels, &model.default_thinking_level)
}

fn capability_to_value(capability: &CommitMessageAgentCapability) -> Value {
    let mut map = Map::new();
    map.insert("id".to_string(), Value::String(capability.id.clone()));
    map.insert("label".to_string(), Value::String(capability.label.clone()));
    map.insert("modelSource".to_string(), Value::String(model_source_id(capability.model_source).to_string()));
    map.insert("defaultModelId".to_string(), Value::String(capability.default_model_id.clone()));
    map.insert(
        "models".to_string(),
        Value::Array(capability.models.iter().map(capability_model_to_value).collect()),
    );
    Value::Object(map)
}

/// TS serializes the `modelSource` union as its string id.
fn model_source_id(source: ModelSource) -> &'static str {
    match source {
        ModelSource::Static => "static",
        ModelSource::Dynamic => "dynamic",
    }
}
