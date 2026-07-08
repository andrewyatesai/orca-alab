//! Parity dispatch for `orca_text::agent_tab_title` vs
//! `src/shared/agent-tab-title.ts`.

use orca_text::agent_tab_title::derive_generated_tab_title;
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        // TS returns `string | null`; `JSON.stringify` maps that to a JSON
        // string or `null`, so None becomes Value::Null (not an omitted key).
        "deriveGeneratedTabTitle" => match input.as_str() {
            Some(prompt) => match derive_generated_tab_title(prompt) {
                Some(title) => Value::String(title),
                None => Value::Null,
            },
            // Vectors only carry string prompts; a non-string is a vector bug.
            None => json!({ "__parity_error__": "deriveGeneratedTabTitle expects a string input" }),
        },
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}
