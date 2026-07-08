//! Parity dispatch for `orca_config::setup_script_imports` (plus its
//! codex-environment and package-manager submodules) vs
//! `src/shared/setup-script-imports.ts`. The TS entry takes async file
//! readers; vectors carry a sync map instead — `contentsByPath` (path ->
//! content, `null` = unreadable) wrapped as the read closure, and an optional
//! `existingPaths` list wrapped as the `file_exists` closure (absent =
//! read-fallback existence checks, mirroring the orca-runtime.ts caller).

use orca_config::inspect_setup_script_import_candidates;
use orca_config::setup_script_imports::SetupScriptImportCandidate;
use serde_json::{json, Map, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "inspectSetupScriptImportCandidates" => inspect(input),
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

fn inspect(input: &Value) -> Value {
    let empty = Map::new();
    let contents_by_path = input
        .get("contentsByPath")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    // Why: mirrors the TS reader contract — missing keys and stored nulls both
    // read as absent (readers resolve null instead of throwing).
    let read_file = |path: &str| -> Option<String> {
        contents_by_path
            .get(path)
            .and_then(Value::as_str)
            .map(str::to_string)
    };

    let existing_paths: Option<Vec<&str>> = input
        .get("existingPaths")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).collect());
    let file_exists = existing_paths.map(|paths| move |path: &str| paths.contains(&path));

    let candidates = inspect_setup_script_import_candidates(
        &read_file,
        file_exists
            .as_ref()
            .map(|check| check as &dyn Fn(&str) -> bool),
    );
    Value::Array(candidates.iter().map(candidate_to_json).collect())
}

/// Match `JSON.stringify` of the TS `SetupScriptImportCandidate`: key order as
/// in the TS object literals, `archive` omitted when absent.
fn candidate_to_json(candidate: &SetupScriptImportCandidate) -> Value {
    let mut map = Map::new();
    map.insert("provider".into(), Value::String(candidate.provider.clone()));
    map.insert("label".into(), Value::String(candidate.label.clone()));
    map.insert(
        "files".into(),
        Value::Array(candidate.files.iter().map(|file| Value::String(file.clone())).collect()),
    );
    map.insert("setup".into(), Value::String(candidate.setup.clone()));
    if let Some(archive) = &candidate.archive {
        map.insert("archive".into(), Value::String(archive.clone()));
    }
    map.insert(
        "unsupportedFields".into(),
        Value::Array(
            candidate
                .unsupported_fields
                .iter()
                .map(|field| Value::String(field.clone()))
                .collect(),
        ),
    );
    Value::Object(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compare::json_semantic_eq;

    const VECTORS: &str =
        include_str!("../../../../../tools/parity/vectors/setup-script-imports.json");

    /// Replays the shared vector corpus through this dispatch and checks every
    /// TS-derived golden — the same check the harness bin performs.
    #[test]
    fn dispatch_matches_ts_goldens_for_all_vectors() {
        let doc: Value = serde_json::from_str(VECTORS).expect("vectors parse");
        let cases = doc.get("cases").and_then(Value::as_array).expect("cases");
        assert!(!cases.is_empty());
        for (index, case) in cases.iter().enumerate() {
            let function = case.get("function").and_then(Value::as_str).expect("function");
            let input = case.get("input").expect("input");
            let expected = case.get("expected").expect("expected");
            let output = dispatch(function, input);
            assert!(
                json_semantic_eq(&output, expected),
                "case #{index} ({}) diverged:\n  rust:     {output}\n  expected: {expected}",
                case.get("note").and_then(Value::as_str).unwrap_or("")
            );
        }
    }

    #[test]
    fn unknown_function_reports_parity_error() {
        let output = dispatch("nope", &Value::Null);
        assert!(output.get("__parity_error__").is_some());
    }
}
