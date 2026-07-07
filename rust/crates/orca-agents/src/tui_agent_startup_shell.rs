//! Shell-quoting helpers for TUI-agent launch plans, ported from
//! `src/shared/tui-agent-startup-shell.ts`.
//!
//! Everything here is per-shell text assembly: picking the default shell for a
//! platform, quoting single arguments, joining an argv into one command line,
//! clearing env vars, and validating/quoting user-configured extra CLI args.

use crate::commit_message_prompt::tokenize_custom_command_template;

/// Target shell whose quoting/clearing syntax a launch plan is built for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentStartupShell {
    Posix,
    Powershell,
    Cmd,
}

impl AgentStartupShell {
    /// Parse the TS string literal (`'posix' | 'powershell' | 'cmd'`).
    pub fn from_label(label: &str) -> Option<Self> {
        match label {
            "posix" => Some(Self::Posix),
            "powershell" => Some(Self::Powershell),
            "cmd" => Some(Self::Cmd),
            _ => None,
        }
    }
}

/// Default the shell from the platform when the caller does not pin one
/// (only `"win32"` changes the default).
pub fn resolve_startup_shell(platform: &str, shell: Option<AgentStartupShell>) -> AgentStartupShell {
    shell.unwrap_or(if platform == "win32" {
        AgentStartupShell::Powershell
    } else {
        AgentStartupShell::Posix
    })
}

/// Quote a single argument for the target shell.
// Trust contract: inert under stock cargo, proved under `--cfg trust_verify`.
// Postcondition — the result is always wrapped (at least the two quote chars).
#[cfg_attr(trust_verify, trust::ensures(|out: &String| out.len() >= 2))]
pub fn quote_startup_arg(value: &str, shell: AgentStartupShell) -> String {
    match shell {
        AgentStartupShell::Powershell => format!("'{}'", value.replace('\'', "''")),
        AgentStartupShell::Cmd => {
            // Prefix each cmd metacharacter (and a literal caret) with `^`.
            let mut escaped = String::with_capacity(value.len());
            for ch in value.chars() {
                if matches!(
                    ch,
                    '^' | '&' | '|' | '<' | '>' | '(' | ')' | '%' | '!' | '"'
                ) {
                    escaped.push('^');
                }
                escaped.push(ch);
            }
            format!("\"{escaped}\"")
        }
        AgentStartupShell::Posix => format!("'{}'", value.replace('\'', "'\\''")),
    }
}

/// Join an argv into one shell command line, quoting each argument.
/// Why: PowerShell needs the `&` call operator to run a quoted executable.
pub fn build_shell_command_from_argv(args: &[&str], shell: AgentStartupShell) -> String {
    let command = args
        .iter()
        .map(|arg| quote_startup_arg(arg, shell))
        .collect::<Vec<_>>()
        .join(" ");
    if matches!(shell, AgentStartupShell::Powershell) && !command.is_empty() {
        return format!("& {command}");
    }
    command
}

/// The shell statement that unsets `name` in the launched session.
pub fn clear_env_command(name: &str, shell: AgentStartupShell) -> String {
    match shell {
        AgentStartupShell::Powershell => {
            format!("Remove-Item Env:{name} -ErrorAction SilentlyContinue")
        }
        AgentStartupShell::Cmd => format!("set \"{name}=\""),
        AgentStartupShell::Posix => format!("unset {name}"),
    }
}

/// The statement separator for chaining commands in the target shell.
pub fn command_separator(shell: AgentStartupShell) -> &'static str {
    if matches!(shell, AgentStartupShell::Cmd) {
        " & "
    } else {
        "; "
    }
}

/// Validated, shell-quoted suffix for user-configured extra CLI args
/// (`Ok("")` when absent or blank). The TS `AgentCliArgsPlan` failure shape
/// (`{ ok: false, error }`) maps to `Err` carrying the same message.
pub fn plan_agent_cli_args_suffix(
    agent_args: Option<&str>,
    shell: AgentStartupShell,
) -> Result<String, String> {
    let trimmed = agent_args.map(str::trim).unwrap_or("");
    if trimmed.is_empty() {
        return Ok(String::new());
    }
    match tokenize_custom_command_template(trimmed) {
        Err(error) => Err(format!("CLI arguments are invalid: {error}")),
        Ok(tokens) => Ok(tokens
            .iter()
            .map(|token| quote_startup_arg(token, shell))
            .collect::<Vec<_>>()
            .join(" ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_arguments_per_shell() {
        assert_eq!(
            quote_startup_arg("fix Bob's branch", AgentStartupShell::Posix),
            "'fix Bob'\\''s branch'"
        );
        assert_eq!(
            quote_startup_arg("fix Bob's branch", AgentStartupShell::Powershell),
            "'fix Bob''s branch'"
        );
        assert_eq!(
            quote_startup_arg("a \"b\" & %PATH%", AgentStartupShell::Cmd),
            "\"a ^\"b^\" ^& ^%PATH^%\""
        );
    }

    #[test]
    fn builds_a_powershell_command_with_the_call_operator() {
        assert_eq!(
            build_shell_command_from_argv(&["git", "commit -m", "it's"], AgentStartupShell::Powershell),
            "& 'git' 'commit -m' 'it''s'"
        );
        assert_eq!(
            build_shell_command_from_argv(&["echo", "hi"], AgentStartupShell::Posix),
            "'echo' 'hi'"
        );
        // Empty argv stays empty in every shell (no dangling `& `).
        assert_eq!(build_shell_command_from_argv(&[], AgentStartupShell::Powershell), "");
        assert_eq!(build_shell_command_from_argv(&[], AgentStartupShell::Posix), "");
    }

    #[test]
    fn clears_env_vars_and_separates_commands_per_shell() {
        assert_eq!(
            clear_env_command("FOO", AgentStartupShell::Powershell),
            "Remove-Item Env:FOO -ErrorAction SilentlyContinue"
        );
        assert_eq!(clear_env_command("FOO", AgentStartupShell::Cmd), "set \"FOO=\"");
        assert_eq!(clear_env_command("FOO", AgentStartupShell::Posix), "unset FOO");
        assert_eq!(command_separator(AgentStartupShell::Cmd), " & ");
        assert_eq!(command_separator(AgentStartupShell::Posix), "; ");
        assert_eq!(command_separator(AgentStartupShell::Powershell), "; ");
    }

    #[test]
    fn plans_an_empty_suffix_for_absent_or_blank_agent_args() {
        assert_eq!(plan_agent_cli_args_suffix(None, AgentStartupShell::Posix), Ok(String::new()));
        assert_eq!(
            plan_agent_cli_args_suffix(Some("   "), AgentStartupShell::Posix),
            Ok(String::new())
        );
    }

    #[test]
    fn plans_a_quoted_suffix_from_tokenized_agent_args() {
        assert_eq!(
            plan_agent_cli_args_suffix(Some("--model opus \"two words\""), AgentStartupShell::Posix),
            Ok("'--model' 'opus' 'two words'".to_string())
        );
        assert_eq!(
            plan_agent_cli_args_suffix(Some("--model opus"), AgentStartupShell::Cmd),
            Ok("\"--model\" \"opus\"".to_string())
        );
    }

    #[test]
    fn fails_the_suffix_plan_with_the_ts_error_string() {
        assert_eq!(
            plan_agent_cli_args_suffix(Some("--flag 'unclosed"), AgentStartupShell::Posix),
            Err("CLI arguments are invalid: Unclosed quote in command template.".to_string())
        );
    }
}
