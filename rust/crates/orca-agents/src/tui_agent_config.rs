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
    pub expected_process: &'static str,
    pub prompt_injection_mode: AgentPromptInjectionMode,
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
        expected_process,
        prompt_injection_mode,
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
            expected_process: "claude",
            prompt_injection_mode: Argv,
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
            expected_process: "openclaude",
            prompt_injection_mode: Argv,
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
            expected_process: "codex",
            prompt_injection_mode: Argv,
            draft_prompt_flag: None,
            draft_prompt_env_var: None,
            // Codex's positional prompt auto-submits the first turn, so Orca
            // still pastes a draft once `chat_composer.rs` emits `›`.
            preflight_trust: Some(PreflightTrust::Codex),
            draft_paste_ready_signal: Some(DraftPasteReadySignal::CodexComposerPrompt),
        },
    ),
    ("autohand", agent("autohand", "autohand", "autohand", StdinAfterStart)),
    ("opencode", agent("opencode", "opencode", "opencode", FlagPrompt)),
    (
        "pi",
        TuiAgentConfig {
            detect_cmd: "pi",
            detect_cmd_aliases: &[],
            launch_cmd: "pi",
            expected_process: "pi",
            prompt_injection_mode: Argv,
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
            expected_process: "omp",
            prompt_injection_mode: Argv,
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
    // `kiro` binary — but the stored id stays `kiro`.
    ("kiro", agent("kiro-cli", "kiro-cli", "kiro-cli", StdinAfterStart)),
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
    ("continue", agent("continue", "continue", "continue", StdinAfterStart)),
    (
        "cursor",
        TuiAgentConfig {
            detect_cmd: "cursor-agent",
            detect_cmd_aliases: &[],
            launch_cmd: "cursor-agent",
            expected_process: "cursor-agent",
            prompt_injection_mode: Argv,
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
            expected_process: "vibe",
            prompt_injection_mode: StdinAfterStart,
            draft_prompt_flag: None,
            draft_prompt_env_var: None,
            preflight_trust: None,
            draft_paste_ready_signal: None,
        },
    ),
    ("qwen-code", agent("qwen-code", "qwen-code", "qwen-code", StdinAfterStart)),
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
            expected_process: "copilot",
            // `copilot --prompt` runs non-interactively and exits; `-i` starts
            // an interactive session with the prompt pre-executed.
            prompt_injection_mode: FlagInteractive,
            draft_prompt_flag: None,
            draft_prompt_env_var: None,
            preflight_trust: Some(PreflightTrust::Copilot),
            draft_paste_ready_signal: None,
        },
    ),
    ("grok", agent("grok", "grok", "grok", StdinAfterStart)),
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
        // The full TuiAgent union has 30 members.
        assert_eq!(TUI_AGENT_CONFIG.len(), 30);
    }

    #[test]
    fn looks_up_renamed_binaries_for_detect_launch_identify() {
        let kiro = tui_agent_config("kiro").unwrap();
        assert_eq!(kiro.detect_cmd, "kiro-cli");
        assert_eq!(kiro.launch_cmd, "kiro-cli");
        assert_eq!(kiro.expected_process, "kiro-cli");

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
        assert_eq!(
            tui_agent_config("aider").unwrap().prompt_injection_mode,
            AgentPromptInjectionMode::StdinAfterStart
        );
    }
}
