//! Parity dispatch for `orca_agents::commit_message_prompt` vs
//! `src/shared/commit-message-prompt.ts`. Only the pure string transforms are
//! covered; both functions return a plain string, mirrored as `Value::String`.

use orca_agents::{
    build_commit_prompt, clean_generated_commit_message, truncate_diff_for_prompt,
    STAGED_DIFF_BYTE_BUDGET,
};
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
        "truncateDiffForPrompt" => {
            let diff = input.get("diff").and_then(Value::as_str).unwrap_or("");
            // TS default arg is STAGED_DIFF_BYTE_BUDGET when budget is omitted.
            let budget = input
                .get("budget")
                .and_then(Value::as_u64)
                .map(|b| b as usize)
                .unwrap_or(STAGED_DIFF_BYTE_BUDGET);
            Value::String(truncate_diff_for_prompt(diff, budget))
        }
        "cleanGeneratedCommitMessage" => {
            let raw = input.as_str().unwrap_or("");
            Value::String(clean_generated_commit_message(raw))
        }
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}
