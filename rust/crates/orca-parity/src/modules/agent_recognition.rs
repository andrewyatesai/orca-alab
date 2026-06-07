//! Parity dispatch for `orca_core::agent_recognition` vs
//! `src/shared/agent-name-token-match.ts` (+ `agent-process-recognition.ts`).

use orca_core::agent_recognition::{
    is_expected_agent_process, title_has_agent_name, title_has_any_legacy_agent_name,
};
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "titleHasAgentName" => {
            let title = input.get("title").and_then(Value::as_str).unwrap_or("");
            let name = input.get("name").and_then(Value::as_str).unwrap_or("");
            Value::Bool(title_has_agent_name(title, name))
        }
        "titleHasAnyLegacyAgentName" => {
            let title = input.get("title").and_then(Value::as_str).unwrap_or("");
            Value::Bool(title_has_any_legacy_agent_name(title))
        }
        "isExpectedAgentProcess" => {
            // TS null/undefined processName maps to Rust None (both -> empty after normalize).
            let process_name = input.get("processName").and_then(Value::as_str);
            let expected = input
                .get("expectedProcess")
                .and_then(Value::as_str)
                .unwrap_or("");
            Value::Bool(is_expected_agent_process(process_name, expected))
        }
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}
