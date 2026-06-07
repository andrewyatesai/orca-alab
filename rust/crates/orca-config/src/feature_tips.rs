//! Feature-tip catalog + completion/ordering, ported from
//! `src/shared/feature-tips.ts`.
//!
//! Decides which onboarding tips are still worth surfacing: tips the user has
//! already seen, completed (CLI installed / voice enabled), or interacted with
//! (via the ported `feature_interactions` catalog) drop out, and unseen "new"
//! tips sort ahead of older "unseen" ones.

use crate::feature_interactions::{has_feature_interaction, FeatureInteractionId};
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FeatureTipId {
    VoiceDictation,
    OrcaCli,
}

impl FeatureTipId {
    pub fn as_str(self) -> &'static str {
        match self {
            FeatureTipId::VoiceDictation => "voice-dictation",
            FeatureTipId::OrcaCli => "orca-cli",
        }
    }

    pub fn from_id(value: &str) -> Option<FeatureTipId> {
        match value {
            "voice-dictation" => Some(FeatureTipId::VoiceDictation),
            "orca-cli" => Some(FeatureTipId::OrcaCli),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FeatureTipPriority {
    New,
    Unseen,
}

impl FeatureTipPriority {
    pub fn as_str(self) -> &'static str {
        match self {
            FeatureTipPriority::New => "new",
            FeatureTipPriority::Unseen => "unseen",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FeatureTipAction {
    EnableVoice,
    SetupCli,
}

impl FeatureTipAction {
    pub fn as_str(self) -> &'static str {
        match self {
            FeatureTipAction::EnableVoice => "enable-voice",
            FeatureTipAction::SetupCli => "setup-cli",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FeatureTip {
    pub id: FeatureTipId,
    pub priority: FeatureTipPriority,
    pub eyebrow: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub action: FeatureTipAction,
    pub cta_label: &'static str,
    /// Feature interactions that mean this tip is no longer useful to show.
    pub completed_by_feature_interactions: &'static [FeatureInteractionId],
}

#[derive(Clone, Debug, Default)]
pub struct CompletedFeatureTipState {
    pub cli_installed: bool,
    pub voice_dictation_enabled: bool,
    /// Persisted feature-interaction blob (untrusted JSON), fed to
    /// `has_feature_interaction`.
    pub feature_interactions: Option<Value>,
}

pub const FEATURE_TIPS: [FeatureTip; 2] = [
    FeatureTip {
        id: FeatureTipId::OrcaCli,
        priority: FeatureTipPriority::New,
        eyebrow: "Tip",
        title: "Let agents drive Orca with the Orca CLI",
        description: "Enable agents to coordinate child worktrees and communicate between worktrees.",
        action: FeatureTipAction::SetupCli,
        cta_label: "Install CLI & Skills",
        completed_by_feature_interactions: &[],
    },
    FeatureTip {
        id: FeatureTipId::VoiceDictation,
        priority: FeatureTipPriority::Unseen,
        eyebrow: "Tip",
        title: "Voice Dictation is here",
        description: "Speak into any focused pane and Orca will transcribe it. Press the dictation shortcut to start and stop.",
        action: FeatureTipAction::EnableVoice,
        cta_label: "Set Up Voice",
        completed_by_feature_interactions: &[FeatureInteractionId::VoiceDictation],
    },
];

pub fn is_feature_tip_id(value: &Value) -> bool {
    value.as_str().is_some_and(|s| FeatureTipId::from_id(s).is_some())
}

// Result is the unique valid tip ids, so it can never exceed the catalog size.
#[cfg_attr(trust_verify, trust::ensures(|out: &Vec<FeatureTipId>| out.len() <= FEATURE_TIPS.len()))]
pub fn normalize_feature_tip_ids(value: &Value) -> Vec<FeatureTipId> {
    let Some(items) = value.as_array() else {
        return Vec::new();
    };
    // First-seen order, deduped — matches the TS `Set` insertion semantics.
    let mut seen: Vec<FeatureTipId> = Vec::new();
    for item in items {
        if let Some(id) = item.as_str().and_then(FeatureTipId::from_id) {
            push_unique(&mut seen, id);
        }
    }
    seen
}

pub fn get_completed_feature_tip_ids(state: &CompletedFeatureTipState) -> Vec<FeatureTipId> {
    let mut completed: Vec<FeatureTipId> = Vec::new();
    if state.cli_installed {
        push_unique(&mut completed, FeatureTipId::OrcaCli);
    }
    if state.voice_dictation_enabled {
        push_unique(&mut completed, FeatureTipId::VoiceDictation);
    }
    for tip in FEATURE_TIPS {
        if tip
            .completed_by_feature_interactions
            .iter()
            .any(|id| has_feature_interaction(state.feature_interactions.as_ref(), *id))
        {
            push_unique(&mut completed, tip.id);
        }
    }
    completed
}

// Output is a subset of the catalog, so its length is bounded by the catalog.
#[cfg_attr(trust_verify, trust::ensures(|out: &Vec<FeatureTip>| out.len() <= FEATURE_TIPS.len()))]
pub fn get_ordered_unseen_feature_tips(
    seen_tip_ids: &[FeatureTipId],
    completed_tip_ids: &[FeatureTipId],
) -> Vec<FeatureTip> {
    let unseen: Vec<FeatureTip> = FEATURE_TIPS
        .iter()
        .copied()
        .filter(|tip| !seen_tip_ids.contains(&tip.id) && !completed_tip_ids.contains(&tip.id))
        .collect();
    let mut ordered: Vec<FeatureTip> = unseen
        .iter()
        .copied()
        .filter(|tip| tip.priority == FeatureTipPriority::New)
        .collect();
    ordered.extend(unseen.iter().copied().filter(|tip| tip.priority != FeatureTipPriority::New));
    ordered
}

fn push_unique(ids: &mut Vec<FeatureTipId>, id: FeatureTipId) {
    if !ids.contains(&id) {
        ids.push(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn orders_new_unseen_tips_before_older_unseen_tips() {
        let tips = get_ordered_unseen_feature_tips(&[], &[]);

        assert_eq!(
            tips.iter().map(|tip| tip.id).collect::<Vec<_>>(),
            vec![FeatureTipId::OrcaCli, FeatureTipId::VoiceDictation]
        );
    }

    #[test]
    fn skips_tips_the_user_has_already_seen() {
        let tips = get_ordered_unseen_feature_tips(
            &[FeatureTipId::VoiceDictation, FeatureTipId::OrcaCli],
            &[],
        );

        assert_eq!(tips.iter().map(|tip| tip.id).collect::<Vec<_>>(), Vec::<FeatureTipId>::new());
    }

    #[test]
    fn skips_tips_for_features_the_user_has_already_completed() {
        let completed = get_completed_feature_tip_ids(&CompletedFeatureTipState {
            cli_installed: true,
            voice_dictation_enabled: true,
            feature_interactions: None,
        });
        let tips = get_ordered_unseen_feature_tips(&[], &completed);

        assert_eq!(tips.iter().map(|tip| tip.id).collect::<Vec<_>>(), Vec::<FeatureTipId>::new());
    }

    #[test]
    fn skips_the_cli_tip_when_the_cli_is_already_installed() {
        let completed = get_completed_feature_tip_ids(&CompletedFeatureTipState {
            cli_installed: true,
            voice_dictation_enabled: false,
            feature_interactions: None,
        });
        let tips = get_ordered_unseen_feature_tips(&[FeatureTipId::VoiceDictation], &completed);

        assert_eq!(tips.iter().map(|tip| tip.id).collect::<Vec<_>>(), Vec::<FeatureTipId>::new());
    }

    #[test]
    fn skips_tips_for_features_the_user_has_already_interacted_with() {
        let completed = get_completed_feature_tip_ids(&CompletedFeatureTipState {
            cli_installed: false,
            voice_dictation_enabled: false,
            feature_interactions: Some(json!({
                "voice-dictation": { "firstInteractedAt": 100, "interactionCount": 1 }
            })),
        });
        let tips = get_ordered_unseen_feature_tips(&[], &completed);

        assert_eq!(tips.iter().map(|tip| tip.id).collect::<Vec<_>>(), vec![FeatureTipId::OrcaCli]);
    }

    #[test]
    fn normalizes_persisted_tip_ids() {
        assert_eq!(
            normalize_feature_tip_ids(&json!(["feature-tour", "orca-cli", "bogus", "voice-dictation"])),
            vec![FeatureTipId::OrcaCli, FeatureTipId::VoiceDictation]
        );
    }

    #[test]
    fn describes_the_cli_tip_as_an_install_action_with_concrete_workflows() {
        let cli_tip = FEATURE_TIPS.iter().find(|tip| tip.id == FeatureTipId::OrcaCli).copied().unwrap();

        assert_eq!(cli_tip.action, FeatureTipAction::SetupCli);
        assert_eq!(cli_tip.title, "Let agents drive Orca with the Orca CLI");
        assert_eq!(cli_tip.cta_label, "Install CLI & Skills");
        assert!(cli_tip.description.contains("coordinate child worktrees"));
        assert!(cli_tip.description.contains("communicate between worktrees"));
    }

    #[test]
    fn does_not_label_the_voice_dictation_tip_as_new() {
        let voice_tip = FEATURE_TIPS.iter().find(|tip| tip.id == FeatureTipId::VoiceDictation).copied().unwrap();

        assert_eq!(voice_tip.eyebrow, "Tip");
        assert_eq!(voice_tip.priority, FeatureTipPriority::Unseen);
    }
}
