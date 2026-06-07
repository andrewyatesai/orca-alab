//! Parity dispatch for `orca_core::tailnet_address` vs
//! `src/shared/tailnet-address.ts`.

use orca_core::tailnet_address::is_tailnet_ipv4_address;
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "isTailnetIPv4Address" => match input.as_str() {
            Some(address) => Value::Bool(is_tailnet_ipv4_address(address)),
            // Vectors only carry string inputs; a non-string is a vector bug.
            None => json!({ "__parity_error__": "expected string input for isTailnetIPv4Address" }),
        },
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}
