//! JSON boundary for the terminal quick-command helpers. One `dispatch` maps a
//! function name + JSON input to JSON output, shared by the napi addon (main),
//! the orca-git wasm (renderer), and the parity oracle — so the three never
//! drift. The pure logic lives in [`crate::terminal_quick_commands`]; this only
//! marshals its typed structs to/from the TS `TerminalQuickCommand` shape.

use crate::terminal_quick_commands::{
    build_terminal_quick_command_input, flatten_terminal_quick_command,
    get_default_terminal_quick_commands, get_terminal_quick_command_action,
    get_terminal_quick_command_body, get_terminal_quick_command_scope,
    is_terminal_agent_quick_command, is_terminal_quick_command_complete,
    normalize_terminal_quick_command_scope, normalize_terminal_quick_commands,
    supports_terminal_agent_quick_command, terminal_quick_command_matches_repo,
    TerminalAgentQuickCommand, TerminalCommandQuickCommand, TerminalQuickCommand,
    TerminalQuickCommandAction, TerminalQuickCommandScope,
};
use serde_json::{json, Value};

/// Run one quick-command helper by name over its JSON input, returning JSON.
/// Unknown names yield a `{ "__parity_error__": … }` marker (surfaced by the
/// parity harness; production callers only pass known names).
pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "normalizeTerminalQuickCommands" => {
            commands_to_json(&normalize_terminal_quick_commands(input))
        }
        "getDefaultTerminalQuickCommands" => {
            commands_to_json(&get_default_terminal_quick_commands())
        }
        "getTerminalQuickCommandScope" => scope_to_json(&get_terminal_quick_command_scope(
            &command_from_value(input),
        )),
        "getTerminalQuickCommandAction" => {
            action_to_json(get_terminal_quick_command_action(&command_from_value(input)))
        }
        "isTerminalAgentQuickCommand" => {
            Value::Bool(is_terminal_agent_quick_command(&command_from_value(input)))
        }
        "supportsTerminalAgentQuickCommand" => Value::Bool(
            input
                .as_str()
                .is_some_and(supports_terminal_agent_quick_command),
        ),
        "getTerminalQuickCommandBody" => {
            Value::String(get_terminal_quick_command_body(&command_from_value(input)).to_string())
        }
        "isTerminalQuickCommandComplete" => {
            Value::Bool(is_terminal_quick_command_complete(&command_from_value(input)))
        }
        "buildTerminalQuickCommandInput" => Value::String(build_terminal_quick_command_input(
            &command_struct_from_value(input),
        )),
        "flattenTerminalQuickCommand" => command_struct_to_json(&flatten_terminal_quick_command(
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
/// preserving the raw `command`/`prompt`/`label` text (the helpers only read).
/// Scope is normalized exactly as the lib does.
fn command_from_value(value: &Value) -> TerminalQuickCommand {
    if value.get("action").and_then(Value::as_str) == Some("agent-prompt") {
        TerminalQuickCommand::Agent(TerminalAgentQuickCommand {
            id: str_field(value, "id"),
            label: str_field(value, "label"),
            scope: normalize_terminal_quick_command_scope(value.get("scope")),
            agent: str_field(value, "agent"),
            prompt: str_field(value, "prompt"),
        })
    } else {
        TerminalQuickCommand::Command(command_struct_from_value(value))
    }
}

fn command_struct_from_value(value: &Value) -> TerminalCommandQuickCommand {
    TerminalCommandQuickCommand {
        id: str_field(value, "id"),
        label: str_field(value, "label"),
        scope: normalize_terminal_quick_command_scope(value.get("scope")),
        command: str_field(value, "command"),
        // `appendEnter !== false`: anything but an explicit `false` is `true`.
        append_enter: value.get("appendEnter").and_then(Value::as_bool) != Some(false),
    }
}

fn str_field(value: &Value, key: &str) -> String {
    value.get(key).and_then(Value::as_str).unwrap_or("").to_string()
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
        TerminalQuickCommand::Command(c) => command_struct_to_json(c),
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

fn command_struct_to_json(command: &TerminalCommandQuickCommand) -> Value {
    json!({
        "id": command.id,
        "label": command.label,
        "scope": scope_to_json(&command.scope),
        "action": "terminal-command",
        "command": command.command,
        "appendEnter": command.append_enter,
    })
}
