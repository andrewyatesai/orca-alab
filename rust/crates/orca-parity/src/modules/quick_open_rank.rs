//! Parity dispatch for `orca_text::quick_open_rank` vs
//! `src/renderer/src/components/quick-open-search.ts` (`rankQuickOpenFiles`).
//!
//! The vector passes the RAW path list + RAW query; the Rust side runs the full
//! prepare+normalize+rank pipeline, matching the TS dispatch which calls
//! `rankQuickOpenFiles(query, prepareQuickOpenFiles(paths), limit)`.

use orca_text::quick_open_rank::{
    rank_quick_open_files, QuickOpenResult, QUICK_OPEN_RESULT_LIMIT,
};
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "rankQuickOpenFiles" => {
            let query = input.get("query").and_then(Value::as_str).unwrap_or("");
            let paths: Vec<String> = input
                .get("paths")
                .and_then(Value::as_array)
                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                .unwrap_or_default();
            let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
            // limit is optional; omitted → the TS default parameter (50).
            let limit = input
                .get("limit")
                .and_then(Value::as_u64)
                .map(|n| n as usize)
                .unwrap_or(QUICK_OPEN_RESULT_LIMIT);
            results_to_json(&rank_quick_open_files(query, &refs, limit))
        }
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

/// Match `JSON.stringify` of the TS `{ path, score }[]` result.
fn results_to_json(results: &[QuickOpenResult]) -> Value {
    Value::Array(
        results
            .iter()
            .map(|r| json!({ "path": r.path, "score": r.score }))
            .collect(),
    )
}
