//! Parity dispatch for `orca_core::open_in_applications` vs
//! `src/shared/open-in-applications.ts`.

use orca_core::open_in_applications::{normalize_open_in_applications, RawOpenInApplication};
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "normalizeOpenInApplications" => normalize_dispatch(input),
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

fn normalize_dispatch(input: &Value) -> Value {
    let seed_defaults = input
        .get("seedDefaults")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    // `createIds` reifies the optional id generator: each blank id pops the next
    // entry (`None`/non-string once exhausted == falls back to a positional id),
    // mirroring the TS closure that returns successive ids.
    let create_ids: Vec<Option<String>> = input
        .get("createIds")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().map(json_string).collect())
        .unwrap_or_default();
    let mut create_index = 0usize;
    let create_id = move || {
        let next = create_ids.get(create_index).cloned().flatten();
        create_index += 1;
        next
    };

    // `None` for any non-array value (matches the TS `Array.isArray` guard);
    // non-object rows map to an all-`None` raw row so positional indices align.
    let rows: Option<Vec<RawOpenInApplication>> = input
        .get("value")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().map(to_raw_row).collect());

    let out = normalize_open_in_applications(rows.as_deref(), create_id, seed_defaults);

    Value::Array(
        out.into_iter()
            .map(|app| {
                json!({
                    "id": app.id,
                    "label": app.label,
                    "command": app.command,
                })
            })
            .collect(),
    )
}

/// `Some(string)` only for JSON strings — non-strings normalize to a blank token
/// in TS (`typeof value === 'string' ? ... : ''`), so they become `None` here.
fn json_string(value: &Value) -> Option<String> {
    value.as_str().map(str::to_string)
}

fn to_raw_row(value: &Value) -> RawOpenInApplication {
    RawOpenInApplication {
        id: json_string(value.get("id").unwrap_or(&Value::Null)),
        label: json_string(value.get("label").unwrap_or(&Value::Null)),
        command: json_string(value.get("command").unwrap_or(&Value::Null)),
    }
}
