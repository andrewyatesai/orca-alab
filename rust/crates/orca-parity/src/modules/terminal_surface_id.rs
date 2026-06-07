//! Parity dispatch for `orca_core::terminal_surface_id` vs
//! `src/shared/terminal-surface-id.ts`.

use orca_core::terminal_surface_id::{
    is_web_terminal_surface_tab_id, to_host_session_tab_id, to_web_terminal_surface_tab_id,
};
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    // Every function takes a single string arg; vectors carry it as the raw input.
    let arg = input.as_str().unwrap_or_default();
    match function {
        "toWebTerminalSurfaceTabId" => Value::String(to_web_terminal_surface_tab_id(arg)),
        "toHostSessionTabId" => Value::String(to_host_session_tab_id(arg)),
        "isWebTerminalSurfaceTabId" => Value::Bool(is_web_terminal_surface_tab_id(arg)),
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}
