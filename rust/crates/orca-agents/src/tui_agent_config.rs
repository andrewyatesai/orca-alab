//! Per-agent TUI configuration table, ported from
//! `src/shared/tui-agent-config.ts`.
//!
//! Centralizes the new-workspace handoff knowledge that the picker, launcher,
//! and preflight checks all read: how Orca detects the agent on PATH, which
//! binary it launches, the expected process name, and how the initial prompt
//! is delivered (argv flag/argument vs. typed into the session after startup).
//!
//! Agents are keyed by their string id (the `TuiAgent` catalog). [`is_tui_agent`]
//! is the membership check; [`tui_agent_config`] is the lookup used by
//! `tui_agent_startup` and `terminal_quick_commands`.

/// How the initial prompt is injected when launching an agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentPromptInjectionMode {
    /// Passed as a positional argv argument.
    Argv,
    /// Passed via a `--prompt <text>` flag.
    FlagPrompt,
    /// Passed via a `--prompt-interactive <text>` flag.
    FlagPromptInteractive,
    /// Passed via a `-i <text>` flag (interactive session, prompt pre-executed).
    FlagInteractive,
    /// Typed into the interactive session after the TUI starts.
    StdinAfterStart,
}

/// Stronger-than-quiet-timer paste-readiness signal a TUI can expose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DraftPasteReadySignal {
    RenderQuietAfterBracketedPaste,
    RenderCursorAfterBracketedPaste,
    CodexComposerPrompt,
}

/// First-launch trust artifact preset to pre-write so the agent's "do you
/// trust this folder?" menu never consumes the bracketed paste.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreflightTrust {
    Cursor,
    Copilot,
    Codex,
}

/// Per-agent launch/detect/identify metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TuiAgentConfig {
    pub detect_cmd: &'static str,
    /// Additional executable names that identify the same agent on PATH.
    pub detect_cmd_aliases: &'static [&'static str],
    pub launch_cmd: &'static str,
    /// Per-platform launch overrides (TS `launchCmdByPlatform`); empty when the
    /// agent uses `launch_cmd` on every platform. Consulted unless an SSH remote
    /// on Linux forces the plain shim. Only `claude-agent-teams` sets it.
    pub launch_cmd_by_platform: &'static [(&'static str, &'static str)],
    pub expected_process: &'static str,
    pub prompt_injection_mode: AgentPromptInjectionMode,
    /// Separator inserted before the argv prompt (Grok's `--`) so a flag- or
    /// subcommand-shaped prompt is treated as literal text, not CLI syntax.
    pub argv_prompt_separator: Option<&'static str>,
    /// Flag that launches the TUI with the given text already in the input box
    /// but NOT submitted (e.g. Claude's `--prefill <text>`).
    pub draft_prompt_flag: Option<&'static str>,
    /// Env var read on startup to seed the input box without submitting (pi/omp).
    pub draft_prompt_env_var: Option<&'static str>,
    /// First-launch trust preset to pre-write before the agent spawns.
    pub preflight_trust: Option<PreflightTrust>,
    /// Stronger paste-readiness signal than the generic quiet-render window.
    pub draft_paste_ready_signal: Option<DraftPasteReadySignal>,
}

use AgentPromptInjectionMode::{
    Argv, FlagInteractive, FlagPrompt, FlagPromptInteractive, StdinAfterStart,
};

/// Build a config with no draft/trust/signal extras — the common case.
const fn agent(
    detect_cmd: &'static str,
    launch_cmd: &'static str,
    expected_process: &'static str,
    prompt_injection_mode: AgentPromptInjectionMode,
) -> TuiAgentConfig {
    TuiAgentConfig {
        detect_cmd,
        detect_cmd_aliases: &[],
        launch_cmd,
        launch_cmd_by_platform: &[],
        expected_process,
        prompt_injection_mode,
        argv_prompt_separator: None,
        draft_prompt_flag: None,
        draft_prompt_env_var: None,
        preflight_trust: None,
        draft_paste_ready_signal: None,
    }
}

/// The per-agent config table (the `TuiAgent` catalog).
///
/// Order mirrors the TS object literal; lookup is by id so order is not
/// semantically significant.
pub const TUI_AGENT_CONFIG: &[(&str, TuiAgentConfig)] = &[
    (
        "claude",
        TuiAgentConfig {
            detect_cmd: "claude",
            detect_cmd_aliases: &[],
            launch_cmd: "claude",
            launch_cmd_by_platform: &[],
            expected_process: "claude",
            prompt_injection_mode: Argv,
            argv_prompt_separator: None,
            // `claude --prefill <text>` lands the TUI with `<text>` in the
            // input box, nothing submitted — strictly better than the
            // paste-after-ready fallback.
            draft_prompt_flag: Some("--prefill"),
            draft_prompt_env_var: None,
            preflight_trust: None,
            draft_paste_ready_signal: None,
        },
    ),
    (
        "openclaude",
        TuiAgentConfig {
            detect_cmd: "openclaude",
            detect_cmd_aliases: &[],
            launch_cmd: "openclaude",
            launch_cmd_by_platform: &[],
            expected_process: "openclaude",
            prompt_injection_mode: Argv,
            argv_prompt_separator: None,
            draft_prompt_flag: Some("--prefill"),
            draft_prompt_env_var: None,
            preflight_trust: None,
            draft_paste_ready_signal: None,
        },
    ),
    (
        "codex",
        TuiAgentConfig {
            detect_cmd: "codex",
            detect_cmd_aliases: &[],
            launch_cmd: "codex",
            launch_cmd_by_platform: &[],
            expected_process: "codex",
            prompt_injection_mode: Argv,
            argv_prompt_separator: None,
            draft_prompt_flag: None,
            draft_prompt_env_var: None,
            // Codex's positional prompt auto-submits the first turn, so Orca
            // still pastes a draft once `chat_composer.rs` emits `›`.
            preflight_trust: Some(PreflightTrust::Codex),
            draft_paste_ready_signal: Some(DraftPasteReadySignal::CodexComposerPrompt),
        },
    ),
    ("autohand", agent("autohand", "autohand", "autohand", StdinAfterStart)),
    (
        "opencode",
        TuiAgentConfig {
            detect_cmd: "opencode",
            detect_cmd_aliases: &[],
            launch_cmd: "opencode",
            launch_cmd_by_platform: &[],
            expected_process: "opencode",
            prompt_injection_mode: FlagPrompt,
            argv_prompt_separator: None,
            draft_prompt_flag: None,
            draft_prompt_env_var: None,
            preflight_trust: None,
            // opencode's flag-prompt paste route is cursor-gated (mimo-code shares it).
            draft_paste_ready_signal: Some(DraftPasteReadySignal::RenderCursorAfterBracketedPaste),
        },
    ),
    (
        "pi",
        TuiAgentConfig {
            detect_cmd: "pi",
            detect_cmd_aliases: &[],
            launch_cmd: "pi",
            launch_cmd_by_platform: &[],
            expected_process: "pi",
            prompt_injection_mode: Argv,
            argv_prompt_separator: None,
            draft_prompt_flag: None,
            // pi has no `--prefill`; Orca's overlay `orca-prefill` extension
            // reads this env var on session_start.
            draft_prompt_env_var: Some("ORCA_PI_PREFILL"),
            preflight_trust: None,
            draft_paste_ready_signal: None,
        },
    ),
    (
        "omp",
        TuiAgentConfig {
            detect_cmd: "omp",
            detect_cmd_aliases: &[],
            launch_cmd: "omp",
            launch_cmd_by_platform: &[],
            expected_process: "omp",
            prompt_injection_mode: Argv,
            argv_prompt_separator: None,
            draft_prompt_flag: None,
            // OMP is a Pi fork with its own binary/overlay/prefill env var.
            draft_prompt_env_var: Some("ORCA_OMP_PREFILL"),
            preflight_trust: None,
            draft_paste_ready_signal: None,
        },
    ),
    ("gemini", agent("gemini", "gemini", "gemini", FlagPromptInteractive)),
    ("antigravity", agent("agy", "agy", "agy", FlagPromptInteractive)),
    ("aider", agent("aider", "aider", "aider", StdinAfterStart)),
    ("goose", agent("goose", "goose", "goose", StdinAfterStart)),
    ("amp", agent("amp", "amp", "amp", StdinAfterStart)),
    ("kilo", agent("kilo", "kilo", "kilo", StdinAfterStart)),
    // The official Kiro installer places `kiro-cli` on PATH — there is no
    // `kiro` binary — but the stored id stays `kiro`. Trust flags are accepted by
    // the `chat` subcommand, so TUI startup is explicit (`chat --tui`) to keep
    // default args like --trust-all-tools where the installed CLI accepts them.
    (
        "kiro",
        TuiAgentConfig {
            detect_cmd: "kiro-cli",
            detect_cmd_aliases: &[],
            launch_cmd: "kiro-cli chat --tui",
            launch_cmd_by_platform: &[],
            expected_process: "kiro-cli",
            prompt_injection_mode: StdinAfterStart,
            argv_prompt_separator: None,
            draft_prompt_flag: None,
            draft_prompt_env_var: None,
            preflight_trust: None,
            draft_paste_ready_signal: None,
        },
    ),
    ("crush", agent("crush", "crush", "crush", StdinAfterStart)),
    // @augmentcode/auggie installs a binary named `auggie` (not `aug`).
    ("aug", agent("auggie", "auggie", "auggie", StdinAfterStart)),
    ("cline", agent("cline", "cline", "cline", StdinAfterStart)),
    ("codebuff", agent("codebuff", "codebuff", "codebuff", StdinAfterStart)),
    // `command-code` (not the `cmd` alias) avoids colliding with Windows
    // `cmd.exe`; `--trust` mirrors Orca's preflight trust for first-run TUIs.
    (
        "command-code",
        agent("command-code", "command-code --trust", "command-code", Argv),
    ),
    // Continue's CLI binary is `cn`; `continue` is a shell builtin, so launching
    // by that name can resolve to the keyword instead of the agent.
    ("continue", agent("cn", "cn", "cn", StdinAfterStart)),
    (
        "cursor",
        TuiAgentConfig {
            detect_cmd: "cursor-agent",
            detect_cmd_aliases: &[],
            launch_cmd: "cursor-agent",
            launch_cmd_by_platform: &[],
            expected_process: "cursor-agent",
            prompt_injection_mode: Argv,
            argv_prompt_separator: None,
            draft_prompt_flag: None,
            draft_prompt_env_var: None,
            // Pre-writing the `.workspace-trusted` marker skips cursor-agent's
            // first-launch trust menu so the draft paste lands.
            preflight_trust: Some(PreflightTrust::Cursor),
            draft_paste_ready_signal: None,
        },
    ),
    ("droid", agent("droid", "droid", "droid", Argv)),
    ("kimi", agent("kimi", "kimi", "kimi", StdinAfterStart)),
    (
        "mistral-vibe",
        TuiAgentConfig {
            detect_cmd: "vibe",
            // Mistral's installer exposes `vibe`; keep the old name as an alias.
            detect_cmd_aliases: &["mistral-vibe"],
            launch_cmd: "vibe",
            launch_cmd_by_platform: &[],
            expected_process: "vibe",
            prompt_injection_mode: StdinAfterStart,
            argv_prompt_separator: None,
            draft_prompt_flag: None,
            draft_prompt_env_var: None,
            preflight_trust: None,
            draft_paste_ready_signal: None,
        },
    ),
    // The upstream package is QwenLM/qwen-code but its installed binary is `qwen`.
    ("qwen-code", agent("qwen", "qwen", "qwen", StdinAfterStart)),
    ("rovo", agent("rovo", "rovo", "rovo", StdinAfterStart)),
    // Bare `hermes` opens the classic REPL; `--tui` starts the agent UI.
    ("hermes", agent("hermes", "hermes --tui", "hermes", StdinAfterStart)),
    ("openclaw", agent("openclaw", "openclaw", "openclaw", StdinAfterStart)),
    (
        "copilot",
        TuiAgentConfig {
            detect_cmd: "copilot",
            detect_cmd_aliases: &[],
            launch_cmd: "copilot",
            launch_cmd_by_platform: &[],
            expected_process: "copilot",
            // `copilot --prompt` runs non-interactively and exits; `-i` starts
            // an interactive session with the prompt pre-executed.
            prompt_injection_mode: FlagInteractive,
            argv_prompt_separator: None,
            draft_prompt_flag: None,
            draft_prompt_env_var: None,
            preflight_trust: Some(PreflightTrust::Copilot),
            draft_paste_ready_signal: None,
        },
    ),
    // Grok takes the initial prompt as a positional argv, with a `--` separator
    // so a prompt like `help`/`--version` is literal text, not Grok CLI syntax.
    (
        "grok",
        TuiAgentConfig {
            argv_prompt_separator: Some("--"),
            ..agent("grok", "grok", "grok", Argv)
        },
    ),
    (
        "claude-agent-teams",
        TuiAgentConfig {
            // Why: an Orca-provided launch mode, not a separate upstream binary —
            // detection follows the Orca CLI; the wrapper validates the real
            // Claude binary at startup.
            detect_cmd: "orca",
            detect_cmd_aliases: &["orca-dev", "orca-ide"],
            launch_cmd: "orca claude-teams",
            // The local `orca-ide` (linux) / `orca.cmd` (win32) rename shim, baked
            // from getOrcaCliCommandNameForPlatform; skipped for SSH remotes so the
            // plain `orca claude-teams` runs remotely.
            launch_cmd_by_platform: &[
                ("linux", "orca-ide claude-teams"),
                ("win32", "orca.cmd claude-teams"),
            ],
            expected_process: "claude",
            prompt_injection_mode: StdinAfterStart,
            argv_prompt_separator: None,
            draft_prompt_flag: None,
            draft_prompt_env_var: None,
            preflight_trust: None,
            draft_paste_ready_signal: None,
        },
    ),
    // Ante's `--prompt` is headless (runs once and exits), so Orca launches the
    // bare TUI and injects the prompt after startup.
    ("ante", agent("ante", "ante", "ante", StdinAfterStart)),
    // `devin -- <prompt>` auto-submits, so launch bare and send the prompt to the
    // PTY after startup.
    ("devin", agent("devin", "devin", "devin", StdinAfterStart)),
    (
        "mimo-code",
        TuiAgentConfig {
            detect_cmd: "mimo",
            detect_cmd_aliases: &[],
            launch_cmd: "mimo",
            launch_cmd_by_platform: &[],
            expected_process: "mimo",
            prompt_injection_mode: FlagPrompt,
            argv_prompt_separator: None,
            draft_prompt_flag: None,
            draft_prompt_env_var: None,
            preflight_trust: None,
            // mimo-code shares opencode's flag-prompt paste route (cursor-gated).
            draft_paste_ready_signal: Some(DraftPasteReadySignal::RenderCursorAfterBracketedPaste),
        },
    ),
];

/// Look up the config for an agent id, or `None` when the id is unknown.
pub fn tui_agent_config(agent: &str) -> Option<&'static TuiAgentConfig> {
    TUI_AGENT_CONFIG
        .iter()
        .find(|(id, _)| *id == agent)
        .map(|(_, config)| config)
}

/// True when `value` is a known `TuiAgent` id (present in [`TUI_AGENT_CONFIG`]).
pub fn is_tui_agent(value: &str) -> bool {
    tui_agent_config(value).is_some()
}

/// Detect command plus any aliases that identify the same agent on PATH.
// Trust contract: inert under stock cargo, proved under `--cfg trust_verify`.
// Postcondition — the result is exactly the primary command plus every alias.
#[cfg_attr(trust_verify, trust::ensures(|out: &Vec<&str>| out.len() == 1 + config.detect_cmd_aliases.len()))]
pub fn get_tui_agent_detect_commands(config: &TuiAgentConfig) -> Vec<&str> {
    let mut commands = Vec::with_capacity(1 + config.detect_cmd_aliases.len());
    commands.push(config.detect_cmd);
    commands.extend_from_slice(config.detect_cmd_aliases);
    commands
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_every_catalog_agent_and_rejects_unknowns() {
        assert!(is_tui_agent("claude"));
        assert!(is_tui_agent("mistral-vibe"));
        assert!(is_tui_agent("command-code"));
        assert!(!is_tui_agent("not-real"));
        assert!(!is_tui_agent(""));
        // The full TuiAgent union has 34 members.
        assert_eq!(TUI_AGENT_CONFIG.len(), 34);
    }

    #[test]
    fn looks_up_renamed_binaries_for_detect_launch_identify() {
        let kiro = tui_agent_config("kiro").unwrap();
        assert_eq!(kiro.detect_cmd, "kiro-cli");
        // Trust flags attach to the `chat` subcommand, so launch is explicit.
        assert_eq!(kiro.launch_cmd, "kiro-cli chat --tui");
        assert_eq!(kiro.expected_process, "kiro-cli");

        let continue_cli = tui_agent_config("continue").unwrap();
        assert_eq!(continue_cli.launch_cmd, "cn");

        let qwen = tui_agent_config("qwen-code").unwrap();
        assert_eq!(qwen.launch_cmd, "qwen");
        assert_eq!(qwen.expected_process, "qwen");

        let aug = tui_agent_config("aug").unwrap();
        assert_eq!(aug.detect_cmd, "auggie");

        let command_code = tui_agent_config("command-code").unwrap();
        assert_eq!(command_code.launch_cmd, "command-code --trust");
        assert_eq!(command_code.expected_process, "command-code");
    }

    #[test]
    fn returns_detect_command_plus_aliases() {
        let vibe = tui_agent_config("mistral-vibe").unwrap();
        assert_eq!(get_tui_agent_detect_commands(vibe), vec!["vibe", "mistral-vibe"]);

        let claude = tui_agent_config("claude").unwrap();
        assert_eq!(get_tui_agent_detect_commands(claude), vec!["claude"]);
    }

    #[test]
    fn carries_prompt_injection_and_draft_metadata() {
        assert_eq!(
            tui_agent_config("claude").unwrap().draft_prompt_flag,
            Some("--prefill")
        );
        assert_eq!(
            tui_agent_config("pi").unwrap().draft_prompt_env_var,
            Some("ORCA_PI_PREFILL")
        );
        assert_eq!(
            tui_agent_config("codex").unwrap().draft_paste_ready_signal,
            Some(DraftPasteReadySignal::CodexComposerPrompt)
        );
        // opencode and mimo-code share the cursor-gated flag-prompt paste route.
        assert_eq!(
            tui_agent_config("opencode").unwrap().draft_paste_ready_signal,
            Some(DraftPasteReadySignal::RenderCursorAfterBracketedPaste)
        );
        assert_eq!(
            tui_agent_config("mimo-code").unwrap().draft_paste_ready_signal,
            Some(DraftPasteReadySignal::RenderCursorAfterBracketedPaste)
        );
        assert_eq!(
            tui_agent_config("aider").unwrap().prompt_injection_mode,
            AgentPromptInjectionMode::StdinAfterStart
        );
    }
}
