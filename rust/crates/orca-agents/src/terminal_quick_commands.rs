//! Terminal quick-command validation/normalization, ported from
//! `src/shared/terminal-quick-commands.ts`.
//!
//! Quick commands are user-defined buttons that either type a shell command or
//! seed an agent prompt. [`normalize_terminal_quick_commands`] sanitizes the
//! persisted (untrusted) JSON: it drops malformed rows, enforces scope/length
//! caps, dedupes ids, and gates agent-prompt rows to launch-time agents. The
//! length caps follow the TS `String.slice` semantics — counted in UTF-16 code
//! units via [`str::encode_utf16`].

use crate::tui_agent_config::{tui_agent_config, AgentPromptInjectionMode};
use serde_json::Value;

const MAX_QUICK_COMMANDS: usize = 40;
const MAX_QUICK_COMMAND_LABEL_LENGTH: usize = 80;
const MAX_QUICK_COMMAND_REPO_ID_LENGTH: usize = 200;
const MAX_QUICK_COMMAND_TEXT_LENGTH: usize = 4000;
const REMOVED_PRESET_IDS: [&str; 2] = ["default-pwd", "default-git-status"];

/// Where a quick command is available.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalQuickCommandScope {
    Global,
    Repo { repo_id: String },
}

/// Whether a quick command types a shell command or seeds an agent prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalQuickCommandAction {
    TerminalCommand,
    AgentPrompt,
}

/// A shell-command quick command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalCommandQuickCommand {
    pub id: String,
    pub label: String,
    pub scope: TerminalQuickCommandScope,
    pub command: String,
    pub append_enter: bool,
}

/// An agent-prompt quick command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalAgentQuickCommand {
    pub id: String,
    pub label: String,
    pub scope: TerminalQuickCommandScope,
    pub agent: String,
    pub prompt: String,
}

/// The quick-command union (shell command vs. agent prompt).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalQuickCommand {
    Command(TerminalCommandQuickCommand),
    Agent(TerminalAgentQuickCommand),
}

impl TerminalQuickCommand {
    fn label(&self) -> &str {
        match self {
            TerminalQuickCommand::Command(c) => &c.label,
            TerminalQuickCommand::Agent(a) => &a.label,
        }
    }

    fn scope(&self) -> &TerminalQuickCommandScope {
        match self {
            TerminalQuickCommand::Command(c) => &c.scope,
            TerminalQuickCommand::Agent(a) => &a.scope,
        }
    }
}

/// Truncate to the first `max_len` UTF-16 code units (TS `String.slice(0, n)`).
// Trust contract: inert under stock cargo, proved under `--cfg trust_verify`.
// Postcondition — the result never exceeds the cap in UTF-16 code units. (A
// cap that splits a surrogate pair yields U+FFFD via the lossy decode rather
// than the lone surrogate TS would keep; the caps here never split real input.)
#[cfg_attr(trust_verify, trust::ensures(|out: &String| out.encode_utf16().count() <= max_len))]
fn slice_utf16(value: &str, max_len: usize) -> String {
    let units: Vec<u16> = value.encode_utf16().collect();
    if units.len() <= max_len {
        return value.to_string();
    }
    String::from_utf16_lossy(&units[..max_len])
}

/// There are no built-in defaults; the list starts empty.
pub fn get_default_terminal_quick_commands() -> Vec<TerminalQuickCommand> {
    Vec::new()
}

/// Normalize a raw (untrusted) scope value: anything but a `repo`-typed object
/// with a non-empty `repoId` collapses to `Global`. Exposed so the parity
/// adapter can build command inputs with identical scope semantics.
pub fn normalize_terminal_quick_command_scope(input: Option<&Value>) -> TerminalQuickCommandScope {
    let Some(record) = input.and_then(Value::as_object) else {
        return TerminalQuickCommandScope::Global;
    };
    if record.get("type").and_then(Value::as_str) != Some("repo") {
        return TerminalQuickCommandScope::Global;
    }
    let repo_id = record
        .get("repoId")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    if repo_id.is_empty() {
        return TerminalQuickCommandScope::Global;
    }
    TerminalQuickCommandScope::Repo {
        repo_id: slice_utf16(repo_id, MAX_QUICK_COMMAND_REPO_ID_LENGTH),
    }
}

/// Normalized scope of an already-typed quick command.
pub fn get_terminal_quick_command_scope(command: &TerminalQuickCommand) -> TerminalQuickCommandScope {
    command.scope().clone()
}

/// True when a command applies to `repo_id` (global commands apply everywhere;
/// `repo_id == None` mirrors the TS `null` "no repo" context).
pub fn terminal_quick_command_matches_repo(
    command: &TerminalQuickCommand,
    repo_id: Option<&str>,
) -> bool {
    match get_terminal_quick_command_scope(command) {
        TerminalQuickCommandScope::Global => true,
        TerminalQuickCommandScope::Repo { repo_id: scope_repo } => {
            repo_id == Some(scope_repo.as_str())
        }
    }
}

/// The action a quick command performs.
pub fn get_terminal_quick_command_action(
    command: &TerminalQuickCommand,
) -> TerminalQuickCommandAction {
    match command {
        TerminalQuickCommand::Agent(_) => TerminalQuickCommandAction::AgentPrompt,
        TerminalQuickCommand::Command(_) => TerminalQuickCommandAction::TerminalCommand,
    }
}

/// True when the command seeds an agent prompt.
pub fn is_terminal_agent_quick_command(command: &TerminalQuickCommand) -> bool {
    matches!(
        get_terminal_quick_command_action(command),
        TerminalQuickCommandAction::AgentPrompt
    )
}

/// True when `agent` is a launch-time-prompt agent (not stdin-after-start),
/// i.e. an agent that can carry a prompt as part of its launch command.
pub fn supports_terminal_agent_quick_command(agent: &str) -> bool {
    tui_agent_config(agent)
        .is_some_and(|config| config.prompt_injection_mode != AgentPromptInjectionMode::StdinAfterStart)
}

/// The body text of a command (its shell command, or its agent prompt).
pub fn get_terminal_quick_command_body(command: &TerminalQuickCommand) -> &str {
    match command {
        TerminalQuickCommand::Agent(a) => &a.prompt,
        TerminalQuickCommand::Command(c) => &c.command,
    }
}

/// True when both the label and the body have non-whitespace content.
pub fn is_terminal_quick_command_complete(command: &TerminalQuickCommand) -> bool {
    !command.label().trim().is_empty()
        && !get_terminal_quick_command_body(command).trim().is_empty()
}

/// Sanitize the persisted (untrusted) quick-command list. Non-array input
/// yields the defaults (empty list).
// Trust contract: inert under stock cargo, proved under `--cfg trust_verify`.
// Postcondition — the result never exceeds the catalog cap.
#[cfg_attr(trust_verify, trust::ensures(|out: &Vec<TerminalQuickCommand>| out.len() <= MAX_QUICK_COMMANDS))]
pub fn normalize_terminal_quick_commands(input: &Value) -> Vec<TerminalQuickCommand> {
    let Some(items) = input.as_array() else {
        return get_default_terminal_quick_commands();
    };

    let mut normalized: Vec<TerminalQuickCommand> = Vec::new();
    let mut seen_ids: Vec<String> = Vec::new();

    for item in items {
        // Non-objects (null / arrays / primitives) are skipped.
        let Some(record) = item.as_object() else {
            continue;
        };
        let raw_id = record
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or("");
        if REMOVED_PRESET_IDS.contains(&raw_id) {
            continue;
        }
        let has_label = record.get("label").is_some_and(Value::is_string);
        let action_is_agent = record.get("action").and_then(Value::as_str) == Some("agent-prompt");
        let has_command = record.get("command").is_some_and(Value::is_string);
        let has_prompt = record.get("prompt").is_some_and(Value::is_string);
        // Why: settings saves on every edit; preserve incomplete rows so a
        // newly added command is not deleted before the user fills it in.
        if !has_label && !has_command && !has_prompt {
            continue;
        }
        let agent = record
            .get("agent")
            .and_then(Value::as_str)
            .filter(|&candidate| supports_terminal_agent_quick_command(candidate));
        if action_is_agent && agent.is_none() {
            continue;
        }
        let label = if has_label {
            record
                .get("label")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string()
        } else {
            String::new()
        };

        let id_base = if raw_id.is_empty() {
            format!("quick-command-{}", normalized.len() + 1)
        } else {
            raw_id.to_string()
        };
        let mut id = slice_utf16(&id_base, MAX_QUICK_COMMAND_LABEL_LENGTH);
        let mut suffix = 2u64;
        while seen_ids.contains(&id) {
            id = format!(
                "{}-{suffix}",
                slice_utf16(&id_base, MAX_QUICK_COMMAND_LABEL_LENGTH - 4)
            );
            suffix += 1;
        }
        seen_ids.push(id.clone());

        let scope = normalize_terminal_quick_command_scope(record.get("scope"));
        let label = slice_utf16(&label, MAX_QUICK_COMMAND_LABEL_LENGTH);

        if action_is_agent {
            // `agent` is guaranteed `Some` by the guard above.
            let Some(agent_id) = agent else {
                continue;
            };
            let prompt = if has_prompt {
                record
                    .get("prompt")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim_end()
                    .to_string()
            } else {
                String::new()
            };
            normalized.push(TerminalQuickCommand::Agent(TerminalAgentQuickCommand {
                id,
                label,
                scope,
                agent: agent_id.to_string(),
                prompt: slice_utf16(&prompt, MAX_QUICK_COMMAND_TEXT_LENGTH),
            }));
        } else {
            let command = if has_command {
                record
                    .get("command")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim_end()
                    .to_string()
            } else {
                String::new()
            };
            let append_enter = record.get("appendEnter").and_then(Value::as_bool) != Some(false);
            normalized.push(TerminalQuickCommand::Command(TerminalCommandQuickCommand {
                id,
                label,
                scope,
                command: slice_utf16(&command, MAX_QUICK_COMMAND_TEXT_LENGTH),
                append_enter,
            }));
        }

        if normalized.len() >= MAX_QUICK_COMMANDS {
            break;
        }
    }

    normalized
}

/// Format a shell-command quick command into terminal input bytes (CR appended
/// when `append_enter` is set).
pub fn build_terminal_quick_command_input(command: &TerminalCommandQuickCommand) -> String {
    if command.append_enter {
        format!("{}\r", command.command)
    } else {
        command.command.clone()
    }
}

fn contains_line_break(value: &str) -> bool {
    value.contains('\r') || value.contains('\n')
}

/// Split on the `\r\n | \r | \n` line-break set, treating CRLF as one break.
fn split_line_breaks(value: &str) -> Vec<&str> {
    let bytes = value.as_bytes();
    let mut lines = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\r' => {
                lines.push(&value[start..i]);
                i += if bytes.get(i + 1) == Some(&b'\n') { 2 } else { 1 };
                start = i;
            }
            b'\n' => {
                lines.push(&value[start..i]);
                i += 1;
                start = i;
            }
            _ => i += 1,
        }
    }
    lines.push(&value[start..]);
    lines
}

/// Collapse a multi-line command into a single `; `-joined shell command line.
// Why: quick-command lines are independent shell commands; one shell command
// list prevents foreground programs from reading later lines as stdin.
pub fn flatten_terminal_quick_command(
    command: &TerminalCommandQuickCommand,
) -> TerminalCommandQuickCommand {
    if !contains_line_break(&command.command) {
        return command.clone();
    }
    let flattened = split_line_breaks(&command.command)
        .into_iter()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("; ");
    TerminalCommandQuickCommand {
        command: flattened,
        ..command.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn command_qc(
        id: &str,
        label: &str,
        command: &str,
        append_enter: bool,
        scope: TerminalQuickCommandScope,
    ) -> TerminalQuickCommand {
        TerminalQuickCommand::Command(TerminalCommandQuickCommand {
            id: id.to_string(),
            label: label.to_string(),
            scope,
            command: command.to_string(),
            append_enter,
        })
    }

    fn repo(repo_id: &str) -> TerminalQuickCommandScope {
        TerminalQuickCommandScope::Repo {
            repo_id: repo_id.to_string(),
        }
    }

    #[test]
    fn returns_safe_defaults_when_persisted_settings_are_missing() {
        assert_eq!(normalize_terminal_quick_commands(&Value::Null), vec![]);
        assert_eq!(get_default_terminal_quick_commands(), vec![]);
    }

    #[test]
    fn keeps_an_intentionally_empty_command_list() {
        assert_eq!(normalize_terminal_quick_commands(&json!([])), vec![]);
    }

    #[test]
    fn removes_quick_commands_from_the_abandoned_preset_rollout() {
        let input = json!([
            { "id": "default-pwd", "label": "Print Working Directory", "command": "pwd", "appendEnter": true },
            { "id": "default-git-status", "label": "Git Status", "command": "git status", "appendEnter": true }
        ]);
        assert_eq!(normalize_terminal_quick_commands(&input), vec![]);
    }

    #[test]
    fn drops_malformed_entries_and_normalizes_valid_commands_and_drafts() {
        let input = json!([
            null,
            { "id": "status", "label": "  Status  ", "command": "git status\n", "appendEnter": false },
            { "id": "empty-command", "label": "Empty", "command": "   " },
            { "id": "status", "label": "Duplicate", "command": "pwd" },
            { "label": "No ID", "command": "date" }
        ]);
        assert_eq!(
            normalize_terminal_quick_commands(&input),
            vec![
                command_qc("status", "Status", "git status", false, TerminalQuickCommandScope::Global),
                command_qc("empty-command", "Empty", "", true, TerminalQuickCommandScope::Global),
                command_qc("status-2", "Duplicate", "pwd", true, TerminalQuickCommandScope::Global),
                command_qc("quick-command-4", "No ID", "date", true, TerminalQuickCommandScope::Global),
            ]
        );
    }

    #[test]
    fn normalizes_repository_scoped_commands_and_falls_back_to_global() {
        let input = json!([
            { "id": "repo-dev", "label": "Dev", "command": "pnpm dev", "scope": { "type": "repo", "repoId": " repo-1 " } },
            { "id": "bad-repo", "label": "Bad", "command": "echo bad", "scope": { "type": "repo", "repoId": "   " } }
        ]);
        assert_eq!(
            normalize_terminal_quick_commands(&input),
            vec![
                command_qc("repo-dev", "Dev", "pnpm dev", true, repo("repo-1")),
                command_qc("bad-repo", "Bad", "echo bad", true, TerminalQuickCommandScope::Global),
            ]
        );
    }

    #[test]
    fn normalizes_agent_prompt_commands_without_storing_generated_shell_text() {
        let input = json!([
            {
                "id": "agent-review",
                "label": "Review",
                "action": "agent-prompt",
                "agent": "codex",
                "prompt": "  Review this diff\n",
                "command": "codex 'old workaround'"
            },
            { "id": "unknown-agent", "label": "Unknown", "action": "agent-prompt", "agent": "not-real", "prompt": "Do work" },
            { "id": "post-start-agent", "label": "Aider", "action": "agent-prompt", "agent": "aider", "prompt": "Do work" }
        ]);
        assert_eq!(
            normalize_terminal_quick_commands(&input),
            vec![TerminalQuickCommand::Agent(TerminalAgentQuickCommand {
                id: "agent-review".to_string(),
                label: "Review".to_string(),
                scope: TerminalQuickCommandScope::Global,
                agent: "codex".to_string(),
                prompt: "  Review this diff".to_string(),
            })]
        );
    }

    #[test]
    fn matches_global_commands_everywhere_and_repo_commands_only_in_their_repo() {
        assert!(terminal_quick_command_matches_repo(
            &command_qc("global", "Global", "date", true, TerminalQuickCommandScope::Global),
            None
        ));
        assert!(terminal_quick_command_matches_repo(
            &command_qc("repo", "Repo", "pnpm dev", true, repo("repo-1")),
            Some("repo-1")
        ));
        assert!(!terminal_quick_command_matches_repo(
            &command_qc("repo", "Repo", "pnpm dev", true, repo("repo-1")),
            Some("repo-2")
        ));
    }

    #[test]
    fn formats_terminal_input_without_assuming_shell_semantics() {
        assert_eq!(
            build_terminal_quick_command_input(&TerminalCommandQuickCommand {
                id: "status".to_string(),
                label: "Status".to_string(),
                scope: TerminalQuickCommandScope::Global,
                command: "git status".to_string(),
                append_enter: true,
            }),
            "git status\r"
        );
        assert_eq!(
            build_terminal_quick_command_input(&TerminalCommandQuickCommand {
                id: "status".to_string(),
                label: "Status".to_string(),
                scope: TerminalQuickCommandScope::Global,
                command: "git status".to_string(),
                append_enter: false,
            }),
            "git status"
        );
    }

    #[test]
    fn classifies_quick_command_actions_and_body_text() {
        let terminal =
            command_qc("status", "Status", "git status", true, TerminalQuickCommandScope::Global);
        let agent = TerminalQuickCommand::Agent(TerminalAgentQuickCommand {
            id: "agent".to_string(),
            label: "Agent".to_string(),
            scope: TerminalQuickCommandScope::Global,
            agent: "claude".to_string(),
            prompt: "Fix the tests".to_string(),
        });

        assert_eq!(
            get_terminal_quick_command_action(&terminal),
            TerminalQuickCommandAction::TerminalCommand
        );
        assert_eq!(get_terminal_quick_command_body(&terminal), "git status");
        assert!(is_terminal_quick_command_complete(&terminal));
        assert_eq!(
            get_terminal_quick_command_action(&agent),
            TerminalQuickCommandAction::AgentPrompt
        );
        assert_eq!(get_terminal_quick_command_body(&agent), "Fix the tests");
        assert!(is_terminal_quick_command_complete(&agent));
    }

    #[test]
    fn only_allows_agent_prompt_quick_commands_for_launch_time_prompt_agents() {
        assert!(supports_terminal_agent_quick_command("claude"));
        assert!(supports_terminal_agent_quick_command("gemini"));
        assert!(!supports_terminal_agent_quick_command("aider"));
        assert!(!supports_terminal_agent_quick_command("not-real"));
    }

    // flattenTerminalQuickCommand

    fn flatten_input(command: &str) -> TerminalCommandQuickCommand {
        TerminalCommandQuickCommand {
            id: "test".to_string(),
            label: "Test".to_string(),
            scope: TerminalQuickCommandScope::Global,
            command: command.to_string(),
            append_enter: true,
        }
    }

    #[test]
    fn flatten_returns_the_same_object_when_there_are_no_line_breaks() {
        // TS asserts reference identity (`toBe`); the behavioural contract is an
        // unchanged value, which is what we assert here.
        let command = flatten_input("git status");
        assert_eq!(flatten_terminal_quick_command(&command), command);
    }

    #[test]
    fn flatten_replaces_newlines_with_semicolons_and_spaces() {
        let result = flatten_terminal_quick_command(&flatten_input("cd packages\nbun run build\ncd .."));
        assert_eq!(result.command, "cd packages; bun run build; cd ..");
    }

    #[test]
    fn flatten_collapses_consecutive_newlines_into_a_single_separator() {
        let result = flatten_terminal_quick_command(&flatten_input("echo one\n\n\necho two"));
        assert_eq!(result.command, "echo one; echo two");
    }

    #[test]
    fn flatten_handles_windows_style_crlf_endings() {
        let result = flatten_terminal_quick_command(&flatten_input("echo one\r\necho two"));
        assert_eq!(result.command, "echo one; echo two");
    }

    #[test]
    fn flatten_drops_empty_edge_lines_without_leaving_dangling_separators() {
        let result = flatten_terminal_quick_command(&flatten_input("\n  echo one  \n\n  echo two\n"));
        assert_eq!(result.command, "echo one; echo two");
    }
}
