//! Parity dispatch for `orca_core::cross_platform_path` vs
//! `src/shared/cross-platform-path.ts`.

use orca_core::cross_platform_path::{
    get_runtime_path_basename, is_path_inside_or_equal, is_windows_absolute_path_like,
    normalize_runtime_path_for_comparison, normalize_runtime_path_separators,
    relative_path_inside_root, resolve_runtime_path,
};
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "isWindowsAbsolutePathLike" => {
            Value::Bool(is_windows_absolute_path_like(&string_field(input, "value")))
        }
        "normalizeRuntimePathSeparators" => {
            Value::String(normalize_runtime_path_separators(&string_field(input, "value")))
        }
        "normalizeRuntimePathForComparison" => Value::String(
            normalize_runtime_path_for_comparison(&string_field(input, "value")),
        ),
        "resolveRuntimePath" => Value::String(resolve_runtime_path(
            &string_field(input, "basePath"),
            &string_field(input, "targetPath"),
        )),
        "getRuntimePathBasename" => {
            Value::String(get_runtime_path_basename(&string_field(input, "value")))
        }
        "isPathInsideOrEqual" => Value::Bool(is_path_inside_or_equal(
            &string_field(input, "rootPath"),
            &string_field(input, "candidatePath"),
        )),
        // TS returns `string | null`; `JSON.stringify` keeps the literal `null`,
        // so a non-contained candidate maps to `Value::Null`, not an omitted key.
        "relativePathInsideRoot" => match relative_path_inside_root(
            &string_field(input, "rootPath"),
            &string_field(input, "candidatePath"),
        ) {
            Some(rel) => Value::String(rel),
            None => Value::Null,
        },
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

/// Reads a string argument from the vector input object. Vectors always carry
/// the keys, so a missing one is a vector bug; default to empty rather than panic.
fn string_field(input: &Value, key: &str) -> String {
    input
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}
