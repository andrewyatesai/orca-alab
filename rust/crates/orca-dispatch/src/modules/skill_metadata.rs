//! Parity dispatch for `orca_text::skill_metadata` vs
//! `src/shared/skill-metadata.ts`.

use orca_text::skill_metadata::summarize_skill_markdown;
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "summarizeSkillMarkdown" => {
            // Single string arg; the vector carries the markdown directly.
            let markdown = input.as_str().unwrap_or("");
            let summary = summarize_skill_markdown(markdown);
            // The TS return type is `{ name: string | null, description: string | null }`:
            // an absent field is emitted as explicit `null`, so map None -> Null (not omit).
            json!({
                "name": opt_to_value(summary.name),
                "description": opt_to_value(summary.description),
            })
        }
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

fn opt_to_value(value: Option<String>) -> Value {
    match value {
        Some(text) => Value::String(text),
        None => Value::Null,
    }
}
