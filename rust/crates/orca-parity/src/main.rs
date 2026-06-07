//! Differential parity harness (Rust half).
//!
//! Reads the shared vector corpus (`tools/parity/vectors/*.json`), runs each
//! case through the registered Rust port, self-checks against the `expected`
//! golden transcribed from the TS test, and writes `rust_outputs.json` for the
//! vitest driver to diff against the live TypeScript reference.
//!
//! Usage: `orca-parity [VECTORS_DIR] [OUT_FILE]`
//! (defaults: `tools/parity/vectors`, `tools/parity/rust_outputs.json`).

mod case;
mod compare;
mod modules;

use case::ParityRun;
use compare::json_semantic_eq;
use serde_json::{json, Value};
use std::fs;
use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let vectors_dir = args.get(1).map(String::as_str).unwrap_or("tools/parity/vectors");
    let out_path = args.get(2).map(String::as_str).unwrap_or("tools/parity/rust_outputs.json");

    let mut files = match fs::read_dir(vectors_dir) {
        Ok(entries) => entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
            .collect::<Vec<_>>(),
        Err(err) => {
            eprintln!("orca-parity: cannot read vectors dir '{vectors_dir}': {err}");
            return ExitCode::FAILURE;
        }
    };
    files.sort();

    let mut runs: Vec<ParityRun> = Vec::new();
    let mut golden_total = 0usize;
    let mut golden_fail = 0usize;
    let mut dispatch_missing = 0usize;
    let mut errors = 0usize;

    for path in &files {
        let doc: Value = match fs::read_to_string(path).map_err(|e| e.to_string()).and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string())) {
            Ok(value) => value,
            Err(err) => {
                eprintln!("orca-parity: {path:?}: {err}");
                errors += 1;
                continue;
            }
        };
        let module = doc.get("module").and_then(Value::as_str).unwrap_or_default().to_string();
        let cases = doc.get("cases").and_then(Value::as_array).cloned().unwrap_or_default();
        for (case_index, case) in cases.iter().enumerate() {
            let function = case.get("function").and_then(Value::as_str).unwrap_or_default().to_string();
            let note = case.get("note").and_then(Value::as_str).unwrap_or_default().to_string();
            let input = case.get("input").cloned().unwrap_or(Value::Null);
            let expected = case.get("expected").cloned();

            let rust_output = match modules::dispatch(&module, &function, &input) {
                Some(value) => value,
                None => {
                    dispatch_missing += 1;
                    json!({ "__parity_error__": format!("no Rust dispatch for {module}::{function}") })
                }
            };

            if let Some(expected) = &expected {
                golden_total += 1;
                if !json_semantic_eq(expected, &rust_output) {
                    golden_fail += 1;
                    eprintln!(
                        "GOLDEN MISMATCH {module}#{case_index} {function} ({note})\n  expected: {expected}\n  rust:     {rust_output}"
                    );
                }
            }

            runs.push(ParityRun { module: module.clone(), case_index, function, note, input, expected, rust_output });
        }
    }

    let serialized: Vec<Value> = runs
        .iter()
        .map(|run| {
            json!({
                "module": run.module,
                "caseIndex": run.case_index,
                "function": run.function,
                "note": run.note,
                "input": run.input,
                "expected": run.expected,
                "rustOutput": run.rust_output,
            })
        })
        .collect();

    if let Some(parent) = Path::new(out_path).parent() {
        let _ = fs::create_dir_all(parent);
    }
    let body = serde_json::to_string_pretty(&Value::Array(serialized)).unwrap_or_else(|_| "[]".to_string());
    if let Err(err) = fs::write(out_path, body) {
        eprintln!("orca-parity: cannot write '{out_path}': {err}");
        return ExitCode::FAILURE;
    }

    println!(
        "orca-parity: {} cases / {} vector files; golden {}/{} ok; dispatch-missing {}; file-errors {}; wrote {out_path}",
        runs.len(),
        files.len(),
        golden_total - golden_fail,
        golden_total,
        dispatch_missing,
        errors,
    );

    if golden_fail > 0 || errors > 0 || dispatch_missing > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
