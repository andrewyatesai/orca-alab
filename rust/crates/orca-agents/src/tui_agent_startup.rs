//! Launch-command plans for TUI agents, ported from
//! `src/shared/tui-agent-startup.ts`.
//!
//! Builds the shell command that launches an agent with the initial prompt
//! delivered per its [`AgentPromptInjectionMode`], quoting the prompt for the
//! target shell (posix / powershell / cmd). [`build_agent_draft_launch_plan`]
//! is the draft variant: it seeds the input box without submitting, via a
//! native `--prefill`-style flag or a startup env var.
//!
//! Note: the TS module also re-exports `isShellProcess` from `agent-detection`.
//! That helper has not been ported into `orca_core::agent_recognition` yet, so
//! it is intentionally not re-exported here (the startup-plan functions do not
//! depend on it); add the re-export once the source function lands.

use crate::tui_agent_config::{tui_agent_config, AgentPromptInjectionMode};

/// A built launch plan for starting an agent with (optionally) a first prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentStartupPlan {
    pub agent: String,
    pub launch_command: String,
    pub expected_process: String,
    /// Prompt to type into the session after start (stdin-after-start agents),
    /// or `None` when the prompt is baked into `launch_command`.
    pub followup_prompt: Option<String>,
}

/// A built launch plan that seeds a reviewable draft into the agent's input box.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentDraftLaunchPlan {
    pub agent: String,
    pub launch_command: String,
    pub expected_process: String,
    /// Single env var (name, value) to export for the launch, or `None` when
    /// the draft is delivered via a CLI flag instead.
    pub env: Option<(String, String)>,
}

/// Target shell whose quoting/clearing syntax the plan is built for.
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

/// Arguments to [`build_agent_startup_plan`].
pub struct AgentStartupPlanArgs<'a> {
    pub agent: &'a str,
    pub prompt: &'a str,
    /// Per-agent launch-command overrides, looked up by agent id. An empty
    /// override string is ignored (matches the TS truthiness check).
    pub cmd_overrides: &'a [(&'a str, &'a str)],
    /// Node `process.platform` string; only `"win32"` changes the default shell.
    pub platform: &'a str,
    pub shell: Option<AgentStartupShell>,
    pub allow_empty_prompt_launch: bool,
}

/// Arguments to [`build_agent_draft_launch_plan`].
pub struct AgentDraftLaunchArgs<'a> {
    pub agent: &'a str,
    pub draft: &'a str,
    pub cmd_overrides: &'a [(&'a str, &'a str)],
    pub platform: &'a str,
    pub shell: Option<AgentStartupShell>,
}

fn resolve_startup_shell(platform: &str, shell: Option<AgentStartupShell>) -> AgentStartupShell {
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
fn quote_startup_arg(value: &str, shell: AgentStartupShell) -> String {
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

fn clear_env_command(name: &str, shell: AgentStartupShell) -> String {
    match shell {
        AgentStartupShell::Powershell => {
            format!("Remove-Item Env:{name} -ErrorAction SilentlyContinue")
        }
        AgentStartupShell::Cmd => format!("set \"{name}=\""),
        AgentStartupShell::Posix => format!("unset {name}"),
    }
}

fn command_separator(shell: AgentStartupShell) -> &'static str {
    if matches!(shell, AgentStartupShell::Cmd) {
        " & "
    } else {
        "; "
    }
}

fn resolve_base_command(agent: &str, cmd_overrides: &[(&str, &str)], launch_cmd: &str) -> String {
    // Why: a present-but-empty override is falsy in the TS source and falls
    // through to the configured launch command.
    if let Some(override_cmd) = cmd_overrides
        .iter()
        .find(|(id, _)| *id == agent)
        .map(|(_, cmd)| *cmd)
    {
        if !override_cmd.is_empty() {
            return override_cmd.to_string();
        }
    }
    launch_cmd.to_string()
}

/// Build the launch plan, or `None` when there is no prompt and empty-prompt
/// launch is not allowed (or the agent id is unknown).
pub fn build_agent_startup_plan(args: &AgentStartupPlanArgs) -> Option<AgentStartupPlan> {
    let shell = resolve_startup_shell(args.platform, args.shell);
    let trimmed_prompt = args.prompt.trim();
    let config = tui_agent_config(args.agent)?;
    let base_command = resolve_base_command(args.agent, args.cmd_overrides, config.launch_cmd);

    if trimmed_prompt.is_empty() {
        if !args.allow_empty_prompt_launch {
            return None;
        }
        return Some(AgentStartupPlan {
            agent: args.agent.to_string(),
            launch_command: base_command,
            expected_process: config.expected_process.to_string(),
            followup_prompt: None,
        });
    }

    let quoted_prompt = quote_startup_arg(trimmed_prompt, shell);
    let launch_command = match config.prompt_injection_mode {
        AgentPromptInjectionMode::Argv => format!("{base_command} {quoted_prompt}"),
        AgentPromptInjectionMode::FlagPrompt => format!("{base_command} --prompt {quoted_prompt}"),
        AgentPromptInjectionMode::FlagPromptInteractive => {
            format!("{base_command} --prompt-interactive {quoted_prompt}")
        }
        AgentPromptInjectionMode::FlagInteractive => format!("{base_command} -i {quoted_prompt}"),
        AgentPromptInjectionMode::StdinAfterStart => {
            return Some(AgentStartupPlan {
                agent: args.agent.to_string(),
                launch_command: base_command,
                expected_process: config.expected_process.to_string(),
                followup_prompt: Some(trimmed_prompt.to_string()),
            });
        }
    };

    Some(AgentStartupPlan {
        agent: args.agent.to_string(),
        launch_command,
        expected_process: config.expected_process.to_string(),
        followup_prompt: None,
    })
}

/// Build the draft-launch plan, or `None` when the draft is empty, the agent
/// has no native draft mechanism, or the agent id is unknown.
pub fn build_agent_draft_launch_plan(args: &AgentDraftLaunchArgs) -> Option<AgentDraftLaunchPlan> {
    let shell = resolve_startup_shell(args.platform, args.shell);
    let config = tui_agent_config(args.agent)?;
    let trimmed = args.draft.trim();
    if trimmed.is_empty() {
        return None;
    }
    let base_command = resolve_base_command(args.agent, args.cmd_overrides, config.launch_cmd);

    if let Some(flag) = config.draft_prompt_flag {
        let quoted = quote_startup_arg(trimmed, shell);
        return Some(AgentDraftLaunchPlan {
            agent: args.agent.to_string(),
            launch_command: format!("{base_command} {flag} {quoted}"),
            expected_process: config.expected_process.to_string(),
            env: None,
        });
    }

    if let Some(env_var) = config.draft_prompt_env_var {
        let clear_var = clear_env_command(env_var, shell);
        return Some(AgentDraftLaunchPlan {
            agent: args.agent.to_string(),
            launch_command: format!("{base_command}{}{clear_var}", command_separator(shell)),
            expected_process: config.expected_process.to_string(),
            env: Some((env_var.to_string(), trimmed.to_string())),
        });
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn startup(args: &AgentStartupPlanArgs) -> AgentStartupPlan {
        build_agent_startup_plan(args).unwrap()
    }

    #[test]
    fn uses_posix_quoting_when_the_target_shell_is_linux() {
        let plan = startup(&AgentStartupPlanArgs {
            agent: "claude",
            prompt: "fix Bob's branch",
            cmd_overrides: &[],
            platform: "linux",
            shell: None,
            allow_empty_prompt_launch: false,
        });
        assert_eq!(plan.launch_command, "claude 'fix Bob'\\''s branch'");
    }

    #[test]
    fn uses_powershell_quoting_by_default_on_windows() {
        let plan = startup(&AgentStartupPlanArgs {
            agent: "claude",
            prompt: "fix Bob's \"quoted\" branch",
            cmd_overrides: &[],
            platform: "win32",
            shell: None,
            allow_empty_prompt_launch: false,
        });
        assert_eq!(plan.launch_command, "claude 'fix Bob''s \"quoted\" branch'");
    }

    #[test]
    fn uses_cmd_escaping_when_requested_explicitly() {
        let plan = startup(&AgentStartupPlanArgs {
            agent: "claude",
            prompt: "fix \"quoted\" & %PATH%",
            cmd_overrides: &[],
            platform: "win32",
            shell: Some(AgentStartupShell::Cmd),
            allow_empty_prompt_launch: false,
        });
        assert_eq!(plan.launch_command, "claude \"fix ^\"quoted^\" ^& ^%PATH^%\"");
    }

    #[test]
    fn does_not_launch_codex_with_the_orca_profile() {
        let plan = startup(&AgentStartupPlanArgs {
            agent: "codex",
            prompt: "fix it",
            cmd_overrides: &[],
            platform: "linux",
            shell: None,
            allow_empty_prompt_launch: false,
        });
        assert_eq!(plan.launch_command, "codex 'fix it'");
    }

    #[test]
    fn launches_claude_without_orca_settings_injection() {
        let plan = startup(&AgentStartupPlanArgs {
            agent: "claude",
            prompt: "fix it",
            cmd_overrides: &[],
            platform: "linux",
            shell: None,
            allow_empty_prompt_launch: false,
        });
        assert_eq!(plan.launch_command, "claude 'fix it'");
        assert!(!plan.launch_command.contains("--settings"));
    }

    #[test]
    fn launches_openclaude_as_a_distinct_argv_agent() {
        let plan = startup(&AgentStartupPlanArgs {
            agent: "openclaude",
            prompt: "fix it",
            cmd_overrides: &[],
            platform: "linux",
            shell: None,
            allow_empty_prompt_launch: false,
        });
        assert_eq!(
            plan,
            AgentStartupPlan {
                agent: "openclaude".to_string(),
                launch_command: "openclaude 'fix it'".to_string(),
                expected_process: "openclaude".to_string(),
                followup_prompt: None,
            }
        );
    }

    #[test]
    fn launches_mistral_vibe_through_the_installed_vibe_executable() {
        let plan = startup(&AgentStartupPlanArgs {
            agent: "mistral-vibe",
            prompt: "fix it",
            cmd_overrides: &[],
            platform: "linux",
            shell: None,
            allow_empty_prompt_launch: false,
        });
        assert_eq!(
            plan,
            AgentStartupPlan {
                agent: "mistral-vibe".to_string(),
                launch_command: "vibe".to_string(),
                expected_process: "vibe".to_string(),
                followup_prompt: Some("fix it".to_string()),
            }
        );
    }

    #[test]
    fn leaves_claude_command_overrides_untouched() {
        let plan = startup(&AgentStartupPlanArgs {
            agent: "claude",
            prompt: "fix it",
            cmd_overrides: &[("claude", "claude --dangerously-skip-permissions")],
            platform: "linux",
            shell: None,
            allow_empty_prompt_launch: false,
        });
        assert_eq!(
            plan.launch_command,
            "claude --dangerously-skip-permissions 'fix it'"
        );
    }

    #[test]
    fn leaves_codex_command_overrides_untouched() {
        let plan = startup(&AgentStartupPlanArgs {
            agent: "codex",
            prompt: "fix it",
            cmd_overrides: &[("codex", "codex --profile work")],
            platform: "linux",
            shell: None,
            allow_empty_prompt_launch: false,
        });
        assert_eq!(plan.launch_command, "codex --profile work 'fix it'");
    }

    #[test]
    fn clears_draft_environment_variables_with_the_target_shell_syntax() {
        assert_eq!(
            build_agent_draft_launch_plan(&AgentDraftLaunchArgs {
                agent: "pi",
                draft: "https://github.com/acme/repo/issues/42",
                cmd_overrides: &[],
                platform: "win32",
                shell: None,
            })
            .unwrap()
            .launch_command,
            "pi; Remove-Item Env:ORCA_PI_PREFILL -ErrorAction SilentlyContinue"
        );

        assert_eq!(
            build_agent_draft_launch_plan(&AgentDraftLaunchArgs {
                agent: "pi",
                draft: "https://github.com/acme/repo/issues/42",
                cmd_overrides: &[],
                platform: "win32",
                shell: Some(AgentStartupShell::Cmd),
            })
            .unwrap()
            .launch_command,
            "pi & set \"ORCA_PI_PREFILL=\""
        );
    }

    #[test]
    fn returns_an_omp_draft_plan_with_omp_scoped_prefill() {
        let plan = build_agent_draft_launch_plan(&AgentDraftLaunchArgs {
            agent: "omp",
            draft: "fix the omp regression",
            cmd_overrides: &[],
            platform: "linux",
            shell: None,
        })
        .unwrap();

        assert_eq!(
            plan.env,
            Some((
                "ORCA_OMP_PREFILL".to_string(),
                "fix the omp regression".to_string()
            ))
        );
        assert_eq!(plan.expected_process, "omp");
        assert_eq!(plan.launch_command, "omp; unset ORCA_OMP_PREFILL");
    }
}
