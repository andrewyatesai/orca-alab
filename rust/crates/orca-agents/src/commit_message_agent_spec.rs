//! Commit-message agent specs + lookups, ported from the spec half of
//! `src/shared/commit-message-agent-spec.ts`.
//!
//! Per-agent non-interactive spawn contract (binary, prompt delivery, argv
//! builder, model catalog, dynamic-model discovery) plus the resolution helpers
//! the Source Control AI feature uses. Composes `commit_message_models`
//! (parsers + label/thinking helpers) and `tui_agent_selection` (enablement).

use crate::commit_message_models::{
    label_from_model_id, openai_thinking_levels, parse_codex_models, parse_cursor_models,
    parse_line_models, parse_pi_models, with_openai_thinking, CommitMessageModel, ThinkingLevel,
};
use crate::tui_agent_selection::is_tui_agent_enabled;
use std::sync::OnceLock;

pub const DEFAULT_COMMIT_MESSAGE_AGENT_ID: &str = "claude";
/// The "custom" choice is not a `TuiAgent`; it points Orca at an arbitrary CLI.
pub const CUSTOM_AGENT_ID: &str = "custom";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptDelivery {
    Argv,
    Stdin,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelSource {
    Static,
    Dynamic,
}

pub struct BuildArgsParams<'a> {
    pub prompt: &'a str,
    pub model: &'a str,
    pub thinking_level: Option<&'a str>,
}

pub struct ModelDiscovery {
    pub binary: &'static str,
    pub args: &'static [&'static str],
    pub parse: fn(&str) -> Vec<CommitMessageModel>,
}

pub struct CommitMessageAgentSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub binary: &'static str,
    pub prompt_delivery: PromptDelivery,
    pub build_args: fn(&BuildArgsParams) -> Vec<String>,
    pub model_source: ModelSource,
    pub model_discovery: Option<ModelDiscovery>,
    pub models: Vec<CommitMessageModel>,
    pub default_model_id: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitMessageModelCapability {
    pub id: String,
    pub label: String,
    pub thinking_levels: Option<Vec<ThinkingLevel>>,
    pub default_thinking_level: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitMessageAgentCapability {
    pub id: String,
    pub label: String,
    pub model_source: ModelSource,
    pub default_model_id: String,
    pub models: Vec<CommitMessageModelCapability>,
}

fn model(id: &str, label: &str) -> CommitMessageModel {
    CommitMessageModel { id: id.to_string(), label: label.to_string(), thinking_levels: None, default_thinking_level: None }
}

fn model_with(id: &str, label: &str, levels: Vec<ThinkingLevel>, default: &str) -> CommitMessageModel {
    CommitMessageModel {
        id: id.to_string(),
        label: label.to_string(),
        thinking_levels: Some(levels),
        default_thinking_level: Some(default.to_string()),
    }
}

fn level(id: &str, label: &str) -> ThinkingLevel {
    ThinkingLevel { id: id.to_string(), label: label.to_string() }
}

fn basic_thinking_levels() -> Vec<ThinkingLevel> {
    vec![level("low", "Low"), level("medium", "Medium"), level("high", "High")]
}

fn claude_thinking_levels() -> Vec<ThinkingLevel> {
    vec![level("low", "Low"), level("medium", "Medium"), level("high", "High"), level("xhigh", "Extra High"), level("max", "Max")]
}

fn flag(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}

fn claude_build_args(params: &BuildArgsParams) -> Vec<String> {
    let mut args = flag(&["-p", "--output-format", "text", "--model"]);
    args.push(params.model.to_string());
    args.extend(flag(&["--permission-mode", "plan"]));
    if let Some(thinking) = params.thinking_level {
        args.extend([String::from("--effort"), thinking.to_string()]);
    }
    args
}

fn codex_build_args(params: &BuildArgsParams) -> Vec<String> {
    let mut args = flag(&["exec", "--ephemeral", "--skip-git-repo-check", "-s", "read-only", "--model"]);
    args.push(params.model.to_string());
    if let Some(thinking) = params.thinking_level {
        args.extend([String::from("-c"), format!("model_reasoning_effort={thinking}")]);
    }
    args
}

fn opencode_build_args(params: &BuildArgsParams) -> Vec<String> {
    let mut args = flag(&["run", "--model"]);
    args.push(params.model.to_string());
    args.extend(flag(&["--agent", "build", "--format", "default"]));
    if let Some(thinking) = params.thinking_level {
        args.extend([String::from("--variant"), thinking.to_string()]);
    }
    args.push(params.prompt.to_string());
    args
}

fn pi_build_args(params: &BuildArgsParams) -> Vec<String> {
    let mut args = flag(&[
        "--print", "--no-session", "--no-tools", "--no-extensions", "--no-skills", "--no-context-files", "--mode",
        "text", "--model",
    ]);
    args.push(params.model.to_string());
    if let Some(thinking) = params.thinking_level {
        args.extend([String::from("--thinking"), thinking.to_string()]);
    }
    args
}

fn amp_build_args(params: &BuildArgsParams) -> Vec<String> {
    let mut args = flag(&["--execute", "--archive", "--no-notifications", "--no-ide", "--no-jetbrains", "--mode"]);
    args.push(params.model.to_string());
    if let Some(thinking) = params.thinking_level {
        args.extend([String::from("--effort"), thinking.to_string()]);
    }
    args
}

fn cursor_build_args(params: &BuildArgsParams) -> Vec<String> {
    let mut args = flag(&["--print", "--mode", "ask", "--trust", "--output-format", "text", "--model"]);
    args.push(params.model.to_string());
    args.push(params.prompt.to_string());
    args
}

fn kimi_build_args(params: &BuildArgsParams) -> Vec<String> {
    let mut args = flag(&["--print", "--quiet"]);
    if !params.model.is_empty() && params.model != "default" {
        args.extend([String::from("--model"), params.model.to_string()]);
    }
    match params.thinking_level {
        Some("on") => args.push("--thinking".to_string()),
        Some("off") => args.push("--no-thinking".to_string()),
        _ => {}
    }
    args
}

fn copilot_build_args(params: &BuildArgsParams) -> Vec<String> {
    let mut args = vec![String::from("--prompt"), params.prompt.to_string()];
    args.extend(flag(&["--silent", "--stream", "off", "--no-custom-instructions", "--model"]));
    args.push(params.model.to_string());
    if let Some(thinking) = params.thinking_level {
        args.extend([String::from("--effort"), thinking.to_string()]);
    }
    args
}

fn openai_model(id: &str, label: &str) -> CommitMessageModel {
    model_with(id, label, openai_thinking_levels(), "low")
}

fn specs() -> &'static [CommitMessageAgentSpec] {
    static SPECS: OnceLock<Vec<CommitMessageAgentSpec>> = OnceLock::new();
    SPECS.get_or_init(|| {
        vec![
            CommitMessageAgentSpec {
                id: "claude",
                label: "Claude",
                binary: "claude",
                prompt_delivery: PromptDelivery::Stdin,
                build_args: claude_build_args,
                model_source: ModelSource::Static,
                model_discovery: None,
                models: vec![
                    model("haiku", "Haiku"),
                    model_with("sonnet", "Sonnet", claude_thinking_levels(), "low"),
                    model_with("opus", "Opus", claude_thinking_levels(), "low"),
                ],
                default_model_id: "sonnet",
            },
            CommitMessageAgentSpec {
                id: "codex",
                label: "Codex",
                binary: "codex",
                prompt_delivery: PromptDelivery::Stdin,
                build_args: codex_build_args,
                model_source: ModelSource::Dynamic,
                model_discovery: Some(ModelDiscovery { binary: "codex", args: &["debug", "models"], parse: parse_codex_models }),
                models: vec![
                    openai_model("gpt-5.5", "GPT-5.5"),
                    openai_model("gpt-5.4", "GPT-5.4"),
                    openai_model("gpt-5.4-mini", "GPT-5.4 Mini"),
                    openai_model("gpt-5.3-codex", "GPT-5.3 Codex"),
                    openai_model("gpt-5.3-codex-spark", "GPT-5.3 Codex Spark"),
                    openai_model("gpt-5.2", "GPT-5.2"),
                ],
                default_model_id: "gpt-5.5",
            },
            CommitMessageAgentSpec {
                id: "opencode",
                label: "OpenCode",
                binary: "opencode",
                prompt_delivery: PromptDelivery::Argv,
                build_args: opencode_build_args,
                model_source: ModelSource::Dynamic,
                model_discovery: Some(ModelDiscovery { binary: "opencode", args: &["models"], parse: parse_line_models }),
                models: vec![
                    model("opencode/deepseek-v4-flash-free", "OpenCode DeepSeek V4 Flash Free"),
                    openai_model("opencode/gpt-5.4-mini", "OpenCode GPT 5.4 Mini"),
                ],
                default_model_id: "opencode/deepseek-v4-flash-free",
            },
            CommitMessageAgentSpec {
                id: "pi",
                label: "Pi",
                binary: "pi",
                prompt_delivery: PromptDelivery::Stdin,
                build_args: pi_build_args,
                model_source: ModelSource::Dynamic,
                model_discovery: Some(ModelDiscovery { binary: "pi", args: &["--list-models"], parse: parse_pi_models }),
                models: vec![openai_model("github-copilot/gpt-5.4-mini", "Github Copilot GPT 5.4 Mini")],
                default_model_id: "github-copilot/gpt-5.4-mini",
            },
            CommitMessageAgentSpec {
                id: "amp",
                label: "Amp",
                binary: "amp",
                prompt_delivery: PromptDelivery::Stdin,
                build_args: amp_build_args,
                model_source: ModelSource::Static,
                model_discovery: None,
                models: vec![
                    model("smart", "Smart"),
                    model("rush", "Rush"),
                    model_with("large", "Large", basic_thinking_levels(), "low"),
                    model_with("deep", "Deep", basic_thinking_levels(), "low"),
                ],
                default_model_id: "smart",
            },
            CommitMessageAgentSpec {
                id: "cursor",
                label: "Cursor",
                binary: "cursor-agent",
                prompt_delivery: PromptDelivery::Argv,
                build_args: cursor_build_args,
                model_source: ModelSource::Dynamic,
                model_discovery: Some(ModelDiscovery { binary: "cursor-agent", args: &["--list-models"], parse: parse_cursor_models }),
                models: vec![model("auto", "Auto")],
                default_model_id: "auto",
            },
            CommitMessageAgentSpec {
                id: "kimi",
                label: "Kimi",
                binary: "kimi",
                prompt_delivery: PromptDelivery::Stdin,
                build_args: kimi_build_args,
                model_source: ModelSource::Static,
                model_discovery: None,
                models: vec![
                    model("default", "Config default"),
                    model_with("kimi-code/kimi-for-coding", "Kimi K2.6", vec![level("on", "On"), level("off", "Off")], "on"),
                ],
                default_model_id: "default",
            },
            CommitMessageAgentSpec {
                id: "copilot",
                label: "GitHub Copilot",
                binary: "copilot",
                prompt_delivery: PromptDelivery::Argv,
                build_args: copilot_build_args,
                model_source: ModelSource::Static,
                model_discovery: None,
                models: vec![
                    model("auto", "Auto"),
                    model("claude-haiku-4.5", "Claude Haiku 4.5"),
                    model("claude-sonnet-4.5", "Claude Sonnet 4.5"),
                    model("claude-sonnet-4.6", "Claude Sonnet 4.6"),
                    model("claude-opus-4.5", "Claude Opus 4.5"),
                    model("claude-opus-4.6", "Claude Opus 4.6"),
                    model("claude-opus-4.6-fast", "Claude Opus 4.6 Fast"),
                    model("claude-opus-4.7", "Claude Opus 4.7"),
                    model("gpt-4.1", "GPT-4.1"),
                    openai_model("gpt-5-mini", "GPT-5 Mini"),
                    openai_model("gpt-5.2", "GPT-5.2"),
                    openai_model("gpt-5.2-codex", "GPT-5.2 Codex"),
                    openai_model("gpt-5.3-codex", "GPT-5.3 Codex"),
                    openai_model("gpt-5.4", "GPT-5.4"),
                    openai_model("gpt-5.4-mini", "GPT-5.4 Mini"),
                    openai_model("gpt-5.5", "GPT-5.5"),
                ],
                default_model_id: "gpt-5.4",
            },
        ]
    })
}

pub fn is_custom_agent_id(id: Option<&str>) -> bool {
    id == Some(CUSTOM_AGENT_ID)
}

pub fn get_commit_message_agent_spec(agent_id: &str) -> Option<&'static CommitMessageAgentSpec> {
    specs().iter().find(|spec| spec.id == agent_id)
}

pub fn resolve_commit_message_agent_choice(
    configured_agent_id: Option<&str>,
    default_tui_agent: Option<&str>,
    disabled_tui_agents: &[&str],
) -> Option<String> {
    if let Some(configured) = configured_agent_id.filter(|id| !id.is_empty()) {
        return Some(configured.to_string());
    }
    if let Some(default) = default_tui_agent.filter(|agent| !agent.is_empty() && *agent != "blank") {
        if is_tui_agent_enabled(default, disabled_tui_agents) {
            return get_commit_message_agent_spec(default).map(|_| default.to_string());
        }
    }
    if is_tui_agent_enabled(DEFAULT_COMMIT_MESSAGE_AGENT_ID, disabled_tui_agents) {
        Some(DEFAULT_COMMIT_MESSAGE_AGENT_ID.to_string())
    } else {
        None
    }
}

pub fn get_commit_message_model(agent_id: &str, model_id: &str) -> Option<CommitMessageModel> {
    let spec = get_commit_message_agent_spec(agent_id)?;
    if let Some(model) = spec.models.iter().find(|model| model.id == model_id) {
        return Some(model.clone());
    }
    if spec.model_source != ModelSource::Dynamic || model_id.trim().is_empty() {
        return None;
    }
    let (thinking_levels, default_thinking_level) = with_openai_thinking(model_id);
    Some(CommitMessageModel { id: model_id.to_string(), label: label_from_model_id(model_id), thinking_levels, default_thinking_level })
}

fn to_capability(spec: &CommitMessageAgentSpec) -> CommitMessageAgentCapability {
    CommitMessageAgentCapability {
        id: spec.id.to_string(),
        label: spec.label.to_string(),
        model_source: spec.model_source,
        default_model_id: spec.default_model_id.to_string(),
        models: spec
            .models
            .iter()
            .map(|model| CommitMessageModelCapability {
                id: model.id.clone(),
                label: model.label.clone(),
                thinking_levels: model.thinking_levels.clone(),
                default_thinking_level: model.default_thinking_level.clone(),
            })
            .collect(),
    }
}

pub fn get_commit_message_agent_capability(agent_id: &str) -> Option<CommitMessageAgentCapability> {
    get_commit_message_agent_spec(agent_id).map(to_capability)
}

pub fn get_commit_message_model_capability(agent_id: &str, model_id: &str) -> Option<CommitMessageModelCapability> {
    get_commit_message_agent_capability(agent_id)?.models.into_iter().find(|model| model.id == model_id)
}

/// Ordered agent ids with a non-interactive mode wired up.
pub fn list_commit_message_agent_ids() -> Vec<&'static str> {
    specs().iter().map(|spec| spec.id).collect()
}

pub fn list_commit_message_agent_capabilities() -> Vec<CommitMessageAgentCapability> {
    specs().iter().map(to_capability).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_the_installed_local_agents_as_commit_message_agents() {
        let mut ids = list_commit_message_agent_ids();
        ids.sort_unstable();
        assert_eq!(ids, ["amp", "claude", "codex", "copilot", "cursor", "kimi", "opencode", "pi"]);
    }

    #[test]
    fn uses_the_strongest_available_defaults_for_core_agents() {
        assert_eq!(get_commit_message_agent_spec("claude").unwrap().default_model_id, "sonnet");
        assert_eq!(get_commit_message_agent_spec("codex").unwrap().default_model_id, "gpt-5.5");
        assert_eq!(get_commit_message_agent_spec("pi").unwrap().default_model_id, "github-copilot/gpt-5.4-mini");
    }

    #[test]
    fn uses_the_provider_qualified_kimi_model_id() {
        let ids: Vec<&str> = get_commit_message_agent_spec("kimi").unwrap().models.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["default", "kimi-code/kimi-for-coding"]);
    }

    #[test]
    fn lists_copilot_hosted_cli_models() {
        let spec = get_commit_message_agent_spec("copilot").unwrap();
        assert_eq!(spec.default_model_id, "gpt-5.4");
        let ids: Vec<&str> = spec.models.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "auto", "claude-haiku-4.5", "claude-sonnet-4.5", "claude-sonnet-4.6", "claude-opus-4.5",
                "claude-opus-4.6", "claude-opus-4.6-fast", "claude-opus-4.7", "gpt-4.1", "gpt-5-mini", "gpt-5.2",
                "gpt-5.2-codex", "gpt-5.3-codex", "gpt-5.4", "gpt-5.4-mini", "gpt-5.5"
            ]
        );
    }

    #[test]
    fn defaults_the_agent_picker_to_claude() {
        assert_eq!(DEFAULT_COMMIT_MESSAGE_AGENT_ID, "claude");
    }

    #[test]
    fn treats_disabled_default_agents_as_unavailable() {
        assert_eq!(resolve_commit_message_agent_choice(None, Some("codex"), &["codex"]).as_deref(), Some("claude"));
        assert_eq!(resolve_commit_message_agent_choice(None, None, &["claude"]), None);
        assert_eq!(resolve_commit_message_agent_choice(Some("codex"), None, &["codex"]).as_deref(), Some("codex"));
    }

    #[test]
    fn gives_every_model_with_thinking_levels_a_valid_default() {
        for spec in specs() {
            for model in &spec.models {
                if let Some(levels) = &model.thinking_levels {
                    let default = model.default_thinking_level.as_deref().expect("default thinking level");
                    assert!(levels.iter().any(|level| level.id == default));
                }
            }
        }
    }

    #[test]
    fn exposes_thinking_levels_on_the_spark_variant() {
        let spark = get_commit_message_model("codex", "gpt-5.3-codex-spark").unwrap();
        let ids: Vec<&str> = spark.thinking_levels.as_ref().unwrap().iter().map(|l| l.id.as_str()).collect();
        assert_eq!(ids, ["low", "medium", "high", "xhigh"]);
        assert_eq!(spark.default_thinking_level.as_deref(), Some("low"));
    }

    #[test]
    fn omits_thinking_levels_on_claude_haiku() {
        let haiku = get_commit_message_model("claude", "haiku").unwrap();
        assert!(haiku.thinking_levels.is_none());
        assert!(haiku.default_thinking_level.is_none());
    }

    #[test]
    fn identifies_the_custom_sentinel() {
        assert!(is_custom_agent_id(Some(CUSTOM_AGENT_ID)));
        assert!(!is_custom_agent_id(Some("claude")));
        assert!(!is_custom_agent_id(Some("codex")));
        assert!(!is_custom_agent_id(None));
    }

    #[test]
    fn does_not_list_custom_alongside_preset_agent_ids() {
        assert!(!list_commit_message_agent_ids().contains(&CUSTOM_AGENT_ID));
    }

    #[test]
    fn orders_codex_models_by_version_descending() {
        let ids: Vec<&str> =
            get_commit_message_agent_spec("codex").unwrap().models.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["gpt-5.5", "gpt-5.4", "gpt-5.4-mini", "gpt-5.3-codex", "gpt-5.3-codex-spark", "gpt-5.2"]);
    }

    #[test]
    fn exposes_ui_capabilities_without_spawn_details() {
        let capabilities = list_commit_message_agent_capabilities();
        assert!(capabilities.iter().any(|capability| capability.id == "opencode"));
        let codex = get_commit_message_agent_capability("codex").unwrap();
        assert_eq!(codex.id, "codex");
        assert_eq!(codex.label, "Codex");
        assert_eq!(codex.model_source, ModelSource::Dynamic);
        assert_eq!(codex.default_model_id, "gpt-5.5");
        assert!(get_commit_message_model_capability("codex", "gpt-5.4-mini").unwrap().thinking_levels.is_some());
    }

    #[test]
    fn claude_build_args_pass_core_flags_and_optional_effort() {
        let spec = get_commit_message_agent_spec("claude").unwrap();
        assert_eq!(
            (spec.build_args)(&BuildArgsParams { prompt: "", model: "haiku", thinking_level: None }),
            ["-p", "--output-format", "text", "--model", "haiku", "--permission-mode", "plan"]
        );
        assert_eq!(
            (spec.build_args)(&BuildArgsParams { prompt: "", model: "sonnet", thinking_level: Some("high") }),
            ["-p", "--output-format", "text", "--model", "sonnet", "--permission-mode", "plan", "--effort", "high"]
        );
        assert!(!(spec.build_args)(&BuildArgsParams { prompt: "", model: "opus", thinking_level: None })
            .contains(&"--effort".to_string()));
    }

    #[test]
    fn codex_build_args_run_exec_without_prompt_and_optional_reasoning_effort() {
        let spec = get_commit_message_agent_spec("codex").unwrap();
        let args = (spec.build_args)(&BuildArgsParams { prompt: "PROMPT", model: "gpt-5.4-mini", thinking_level: None });
        assert_eq!(args, ["exec", "--ephemeral", "--skip-git-repo-check", "-s", "read-only", "--model", "gpt-5.4-mini"]);
        assert!(!args.contains(&"PROMPT".to_string()));
        assert_eq!(spec.prompt_delivery, PromptDelivery::Stdin);

        let with_effort =
            (spec.build_args)(&BuildArgsParams { prompt: "PROMPT", model: "gpt-5.4", thinking_level: Some("medium") });
        assert!(with_effort.contains(&"-c".to_string()));
        assert!(with_effort.contains(&"model_reasoning_effort=medium".to_string()));
    }
}
