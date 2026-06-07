//! Commit-message generation planning, ported from `src/shared/commit-message-plan.ts`.
//!
//! Pure transform from "agent choice + prompt" to a spawn-ready binary + argv +
//! stdin payload — shared so the local generator and the SSH/relay provider
//! reuse identical validation and arg-building. Composes the agent-spec lookups
//! and the custom-command tokenizer.

use crate::commit_message_agent_spec::{
    get_commit_message_agent_spec, get_commit_message_model, is_custom_agent_id, BuildArgsParams, ModelSource,
    PromptDelivery,
};
use crate::commit_message_prompt::{plan_custom_command, tokenize_custom_command_template};

#[derive(Clone, Debug, Default)]
pub struct CommitMessagePlanInput<'a> {
    /// A `TuiAgent` id or the `"custom"` sentinel.
    pub agent_id: &'a str,
    pub model: &'a str,
    pub thinking_level: Option<&'a str>,
    pub custom_agent_command: Option<&'a str>,
    pub agent_command_override: Option<&'a str>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitMessagePlan {
    pub binary: String,
    pub args: Vec<String>,
    /// `Some` when the prompt is piped via stdin.
    pub stdin_payload: Option<String>,
    /// Human-readable label for error prefixes (e.g. "Claude failed: …").
    pub label: String,
}

/// Resolve the spawn binary + any prefix args from an optional command override
/// (e.g. `npx codex`); without an override the spec binary is used directly.
fn plan_agent_binary(default_binary: &str, command_override: Option<&str>) -> Result<(String, Vec<String>), String> {
    let Some(command) = command_override.map(str::trim).filter(|c| !c.is_empty()) else {
        return Ok((default_binary.to_string(), Vec::new()));
    };
    let tokens = tokenize_custom_command_template(command)
        .map_err(|error| format!("Agent command override is invalid: {error}"))?;
    match tokens.split_first() {
        Some((binary, prefix_args)) if !binary.is_empty() => Ok((binary.clone(), prefix_args.to_vec())),
        _ => Err("Agent command override must start with a binary name.".to_string()),
    }
}

pub fn plan_commit_message_generation(input: &CommitMessagePlanInput, prompt: &str) -> Result<CommitMessagePlan, String> {
    if is_custom_agent_id(Some(input.agent_id)) {
        let Some(command) = input.custom_agent_command.map(str::trim).filter(|c| !c.is_empty()) else {
            return Err("Custom command is empty. Add one in Settings → Git → AI Commit Messages.".to_string());
        };
        let planned = plan_custom_command(command, prompt)?;
        // A custom command has no friendly name, so the binary doubles as label.
        return Ok(CommitMessagePlan {
            label: planned.binary.clone(),
            binary: planned.binary,
            args: planned.args,
            stdin_payload: planned.stdin_payload,
        });
    }

    let spec = get_commit_message_agent_spec(input.agent_id)
        .ok_or_else(|| format!("Agent \"{}\" does not support AI commit messages.", input.agent_id))?;
    let model = get_commit_message_model(input.agent_id, input.model)
        .ok_or_else(|| format!("Model \"{}\" is not available for {}.", input.model, spec.label))?;

    if let Some(thinking) = input.thinking_level {
        if model.thinking_levels.is_none() && spec.model_source != ModelSource::Dynamic {
            return Err(format!("Model \"{}\" does not support a thinking effort level.", model.label));
        }
        if let Some(levels) = &model.thinking_levels {
            if !levels.iter().any(|level| level.id == thinking) {
                return Err(format!("Thinking level \"{thinking}\" is not valid for {}.", model.label));
            }
        }
    }

    let argv_prompt = if spec.prompt_delivery == PromptDelivery::Argv { prompt } else { "" };
    let args = (spec.build_args)(&BuildArgsParams { prompt: argv_prompt, model: input.model, thinking_level: input.thinking_level });
    let (binary, mut full_args) = plan_agent_binary(spec.binary, input.agent_command_override)?;
    full_args.extend(args);

    Ok(CommitMessagePlan {
        binary,
        args: full_args,
        stdin_payload: (spec.prompt_delivery == PromptDelivery::Stdin).then(|| prompt.to_string()),
        label: spec.label.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strs(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn plans_claude_non_interactive_generation_with_prompt_on_stdin_only() {
        let result = plan_commit_message_generation(
            &CommitMessagePlanInput { agent_id: "claude", model: "sonnet", thinking_level: Some("high"), ..Default::default() },
            "PROMPT",
        );
        assert_eq!(
            result,
            Ok(CommitMessagePlan {
                binary: "claude".to_string(),
                args: strs(&["-p", "--output-format", "text", "--model", "sonnet", "--permission-mode", "plan", "--effort", "high"]),
                stdin_payload: Some("PROMPT".to_string()),
                label: "Claude".to_string(),
            })
        );
    }

    #[test]
    fn plans_opencode_run_with_prompt_in_argv_and_model_variant() {
        let result = plan_commit_message_generation(
            &CommitMessagePlanInput {
                agent_id: "opencode",
                model: "opencode/gpt-5.4-mini",
                thinking_level: Some("high"),
                ..Default::default()
            },
            "PROMPT",
        );
        assert_eq!(
            result,
            Ok(CommitMessagePlan {
                binary: "opencode".to_string(),
                args: strs(&["run", "--model", "opencode/gpt-5.4-mini", "--agent", "build", "--format", "default", "--variant", "high", "PROMPT"]),
                stdin_payload: None,
                label: "OpenCode".to_string(),
            })
        );
    }

    #[test]
    fn allows_discovered_dynamic_models_not_in_the_seed_catalog() {
        let result = plan_commit_message_generation(
            &CommitMessagePlanInput { agent_id: "cursor", model: "gpt-5.2", thinking_level: Some("xhigh"), ..Default::default() },
            "PROMPT",
        );
        assert_eq!(
            result,
            Ok(CommitMessagePlan {
                binary: "cursor-agent".to_string(),
                args: strs(&["--print", "--mode", "ask", "--trust", "--output-format", "text", "--model", "gpt-5.2", "PROMPT"]),
                stdin_payload: None,
                label: "Cursor".to_string(),
            })
        );
    }

    #[test]
    fn plans_codex_exec_as_read_only_generation_with_prompt_on_stdin_only() {
        let result = plan_commit_message_generation(
            &CommitMessagePlanInput { agent_id: "codex", model: "gpt-5.4-mini", thinking_level: Some("medium"), ..Default::default() },
            "PROMPT",
        );
        assert_eq!(
            result,
            Ok(CommitMessagePlan {
                binary: "codex".to_string(),
                args: strs(&["exec", "--ephemeral", "--skip-git-repo-check", "-s", "read-only", "--model", "gpt-5.4-mini", "-c", "model_reasoning_effort=medium"]),
                stdin_payload: Some("PROMPT".to_string()),
                label: "Codex".to_string(),
            })
        );
    }

    #[test]
    fn uses_preset_agent_command_overrides_as_the_spawn_command_prefix() {
        let result = plan_commit_message_generation(
            &CommitMessagePlanInput { agent_id: "codex", model: "gpt-5.4-mini", agent_command_override: Some("npx codex"), ..Default::default() },
            "PROMPT",
        )
        .unwrap();
        assert_eq!(result.binary, "npx");
        assert_eq!(
            result.args,
            strs(&["codex", "exec", "--ephemeral", "--skip-git-repo-check", "-s", "read-only", "--model", "gpt-5.4-mini"])
        );
        assert_eq!(result.stdin_payload.as_deref(), Some("PROMPT"));
    }

    #[test]
    fn rejects_invalid_preset_agent_command_overrides_before_spawning() {
        let result = plan_commit_message_generation(
            &CommitMessagePlanInput { agent_id: "claude", model: "haiku", agent_command_override: Some("claude \"unterminated"), ..Default::default() },
            "PROMPT",
        );
        assert_eq!(result, Err("Agent command override is invalid: Unclosed quote in command template.".to_string()));
    }
}
