//! Parity dispatch for `orca_agents::tui_agent_selection` vs
//! `src/shared/tui-agent-selection.ts`.

use orca_agents::{
    filter_enabled_tui_agents, is_tui_agent_enabled, normalize_disabled_tui_agents, pick_tui_agent,
};
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "pickTuiAgent" => {
            let preferred = input.get("preferred").and_then(Value::as_str);
            let detected = json_str_args(input.get("detected"));
            let disabled = json_str_args(input.get("disabled"));
            match pick_tui_agent(preferred, &detected, &disabled) {
                Some(agent) => Value::String(agent),
                None => Value::Null,
            }
        }
        "normalizeDisabledTuiAgents" => {
            // Single-arg: input is the raw value; non-arrays yield [] like the TS guard.
            let value = json_str_args(Some(input));
            strings_to_json(normalize_disabled_tui_agents(&value))
        }
        "isTuiAgentEnabled" => {
            let agent = input.get("agent").and_then(Value::as_str).unwrap_or("");
            let disabled = json_str_args(input.get("disabled"));
            Value::Bool(is_tui_agent_enabled(agent, &disabled))
        }
        "filterEnabledTuiAgents" => {
            let agents = json_str_args(input.get("agents"));
            let disabled = json_str_args(input.get("disabled"));
            strings_to_json(filter_enabled_tui_agents(&agents, &disabled))
        }
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

/// Borrow a JSON array as `&str` args; non-strings (e.g. the TS `null` member)
/// become "" so positions are preserved but they drop out as non-agents — the
/// same outcome as the TS `isTuiAgent` filter. Non-arrays yield an empty slice.
fn json_str_args(value: Option<&Value>) -> Vec<&str> {
    value
        .and_then(Value::as_array)
        .map(|items| items.iter().map(|v| v.as_str().unwrap_or("")).collect())
        .unwrap_or_default()
}

fn strings_to_json(values: Vec<String>) -> Value {
    Value::Array(values.into_iter().map(Value::String).collect())
}
