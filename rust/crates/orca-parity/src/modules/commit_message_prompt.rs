//! Parity dispatch for `orca_agents::commit_message_prompt` vs
//! `src/shared/commit-message-prompt.ts`. Only the pure string transforms are
//! covered; both functions return a plain string, mirrored as `Value::String`.

use orca_agents::{build_commit_prompt, clean_generated_commit_message};
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "buildCommitPrompt" => {
            // Vectors always carry both args; missing/non-string is a vector bug,
            // so default to "" rather than diverge silently from the TS port.
            let diff = input.get("diff").and_then(Value::as_str).unwrap_or("");
            let suffix = input.get("suffix").and_then(Value::as_str).unwrap_or("");
            Value::String(build_commit_prompt(diff, suffix))
        }
        "cleanGeneratedCommitMessage" => {
            let raw = input.as_str().unwrap_or("");
            Value::String(clean_generated_commit_message(raw))
        }
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}
