//! Parity dispatch for `orca_core::agent_hook_endpoint_file` vs
//! `src/shared/agent-hook-endpoint-file.ts`.

use orca_core::agent_hook_endpoint_file::is_agent_hook_endpoint_file_name;
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        // `isAgentHookEndpointFileName(name: string)`: vectors only carry strings;
        // a non-string is treated as the empty name (never a known file name).
        "isAgentHookEndpointFileName" => {
            Value::Bool(is_agent_hook_endpoint_file_name(input.as_str().unwrap_or_default()))
        }
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}
