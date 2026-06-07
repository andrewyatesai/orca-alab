//! `orca-agents` — agent-CLI domain logic for Orca (provider specs, commit-
//! message generation, output cleanup). Pure logic over vendored `regex` +
//! `serde_json`; the actual process spawning lives in a higher IO tier.

pub mod agent_status_types;
pub mod commit_message_agent_spec;
pub mod commit_message_generation;
pub mod commit_message_models;
pub mod commit_message_plan;
pub mod commit_message_prompt;
pub mod pull_request_generation;
pub mod tui_agent_selection;

pub use commit_message_prompt::{
    build_commit_prompt, clean_generated_commit_message, extract_agent_error_message,
    plan_custom_command, tokenize_custom_command_template, truncate_diff_for_prompt,
    CustomCommandPlan, CUSTOM_PROMPT_PLACEHOLDER, STAGED_DIFF_BYTE_BUDGET,
};
pub use agent_status_types::{
    normalize_agent_status_payload, parse_agent_status_payload, AgentStatusState,
    ParsedAgentStatusPayload, AGENT_STATUS_STATES,
};
pub use commit_message_agent_spec::{
    get_commit_message_agent_capability, get_commit_message_agent_spec, get_commit_message_model,
    get_commit_message_model_capability, is_custom_agent_id, list_commit_message_agent_capabilities,
    list_commit_message_agent_ids, resolve_commit_message_agent_choice, BuildArgsParams,
    CommitMessageAgentCapability, CommitMessageAgentSpec, CommitMessageModelCapability, ModelSource,
    PromptDelivery, CUSTOM_AGENT_ID, DEFAULT_COMMIT_MESSAGE_AGENT_ID,
};
pub use commit_message_generation::{
    build_commit_message_prompt, split_generated_commit_message, CommitMessageDraftContext,
    GeneratedCommitMessage,
};
pub use commit_message_models::{
    parse_codex_models, parse_cursor_models, parse_line_models, parse_pi_models,
    CommitMessageModel, ThinkingLevel,
};
pub use commit_message_plan::{plan_commit_message_generation, CommitMessagePlan, CommitMessagePlanInput};
pub use pull_request_generation::{
    build_pull_request_fields_prompt, parse_generated_pull_request_fields,
    GeneratedPullRequestFields, PullRequestDraftContext,
};
pub use tui_agent_selection::{
    filter_enabled_tui_agents, is_tui_agent, is_tui_agent_enabled, normalize_disabled_tui_agents,
    pick_tui_agent, TUI_AGENT_AUTO_PICK_ORDER,
};


// --- ported user-story slice (workflow w8rbqzuzc) ---
pub mod tui_agent_config;
pub mod tui_agent_startup;
pub mod terminal_quick_commands;

pub use tui_agent_config::{
    get_tui_agent_detect_commands, tui_agent_config, AgentPromptInjectionMode,
    DraftPasteReadySignal, PreflightTrust, TuiAgentConfig, TUI_AGENT_CONFIG,
};
pub use tui_agent_startup::{
    build_agent_draft_launch_plan, build_agent_startup_plan, AgentDraftLaunchArgs,
    AgentDraftLaunchPlan, AgentStartupPlan, AgentStartupPlanArgs, AgentStartupShell,
};
pub use terminal_quick_commands::{
    build_terminal_quick_command_input, flatten_terminal_quick_command,
    get_default_terminal_quick_commands, get_terminal_quick_command_action,
    get_terminal_quick_command_body, get_terminal_quick_command_scope,
    is_terminal_agent_quick_command, is_terminal_quick_command_complete,
    normalize_terminal_quick_command_scope, normalize_terminal_quick_commands,
    supports_terminal_agent_quick_command, terminal_quick_command_matches_repo,
    TerminalAgentQuickCommand, TerminalCommandQuickCommand, TerminalQuickCommand,
    TerminalQuickCommandAction, TerminalQuickCommandScope,
};
