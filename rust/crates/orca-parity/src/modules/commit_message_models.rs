//! Parity dispatch for `orca_agents::commit_message_models` vs the parser half
//! of `src/shared/commit-message-agent-spec.ts`.
//!
//! Every parser takes a single `stdout` string and returns `CommitMessageModel[]`.
//! We shape the result Value to match `JSON.stringify` of the TS return: absent
//! optionals (`thinking_levels`/`default_thinking_level` => `None`) omit the key
//! rather than emitting `null`.

use orca_agents::{
    parse_codex_models, parse_cursor_models, parse_line_models, parse_pi_models, CommitMessageModel,
};
use serde_json::{json, Map, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    // Vectors carry the single `stdout` arg as a bare JSON string.
    let stdout = input.as_str().unwrap_or("");
    match function {
        "parseCodexModels" => models_to_json(&parse_codex_models(stdout)),
        "parseLineModels" => models_to_json(&parse_line_models(stdout)),
        "parsePiModels" => models_to_json(&parse_pi_models(stdout)),
        "parseCursorModels" => models_to_json(&parse_cursor_models(stdout)),
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

/// Match `JSON.stringify` of the TS `CommitMessageModel[]` return.
fn models_to_json(models: &[CommitMessageModel]) -> Value {
    Value::Array(models.iter().map(model_to_json).collect())
}

fn model_to_json(model: &CommitMessageModel) -> Value {
    let mut map = Map::new();
    map.insert("id".to_string(), Value::String(model.id.clone()));
    map.insert("label".to_string(), Value::String(model.label.clone()));
    // None => omit the key (TS leaves the optional field off the object).
    if let Some(levels) = &model.thinking_levels {
        let levels = levels
            .iter()
            .map(|l| json!({ "id": l.id, "label": l.label }))
            .collect::<Vec<_>>();
        map.insert("thinkingLevels".to_string(), Value::Array(levels));
    }
    if let Some(default) = &model.default_thinking_level {
        map.insert("defaultThinkingLevel".to_string(), Value::String(default.clone()));
    }
    Value::Object(map)
}
