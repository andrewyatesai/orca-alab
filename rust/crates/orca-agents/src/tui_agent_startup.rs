//! Launch-command plans for TUI agents, ported from
//! `src/shared/tui-agent-startup.ts`.
//!
//! Builds the shell command that launches an agent with the initial prompt
//! delivered per its [`AgentPromptInjectionMode`], quoting for the target
//! shell via [`crate::tui_agent_startup_shell`]. [`build_agent_draft_launch_plan`]
//! is the draft variant: it seeds the input box without submitting, via a
//! native `--prefill`-style flag or a startup env var.
//! [`build_agent_resume_startup_plan`] relaunches a resumable provider session
//! from its per-agent resume argv.
//!
//! Note: the TS module also re-exports `isShellProcess` from `agent-detection`.
//! That helper has not been ported yet, so it is intentionally not re-exported
//! here (the plan builders do not depend on it); add the re-export once the
//! source function lands.
//!
//! Session options (upstream #9085): the native-chat per-model picker flags are
//! layered over this core's option-free plan by the TS wrappers
//! (`tui-agent-session-option-splice.ts`) — the catalogs are TS closures, and
//! this core's resolved command IS `commandWithoutSessionOptions` (what the
//! launch-config snapshot must keep), so the base plan here stays unchanged.

use crate::tui_agent_config::{tui_agent_config, AgentPromptInjectionMode, TuiAgentConfig};
use crate::tui_agent_startup_shell::{clear_env_command, command_separator};
// Mirror the TS module's re-exports from `tui-agent-startup-shell.ts`.
pub use crate::tui_agent_startup_shell::{
    build_shell_command_from_argv, plan_agent_cli_args_suffix, quote_startup_arg,
    resolve_startup_shell, AgentStartupShell,
};

// Why: Windows CreateProcess/env blocks have tight length ceilings. Large
// generated drafts should use the existing post-ready paste fallback.
const WIN32_INLINE_DRAFT_LIMIT_CHARS: usize = 24_000;

/// Durable resume snapshot of the Orca-managed launch inputs, ported from
/// `SleepingAgentLaunchConfig` in `agent-session-resume.ts`. Every startup and
/// draft plan carries one so a sleeping agent can be relaunched identically.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SleepingAgentLaunchConfig {
    /// The resolved base launch command; `None` only when it trims to empty
    /// (matches the TS `agentCommand?.trim()` truthiness guard).
    pub agent_command: Option<String>,
    pub agent_args: String,
    pub agent_env: Vec<(String, String)>,
}

/// A built launch plan for starting an agent with (optionally) a first prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentStartupPlan {
    pub agent: String,
    pub launch_command: String,
    pub expected_process: String,
    /// Prompt to type into the session after start (stdin-after-start agents),
    /// or `None` when the prompt is baked into `launch_command`.
    pub followup_prompt: Option<String>,
    pub launch_config: SleepingAgentLaunchConfig,
    /// Extra env for the launched session; `Some` whenever the caller passed
    /// `agent_env` (the TS spreads the key in for any object, even `{}`).
    pub env: Option<Vec<(String, String)>>,
    /// Codex-only: how the CLI ingests its startup command (`shell-ready`); `None`
    /// for other agents (the TS spreads this key in only when `agent === 'codex'`).
    pub startup_command_delivery: Option<String>,
}

/// A built launch plan that seeds a reviewable draft into the agent's input box.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentDraftLaunchPlan {
    pub agent: String,
    pub launch_command: String,
    pub expected_process: String,
    /// Env for the launch: on the env-var draft path this is the caller env
    /// plus the prefill var; on the flag path it mirrors `agent_env` (`None`
    /// when the caller passed none).
    pub env: Option<Vec<(String, String)>>,
    pub launch_config: SleepingAgentLaunchConfig,
    /// Codex-only startup-command delivery (`shell-ready`); `None` otherwise.
    pub startup_command_delivery: Option<String>,
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
    /// User-configured extra CLI args (tokenized + quoted onto the base command).
    pub agent_args: Option<&'a str>,
    /// Orca-managed agent env inputs (threaded into the plan and the snapshot).
    pub agent_env: Option<&'a [(String, String)]>,
    /// Why: SSH remotes deploy the CLI shim as plain `orca`, so the Linux-only
    /// `orca-ide` rename must be skipped for remote launches.
    pub is_remote: bool,
}

/// Arguments to [`build_agent_resume_startup_plan`].
pub struct AgentResumeStartupPlanArgs<'a> {
    pub agent: &'a str,
    /// The provider-session `key`/`id` pair (`AgentProviderSessionMetadata`).
    pub provider_session_key: ProviderSessionKey,
    pub provider_session_id: &'a str,
    pub cmd_overrides: &'a [(&'a str, &'a str)],
    pub platform: &'a str,
    pub shell: Option<AgentStartupShell>,
    pub agent_args: Option<&'a str>,
    pub agent_env: Option<&'a [(String, String)]>,
    /// Pre-resolved launch command (from the sleep snapshot); when it trims
    /// non-empty it bypasses base-command resolution entirely.
    pub agent_command: Option<&'a str>,
    /// Why: see [`AgentStartupPlanArgs::is_remote`] — remote launches use the
    /// plain `orca` shim.
    pub is_remote: bool,
}

/// Arguments to [`build_agent_draft_launch_plan`].
pub struct AgentDraftLaunchArgs<'a> {
    pub agent: &'a str,
    pub draft: &'a str,
    pub cmd_overrides: &'a [(&'a str, &'a str)],
    pub platform: &'a str,
    pub shell: Option<AgentStartupShell>,
    pub agent_args: Option<&'a str>,
    pub agent_env: Option<&'a [(String, String)]>,
    /// Why: see [`AgentStartupPlanArgs::is_remote`].
    pub is_remote: bool,
}

/// Port of `getTuiAgentLaunchCommand` (tui-agent-config.ts).
fn get_tui_agent_launch_command(
    config: &TuiAgentConfig,
    platform: &str,
    is_remote: bool,
) -> &'static str {
    // Why: the SSH relay shim is always named `orca` on Unix, so the local-only
    // `orca-ide` rename (Linux launchCmdByPlatform) must not leak to remotes.
    if is_remote && platform == "linux" {
        return config.launch_cmd;
    }
    // Mirror TS `config.launchCmdByPlatform?.[platform] ?? config.launchCmd`.
    config
        .launch_cmd_by_platform
        .iter()
        .find(|(candidate, _)| *candidate == platform)
        .map(|(_, cmd)| *cmd)
        .unwrap_or(config.launch_cmd)
}

struct ResolveBaseCommandArgs<'a> {
    agent: &'a str,
    config: &'static TuiAgentConfig,
    cmd_overrides: &'a [(&'a str, &'a str)],
    platform: &'a str,
    shell: AgentStartupShell,
    agent_args: Option<&'a str>,
    is_remote: bool,
}

/// Port of the TS `resolveBaseCommand`: the per-agent override OR the
/// configured launch command, then the validated agent-args suffix (which can
/// fail — the `Err` mirrors the TS `{ ok: false, error }`).
fn resolve_base_command(args: &ResolveBaseCommandArgs) -> Result<String, String> {
    // Why: a present-but-empty override is falsy in the TS source and falls
    // through to the configured launch command.
    let override_cmd = args
        .cmd_overrides
        .iter()
        .find(|(id, _)| *id == args.agent)
        .map(|(_, cmd)| *cmd)
        .filter(|cmd| !cmd.is_empty());
    let command = override_cmd
        .unwrap_or_else(|| get_tui_agent_launch_command(args.config, args.platform, args.is_remote));
    let suffix = plan_agent_cli_args_suffix(args.agent_args, args.shell)?;
    // Why: Codex status hooks live in Orca's runtime CODEX_HOME; adding
    // --profile-v2 makes Codex load a second hook representation and warn.
    Ok(if suffix.is_empty() {
        command.to_string()
    } else {
        format!("{command} {suffix}")
    })
}

/// Build the durable resume snapshot: the resolved base command plus the raw
/// caller-supplied `agent_args`/`agent_env` (the plan env may carry extra
/// transport/identity vars, but the snapshot is Orca-managed inputs only).
fn build_sleeping_agent_launch_config(
    agent_command: &str,
    agent_args: Option<&str>,
    agent_env: Option<&[(String, String)]>,
) -> SleepingAgentLaunchConfig {
    SleepingAgentLaunchConfig {
        agent_command: if agent_command.trim().is_empty() {
            None
        } else {
            Some(agent_command.to_string())
        },
        agent_args: agent_args.unwrap_or("").to_string(),
        agent_env: agent_env.map(<[(String, String)]>::to_vec).unwrap_or_default(),
    }
}

/// Codex ingests its startup command shell-ready; other agents omit the field —
/// mirrors the TS `agent === 'codex' ? { startupCommandDelivery: 'shell-ready' } : {}`.
fn codex_startup_delivery(agent: &str) -> Option<String> {
    (agent == "codex").then(|| "shell-ready".to_string())
}

/// UTF-16 code units — JS `String.prototype.length` for the win32 draft budget.
fn utf16_len(value: &str) -> usize {
    value.encode_utf16().count()
}

/// The provider-session key kind a resume argv is valid for (the TS
/// `AgentProviderSessionMetadata['key']` union).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderSessionKey {
    SessionId,
    ConversationId,
}

impl ProviderSessionKey {
    /// Parse the TS wire literal (`'session_id' | 'conversation_id'`).
    pub fn from_label(label: &str) -> Option<Self> {
        match label {
            "session_id" => Some(Self::SessionId),
            "conversation_id" => Some(Self::ConversationId),
            _ => None,
        }
    }
}

/// Port of `getAgentResumeArgv` (agent-session-resume.ts): the per-agent argv
/// that resumes a provider session, or `None` when the agent cannot resume
/// from the given key kind (or is not a resumable agent).
pub fn get_agent_resume_argv(
    agent: &str,
    key: ProviderSessionKey,
    id: &str,
) -> Option<Vec<String>> {
    let argv: &[&str] = match (agent, key) {
        ("claude", ProviderSessionKey::SessionId) => &["claude", "--resume", id],
        ("codex", ProviderSessionKey::SessionId) => &["codex", "resume", id],
        ("gemini", ProviderSessionKey::SessionId) => &["gemini", "--resume", id],
        ("antigravity", ProviderSessionKey::ConversationId) => &["agy", "--conversation", id],
        ("opencode", ProviderSessionKey::SessionId) => &["opencode", "--session", id],
        ("mimo-code", ProviderSessionKey::SessionId) => &["mimo", "--session", id],
        ("droid", ProviderSessionKey::SessionId) => &["droid", "--resume", id],
        ("grok", ProviderSessionKey::SessionId) => &["grok", "--resume", id],
        ("devin", ProviderSessionKey::SessionId) => &["devin", "--resume", id],
        _ => return None,
    };
    Some(argv.iter().map(|s| s.to_string()).collect())
}

/// Build the launch plan, or `None` when the agent-args suffix is invalid or
/// there is no prompt and empty-prompt launch is not allowed (or the agent id
/// is unknown).
pub fn build_agent_startup_plan(args: &AgentStartupPlanArgs) -> Option<AgentStartupPlan> {
    let shell = resolve_startup_shell(args.platform, args.shell);
    let trimmed_prompt = args.prompt.trim();
    let config = tui_agent_config(args.agent)?;
    // Why: TS resolves the base command before the empty-prompt check, so an
    // invalid agent-args suffix fails the plan even for empty prompts.
    let base_command = resolve_base_command(&ResolveBaseCommandArgs {
        agent: args.agent,
        config,
        cmd_overrides: args.cmd_overrides,
        platform: args.platform,
        shell,
        agent_args: args.agent_args,
        is_remote: args.is_remote,
    })
    .ok()?;
    let launch_config =
        build_sleeping_agent_launch_config(&base_command, args.agent_args, args.agent_env);
    let env = args.agent_env.map(<[(String, String)]>::to_vec);

    if trimmed_prompt.is_empty() {
        if !args.allow_empty_prompt_launch {
            return None;
        }
        return Some(AgentStartupPlan {
            agent: args.agent.to_string(),
            launch_command: base_command,
            expected_process: config.expected_process.to_string(),
            followup_prompt: None,
            launch_config,
            env,
            // Why: only the argv branch carries codex delivery in the TS.
            startup_command_delivery: None,
        });
    }

    let quoted_prompt = quote_startup_arg(trimmed_prompt, shell);
    let launch_command = match config.prompt_injection_mode {
        AgentPromptInjectionMode::Argv => {
            // Grok's `--` separator keeps a flag/subcommand-shaped prompt literal.
            let separator = config
                .argv_prompt_separator
                .map(|s| format!(" {s}"))
                .unwrap_or_default();
            return Some(AgentStartupPlan {
                agent: args.agent.to_string(),
                launch_command: format!("{base_command}{separator} {quoted_prompt}"),
                expected_process: config.expected_process.to_string(),
                followup_prompt: None,
                launch_config,
                env,
                startup_command_delivery: codex_startup_delivery(args.agent),
            });
        }
        AgentPromptInjectionMode::FlagPrompt => format!("{base_command} --prompt {quoted_prompt}"),
        AgentPromptInjectionMode::FlagPromptInteractive => {
            format!("{base_command} --prompt-interactive {quoted_prompt}")
        }
        AgentPromptInjectionMode::FlagInteractive => format!("{base_command} -i {quoted_prompt}"),
        // HermesQuery produces the same BASE plan as StdinAfterStart; the TS
        // wrapper (`git-wasm/tui-agent-startup.ts`, keyed on agent === 'hermes')
        // rebuilds the launch command through `planHermesStartupQuery` for a
        // non-empty prompt. Sharing the arm keeps the base plan byte-identical to
        // what shipped when hermes mapped to StdinAfterStart.
        AgentPromptInjectionMode::StdinAfterStart | AgentPromptInjectionMode::HermesQuery => {
            return Some(AgentStartupPlan {
                agent: args.agent.to_string(),
                launch_command: base_command,
                expected_process: config.expected_process.to_string(),
                followup_prompt: Some(trimmed_prompt.to_string()),
                launch_config,
                env,
                startup_command_delivery: None,
            });
        }
    };

    Some(AgentStartupPlan {
        agent: args.agent.to_string(),
        launch_command,
        expected_process: config.expected_process.to_string(),
        followup_prompt: None,
        launch_config,
        env,
        startup_command_delivery: None,
    })
}

/// Port of `buildAgentResumeStartupPlan`: relaunch a resumable provider
/// session by appending the quoted resume argv tail to the base command.
/// A trimmed-non-empty `agent_command` bypasses base-command resolution
/// entirely — including agent-args validation — matching the TS.
pub fn build_agent_resume_startup_plan(
    args: &AgentResumeStartupPlanArgs,
) -> Option<AgentStartupPlan> {
    let argv =
        get_agent_resume_argv(args.agent, args.provider_session_key, args.provider_session_id)?;
    let shell = resolve_startup_shell(args.platform, args.shell);
    let config = tui_agent_config(args.agent)?;
    let resolved_agent_command = args.agent_command.map(str::trim).filter(|cmd| !cmd.is_empty());
    let base_command = match resolved_agent_command {
        Some(command) => command.to_string(),
        None => resolve_base_command(&ResolveBaseCommandArgs {
            agent: args.agent,
            config,
            cmd_overrides: args.cmd_overrides,
            platform: args.platform,
            shell,
            agent_args: args.agent_args,
            is_remote: args.is_remote,
        })
        .ok()?,
    };
    let launch_config =
        build_sleeping_agent_launch_config(&base_command, args.agent_args, args.agent_env);
    let resume_args = argv[1..]
        .iter()
        .map(|arg| quote_startup_arg(arg, shell))
        .collect::<Vec<_>>()
        .join(" ");
    let launch_command = if resume_args.is_empty() {
        base_command
    } else {
        format!("{base_command} {resume_args}")
    };
    Some(AgentStartupPlan {
        agent: args.agent.to_string(),
        launch_command,
        expected_process: config.expected_process.to_string(),
        followup_prompt: None,
        launch_config,
        env: args.agent_env.map(<[(String, String)]>::to_vec),
        // Why: the TS resume plan never sets startupCommandDelivery, even for codex.
        startup_command_delivery: None,
    })
}

/// Mirror the TS `{ ...agentEnv, [envVar]: draft }` spread: an existing key
/// keeps its position but takes the new value; otherwise it is appended.
fn merged_draft_env(
    agent_env: Option<&[(String, String)]>,
    env_var: &str,
    draft: &str,
) -> Vec<(String, String)> {
    let mut env = agent_env.map(<[(String, String)]>::to_vec).unwrap_or_default();
    if let Some(entry) = env.iter_mut().find(|(key, _)| key == env_var) {
        entry.1 = draft.to_string();
    } else {
        env.push((env_var.to_string(), draft.to_string()));
    }
    env
}

fn inline_draft_plan_fits_platform(plan: &AgentDraftLaunchPlan, platform: &str) -> bool {
    if platform != "win32" {
        return true;
    }
    let env_chars: usize = plan
        .env
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(|(key, value)| utf16_len(key) + utf16_len(value))
        .sum();
    utf16_len(&plan.launch_command) + env_chars <= WIN32_INLINE_DRAFT_LIMIT_CHARS
}

/// Build the draft-launch plan, or `None` when the draft is empty, the
/// agent-args suffix is invalid, the agent has no native draft mechanism, the
/// inline payload exceeds the win32 budget, or the agent id is unknown.
pub fn build_agent_draft_launch_plan(args: &AgentDraftLaunchArgs) -> Option<AgentDraftLaunchPlan> {
    let shell = resolve_startup_shell(args.platform, args.shell);
    let config = tui_agent_config(args.agent)?;
    let trimmed = args.draft.trim();
    if trimmed.is_empty() {
        return None;
    }
    let base_command = resolve_base_command(&ResolveBaseCommandArgs {
        agent: args.agent,
        config,
        cmd_overrides: args.cmd_overrides,
        platform: args.platform,
        shell,
        agent_args: args.agent_args,
        is_remote: args.is_remote,
    })
    .ok()?;
    let launch_config =
        build_sleeping_agent_launch_config(&base_command, args.agent_args, args.agent_env);

    let plan = if let Some(flag) = config.draft_prompt_flag {
        let quoted = quote_startup_arg(trimmed, shell);
        Some(AgentDraftLaunchPlan {
            agent: args.agent.to_string(),
            launch_command: format!("{base_command} {flag} {quoted}"),
            expected_process: config.expected_process.to_string(),
            env: args.agent_env.map(<[(String, String)]>::to_vec),
            launch_config,
            // Why: native draft flags carry user text on argv and must survive
            // rc-file startup.
            startup_command_delivery: codex_startup_delivery(args.agent),
        })
    } else if let Some(env_var) = config.draft_prompt_env_var {
        let clear_var = clear_env_command(env_var, shell);
        Some(AgentDraftLaunchPlan {
            agent: args.agent.to_string(),
            launch_command: format!("{base_command}{}{clear_var}", command_separator(shell)),
            expected_process: config.expected_process.to_string(),
            env: Some(merged_draft_env(args.agent_env, env_var, trimmed)),
            launch_config,
            startup_command_delivery: None,
        })
    } else {
        None
    };

    plan.filter(|plan| inline_draft_plan_fits_platform(plan, args.platform))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resume_argv_matches_the_ts_table_per_agent_and_key() {
        use ProviderSessionKey::{ConversationId, SessionId};
        let argv = |a: &str, k, id: &str| get_agent_resume_argv(a, k, id);
        assert_eq!(argv("claude", SessionId, "s1").unwrap(), ["claude", "--resume", "s1"]);
        assert_eq!(argv("codex", SessionId, "s2").unwrap(), ["codex", "resume", "s2"]);
        assert_eq!(argv("antigravity", ConversationId, "c1").unwrap(), ["agy", "--conversation", "c1"]);
        assert_eq!(argv("opencode", SessionId, "s3").unwrap(), ["opencode", "--session", "s3"]);
        // Wrong key kind for the agent -> None (the TS key guard).
        assert_eq!(argv("claude", ConversationId, "x"), None);
        assert_eq!(argv("antigravity", SessionId, "x"), None);
        // Non-resumable agent -> None.
        assert_eq!(argv("pi", SessionId, "x"), None);
    }

    fn startup_args<'a>(agent: &'a str, prompt: &'a str, platform: &'a str) -> AgentStartupPlanArgs<'a> {
        AgentStartupPlanArgs {
            agent,
            prompt,
            cmd_overrides: &[],
            platform,
            shell: None,
            allow_empty_prompt_launch: false,
            agent_args: None,
            agent_env: None,
            is_remote: false,
        }
    }

    fn startup(args: &AgentStartupPlanArgs) -> AgentStartupPlan {
        build_agent_startup_plan(args).unwrap()
    }

    #[test]
    fn uses_posix_quoting_when_the_target_shell_is_linux() {
        let plan = startup(&startup_args("claude", "fix Bob's branch", "linux"));
        assert_eq!(plan.launch_command, "claude 'fix Bob'\\''s branch'");
    }

    #[test]
    fn uses_powershell_quoting_by_default_on_windows() {
        let plan = startup(&startup_args("claude", "fix Bob's \"quoted\" branch", "win32"));
        assert_eq!(plan.launch_command, "claude 'fix Bob''s \"quoted\" branch'");
    }

    #[test]
    fn uses_cmd_escaping_when_requested_explicitly() {
        let plan = startup(&AgentStartupPlanArgs {
            shell: Some(AgentStartupShell::Cmd),
            ..startup_args("claude", "fix \"quoted\" & %PATH%", "win32")
        });
        assert_eq!(plan.launch_command, "claude \"fix ^\"quoted^\" ^& ^%PATH^%\"");
    }

    #[test]
    fn does_not_launch_codex_with_the_orca_profile() {
        let plan = startup(&startup_args("codex", "fix it", "linux"));
        assert_eq!(plan.launch_command, "codex 'fix it'");
        assert_eq!(plan.startup_command_delivery.as_deref(), Some("shell-ready"));
    }

    #[test]
    fn launches_claude_without_orca_settings_injection() {
        let plan = startup(&startup_args("claude", "fix it", "linux"));
        assert_eq!(plan.launch_command, "claude 'fix it'");
        assert!(!plan.launch_command.contains("--settings"));
    }

    #[test]
    fn launches_openclaude_as_a_distinct_argv_agent() {
        let plan = startup(&startup_args("openclaude", "fix it", "linux"));
        assert_eq!(
            plan,
            AgentStartupPlan {
                agent: "openclaude".to_string(),
                launch_command: "openclaude 'fix it'".to_string(),
                expected_process: "openclaude".to_string(),
                followup_prompt: None,
                launch_config: SleepingAgentLaunchConfig {
                    agent_command: Some("openclaude".to_string()),
                    agent_args: String::new(),
                    agent_env: Vec::new(),
                },
                env: None,
                startup_command_delivery: None,
            }
        );
    }

    #[test]
    fn launches_mistral_vibe_through_the_installed_vibe_executable() {
        let plan = startup(&startup_args("mistral-vibe", "fix it", "linux"));
        assert_eq!(
            plan,
            AgentStartupPlan {
                agent: "mistral-vibe".to_string(),
                launch_command: "vibe".to_string(),
                expected_process: "vibe".to_string(),
                followup_prompt: Some("fix it".to_string()),
                launch_config: SleepingAgentLaunchConfig {
                    agent_command: Some("vibe".to_string()),
                    agent_args: String::new(),
                    agent_env: Vec::new(),
                },
                env: None,
                startup_command_delivery: None,
            }
        );
    }

    #[test]
    fn leaves_claude_command_overrides_untouched() {
        let plan = startup(&AgentStartupPlanArgs {
            cmd_overrides: &[("claude", "claude --dangerously-skip-permissions")],
            ..startup_args("claude", "fix it", "linux")
        });
        assert_eq!(
            plan.launch_command,
            "claude --dangerously-skip-permissions 'fix it'"
        );
    }

    #[test]
    fn leaves_codex_command_overrides_untouched() {
        let plan = startup(&AgentStartupPlanArgs {
            cmd_overrides: &[("codex", "codex --profile work")],
            ..startup_args("codex", "fix it", "linux")
        });
        assert_eq!(plan.launch_command, "codex --profile work 'fix it'");
    }

    #[test]
    fn appends_the_quoted_agent_args_suffix_to_the_base_command() {
        let plan = startup(&AgentStartupPlanArgs {
            agent_args: Some("--model opus \"two words\""),
            ..startup_args("claude", "fix it", "linux")
        });
        assert_eq!(
            plan.launch_command,
            "claude '--model' 'opus' 'two words' 'fix it'"
        );
        // The snapshot stores the resolved base (with suffix) + the raw args.
        assert_eq!(
            plan.launch_config.agent_command.as_deref(),
            Some("claude '--model' 'opus' 'two words'")
        );
        assert_eq!(plan.launch_config.agent_args, "--model opus \"two words\"");
    }

    #[test]
    fn fails_the_plan_when_agent_args_do_not_tokenize() {
        // Even an allowed empty-prompt launch fails: TS resolves the base
        // command (and validates agentArgs) before the empty-prompt check.
        assert_eq!(
            build_agent_startup_plan(&AgentStartupPlanArgs {
                allow_empty_prompt_launch: true,
                agent_args: Some("--flag 'unclosed"),
                ..startup_args("claude", "", "linux")
            }),
            None
        );
    }

    #[test]
    fn omits_codex_delivery_for_an_allowed_empty_prompt_launch() {
        let plan = startup(&AgentStartupPlanArgs {
            allow_empty_prompt_launch: true,
            ..startup_args("codex", "   ", "linux")
        });
        assert_eq!(plan.launch_command, "codex");
        // The TS empty-prompt branch never spreads startupCommandDelivery in.
        assert_eq!(plan.startup_command_delivery, None);
    }

    #[test]
    fn threads_agent_env_into_the_plan_and_the_snapshot() {
        let env = [("ORCA_PANE".to_string(), "p1".to_string())];
        let plan = startup(&AgentStartupPlanArgs {
            agent_env: Some(&env),
            ..startup_args("claude", "fix it", "linux")
        });
        assert_eq!(plan.env.as_deref(), Some(&env[..]));
        assert_eq!(plan.launch_config.agent_env, env.to_vec());

        // An empty env object is truthy in TS, so the plan still carries `env`.
        let empty: [(String, String); 0] = [];
        let plan = startup(&AgentStartupPlanArgs {
            agent_env: Some(&empty),
            ..startup_args("claude", "fix it", "linux")
        });
        assert_eq!(plan.env, Some(Vec::new()));
    }

    #[test]
    fn is_remote_is_a_no_op_for_agents_without_a_by_platform_override() {
        // claude has no launchCmdByPlatform, so the remote SSH-shim guard leaves
        // its command unchanged.
        let local = startup(&startup_args("claude", "fix it", "linux"));
        let remote = startup(&AgentStartupPlanArgs {
            is_remote: true,
            ..startup_args("claude", "fix it", "linux")
        });
        assert_eq!(local.launch_command, remote.launch_command);
    }

    #[test]
    fn applies_the_by_platform_launch_rename_except_over_ssh() {
        // claude-agent-teams is the only agent with launchCmdByPlatform: the
        // local `orca-ide`/`orca.cmd` rename applies, but an SSH remote on Linux
        // must fall back to the plain `orca` shim.
        let linux_local = startup(&startup_args("claude-agent-teams", "fix it", "linux"));
        assert_eq!(linux_local.launch_command, "orca-ide claude-teams");

        let win32 = startup(&startup_args("claude-agent-teams", "fix it", "win32"));
        assert_eq!(win32.launch_command, "orca.cmd claude-teams");

        let linux_remote = startup(&AgentStartupPlanArgs {
            is_remote: true,
            ..startup_args("claude-agent-teams", "fix it", "linux")
        });
        assert_eq!(linux_remote.launch_command, "orca claude-teams");
        // stdin-after-start agents carry the prompt as a followup, not on argv.
        assert_eq!(linux_remote.followup_prompt.as_deref(), Some("fix it"));
    }

    fn resume_args<'a>(agent: &'a str, id: &'a str) -> AgentResumeStartupPlanArgs<'a> {
        AgentResumeStartupPlanArgs {
            agent,
            provider_session_key: ProviderSessionKey::SessionId,
            provider_session_id: id,
            cmd_overrides: &[],
            platform: "linux",
            shell: None,
            agent_args: None,
            agent_env: None,
            agent_command: None,
            is_remote: false,
        }
    }

    #[test]
    fn builds_resume_commands_quoted_per_shell() {
        let posix = build_agent_resume_startup_plan(&resume_args("codex", "sess'1")).unwrap();
        assert_eq!(posix.launch_command, "codex 'resume' 'sess'\\''1'");

        let powershell = build_agent_resume_startup_plan(&AgentResumeStartupPlanArgs {
            shell: Some(AgentStartupShell::Powershell),
            ..resume_args("codex", "sess'1")
        })
        .unwrap();
        assert_eq!(powershell.launch_command, "codex 'resume' 'sess''1'");

        let cmd = build_agent_resume_startup_plan(&AgentResumeStartupPlanArgs {
            shell: Some(AgentStartupShell::Cmd),
            ..resume_args("codex", "sess'1")
        })
        .unwrap();
        assert_eq!(cmd.launch_command, "codex \"resume\" \"sess'1\"");
    }

    #[test]
    fn builds_a_full_resume_plan_without_codex_delivery() {
        let plan = build_agent_resume_startup_plan(&resume_args("claude", "abc-123")).unwrap();
        assert_eq!(
            plan,
            AgentStartupPlan {
                agent: "claude".to_string(),
                launch_command: "claude '--resume' 'abc-123'".to_string(),
                expected_process: "claude".to_string(),
                followup_prompt: None,
                launch_config: SleepingAgentLaunchConfig {
                    agent_command: Some("claude".to_string()),
                    agent_args: String::new(),
                    agent_env: Vec::new(),
                },
                env: None,
                startup_command_delivery: None,
            }
        );
        // The TS resume plan never sets startupCommandDelivery, even for codex.
        let codex = build_agent_resume_startup_plan(&resume_args("codex", "s1")).unwrap();
        assert_eq!(codex.startup_command_delivery, None);
    }

    #[test]
    fn resume_appends_agent_args_before_the_resume_argv_tail() {
        let plan = build_agent_resume_startup_plan(&AgentResumeStartupPlanArgs {
            agent_args: Some("--model opus"),
            ..resume_args("claude", "s1")
        })
        .unwrap();
        assert_eq!(plan.launch_command, "claude '--model' 'opus' '--resume' 's1'");
        assert_eq!(plan.launch_config.agent_args, "--model opus");
    }

    #[test]
    fn resume_agent_command_override_bypasses_resolution_and_args_validation() {
        let plan = build_agent_resume_startup_plan(&AgentResumeStartupPlanArgs {
            // Both would change the resolved base — the override wins over both.
            cmd_overrides: &[("claude", "claude --continue")],
            agent_command: Some("  /opt/claude --stable  "),
            // Invalid agentArgs do not fail the plan: planAgentCliArgsSuffix
            // never runs on the override path in the TS.
            agent_args: Some("--flag 'unclosed"),
            ..resume_args("claude", "s1")
        })
        .unwrap();
        assert_eq!(plan.launch_command, "/opt/claude --stable '--resume' 's1'");
        // The snapshot stores the trimmed override + the raw (invalid) args.
        assert_eq!(plan.launch_config.agent_command.as_deref(), Some("/opt/claude --stable"));
        assert_eq!(plan.launch_config.agent_args, "--flag 'unclosed");
    }

    #[test]
    fn resume_without_override_fails_on_invalid_agent_args() {
        assert_eq!(
            build_agent_resume_startup_plan(&AgentResumeStartupPlanArgs {
                agent_args: Some("--flag 'unclosed"),
                ..resume_args("claude", "s1")
            }),
            None
        );
        // A whitespace-only override is falsy after trim and falls through to
        // resolution, so the invalid args still fail the plan.
        assert_eq!(
            build_agent_resume_startup_plan(&AgentResumeStartupPlanArgs {
                agent_command: Some("   "),
                agent_args: Some("--flag 'unclosed"),
                ..resume_args("claude", "s1")
            }),
            None
        );
    }

    #[test]
    fn resume_threads_agent_env_into_the_plan_and_the_snapshot() {
        let env = [("ORCA_PANE".to_string(), "p1".to_string())];
        let plan = build_agent_resume_startup_plan(&AgentResumeStartupPlanArgs {
            agent_env: Some(&env),
            ..resume_args("claude", "s1")
        })
        .unwrap();
        assert_eq!(plan.env.as_deref(), Some(&env[..]));
        assert_eq!(plan.launch_config.agent_env, env.to_vec());
    }

    #[test]
    fn resume_returns_none_for_non_resumable_agents_or_wrong_key() {
        assert_eq!(build_agent_resume_startup_plan(&resume_args("pi", "s1")), None);
        assert_eq!(
            build_agent_resume_startup_plan(&AgentResumeStartupPlanArgs {
                provider_session_key: ProviderSessionKey::ConversationId,
                ..resume_args("claude", "s1")
            }),
            None
        );
    }

    fn draft_args<'a>(agent: &'a str, draft: &'a str, platform: &'a str) -> AgentDraftLaunchArgs<'a> {
        AgentDraftLaunchArgs {
            agent,
            draft,
            cmd_overrides: &[],
            platform,
            shell: None,
            agent_args: None,
            agent_env: None,
            is_remote: false,
        }
    }

    #[test]
    fn clears_draft_environment_variables_with_the_target_shell_syntax() {
        assert_eq!(
            build_agent_draft_launch_plan(&draft_args(
                "pi",
                "https://github.com/acme/repo/issues/42",
                "win32"
            ))
            .unwrap()
            .launch_command,
            "pi; Remove-Item Env:ORCA_PI_PREFILL -ErrorAction SilentlyContinue"
        );

        assert_eq!(
            build_agent_draft_launch_plan(&AgentDraftLaunchArgs {
                shell: Some(AgentStartupShell::Cmd),
                ..draft_args("pi", "https://github.com/acme/repo/issues/42", "win32")
            })
            .unwrap()
            .launch_command,
            "pi & set \"ORCA_PI_PREFILL=\""
        );
    }

    #[test]
    fn returns_an_omp_draft_plan_with_omp_scoped_prefill() {
        let plan = build_agent_draft_launch_plan(&draft_args("omp", "fix the omp regression", "linux"))
            .unwrap();

        assert_eq!(
            plan.env,
            Some(vec![(
                "ORCA_OMP_PREFILL".to_string(),
                "fix the omp regression".to_string()
            )])
        );
        assert_eq!(plan.expected_process, "omp");
        assert_eq!(plan.launch_command, "omp; unset ORCA_OMP_PREFILL");
    }

    #[test]
    fn merges_agent_env_before_the_draft_prefill_var() {
        let env = [("ORCA_PANE".to_string(), "p1".to_string())];
        let plan = build_agent_draft_launch_plan(&AgentDraftLaunchArgs {
            agent_env: Some(&env),
            ..draft_args("pi", "seed text", "linux")
        })
        .unwrap();
        assert_eq!(
            plan.env,
            Some(vec![
                ("ORCA_PANE".to_string(), "p1".to_string()),
                ("ORCA_PI_PREFILL".to_string(), "seed text".to_string()),
            ])
        );
        // Snapshot env stays the raw caller env (no prefill var).
        assert_eq!(plan.launch_config.agent_env, env.to_vec());

        // A caller-supplied prefill key keeps its position, new value (JS spread).
        let stale = [
            ("ORCA_PI_PREFILL".to_string(), "stale".to_string()),
            ("ORCA_PANE".to_string(), "p1".to_string()),
        ];
        let plan = build_agent_draft_launch_plan(&AgentDraftLaunchArgs {
            agent_env: Some(&stale),
            ..draft_args("pi", "seed text", "linux")
        })
        .unwrap();
        assert_eq!(
            plan.env,
            Some(vec![
                ("ORCA_PI_PREFILL".to_string(), "seed text".to_string()),
                ("ORCA_PANE".to_string(), "p1".to_string()),
            ])
        );
    }

    #[test]
    fn threads_agent_env_through_the_flag_draft_path() {
        let env = [("ORCA_PANE".to_string(), "p1".to_string())];
        let plan = build_agent_draft_launch_plan(&AgentDraftLaunchArgs {
            agent_env: Some(&env),
            agent_args: Some("--model opus"),
            ..draft_args("claude", "seed text", "linux")
        })
        .unwrap();
        assert_eq!(plan.launch_command, "claude '--model' 'opus' --prefill 'seed text'");
        assert_eq!(plan.env.as_deref(), Some(&env[..]));

        // No caller env -> the flag path has no env key at all (TS spread guard).
        let plan = build_agent_draft_launch_plan(&draft_args("claude", "seed text", "linux")).unwrap();
        assert_eq!(plan.env, None);
    }

    #[test]
    fn rejects_oversized_inline_drafts_on_win32_only() {
        let big = "x".repeat(24_001);
        assert_eq!(build_agent_draft_launch_plan(&draft_args("claude", &big, "win32")), None);
        assert!(build_agent_draft_launch_plan(&draft_args("claude", &big, "linux")).is_some());
        // Env-var drafts count env chars against the same budget.
        assert_eq!(build_agent_draft_launch_plan(&draft_args("pi", &big, "win32")), None);
        assert!(build_agent_draft_launch_plan(&draft_args("pi", &big, "linux")).is_some());
    }
}
