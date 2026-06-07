//! Parity dispatch for `orca_core::synthetic_agent_title` vs
//! `src/shared/synthetic-agent-title.ts`.

use orca_core::synthetic_agent_title::{
    get_synthetic_agent_terminal_title, get_synthetic_agent_title_profile,
    should_drive_synthetic_agent_title_from_hook, SyntheticAgentTitleProfile,
};
use serde_json::{json, Map, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        // Single arg: input is the raw agentType (string, or null ≙ TS null/undefined).
        "getSyntheticAgentTitleProfile" => match get_synthetic_agent_title_profile(input.as_str()) {
            Some(profile) => profile_to_json(&profile),
            None => Value::Null,
        },
        "getSyntheticAgentTerminalTitle" => {
            let (agent_type, state) = agent_type_and_state(input);
            match get_synthetic_agent_terminal_title(agent_type, state) {
                Some(title) => Value::String(title.to_string()),
                None => Value::Null,
            }
        }
        "shouldDriveSyntheticAgentTitleFromHook" => {
            let (agent_type, state) = agent_type_and_state(input);
            Value::Bool(should_drive_synthetic_agent_title_from_hook(agent_type, state))
        }
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

/// Destructure the `{ agentType, state }` object the multi-arg vectors carry.
/// A missing/null state degrades to "" (TS would pass `undefined`, which is
/// neither "working"/"blocked"/"waiting"), matching the idle-label fall-through.
fn agent_type_and_state(input: &Value) -> (Option<&str>, &str) {
    let agent_type = input.get("agentType").and_then(Value::as_str);
    let state = input.get("state").and_then(Value::as_str).unwrap_or("");
    (agent_type, state)
}

/// Match `JSON.stringify` of the TS `SyntheticAgentTitleProfile`: the optional
/// `synthesizeWorkingTitle` key is omitted when `None` (undefined in TS), not
/// emitted as `null`.
fn profile_to_json(profile: &SyntheticAgentTitleProfile) -> Value {
    let mut map = Map::new();
    map.insert("workingLabel".to_string(), Value::String(profile.working_label.to_string()));
    map.insert("permissionLabel".to_string(), Value::String(profile.permission_label.to_string()));
    map.insert("idleLabel".to_string(), Value::String(profile.idle_label.to_string()));
    if let Some(synthesize) = profile.synthesize_working_title {
        map.insert("synthesizeWorkingTitle".to_string(), Value::Bool(synthesize));
    }
    Value::Object(map)
}
