//! Parity dispatch for `orca_core::terminal_tab_id` vs
//! `src/shared/terminal-tab-id.ts`.

use orca_core::terminal_tab_id::{is_valid_host_terminal_tab_id, is_valid_terminal_tab_id};
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    // Both functions take a single string `value`; vectors carry it as the raw
    // JSON string. A non-string is a vector bug, not a port divergence.
    let value = input.as_str().unwrap_or_default();
    match function {
        "isValidTerminalTabId" => Value::Bool(is_valid_terminal_tab_id(value)),
        "isValidHostTerminalTabId" => Value::Bool(is_valid_host_terminal_tab_id(value)),
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}
