//! Parity dispatch for `orca_core::setup_runner_command` vs
//! `src/shared/setup-runner-command.ts`.

use orca_core::setup_runner_command::{
    build_setup_runner_command, get_setup_runner_command_platform_for_path,
    SetupRunnerCommandPlatform,
};
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "buildSetupRunnerCommand" => {
            let runner_script_path = input.get("runnerScriptPath").and_then(Value::as_str).unwrap_or("");
            match input.get("platform").and_then(Value::as_str).and_then(platform_from_id) {
                // TS returns a plain `string`; mirror it directly.
                Some(platform) => Value::String(build_setup_runner_command(runner_script_path, platform)),
                // Vectors only carry known platform ids; an unknown one is a vector bug.
                None => json!({ "__parity_error__": "unknown SetupRunnerCommandPlatform in input.platform" }),
            }
        }
        "getSetupRunnerCommandPlatformForPath" => {
            let runner_script_path = input.get("runnerScriptPath").and_then(Value::as_str).unwrap_or("");
            match input.get("fallbackPlatform").and_then(Value::as_str).and_then(platform_from_id) {
                // TS returns the `SetupRunnerCommandPlatform` string union.
                Some(fallback) => Value::String(
                    platform_to_id(get_setup_runner_command_platform_for_path(runner_script_path, fallback))
                        .to_string(),
                ),
                None => json!({ "__parity_error__": "unknown SetupRunnerCommandPlatform in input.fallbackPlatform" }),
            }
        }
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

/// Maps the TS `SetupRunnerCommandPlatform` string ids to the Rust enum.
fn platform_from_id(id: &str) -> Option<SetupRunnerCommandPlatform> {
    match id {
        "windows" => Some(SetupRunnerCommandPlatform::Windows),
        "posix" => Some(SetupRunnerCommandPlatform::Posix),
        _ => None,
    }
}

/// Maps the Rust enum back to the TS `SetupRunnerCommandPlatform` string id.
fn platform_to_id(platform: SetupRunnerCommandPlatform) -> &'static str {
    match platform {
        SetupRunnerCommandPlatform::Windows => "windows",
        SetupRunnerCommandPlatform::Posix => "posix",
    }
}
