//! Parity dispatch for `orca_git::source_control_ai` vs
//! `src/shared/source-control-ai.ts`.
//!
//! Only the genuinely pure JSON-in/JSON-out surface is exercised here:
//! `normalizeRepoSourceControlAiOverrides`, which defends an untrusted
//! `serde_json::Value` (the repo override blob) into the typed shape. The
//! resolver functions read a typed `GlobalSettings` slice + the agent catalog,
//! so they are covered by the in-crate test port rather than the JSON corpus.

use orca_git::source_control_ai::{
    normalize_repo_source_control_ai_overrides, RepoPrCreationDefaults,
    RepoSourceControlAiOverrides, SourceControlAiModelChoice, SourceControlAiOperation,
};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "normalizeRepoSourceControlAiOverrides" => {
            match normalize_repo_source_control_ai_overrides(input) {
                Some(overrides) => overrides_to_json(&overrides),
                // Non-record input → TS `undefined`; vectors only seed records.
                None => Value::Null,
            }
        }
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

fn overrides_to_json(overrides: &RepoSourceControlAiOverrides) -> Value {
    let mut map = Map::new();
    if let Some(by_op) = &overrides.model_overrides_by_operation {
        let mut inner = Map::new();
        for operation in SourceControlAiOperation::ALL {
            if let Some(choice) = by_op.get(&operation) {
                inner.insert(operation.as_str().to_string(), choice_to_json(choice));
            }
        }
        map.insert("modelOverridesByOperation".to_string(), Value::Object(inner));
    }
    if let Some(by_op) = &overrides.instructions_by_operation {
        let mut inner = Map::new();
        for operation in SourceControlAiOperation::ALL {
            if let Some(instruction) = by_op.get(&operation) {
                inner.insert(
                    operation.as_str().to_string(),
                    match instruction {
                        Some(text) => Value::String(text.clone()),
                        None => Value::Null,
                    },
                );
            }
        }
        map.insert("instructionsByOperation".to_string(), Value::Object(inner));
    }
    if let Some(pr) = &overrides.pr_creation_defaults {
        map.insert("prCreationDefaults".to_string(), pr_defaults_to_json(pr));
    }
    Value::Object(map)
}

fn choice_to_json(choice: &SourceControlAiModelChoice) -> Value {
    let mut map = Map::new();
    if let Some(by_agent) = &choice.selected_model_by_agent {
        map.insert("selectedModelByAgent".to_string(), string_map_to_json(by_agent));
    }
    if let Some(by_host) = &choice.selected_model_by_agent_by_host {
        let mut inner = Map::new();
        for (host, models) in by_host {
            inner.insert(host.clone(), string_map_to_json(models));
        }
        map.insert("selectedModelByAgentByHost".to_string(), Value::Object(inner));
    }
    if let Some(by_model) = &choice.selected_thinking_by_model {
        map.insert("selectedThinkingByModel".to_string(), string_map_to_json(by_model));
    }
    Value::Object(map)
}

fn pr_defaults_to_json(pr: &RepoPrCreationDefaults) -> Value {
    let mut map = Map::new();
    insert_tri_state(&mut map, "draft", pr.draft);
    insert_tri_state(&mut map, "useTemplate", pr.use_template);
    insert_tri_state(&mut map, "generateDetailsOnOpen", pr.generate_details_on_open);
    insert_tri_state(&mut map, "openAfterCreate", pr.open_after_create);
    Value::Object(map)
}

fn insert_tri_state(map: &mut Map<String, Value>, key: &str, field: Option<Option<bool>>) {
    if let Some(value) = field {
        map.insert(
            key.to_string(),
            match value {
                Some(flag) => Value::Bool(flag),
                None => Value::Null,
            },
        );
    }
}

fn string_map_to_json(values: &BTreeMap<String, String>) -> Value {
    let mut map = Map::new();
    for (key, value) in values {
        map.insert(key.clone(), Value::String(value.clone()));
    }
    Value::Object(map)
}
