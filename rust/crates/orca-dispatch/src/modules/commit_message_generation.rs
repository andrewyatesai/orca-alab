//! Parity dispatch for `orca_agents::commit_message_generation` vs
//! `src/shared/commit-message-generation.ts`.

use orca_agents::{
    build_commit_message_prompt, split_generated_commit_message, CommitMessageDraftContext,
};
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "buildCommitMessagePrompt" => {
            let ctx = input.get("context");
            let context = CommitMessageDraftContext {
                // TS `branch: string | null`: null/absent → None, matching `?? '(detached)'`.
                branch: ctx
                    .and_then(|c| c.get("branch"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
                staged_summary: string_field(ctx, "stagedSummary"),
                staged_patch: string_field(ctx, "stagedPatch"),
            };
            let custom_prompt = input.get("customPrompt").and_then(Value::as_str).unwrap_or("");
            Value::String(build_commit_message_prompt(&context, custom_prompt))
        }
        "splitGeneratedCommitMessage" => {
            let message = input.as_str().unwrap_or("");
            let result = split_generated_commit_message(message);
            // Match `JSON.stringify` of the TS `GeneratedCommitMessage` record.
            json!({
                "subject": result.subject,
                "body": result.body,
                "message": result.message,
            })
        }
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

fn string_field(object: Option<&Value>, key: &str) -> String {
    object
        .and_then(|value| value.get(key))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}
