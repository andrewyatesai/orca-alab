//! TUI-agent auto-pick + enable/disable filtering, ported from
//! `src/shared/tui-agent-selection.ts`.
//!
//! Agents are referenced by their string id (the `TuiAgent` catalog).
//! [`TUI_AGENT_AUTO_PICK_ORDER`] is the desktop catalog in fallback-priority
//! order; per the source it is kept in sync with the full catalog, so
//! membership in it is the validity check (`is_tui_agent`).

/// Desktop agent catalog in automatic-fallback priority order.
pub const TUI_AGENT_AUTO_PICK_ORDER: [&str; 30] = [
    "claude",
    "openclaude",
    "codex",
    "grok",
    "copilot",
    "opencode",
    "pi",
    "omp",
    "gemini",
    "antigravity",
    "aider",
    "goose",
    "amp",
    "kilo",
    "kiro",
    "crush",
    "aug",
    "autohand",
    "cline",
    "codebuff",
    "command-code",
    "continue",
    "cursor",
    "droid",
    "kimi",
    "mistral-vibe",
    "qwen-code",
    "rovo",
    "hermes",
    "openclaw",
];

pub fn is_tui_agent(value: &str) -> bool {
    TUI_AGENT_AUTO_PICK_ORDER.contains(&value)
}

/// Pick the agent to launch: an installed, enabled `preferred`; else the first
/// installed, enabled agent in catalog order. `preferred == Some("blank")` is
/// the explicit "no agent" choice. `None` if nothing qualifies.
pub fn pick_tui_agent(preferred: Option<&str>, detected: &[&str], disabled: &[&str]) -> Option<String> {
    if preferred == Some("blank") {
        return None;
    }
    let disabled = normalize_disabled_tui_agents(disabled);
    let enabled_and_detected =
        |agent: &str| detected.contains(&agent) && !disabled.iter().any(|d| d == agent);

    if let Some(preferred) = preferred {
        if enabled_and_detected(preferred) {
            return Some(preferred.to_string());
        }
    }
    TUI_AGENT_AUTO_PICK_ORDER.into_iter().find(|agent| enabled_and_detected(agent)).map(str::to_string)
}

/// Valid agent ids from a raw list, deduped and order-preserving; unsupported
/// values are dropped.
pub fn normalize_disabled_tui_agents(value: &[&str]) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    for item in value {
        if is_tui_agent(item) && !seen.iter().any(|s| s == item) {
            seen.push((*item).to_string());
        }
    }
    seen
}

pub fn is_tui_agent_enabled(agent: &str, disabled: &[&str]) -> bool {
    !normalize_disabled_tui_agents(disabled).iter().any(|d| d == agent)
}

pub fn filter_enabled_tui_agents(agents: &[&str], disabled: &[&str]) -> Vec<String> {
    let disabled = normalize_disabled_tui_agents(disabled);
    agents.iter().filter(|agent| !disabled.iter().any(|d| d == *agent)).map(|agent| (*agent).to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_an_installed_preferred_agent() {
        assert_eq!(pick_tui_agent(Some("codex"), &["claude", "codex"], &[]).as_deref(), Some("codex"));
    }

    #[test]
    fn falls_back_in_desktop_catalog_order_when_preference_absent_or_stale() {
        assert_eq!(pick_tui_agent(None, &["cursor", "codex"], &[]).as_deref(), Some("codex"));
        assert_eq!(pick_tui_agent(Some("gemini"), &["cursor", "codex"], &[]).as_deref(), Some("codex"));
        assert_eq!(
            pick_tui_agent(None, &["continue", "command-code"], &[]).as_deref(),
            Some("command-code")
        );
    }

    #[test]
    fn respects_the_explicit_blank_terminal_preference() {
        assert_eq!(pick_tui_agent(Some("blank"), &["cursor", "claude"], &[]), None);
    }

    #[test]
    fn ignores_disabled_preferred_and_fallback_agents() {
        assert_eq!(pick_tui_agent(Some("codex"), &["claude", "codex"], &["codex"]).as_deref(), Some("claude"));
        assert_eq!(pick_tui_agent(None, &["claude", "codex"], &["claude", "codex"]), None);
    }

    #[test]
    fn dedupes_supported_agent_ids_and_drops_unsupported_values() {
        // The "" entries stand in for the TS `null`/non-string members (both
        // are dropped as non-agents).
        assert_eq!(
            normalize_disabled_tui_agents(&["codex", "unknown", "codex", "", "claude"]),
            vec!["codex".to_string(), "claude".to_string()]
        );
    }
}
