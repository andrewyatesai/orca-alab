//! Synthetic agent terminal titles, ported from
//! `src/shared/synthetic-agent-title.ts`.
//!
//! Some agents (e.g. Codex) emit working OSC titles but can miss the final
//! frame, so Orca synthesizes terminal-state titles from hook state. Agent type
//! and status state are strings here (the TS types are unions).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SyntheticAgentTitleProfile {
    pub working_label: &'static str,
    pub permission_label: &'static str,
    pub idle_label: &'static str,
    /// `None` ≙ the TS field being absent (treated as "synthesize"); `Some(false)`
    /// means do not synthesize the working state (keep the native spinner).
    pub synthesize_working_title: Option<bool>,
}

pub fn get_synthetic_agent_title_profile(
    agent_type: Option<&str>,
) -> Option<SyntheticAgentTitleProfile> {
    let profile = |working, permission, idle, synth| SyntheticAgentTitleProfile {
        working_label: working,
        permission_label: permission,
        idle_label: idle,
        synthesize_working_title: synth,
    };
    match agent_type? {
        // Codex emits working titles but can miss the final frame — only
        // synthesize terminal states so native spinner behavior stays intact.
        "codex" => Some(profile("Codex", "Codex - action required", "Codex ready", Some(false))),
        "cursor" => Some(profile("Cursor Agent", "Cursor - action required", "Cursor ready", None)),
        "opencode" => Some(profile("OpenCode", "OpenCode - action required", "OpenCode ready", None)),
        "droid" => Some(profile("Droid", "Droid - action required", "Droid ready", None)),
        "hermes" => Some(profile("Hermes", "Hermes - action required", "Hermes ready", None)),
        _ => None,
    }
}

pub fn get_synthetic_agent_terminal_title(
    agent_type: Option<&str>,
    state: &str,
) -> Option<&'static str> {
    let profile = get_synthetic_agent_title_profile(agent_type)?;
    if state == "working" {
        return None;
    }
    Some(if state == "blocked" || state == "waiting" {
        profile.permission_label
    } else {
        profile.idle_label
    })
}

pub fn should_drive_synthetic_agent_title_from_hook(agent_type: Option<&str>, state: &str) -> bool {
    match get_synthetic_agent_title_profile(agent_type) {
        None => false,
        Some(profile) => state != "working" || profile.synthesize_working_title != Some(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provides_terminal_state_titles_for_codex_hook_completion() {
        assert_eq!(get_synthetic_agent_terminal_title(Some("codex"), "done"), Some("Codex ready"));
        assert_eq!(
            get_synthetic_agent_terminal_title(Some("codex"), "waiting"),
            Some("Codex - action required")
        );
    }

    #[test]
    fn does_not_synthesize_codex_working_titles() {
        assert!(!should_drive_synthetic_agent_title_from_hook(Some("codex"), "working"));
        assert!(should_drive_synthetic_agent_title_from_hook(Some("codex"), "done"));
    }
}
