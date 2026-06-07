//! TuiAgent ↔ telemetry `AgentKind` mapping, ported from
//! `src/shared/agent-kind.ts`.
//!
//! Every agent Orca can launch maps to a concrete telemetry kind so dashboards
//! distinguish launch interest instead of collapsing the long tail to `other`.
//! Agents and kinds are represented as strings here (the TS unions are large
//! string literal enums); a value outside the table falls back to `other`.

/// `(TuiAgent, concrete AgentKind)` pairs, in source order.
pub const TUI_AGENT_KIND_PAIRS: &[(&str, &str)] = &[
    ("claude", "claude-code"),
    ("openclaude", "openclaude"),
    ("codex", "codex"),
    ("autohand", "autohand"),
    ("opencode", "opencode"),
    ("pi", "pi"),
    ("omp", "omp"),
    ("gemini", "gemini"),
    ("antigravity", "antigravity"),
    ("aider", "aider"),
    ("goose", "goose"),
    ("amp", "amp"),
    ("kilo", "kilo"),
    ("kiro", "kiro"),
    ("crush", "crush"),
    ("aug", "aug"),
    ("cline", "cline"),
    ("codebuff", "codebuff"),
    ("command-code", "command-code"),
    ("continue", "continue"),
    ("cursor", "cursor"),
    ("droid", "droid"),
    ("kimi", "kimi"),
    ("mistral-vibe", "mistral-vibe"),
    ("qwen-code", "qwen-code"),
    ("rovo", "rovo"),
    ("hermes", "hermes"),
    ("openclaw", "openclaw"),
    ("copilot", "copilot"),
    ("grok", "grok"),
];

/// Maps a TuiAgent to its telemetry kind; unknown agents → `"other"` so the
/// event still emits instead of failing validation.
pub fn tui_agent_to_agent_kind(agent: &str) -> &'static str {
    for (a, kind) in TUI_AGENT_KIND_PAIRS {
        if *a == agent {
            return kind;
        }
    }
    "other"
}

/// Reverses a telemetry kind back to its TuiAgent; `None` for `other`/missing.
pub fn agent_kind_to_tui_agent(kind: Option<&str>) -> Option<&'static str> {
    let kind = kind?;
    if kind.is_empty() {
        return None;
    }
    TUI_AGENT_KIND_PAIRS
        .iter()
        .find(|(_, k)| *k == kind)
        .map(|(agent, _)| *agent)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_product_id_for_claude_and_tui_id_for_pi() {
        assert_eq!(tui_agent_to_agent_kind("claude"), "claude-code");
        assert_eq!(tui_agent_to_agent_kind("pi"), "pi");
    }

    #[test]
    fn unknown_agent_falls_back_to_other() {
        assert_eq!(tui_agent_to_agent_kind("definitely-not-an-agent"), "other");
    }

    #[test]
    fn reverses_claude_product_id_back_to_tui_agent() {
        assert_eq!(agent_kind_to_tui_agent(Some("claude-code")), Some("claude"));
    }

    #[test]
    fn round_trips_every_shipped_agent_through_its_kind() {
        for (agent, _) in TUI_AGENT_KIND_PAIRS {
            assert_eq!(
                agent_kind_to_tui_agent(Some(tui_agent_to_agent_kind(agent))),
                Some(*agent)
            );
        }
    }

    #[test]
    fn returns_none_for_catch_all_and_missing_kinds() {
        assert_eq!(agent_kind_to_tui_agent(Some("other")), None);
        assert_eq!(agent_kind_to_tui_agent(None), None);
        assert_eq!(agent_kind_to_tui_agent(Some("")), None);
    }
}
