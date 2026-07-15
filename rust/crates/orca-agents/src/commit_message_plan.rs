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
use crate::commit_message_prompt::{
    plan_custom_command, tokenize_custom_command_template, CUSTOM_PROMPT_PLACEHOLDER,
};

#[derive(Clone, Debug, Default)]
pub struct CommitMessagePlanInput<'a> {
    /// A `TuiAgent` id or the `"custom"` sentinel.
    pub agent_id: &'a str,
    pub model: &'a str,
    pub thinking_level: Option<&'a str>,
    pub custom_agent_command: Option<&'a str>,
    pub agent_command_override: Option<&'a str>,
    /// User-supplied extra CLI args (same tokenizer as the command override),
    /// woven into the base argv around any `{prompt}` placeholder / argv prompt.
    pub agent_args: Option<&'a str>,
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
/// Public: the napi whole-plan surface resolves binaries through this too.
pub fn plan_agent_binary(default_binary: &str, command_override: Option<&str>) -> Result<(String, Vec<String>), String> {
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

/// Tokenize the user's extra CLI args (same POSIX-ish tokenizer as the command
/// override); a blank value contributes nothing. The error is prefixed to match
/// the TS "CLI arguments are invalid: …" surface.
fn plan_additional_agent_args(agent_args: Option<&str>) -> Result<Vec<String>, String> {
    let Some(trimmed) = agent_args.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(Vec::new());
    };
    tokenize_custom_command_template(trimmed).map_err(|error| format!("CLI arguments are invalid: {error}"))
}

/// Weave the extra args into the base argv: before a `{prompt}` placeholder if
/// present, else before a trailing argv-delivered prompt token, else appended.
fn insert_additional_agent_args(
    base_args: Vec<String>,
    agent_args: &[String],
    prompt_delivery: PromptDelivery,
    prompt: &str,
) -> Vec<String> {
    if agent_args.is_empty() {
        return base_args;
    }
    if let Some(index) = base_args.iter().rposition(|arg| arg == CUSTOM_PROMPT_PLACEHOLDER) {
        let mut merged = base_args;
        merged.splice(index..index, agent_args.iter().cloned());
        return merged;
    }
    if prompt_delivery == PromptDelivery::Argv
        && !prompt.is_empty()
        && base_args.last().map(String::as_str) == Some(prompt)
    {
        let mut result = base_args[..base_args.len() - 1].to_vec();
        result.extend(agent_args.iter().cloned());
        result.push(prompt.to_string());
        return result;
    }
    let mut result = base_args;
    result.extend(agent_args.iter().cloned());
    result
}

/// Model-flag aliases Codex accepts. Codex rejects a repeated singleton model
/// flag, so a recipe's model arg must REPLACE Orca's generated one (#8773).
const CODEX_MODEL_OPTION_ALIASES: [&str; 2] = ["--model", "-m"];

/// A located option: its token index and how many tokens it spans (1 for a
/// `--flag=value` / bare short-glued form, 2 when it consumes the next token).
struct OptionOccurrence {
    index: usize,
    consumed: usize,
}

/// Mirror of the TS `matchesOption`: exact alias, `alias=` prefix, or a glued
/// short form like `-mgpt` for a single-dash alias.
fn matches_option(token: &str, aliases: &[&str]) -> bool {
    aliases.iter().any(|&alias| {
        token == alias
            || token.starts_with(&format!("{alias}="))
            || (alias.starts_with('-')
                && !alias.starts_with("--")
                && token.starts_with(alias)
                && token.len() > alias.len())
    })
}

/// Find the first option matching `aliases`. `stop_at_terminator` halts the scan
/// at a `--` argv terminator (recipe args only). A bare alias that exactly equals
/// a known alias consumes the following non-dash token as its value.
fn find_option_occurrence(tokens: &[String], aliases: &[&str], stop_at_terminator: bool) -> Option<OptionOccurrence> {
    for (index, token) in tokens.iter().enumerate() {
        if stop_at_terminator && token == "--" {
            break;
        }
        if !matches_option(token, aliases) {
            continue;
        }
        let consumes_next =
            aliases.contains(&token.as_str()) && tokens.get(index + 1).is_some_and(|next| !next.starts_with('-'));
        return Some(OptionOccurrence { index, consumed: if consumes_next { 2 } else { 1 } });
    }
    None
}

/// When both the generated argv and the recipe args carry the same option,
/// splice the recipe's option tokens over the generated ones and drop them from
/// the recipe list so they are not appended a second time (the Codex #8773 fix).
fn apply_recipe_option_override(
    generated_args: Vec<String>,
    recipe_args: Vec<String>,
    aliases: &[&str],
) -> (Vec<String>, Vec<String>) {
    let (Some(recipe_option), Some(generated_option)) = (
        find_option_occurrence(&recipe_args, aliases, true),
        find_option_occurrence(&generated_args, aliases, false),
    ) else {
        return (generated_args, recipe_args);
    };
    let override_tokens = recipe_args[recipe_option.index..recipe_option.index + recipe_option.consumed].to_vec();
    let mut new_generated = generated_args[..generated_option.index].to_vec();
    new_generated.extend(override_tokens);
    new_generated.extend_from_slice(&generated_args[generated_option.index + generated_option.consumed..]);
    let mut new_recipe = recipe_args[..recipe_option.index].to_vec();
    new_recipe.extend_from_slice(&recipe_args[recipe_option.index + recipe_option.consumed..]);
    (new_generated, new_recipe)
}

pub fn plan_commit_message_generation(input: &CommitMessagePlanInput, prompt: &str) -> Result<CommitMessagePlan, String> {
    if is_custom_agent_id(Some(input.agent_id)) {
        let Some(command) = input.custom_agent_command.map(str::trim).filter(|c| !c.is_empty()) else {
            return Err("Custom command is empty. Add one in Settings → Git → AI Commit Messages.".to_string());
        };
        let planned = plan_custom_command(command, prompt)?;
        let agent_args = plan_additional_agent_args(input.agent_args)?;
        // Custom stdin delivery is signalled by a `None` payload sibling in the plan.
        let delivery =
            if planned.stdin_payload.is_none() { PromptDelivery::Argv } else { PromptDelivery::Stdin };
        let args = insert_additional_agent_args(planned.args, &agent_args, delivery, prompt);
        // A custom command has no friendly name, so the binary doubles as label.
        return Ok(CommitMessagePlan {
            label: planned.binary.clone(),
            binary: planned.binary,
            args,
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
    let base_args = (spec.build_args)(&BuildArgsParams { prompt: argv_prompt, model: input.model, thinking_level: input.thinking_level });
    let agent_args = plan_additional_agent_args(input.agent_args)?;
    // Why: Codex rejects repeated singleton model flags, so a recipe's model arg
    // replaces Orca's generated model rather than being appended alongside it (#8773).
    let (base_args, agent_args) = if input.agent_id == "codex" {
        apply_recipe_option_override(base_args, agent_args, &CODEX_MODEL_OPTION_ALIASES)
    } else {
        (base_args, agent_args)
    };
    let args = insert_additional_agent_args(base_args, &agent_args, spec.prompt_delivery, argv_prompt);
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
    fn plans_opencode_run_with_prompt_on_stdin_only_and_model_variant() {
        // Why: OpenCode reads the prompt from stdin (issue #4859); the large diff
        // must never land in argv, so args carry no positional prompt.
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
                args: strs(&["run", "--model", "opencode/gpt-5.4-mini", "--agent", "build", "--format", "default", "--variant", "high"]),
                stdin_payload: Some("PROMPT".to_string()),
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
    fn codex_recipe_model_arg_overrides_the_generated_model_flag() {
        // #8773: Codex rejects a repeated singleton --model, so a recipe's model
        // arg must REPLACE the generated one in place, not be appended alongside.
        let result = plan_commit_message_generation(
            &CommitMessagePlanInput {
                agent_id: "codex",
                model: "gpt-5.4-mini",
                thinking_level: Some("medium"),
                agent_args: Some("--model gpt-5.4"),
                ..Default::default()
            },
            "PROMPT",
        )
        .unwrap();
        assert_eq!(
            result.args,
            strs(&["exec", "--ephemeral", "--skip-git-repo-check", "-s", "read-only", "--model", "gpt-5.4", "-c", "model_reasoning_effort=medium"])
        );
        assert_eq!(result.args.iter().filter(|a| a.as_str() == "--model").count(), 1);
    }

    #[test]
    fn codex_recipe_non_model_arg_is_still_appended() {
        // The override only rewrites the model flag; other recipe args append normally.
        let result = plan_commit_message_generation(
            &CommitMessagePlanInput {
                agent_id: "codex",
                model: "gpt-5.4-mini",
                thinking_level: Some("medium"),
                agent_args: Some("--reasoning high"),
                ..Default::default()
            },
            "PROMPT",
        )
        .unwrap();
        assert_eq!(&result.args[result.args.len() - 2..], &strs(&["--reasoning", "high"])[..]);
        assert!(result.args.iter().any(|a| a == "gpt-5.4-mini"), "generated model untouched");
    }

    #[test]
    fn rejects_invalid_preset_agent_command_overrides_before_spawning() {
        let result = plan_commit_message_generation(
            &CommitMessagePlanInput { agent_id: "claude", model: "haiku", agent_command_override: Some("claude \"unterminated"), ..Default::default() },
            "PROMPT",
        );
        assert_eq!(result, Err("Agent command override is invalid: Unclosed quote in command template.".to_string()));
    }

    #[test]
    fn weaves_extra_cli_args_before_a_trailing_argv_prompt() {
        // Cursor delivers the prompt as the last argv token, so extra args must
        // land BEFORE it, never after (they'd be read as more prompt text).
        let result = plan_commit_message_generation(
            &CommitMessagePlanInput { agent_id: "cursor", model: "gpt-5.2", agent_args: Some("--foo bar"), ..Default::default() },
            "PROMPT",
        )
        .unwrap();
        assert_eq!(result.args.last().map(String::as_str), Some("PROMPT"));
        assert_eq!(&result.args[result.args.len() - 3..], &strs(&["--foo", "bar", "PROMPT"])[..]);
    }

    #[test]
    fn appends_extra_cli_args_for_stdin_delivered_agents() {
        // Claude reads the prompt from stdin, so there is no argv prompt to
        // protect — extra args simply append to the base argv.
        let result = plan_commit_message_generation(
            &CommitMessagePlanInput { agent_id: "claude", model: "sonnet", agent_args: Some("--verbose"), ..Default::default() },
            "PROMPT",
        )
        .unwrap();
        assert_eq!(result.args.last().map(String::as_str), Some("--verbose"));
        assert_eq!(result.stdin_payload.as_deref(), Some("PROMPT"));
    }

    #[test]
    fn rejects_invalid_extra_cli_args_with_the_cli_arguments_prefix() {
        let result = plan_commit_message_generation(
            &CommitMessagePlanInput { agent_id: "claude", model: "sonnet", agent_args: Some("--flag \"unterminated"), ..Default::default() },
            "PROMPT",
        );
        assert_eq!(result, Err("CLI arguments are invalid: Unclosed quote in command template.".to_string()));
    }

    #[test]
    fn insert_extra_args_at_prompt_placeholder_then_argv_tail_then_append() {
        // Placeholder splice: extra args land immediately before `{prompt}`.
        assert_eq!(
            insert_additional_agent_args(
                strs(&["run", "{prompt}", "--json"]),
                &strs(&["-x"]),
                PromptDelivery::Argv,
                "PROMPT",
            ),
            strs(&["run", "-x", "{prompt}", "--json"])
        );
        // Argv tail: no placeholder, prompt is the last token.
        assert_eq!(
            insert_additional_agent_args(strs(&["run", "PROMPT"]), &strs(&["-x"]), PromptDelivery::Argv, "PROMPT"),
            strs(&["run", "-x", "PROMPT"])
        );
        // Append: stdin delivery has no argv prompt to protect.
        assert_eq!(
            insert_additional_agent_args(strs(&["run", "-p"]), &strs(&["-x"]), PromptDelivery::Stdin, ""),
            strs(&["run", "-p", "-x"])
        );
        // Empty extra args pass the base argv through untouched.
        assert_eq!(
            insert_additional_agent_args(strs(&["run", "-p"]), &[], PromptDelivery::Stdin, ""),
            strs(&["run", "-p"])
        );
    }
}
