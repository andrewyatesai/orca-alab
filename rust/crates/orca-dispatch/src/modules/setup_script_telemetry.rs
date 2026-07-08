//! Parity dispatch for `orca_core::setup_script_telemetry` vs
//! `src/shared/setup-script-telemetry.ts`.

use orca_core::setup_script_telemetry::{
    build_setup_script_prompt_action_telemetry, build_setup_script_prompt_telemetry,
    SetupScriptCandidateInput, SetupScriptCountBucket, SetupScriptPromptActionTelemetry,
    SetupScriptPromptMode, SetupScriptPromptTelemetry,
};
use serde_json::{json, Map, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "buildSetupScriptPromptTelemetry" => {
            let candidate = candidate_from_json(input.get("candidate"));
            prompt_telemetry_to_json(&build_setup_script_prompt_telemetry(
                candidate.as_ref(),
                input.get("hasSharedHooks").and_then(Value::as_bool).unwrap_or(false),
            ))
        }
        "buildSetupScriptPromptActionTelemetry" => {
            let candidate = candidate_from_json(input.get("candidate"));
            action_telemetry_to_json(&build_setup_script_prompt_action_telemetry(
                input.get("action").and_then(Value::as_str).unwrap_or_default(),
                candidate.as_ref(),
                input.get("hasSharedHooks").and_then(Value::as_bool).unwrap_or(false),
                // TS treats an absent `editedBeforeSave` as undefined (key omitted
                // from the payload); the vectors only carry absent or a real bool.
                input.get("editedBeforeSave").and_then(Value::as_bool),
            ))
        }
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

/// The TS builder only reads the provider enum plus the *lengths* of `files` and
/// `unsupportedFields` off the candidate; a `null`/absent candidate is `None`.
fn candidate_from_json(value: Option<&Value>) -> Option<SetupScriptCandidateInput> {
    let candidate = value?.as_object()?;
    Some(SetupScriptCandidateInput {
        provider: candidate
            .get("provider")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        file_count: array_len(candidate.get("files")),
        unsupported_field_count: array_len(candidate.get("unsupportedFields")),
    })
}

fn array_len(value: Option<&Value>) -> usize {
    value.and_then(Value::as_array).map_or(0, |items| items.len())
}

/// Match `JSON.stringify` of the TS `SetupScriptPromptTelemetry`: keys in the TS
/// literal's order, with `provider` omitted entirely when there is no candidate.
fn prompt_telemetry_to_json(telemetry: &SetupScriptPromptTelemetry) -> Value {
    Value::Object(base_fields(
        telemetry.mode,
        telemetry.file_count_bucket,
        telemetry.unsupported_field_count_bucket,
        telemetry.has_shared_hooks,
        &telemetry.provider,
    ))
}

/// Match `JSON.stringify` of the TS `SetupScriptPromptActionTelemetry`: the base
/// prompt fields, then `action`, then `edited_before_save` only when the TS arg
/// was provided (None ≙ undefined ≙ key omitted).
fn action_telemetry_to_json(telemetry: &SetupScriptPromptActionTelemetry) -> Value {
    let mut map = base_fields(
        telemetry.mode,
        telemetry.file_count_bucket,
        telemetry.unsupported_field_count_bucket,
        telemetry.has_shared_hooks,
        &telemetry.provider,
    );
    map.insert("action".to_string(), json!(telemetry.action));
    if let Some(edited) = telemetry.edited_before_save {
        map.insert("edited_before_save".to_string(), json!(edited));
    }
    Value::Object(map)
}

fn base_fields(
    mode: SetupScriptPromptMode,
    file_count_bucket: SetupScriptCountBucket,
    unsupported_field_count_bucket: SetupScriptCountBucket,
    has_shared_hooks: bool,
    provider: &Option<String>,
) -> Map<String, Value> {
    let mut map = Map::new();
    map.insert("mode".to_string(), json!(mode.as_wire()));
    map.insert("file_count_bucket".to_string(), json!(file_count_bucket.as_wire()));
    map.insert(
        "unsupported_field_count_bucket".to_string(),
        json!(unsupported_field_count_bucket.as_wire()),
    );
    map.insert("has_shared_hooks".to_string(), json!(has_shared_hooks));
    if let Some(provider) = provider {
        map.insert("provider".to_string(), json!(provider));
    }
    map
}
