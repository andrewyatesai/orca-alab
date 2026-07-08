//! JSON boundary for the TUI agent-startup plan builders. One `dispatch` maps a
//! function name + JSON input to JSON output, shared by the napi addon (main),
//! the orca-git wasm (renderer), and the parity oracle — so the three never
//! drift. The pure logic lives in [`crate::tui_agent_startup`]; this only marshals
//! its typed args/plans to/from the TS shapes (`AgentStartupPlan`, etc.).

use crate::tui_agent_startup::{
    build_agent_draft_launch_plan, build_agent_resume_startup_plan, build_agent_startup_plan,
    AgentDraftLaunchArgs, AgentDraftLaunchPlan, AgentResumeStartupPlanArgs, AgentStartupPlan,
    AgentStartupPlanArgs, AgentStartupShell, ProviderSessionKey, SleepingAgentLaunchConfig,
};
use serde_json::{json, Map, Value};

/// Run one agent-startup builder by name over its JSON input, returning JSON.
/// Unknown names yield a `{ "__parity_error__": … }` marker (surfaced by the
/// parity harness; production callers only pass known names).
pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "buildAgentStartupPlan" => {
            let overrides = collect_overrides(input.get("cmdOverrides"));
            let cmd_overrides: Vec<(&str, &str)> =
                overrides.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
            let agent_env = collect_env(input.get("agentEnv"));
            let args = AgentStartupPlanArgs {
                agent: str_field(input, "agent"),
                prompt: str_field(input, "prompt"),
                cmd_overrides: &cmd_overrides,
                platform: str_field(input, "platform"),
                shell: parse_shell(input.get("shell")),
                allow_empty_prompt_launch: bool_field(input, "allowEmptyPromptLaunch"),
                agent_args: input.get("agentArgs").and_then(Value::as_str),
                agent_env: agent_env.as_deref(),
                is_remote: bool_field(input, "isRemote"),
            };
            match build_agent_startup_plan(&args) {
                Some(plan) => startup_plan_to_json(&plan),
                None => Value::Null,
            }
        }
        "buildAgentResumeStartupPlan" => {
            let overrides = collect_overrides(input.get("cmdOverrides"));
            let cmd_overrides: Vec<(&str, &str)> =
                overrides.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
            let agent_env = collect_env(input.get("agentEnv"));
            let session = input.get("providerSession");
            // An unknown key kind cannot match any resume argv (the TS key
            // guard yields null), so it maps straight to a null plan.
            let Some(key) = session
                .and_then(|value| value.get("key"))
                .and_then(Value::as_str)
                .and_then(ProviderSessionKey::from_label)
            else {
                return Value::Null;
            };
            let args = AgentResumeStartupPlanArgs {
                agent: str_field(input, "agent"),
                provider_session_key: key,
                provider_session_id: session
                    .and_then(|value| value.get("id"))
                    .and_then(Value::as_str)
                    .unwrap_or(""),
                cmd_overrides: &cmd_overrides,
                platform: str_field(input, "platform"),
                shell: parse_shell(input.get("shell")),
                agent_args: input.get("agentArgs").and_then(Value::as_str),
                agent_env: agent_env.as_deref(),
                agent_command: input.get("agentCommand").and_then(Value::as_str),
                is_remote: bool_field(input, "isRemote"),
            };
            match build_agent_resume_startup_plan(&args) {
                Some(plan) => startup_plan_to_json(&plan),
                None => Value::Null,
            }
        }
        "buildAgentDraftLaunchPlan" => {
            let overrides = collect_overrides(input.get("cmdOverrides"));
            let cmd_overrides: Vec<(&str, &str)> =
                overrides.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
            let agent_env = collect_env(input.get("agentEnv"));
            let args = AgentDraftLaunchArgs {
                agent: str_field(input, "agent"),
                draft: str_field(input, "draft"),
                cmd_overrides: &cmd_overrides,
                platform: str_field(input, "platform"),
                shell: parse_shell(input.get("shell")),
                agent_args: input.get("agentArgs").and_then(Value::as_str),
                agent_env: agent_env.as_deref(),
                is_remote: bool_field(input, "isRemote"),
            };
            match build_agent_draft_launch_plan(&args) {
                Some(plan) => draft_plan_to_json(&plan),
                None => Value::Null,
            }
        }
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

fn str_field<'a>(input: &'a Value, key: &str) -> &'a str {
    input.get(key).and_then(Value::as_str).unwrap_or("")
}

fn bool_field(input: &Value, key: &str) -> bool {
    input.get(key).and_then(Value::as_bool).unwrap_or(false)
}

/// Flatten a `cmdOverrides` JSON object into `(agent, command)` pairs, dropping
/// non-string values (a caller that carried one would be a bug).
fn collect_overrides(value: Option<&Value>) -> Vec<(String, String)> {
    value
        .and_then(Value::as_object)
        .map(|obj| {
            obj.iter()
                .filter_map(|(key, val)| val.as_str().map(|s| (key.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

/// Flatten an `agentEnv` JSON object into ordered `(name, value)` pairs;
/// `None` for absent/null so the TS truthiness guard round-trips (any object,
/// even `{}`, still threads an `env` key into the plan).
fn collect_env(value: Option<&Value>) -> Option<Vec<(String, String)>> {
    value.and_then(Value::as_object).map(|obj| {
        obj.iter()
            .filter_map(|(key, val)| val.as_str().map(|s| (key.clone(), s.to_string())))
            .collect()
    })
}

fn parse_shell(value: Option<&Value>) -> Option<AgentStartupShell> {
    value.and_then(Value::as_str).and_then(AgentStartupShell::from_label)
}

fn env_to_json(env: &[(String, String)]) -> Value {
    let map: Map<String, Value> = env
        .iter()
        .map(|(key, value)| (key.clone(), Value::String(value.clone())))
        .collect();
    Value::Object(map)
}

/// Match `JSON.stringify` of the TS `SleepingAgentLaunchConfig`; `agentCommand`
/// is omitted when the base command trims to empty (TS truthiness guard).
fn launch_config_to_json(config: &SleepingAgentLaunchConfig) -> Value {
    let mut map = Map::new();
    if let Some(command) = &config.agent_command {
        map.insert("agentCommand".to_string(), Value::String(command.clone()));
    }
    map.insert("agentArgs".to_string(), Value::String(config.agent_args.clone()));
    map.insert("agentEnv".to_string(), env_to_json(&config.agent_env));
    Value::Object(map)
}

/// Match `JSON.stringify` of the TS `AgentStartupPlan` (the optional
/// `launchToken`/`draftPrompt` fields are never set by the plan builders).
fn startup_plan_to_json(plan: &AgentStartupPlan) -> Value {
    let mut map = Map::new();
    map.insert("agent".to_string(), Value::String(plan.agent.clone()));
    map.insert("launchCommand".to_string(), Value::String(plan.launch_command.clone()));
    map.insert("expectedProcess".to_string(), Value::String(plan.expected_process.clone()));
    map.insert(
        "followupPrompt".to_string(),
        plan.followup_prompt.clone().map_or(Value::Null, Value::String),
    );
    map.insert("launchConfig".to_string(), launch_config_to_json(&plan.launch_config));
    // Codex-only key; OMITTED (not null) otherwise, matching the TS conditional spread.
    if let Some(delivery) = &plan.startup_command_delivery {
        map.insert("startupCommandDelivery".to_string(), Value::String(delivery.clone()));
    }
    // OMITTED when the caller passed no agentEnv, matching the TS spread guard.
    if let Some(env) = &plan.env {
        map.insert("env".to_string(), env_to_json(env));
    }
    Value::Object(map)
}

/// Match `JSON.stringify` of the TS `AgentDraftLaunchPlan`; `env` is omitted
/// on the flag path without caller env, matching the TS object shape.
fn draft_plan_to_json(plan: &AgentDraftLaunchPlan) -> Value {
    let mut map = Map::new();
    map.insert("agent".to_string(), Value::String(plan.agent.clone()));
    map.insert("launchCommand".to_string(), Value::String(plan.launch_command.clone()));
    map.insert("expectedProcess".to_string(), Value::String(plan.expected_process.clone()));
    map.insert("launchConfig".to_string(), launch_config_to_json(&plan.launch_config));
    if let Some(delivery) = &plan.startup_command_delivery {
        map.insert("startupCommandDelivery".to_string(), Value::String(delivery.clone()));
    }
    if let Some(env) = &plan.env {
        map.insert("env".to_string(), env_to_json(env));
    }
    Value::Object(map)
}
