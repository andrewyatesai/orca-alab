//! Parity dispatch for `orca_core::nested_repo_telemetry` vs
//! `src/shared/nested-repo-telemetry.ts`.

use orca_core::nested_repo_telemetry::{
    build_nested_repo_import_action_telemetry, build_nested_repo_scan_telemetry,
    bucket_nested_repo_telemetry_count, cap_nested_repo_telemetry_count,
    should_emit_nested_repo_import_submit_telemetry, NestedRepoImportActionTelemetry,
    NestedRepoScanResult, NestedRepoScanTelemetry,
};
use serde_json::{json, Map, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "capNestedRepoTelemetryCount" => json!(cap_nested_repo_telemetry_count(count_arg(input))),
        "bucketNestedRepoTelemetryCount" => {
            Value::String(bucket_nested_repo_telemetry_count(count_arg(input)).to_string())
        }
        "shouldEmitNestedRepoImportSubmitTelemetry" => {
            // A `null` attempt id is not a string, so `as_str` yields None — matching
            // `Boolean(null && ...)` in TS.
            let attempt_id = input.get("attemptId").and_then(Value::as_str);
            let selected_count = input.get("selectedCount").and_then(Value::as_i64).unwrap_or(0);
            let is_busy = input.get("isBusy").and_then(Value::as_bool).unwrap_or(false);
            Value::Bool(should_emit_nested_repo_import_submit_telemetry(
                attempt_id,
                selected_count,
                is_busy,
            ))
        }
        "buildNestedRepoScanTelemetry" => {
            let scan = parse_scan(input.get("scan"));
            scan_telemetry_to_json(&build_nested_repo_scan_telemetry(
                str_arg(input, "attemptId"),
                str_arg(input, "surface"),
                str_arg(input, "runtimeKind"),
                scan.as_ref(),
            ))
        }
        "buildNestedRepoImportActionTelemetry" => {
            let found_count = input.get("foundCount").and_then(Value::as_i64).unwrap_or(0);
            let selected_count = input.get("selectedCount").and_then(Value::as_i64).unwrap_or(0);
            action_telemetry_to_json(&build_nested_repo_import_action_telemetry(
                str_arg(input, "attemptId"),
                str_arg(input, "surface"),
                str_arg(input, "runtimeKind"),
                str_arg(input, "action"),
                found_count,
                selected_count,
            ))
        }
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

/// `null` (the JSON image of NaN/Infinity) and missing numbers map to NaN so the
/// non-finite guard returns 0, matching `Number.isFinite` in TS.
fn count_arg(input: &Value) -> f64 {
    input.as_f64().unwrap_or(f64::NAN)
}

fn str_arg<'a>(input: &'a Value, key: &str) -> &'a str {
    input.get(key).and_then(Value::as_str).unwrap_or("")
}

/// Absent/`null` scan → None (`scan_failed`); otherwise project the few fields
/// the builder reads.
fn parse_scan(scan: Option<&Value>) -> Option<NestedRepoScanResult> {
    let scan = scan?;
    if scan.is_null() {
        return None;
    }
    Some(NestedRepoScanResult {
        repo_count: scan.get("repos").and_then(Value::as_array).map_or(0, Vec::len),
        selected_path_kind: scan
            .get("selectedPathKind")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        truncated: scan.get("truncated").and_then(Value::as_bool).unwrap_or(false),
        timed_out: scan.get("timedOut").and_then(Value::as_bool).unwrap_or(false),
    })
}

/// Match `JSON.stringify` of the TS `NestedRepoScanTelemetry`: `selected_path_kind`
/// is spread only when a scan is present, so a None is omitted (not serialized as null).
fn scan_telemetry_to_json(t: &NestedRepoScanTelemetry) -> Value {
    let mut map = Map::new();
    map.insert("attempt_id".to_string(), json!(t.attempt_id));
    map.insert("surface".to_string(), json!(t.surface));
    map.insert("runtime_kind".to_string(), json!(t.runtime_kind));
    map.insert("result".to_string(), json!(t.result));
    if let Some(kind) = &t.selected_path_kind {
        map.insert("selected_path_kind".to_string(), json!(kind));
    }
    map.insert("found_count".to_string(), json!(t.found_count));
    map.insert("found_count_bucket".to_string(), json!(t.found_count_bucket));
    map.insert("truncated".to_string(), json!(t.truncated));
    map.insert("timed_out".to_string(), json!(t.timed_out));
    Value::Object(map)
}

fn action_telemetry_to_json(t: &NestedRepoImportActionTelemetry) -> Value {
    json!({
        "attempt_id": t.attempt_id,
        "surface": t.surface,
        "runtime_kind": t.runtime_kind,
        "action": t.action,
        "found_count": t.found_count,
        "found_count_bucket": t.found_count_bucket,
        "selected_count": t.selected_count,
        "selected_count_bucket": t.selected_count_bucket,
        "all_selected": t.all_selected,
    })
}
