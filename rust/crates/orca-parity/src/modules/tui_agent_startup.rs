//! Parity dispatch for `orca_agents::tui_agent_startup` vs
//! `src/shared/tui-agent-startup.ts`.

use orca_agents::tui_agent_startup::{
    build_agent_draft_launch_plan, build_agent_startup_plan, AgentDraftLaunchArgs,
    AgentDraftLaunchPlan, AgentStartupPlan, AgentStartupPlanArgs, AgentStartupShell,
};
use serde_json::{json, Map, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "buildAgentStartupPlan" => {
            let overrides = collect_overrides(input.get("cmdOverrides"));
            let cmd_overrides: Vec<(&str, &str)> =
                overrides.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
            let args = AgentStartupPlanArgs {
                agent: str_field(input, "agent"),
                prompt: str_field(input, "prompt"),
                cmd_overrides: &cmd_overrides,
                platform: str_field(input, "platform"),
                shell: parse_shell(input.get("shell")),
                allow_empty_prompt_launch: input
                    .get("allowEmptyPromptLaunch")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            };
            match build_agent_startup_plan(&args) {
                Some(plan) => startup_plan_to_json(&plan),
                None => Value::Null,
            }
        }
        "buildAgentDraftLaunchPlan" => {
            let overrides = collect_overrides(input.get("cmdOverrides"));
            let cmd_overrides: Vec<(&str, &str)> =
                overrides.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
            let args = AgentDraftLaunchArgs {
                agent: str_field(input, "agent"),
                draft: str_field(input, "draft"),
                cmd_overrides: &cmd_overrides,
                platform: str_field(input, "platform"),
                shell: parse_shell(input.get("shell")),
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

/// Flatten a `cmdOverrides` JSON object into `(agent, command)` pairs, dropping
/// non-string values (a vector that carried one would be a vector bug).
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

fn parse_shell(value: Option<&Value>) -> Option<AgentStartupShell> {
    value.and_then(Value::as_str).and_then(AgentStartupShell::from_label)
}

/// Match `JSON.stringify` of the TS `AgentStartupPlan` (the optional
/// `draftPrompt`/`env` fields are never set by `buildAgentStartupPlan`).
fn startup_plan_to_json(plan: &AgentStartupPlan) -> Value {
    json!({
        "agent": plan.agent,
        "launchCommand": plan.launch_command,
        "expectedProcess": plan.expected_process,
        "followupPrompt": plan.followup_prompt,
    })
}

/// Match `JSON.stringify` of the TS `AgentDraftLaunchPlan`; `env` is omitted
/// (undefined) on the flag path, matching the TS object shape.
fn draft_plan_to_json(plan: &AgentDraftLaunchPlan) -> Value {
    let mut map = Map::new();
    map.insert("agent".to_string(), Value::String(plan.agent.clone()));
    map.insert("launchCommand".to_string(), Value::String(plan.launch_command.clone()));
    map.insert("expectedProcess".to_string(), Value::String(plan.expected_process.clone()));
    if let Some((name, value)) = &plan.env {
        let mut env = Map::new();
        env.insert(name.clone(), Value::String(value.clone()));
        map.insert("env".to_string(), Value::Object(env));
    }
    Value::Object(map)
}
