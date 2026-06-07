//! Parity dispatch for `orca_core::agent_kind` vs `src/shared/agent-kind.ts`.

use orca_core::agent_kind::{agent_kind_to_tui_agent, tui_agent_to_agent_kind};
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "tuiAgentToAgentKind" => {
            // Absent/non-string agent mirrors TS reading an out-of-union value:
            // both fall through the lookup to the `other` catch-all.
            let agent = input.get("agent").and_then(Value::as_str).unwrap_or("");
            Value::String(tui_agent_to_agent_kind(agent).to_string())
        }
        "agentKindToTuiAgent" => {
            // Missing key (TS undefined) and JSON null both read as None here.
            let kind = input.get("kind").and_then(Value::as_str);
            match agent_kind_to_tui_agent(kind) {
                Some(agent) => Value::String(agent.to_string()),
                None => Value::Null,
            }
        }
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}
