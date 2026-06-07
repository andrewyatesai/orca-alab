//! Setup-script prompt telemetry payloads, ported from
//! `src/shared/setup-script-telemetry.ts`.
//!
//! Pure payload shaping for the setup-script prompt funnel: bucket the file /
//! unsupported-field counts, derive the prompt mode, and carry only the
//! provider enum (never raw file details) into analytics.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetupScriptPromptMode {
    ImportAvailable,
    ConfigureNeeded,
}

impl SetupScriptPromptMode {
    pub fn as_wire(self) -> &'static str {
        match self {
            SetupScriptPromptMode::ImportAvailable => "import_available",
            SetupScriptPromptMode::ConfigureNeeded => "configure_needed",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetupScriptCountBucket {
    Zero,
    One,
    TwoToThree,
    FourPlus,
}

impl SetupScriptCountBucket {
    pub fn as_wire(self) -> &'static str {
        match self {
            SetupScriptCountBucket::Zero => "0",
            SetupScriptCountBucket::One => "1",
            SetupScriptCountBucket::TwoToThree => "2-3",
            SetupScriptCountBucket::FourPlus => "4+",
        }
    }
}

/// The fields this telemetry builder reads from a setup-script import candidate
/// (the full candidate carries label/setup too, which analytics never sends).
#[derive(Clone, Debug, Default)]
pub struct SetupScriptCandidateInput {
    pub provider: String,
    pub file_count: usize,
    pub unsupported_field_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetupScriptPromptTelemetry {
    pub mode: SetupScriptPromptMode,
    pub provider: Option<String>,
    pub file_count_bucket: SetupScriptCountBucket,
    pub unsupported_field_count_bucket: SetupScriptCountBucket,
    pub has_shared_hooks: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetupScriptPromptActionTelemetry {
    pub action: String,
    pub mode: SetupScriptPromptMode,
    pub provider: Option<String>,
    pub file_count_bucket: SetupScriptCountBucket,
    pub unsupported_field_count_bucket: SetupScriptCountBucket,
    pub has_shared_hooks: bool,
    pub edited_before_save: Option<bool>,
}

fn bucket_setup_script_count(count: usize) -> SetupScriptCountBucket {
    match count {
        0 => SetupScriptCountBucket::Zero,
        1 => SetupScriptCountBucket::One,
        2 | 3 => SetupScriptCountBucket::TwoToThree,
        _ => SetupScriptCountBucket::FourPlus,
    }
}

pub fn build_setup_script_prompt_telemetry(
    candidate: Option<&SetupScriptCandidateInput>,
    has_shared_hooks: bool,
) -> SetupScriptPromptTelemetry {
    SetupScriptPromptTelemetry {
        // The UI treats package-manager suggestions as detected setup, but the
        // analytics wire value stays stable for historical funnels.
        mode: if candidate.is_some() {
            SetupScriptPromptMode::ImportAvailable
        } else {
            SetupScriptPromptMode::ConfigureNeeded
        },
        provider: candidate.map(|c| c.provider.clone()),
        file_count_bucket: bucket_setup_script_count(candidate.map_or(0, |c| c.file_count)),
        unsupported_field_count_bucket: bucket_setup_script_count(
            candidate.map_or(0, |c| c.unsupported_field_count),
        ),
        has_shared_hooks,
    }
}

pub fn build_setup_script_prompt_action_telemetry(
    action: &str,
    candidate: Option<&SetupScriptCandidateInput>,
    has_shared_hooks: bool,
    edited_before_save: Option<bool>,
) -> SetupScriptPromptActionTelemetry {
    let base = build_setup_script_prompt_telemetry(candidate, has_shared_hooks);
    SetupScriptPromptActionTelemetry {
        action: action.to_string(),
        mode: base.mode,
        provider: base.provider,
        file_count_bucket: base.file_count_bucket,
        unsupported_field_count_bucket: base.unsupported_field_count_bucket,
        has_shared_hooks: base.has_shared_hooks,
        edited_before_save,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use SetupScriptCountBucket::{FourPlus, One, TwoToThree, Zero};
    use SetupScriptPromptMode::{ConfigureNeeded, ImportAvailable};

    fn candidate(provider: &str, file_count: usize, unsupported_field_count: usize) -> SetupScriptCandidateInput {
        SetupScriptCandidateInput { provider: provider.to_string(), file_count, unsupported_field_count }
    }

    #[test]
    fn builds_configure_prompt_telemetry_without_provider_or_raw_details() {
        assert_eq!(
            build_setup_script_prompt_telemetry(None, true),
            SetupScriptPromptTelemetry {
                mode: ConfigureNeeded,
                provider: None,
                file_count_bucket: Zero,
                unsupported_field_count_bucket: Zero,
                has_shared_hooks: true,
            }
        );
    }

    #[test]
    fn buckets_candidate_prompt_counts_and_preserves_only_the_provider_enum() {
        assert_eq!(
            build_setup_script_prompt_telemetry(Some(&candidate("codex", 3, 4)), false),
            SetupScriptPromptTelemetry {
                mode: ImportAvailable,
                provider: Some("codex".to_string()),
                file_count_bucket: TwoToThree,
                unsupported_field_count_bucket: FourPlus,
                has_shared_hooks: false,
            }
        );
    }

    #[test]
    fn adds_the_action_without_changing_the_bucketed_context() {
        assert_eq!(
            build_setup_script_prompt_action_telemetry(
                "import_completed",
                Some(&candidate("conductor", 1, 0)),
                true,
                None,
            ),
            SetupScriptPromptActionTelemetry {
                action: "import_completed".to_string(),
                mode: ImportAvailable,
                provider: Some("conductor".to_string()),
                file_count_bucket: One,
                unsupported_field_count_bucket: Zero,
                has_shared_hooks: true,
                edited_before_save: None,
            }
        );
    }

    #[test]
    fn builds_configure_action_telemetry_for_the_no_candidate_state() {
        assert_eq!(
            build_setup_script_prompt_action_telemetry("configure_clicked", None, false, None),
            SetupScriptPromptActionTelemetry {
                action: "configure_clicked".to_string(),
                mode: ConfigureNeeded,
                provider: None,
                file_count_bucket: Zero,
                unsupported_field_count_bucket: Zero,
                has_shared_hooks: false,
                edited_before_save: None,
            }
        );
    }

    #[test]
    fn records_whether_package_manager_setup_was_edited_before_save() {
        assert_eq!(
            build_setup_script_prompt_action_telemetry(
                "save_detected_setup_completed",
                Some(&candidate("package-manager", 1, 0)),
                false,
                Some(true),
            ),
            SetupScriptPromptActionTelemetry {
                action: "save_detected_setup_completed".to_string(),
                mode: ImportAvailable,
                provider: Some("package-manager".to_string()),
                file_count_bucket: One,
                unsupported_field_count_bucket: Zero,
                has_shared_hooks: false,
                edited_before_save: Some(true),
            }
        );
    }
}
