//! Parity dispatch for `orca_agents::agent_status_types` vs
//! `src/shared/agent-status-types.ts`.

use orca_agents::{parse_agent_status_payload, AgentStatusState, ParsedAgentStatusPayload};
use serde_json::{json, Map, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        // Single arg: the raw JSON payload string the agent sent. A non-string
        // vector value can't be parsed, which mirrors the TS `null` result.
        "parseAgentStatusPayload" => match input.as_str().and_then(parse_agent_status_payload) {
            Some(parsed) => payload_to_json(&parsed),
            None => Value::Null,
        },
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

/// Match `JSON.stringify` of the TS `ParsedAgentStatusPayload`: `state` as its
/// string id, `prompt` always present, every optional omitted (not emitted as
/// `null`) when `None` — exactly what `JSON.stringify` drops for `undefined`.
fn payload_to_json(payload: &ParsedAgentStatusPayload) -> Value {
    let mut map = Map::new();
    map.insert("state".to_string(), Value::String(state_id(payload.state).to_string()));
    map.insert("prompt".to_string(), Value::String(payload.prompt.clone()));
    if let Some(agent_type) = &payload.agent_type {
        map.insert("agentType".to_string(), Value::String(agent_type.clone()));
    }
    if let Some(tool_name) = &payload.tool_name {
        map.insert("toolName".to_string(), Value::String(tool_name.clone()));
    }
    if let Some(tool_input) = &payload.tool_input {
        map.insert("toolInput".to_string(), Value::String(tool_input.clone()));
    }
    if let Some(message) = &payload.last_assistant_message {
        map.insert("lastAssistantMessage".to_string(), Value::String(message.clone()));
    }
    if let Some(interrupted) = payload.interrupted {
        map.insert("interrupted".to_string(), Value::Bool(interrupted));
    }
    Value::Object(map)
}

fn state_id(state: AgentStatusState) -> &'static str {
    match state {
        AgentStatusState::Working => "working",
        AgentStatusState::Blocked => "blocked",
        AgentStatusState::Waiting => "waiting",
        AgentStatusState::Done => "done",
    }
}
