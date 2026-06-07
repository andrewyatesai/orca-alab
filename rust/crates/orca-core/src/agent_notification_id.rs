//! Stable agent-notification id, ported from `src/shared/agent-notification-id.ts`.
//!
//! Builds a deterministic dedupe id from an agent event's worktree, pane, and
//! state-start time, so the same event collapses to one notification. `None`
//! when any required field is missing or the timestamp is non-finite.

use crate::uri_component::encode_uri_component;

#[derive(Clone, Debug, Default)]
pub struct BuildAgentNotificationIdArgs<'a> {
    pub worktree_id: Option<&'a str>,
    pub pane_key: Option<&'a str>,
    pub state_started_at: Option<f64>,
}

pub fn build_agent_notification_id(args: &BuildAgentNotificationIdArgs) -> Option<String> {
    let worktree_id = args.worktree_id.filter(|id| !id.is_empty())?;
    let pane_key = args.pane_key.filter(|key| !key.is_empty())?;
    let state_started_at = args.state_started_at.filter(|value| value.is_finite())?;
    Some(format!(
        "agent:{}:{}:{}",
        encode_uri_component(worktree_id),
        encode_uri_component(pane_key),
        state_started_at.trunc()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_a_stable_id_for_the_same_agent_event_metadata() {
        let args = BuildAgentNotificationIdArgs {
            worktree_id: Some("repo::/Users/me/orca/workspaces/feature"),
            pane_key: Some("tab-1:11111111-1111-4111-8111-111111111111"),
            state_started_at: Some(1_780_000_000_123.0),
        };
        assert_eq!(build_agent_notification_id(&args), build_agent_notification_id(&args));
    }

    #[test]
    fn changes_when_the_agent_state_start_time_changes() {
        let base = BuildAgentNotificationIdArgs {
            worktree_id: Some("repo::/Users/me/orca/workspaces/feature"),
            pane_key: Some("tab-1:11111111-1111-4111-8111-111111111111"),
            state_started_at: None,
        };
        let first = BuildAgentNotificationIdArgs { state_started_at: Some(1_780_000_000_123.0), ..base.clone() };
        let second = BuildAgentNotificationIdArgs { state_started_at: Some(1_780_000_000_456.0), ..base };
        assert_ne!(build_agent_notification_id(&first), build_agent_notification_id(&second));
    }

    #[test]
    fn returns_none_when_required_fields_are_missing() {
        assert_eq!(
            build_agent_notification_id(&BuildAgentNotificationIdArgs {
                pane_key: Some("tab-1:11111111-1111-4111-8111-111111111111"),
                state_started_at: Some(1_780_000_000_123.0),
                ..Default::default()
            }),
            None
        );
        assert_eq!(
            build_agent_notification_id(&BuildAgentNotificationIdArgs {
                worktree_id: Some("repo::/Users/me/orca/workspaces/feature"),
                state_started_at: Some(1_780_000_000_123.0),
                ..Default::default()
            }),
            None
        );
        assert_eq!(
            build_agent_notification_id(&BuildAgentNotificationIdArgs {
                worktree_id: Some("repo::/Users/me/orca/workspaces/feature"),
                pane_key: Some("tab-1:11111111-1111-4111-8111-111111111111"),
                ..Default::default()
            }),
            None
        );
    }
}
