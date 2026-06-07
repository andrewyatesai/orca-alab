//! Parity dispatch for `orca_agents::terminal_quick_commands` vs
//! `src/shared/terminal-quick-commands.ts`.

use orca_agents::terminal_quick_commands::{
    build_terminal_quick_command_input, get_default_terminal_quick_commands,
    get_terminal_quick_command_action, get_terminal_quick_command_body,
    is_terminal_quick_command_complete, normalize_terminal_quick_command_scope,
    normalize_terminal_quick_commands, supports_terminal_agent_quick_command,
    terminal_quick_command_matches_repo, TerminalAgentQuickCommand, TerminalCommandQuickCommand,
    TerminalQuickCommand, TerminalQuickCommandAction, TerminalQuickCommandScope,
};
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "normalizeTerminalQuickCommands" => commands_to_json(&normalize_terminal_quick_commands(input)),
        "getDefaultTerminalQuickCommands" => commands_to_json(&get_default_terminal_quick_commands()),
        "supportsTerminalAgentQuickCommand" => Value::Bool(
            input
                .as_str()
                .is_some_and(supports_terminal_agent_quick_command),
        ),
        "getTerminalQuickCommandAction" => {
            action_to_json(get_terminal_quick_command_action(&command_from_value(input)))
        }
        "getTerminalQuickCommandBody" => {
            Value::String(get_terminal_quick_command_body(&command_from_value(input)).to_string())
        }
        "isTerminalQuickCommandComplete" => {
            Value::Bool(is_terminal_quick_command_complete(&command_from_value(input)))
        }
        "buildTerminalQuickCommandInput" => Value::String(build_terminal_quick_command_input(
            &command_struct_from_value(input),
        )),
        // Multi-arg: `{ command, repoId }`.
        "terminalQuickCommandMatchesRepo" => Value::Bool(terminal_quick_command_matches_repo(
            &command_from_value(input.get("command").unwrap_or(&Value::Null)),
            input.get("repoId").and_then(Value::as_str),
        )),
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

/// Build the union from a raw command object, mirroring the TS field reads and
/// preserving the raw `command`/`prompt`/`label` text (the helpers transform
/// nothing — they only read). Scope is normalized exactly as the lib does.
fn command_from_value(value: &Value) -> TerminalQuickCommand {
    let id = value.get("id").and_then(Value::as_str).unwrap_or("").to_string();
    let label = value.get("label").and_then(Value::as_str).unwrap_or("").to_string();
    let scope = normalize_terminal_quick_command_scope(value.get("scope"));
    if value.get("action").and_then(Value::as_str) == Some("agent-prompt") {
        TerminalQuickCommand::Agent(TerminalAgentQuickCommand {
            id,
            label,
            scope,
            agent: value.get("agent").and_then(Value::as_str).unwrap_or("").to_string(),
            prompt: value.get("prompt").and_then(Value::as_str).unwrap_or("").to_string(),
        })
    } else {
        TerminalQuickCommand::Command(command_struct_from_value(value))
    }
}

fn command_struct_from_value(value: &Value) -> TerminalCommandQuickCommand {
    TerminalCommandQuickCommand {
        id: value.get("id").and_then(Value::as_str).unwrap_or("").to_string(),
        label: value.get("label").and_then(Value::as_str).unwrap_or("").to_string(),
        scope: normalize_terminal_quick_command_scope(value.get("scope")),
        command: value.get("command").and_then(Value::as_str).unwrap_or("").to_string(),
        // `appendEnter !== false`: anything but an explicit `false` is `true`.
        append_enter: value.get("appendEnter").and_then(Value::as_bool) != Some(false),
    }
}

fn action_to_json(action: TerminalQuickCommandAction) -> Value {
    Value::String(
        match action {
            TerminalQuickCommandAction::TerminalCommand => "terminal-command",
            TerminalQuickCommandAction::AgentPrompt => "agent-prompt",
        }
        .to_string(),
    )
}

fn scope_to_json(scope: &TerminalQuickCommandScope) -> Value {
    match scope {
        TerminalQuickCommandScope::Global => json!({ "type": "global" }),
        TerminalQuickCommandScope::Repo { repo_id } => json!({ "type": "repo", "repoId": repo_id }),
    }
}

/// Match `JSON.stringify` of the normalized `TerminalQuickCommand[]`.
fn commands_to_json(commands: &[TerminalQuickCommand]) -> Value {
    Value::Array(commands.iter().map(command_to_json).collect())
}

fn command_to_json(command: &TerminalQuickCommand) -> Value {
    match command {
        TerminalQuickCommand::Command(c) => json!({
            "id": c.id,
            "label": c.label,
            "scope": scope_to_json(&c.scope),
            "action": "terminal-command",
            "command": c.command,
            "appendEnter": c.append_enter,
        }),
        TerminalQuickCommand::Agent(a) => json!({
            "id": a.id,
            "label": a.label,
            "scope": scope_to_json(&a.scope),
            "action": "agent-prompt",
            "agent": a.agent,
            "prompt": a.prompt,
        }),
    }
}
