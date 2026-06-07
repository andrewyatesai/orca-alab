//! Parity dispatch for `orca_core::github_pr_merge_methods` vs
//! `src/shared/github-pr-merge-methods.ts`.

use orca_core::github_pr_merge_methods::{
    map_github_default_merge_method, normalize_github_pr_merge_method_settings,
    resolve_github_pr_merge_methods, AllowedMethods, GitHubPrMergeMethod,
    GitHubPrMergeMethodPresentation, GitHubPrMergeMethodSettings,
};
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        // Non-string input falls through to `None`, matching the TS `typeof`
        // guard that maps any non-string to `''` (and thus `null`).
        "mapGitHubDefaultMergeMethod" => match map_github_default_merge_method(input.as_str()) {
            Some(method) => Value::String(method_id(method).to_string()),
            None => Value::Null,
        },
        "normalizeGitHubPRMergeMethodSettings" => {
            let default_method = input.get("defaultMethod").and_then(Value::as_str);
            // `=== true` in TS: only an explicit JSON `true` allows the method.
            let allowed_true = |key: &str| input.get(key).and_then(Value::as_bool) == Some(true);
            match normalize_github_pr_merge_method_settings(
                default_method,
                allowed_true("mergeCommitAllowed"),
                allowed_true("rebaseMergeAllowed"),
                allowed_true("squashMergeAllowed"),
            ) {
                Some(settings) => settings_to_json(&settings),
                None => Value::Null,
            }
        }
        "resolveGitHubPRMergeMethods" => {
            let settings = parse_settings(input);
            presentation_to_json(&resolve_github_pr_merge_methods(settings.as_ref()))
        }
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

/// TS lowercase string id for a method (the shape `JSON.stringify` emits).
fn method_id(method: GitHubPrMergeMethod) -> &'static str {
    match method {
        GitHubPrMergeMethod::Squash => "squash",
        GitHubPrMergeMethod::Merge => "merge",
        GitHubPrMergeMethod::Rebase => "rebase",
    }
}

fn method_from_id(id: &str) -> Option<GitHubPrMergeMethod> {
    match id {
        "squash" => Some(GitHubPrMergeMethod::Squash),
        "merge" => Some(GitHubPrMergeMethod::Merge),
        "rebase" => Some(GitHubPrMergeMethod::Rebase),
        _ => None,
    }
}

/// Parse a `GitHubPRMergeMethodSettings` object (or `null`) from the vector
/// input. `null`/non-object yields `None`, matching the optional TS arg.
fn parse_settings(input: &Value) -> Option<GitHubPrMergeMethodSettings> {
    let object = input.as_object()?;
    let default_method = method_from_id(object.get("defaultMethod")?.as_str()?)?;
    let allowed = object.get("allowedMethods").and_then(Value::as_object);
    let flag = |key: &str| {
        allowed
            .and_then(|methods| methods.get(key))
            .and_then(Value::as_bool)
            .unwrap_or(false)
    };
    Some(GitHubPrMergeMethodSettings {
        default_method,
        allowed_methods: AllowedMethods {
            squash: flag("squash"),
            merge: flag("merge"),
            rebase: flag("rebase"),
        },
    })
}

fn settings_to_json(settings: &GitHubPrMergeMethodSettings) -> Value {
    json!({
        "defaultMethod": method_id(settings.default_method),
        "allowedMethods": {
            "squash": settings.allowed_methods.squash,
            "merge": settings.allowed_methods.merge,
            "rebase": settings.allowed_methods.rebase,
        }
    })
}

fn presentation_to_json(presentation: &GitHubPrMergeMethodPresentation) -> Value {
    let methods: Vec<Value> = presentation
        .methods
        .iter()
        .map(|option| json!({ "method": method_id(option.method), "label": option.label }))
        .collect();
    json!({
        "defaultMethod": method_id(presentation.default_method),
        "defaultLabel": presentation.default_label,
        "methods": methods,
    })
}
