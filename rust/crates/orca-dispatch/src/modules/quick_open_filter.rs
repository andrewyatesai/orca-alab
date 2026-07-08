//! Parity dispatch for `orca_core::quick_open_filter` vs
//! `src/shared/quick-open-filter.ts`.

use orca_core::quick_open_filter::{
    build_exclude_path_prefixes, build_git_ls_files_args_for_quick_open,
    build_hidden_dir_exclude_globs, build_rg_args_for_quick_open, normalize_quick_open_rg_line,
    should_exclude_quick_open_rel_path, should_include_quick_open_path, GitLsFilesArgs, RgArgs,
    RgArgsOptions, RgOutputMode,
};
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "shouldIncludeQuickOpenPath" => {
            let path = input.get("path").and_then(Value::as_str).unwrap_or("");
            Value::Bool(should_include_quick_open_path(path))
        }
        "buildExcludePathPrefixes" => {
            let root_path = input.get("rootPath").and_then(Value::as_str).unwrap_or("");
            // A non-array excludePaths (or omitted key) ≙ None; non-string
            // entries ≙ None elements — matching the TS `Array.isArray` /
            // `typeof === 'string'` guards exactly.
            let owned: Option<Vec<Option<&str>>> = match input.get("excludePaths") {
                Some(Value::Array(items)) => Some(items.iter().map(Value::as_str).collect()),
                _ => None,
            };
            strings_to_json(build_exclude_path_prefixes(root_path, owned.as_deref()))
        }
        "shouldExcludeQuickOpenRelPath" => {
            let rel_path = input.get("relPath").and_then(Value::as_str).unwrap_or("");
            let prefixes = str_array(input.get("excludePathPrefixes"));
            Value::Bool(should_exclude_quick_open_rel_path(rel_path, &prefixes))
        }
        "buildHiddenDirExcludeGlobs" => strings_to_json(build_hidden_dir_exclude_globs()),
        "buildRgArgsForQuickOpen" => {
            let search_root = input.get("searchRoot").and_then(Value::as_str).unwrap_or("");
            let prefixes = str_array(input.get("excludePathPrefixes"));
            let force_slash_separator = input
                .get("forceSlashSeparator")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let RgArgs { primary, ignored_pass } = build_rg_args_for_quick_open(&RgArgsOptions {
                search_root,
                exclude_path_prefixes: &prefixes,
                force_slash_separator,
            });
            json!({ "primary": primary, "ignoredPass": ignored_pass })
        }
        "normalizeQuickOpenRgLine" => {
            let raw_line = input.get("rawLine").and_then(Value::as_str).unwrap_or("");
            let mode = input.get("outputMode");
            let kind = mode
                .and_then(|m| m.get("kind"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let output_mode = if kind == "absolute" {
                let root_path = mode
                    .and_then(|m| m.get("rootPath"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                RgOutputMode::Absolute { root_path }
            } else {
                RgOutputMode::CwdRelative
            };
            // TS returns `string | null`; None serialises to JSON `null`.
            match normalize_quick_open_rg_line(raw_line, &output_mode) {
                Some(rel) => Value::String(rel),
                None => Value::Null,
            }
        }
        "buildGitLsFilesArgsForQuickOpen" => {
            let prefixes = str_array(input.get("excludePathPrefixes"));
            let GitLsFilesArgs { primary, ignored_pass } =
                build_git_ls_files_args_for_quick_open(&prefixes);
            json!({ "primary": primary, "ignoredPass": ignored_pass })
        }
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

/// Collect a JSON string array into borrowed `&str`s (non-string entries dropped).
fn str_array(value: Option<&Value>) -> Vec<&str> {
    value
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

/// Match `JSON.stringify` of a TS `string[]`.
fn strings_to_json(items: Vec<String>) -> Value {
    Value::Array(items.into_iter().map(Value::String).collect())
}
