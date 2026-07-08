//! Parity dispatch for `orca_text::mcp_env` vs `maskMcpEnv` in
//! `src/shared/mcp-config.ts`.

use orca_text::mcp_env::mask_mcp_env;
use serde_json::{json, Map, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "maskMcpEnv" => mask_to_json(input),
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

/// Match `JSON.stringify` of the TS `Record<string, string>` return.
///
/// Vectors only carry object env inputs with string values: TS returns
/// `undefined` for a non-object env, but a top-level `undefined` has no JSON
/// image, so those cases aren't exercised here.
fn mask_to_json(input: &Value) -> Value {
    let Some(obj) = input.as_object() else {
        return Value::Null;
    };
    let pairs: Vec<(&str, &str)> = obj
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str().unwrap_or_default()))
        .collect();
    match mask_mcp_env(Some(&pairs)) {
        Some(masked) => {
            let mut map = Map::new();
            for (key, value) in masked {
                map.insert(key, Value::String(value));
            }
            Value::Object(map)
        }
        None => Value::Null,
    }
}
