//! Nested-repo import telemetry payloads, ported from `src/shared/nested-repo-telemetry.ts`.
//!
//! Low-cardinality telemetry for the nested-repo scan/import funnel: cap +
//! bucket counts, classify scan/import outcomes, and thread one attempt id. The
//! random attempt-id bytes are injected at the edge (UUIDv4 formatting here);
//! everything else is pure.

pub const NESTED_REPO_TELEMETRY_MAX_REPO_COUNT: i64 = 500;

/// Lean projection of a nested-repo scan result (the builder reads only these).
#[derive(Clone, Debug)]
pub struct NestedRepoScanResult {
    pub repo_count: usize,
    /// `"git_repo"` | `"non_git_folder"` | …
    pub selected_path_kind: String,
    pub truncated: bool,
    pub timed_out: bool,
}

#[derive(Clone, Debug, Default)]
pub struct ProjectGroupImportResult {
    pub imported_count: i64,
    pub already_known_count: i64,
    pub failed_count: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NestedRepoScanTelemetry {
    pub attempt_id: String,
    pub surface: String,
    pub runtime_kind: String,
    pub result: String,
    pub selected_path_kind: Option<String>,
    pub found_count: i64,
    pub found_count_bucket: String,
    pub truncated: bool,
    pub timed_out: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NestedRepoImportActionTelemetry {
    pub attempt_id: String,
    pub surface: String,
    pub runtime_kind: String,
    pub action: String,
    pub found_count: i64,
    pub found_count_bucket: String,
    pub selected_count: i64,
    pub selected_count_bucket: String,
    pub all_selected: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NestedRepoImportResultTelemetry {
    pub attempt_id: String,
    pub surface: String,
    pub runtime_kind: String,
    pub mode: String,
    pub outcome: String,
    pub found_count: i64,
    pub found_count_bucket: String,
    pub selected_count: i64,
    pub selected_count_bucket: String,
    pub imported_count: i64,
    pub imported_count_bucket: String,
    pub already_known_count: i64,
    pub already_known_count_bucket: String,
    pub failed_count: i64,
    pub failed_count_bucket: String,
    pub all_selected: bool,
}

pub fn cap_nested_repo_telemetry_count(count: f64) -> i64 {
    if !count.is_finite() {
        return 0;
    }
    count.floor().clamp(0.0, NESTED_REPO_TELEMETRY_MAX_REPO_COUNT as f64) as i64
}

fn normalize_nested_repo_telemetry_count(count: f64) -> i64 {
    if !count.is_finite() {
        return 0;
    }
    count.floor().max(0.0) as i64
}

pub fn bucket_nested_repo_telemetry_count(count: f64) -> &'static str {
    match cap_nested_repo_telemetry_count(count) {
        0 => "0",
        1 => "1",
        c if c <= 3 => "2-3",
        c if c <= 7 => "4-7",
        c if c <= 15 => "8-15",
        _ => "16+",
    }
}

pub fn should_emit_nested_repo_import_submit_telemetry(attempt_id: Option<&str>, selected_count: i64, is_busy: bool) -> bool {
    attempt_id.is_some_and(|id| !id.is_empty()) && selected_count > 0 && !is_busy
}

/// Format 16 caller-supplied random bytes as a schema-compatible UUIDv4 string.
pub fn create_nested_repo_telemetry_attempt_id(random_bytes: [u8; 16]) -> String {
    let mut bytes = random_bytes;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: Vec<String> = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        hex[0..4].concat(),
        hex[4..6].concat(),
        hex[6..8].concat(),
        hex[8..10].concat(),
        hex[10..16].concat()
    )
}

pub fn build_nested_repo_scan_telemetry(
    attempt_id: &str,
    surface: &str,
    runtime_kind: &str,
    scan: Option<&NestedRepoScanResult>,
) -> NestedRepoScanTelemetry {
    let found_count = cap_nested_repo_telemetry_count(scan.map_or(0.0, |s| s.repo_count as f64));
    let result = match scan {
        None => "scan_failed",
        Some(s) if s.selected_path_kind == "git_repo" => "git_repo",
        Some(_) if found_count > 0 => "review_shown",
        Some(_) => "no_nested_repos",
    };
    NestedRepoScanTelemetry {
        attempt_id: attempt_id.to_string(),
        surface: surface.to_string(),
        runtime_kind: runtime_kind.to_string(),
        result: result.to_string(),
        selected_path_kind: scan.map(|s| s.selected_path_kind.clone()),
        found_count,
        found_count_bucket: bucket_nested_repo_telemetry_count(found_count as f64).to_string(),
        truncated: scan.is_some_and(|s| s.truncated),
        timed_out: scan.is_some_and(|s| s.timed_out),
    }
}

pub fn build_nested_repo_import_action_telemetry(
    attempt_id: &str,
    surface: &str,
    runtime_kind: &str,
    action: &str,
    found_count: i64,
    selected_count: i64,
) -> NestedRepoImportActionTelemetry {
    let raw_found = normalize_nested_repo_telemetry_count(found_count as f64);
    let raw_selected = normalize_nested_repo_telemetry_count(selected_count as f64);
    let capped_found = cap_nested_repo_telemetry_count(found_count as f64);
    let capped_selected = cap_nested_repo_telemetry_count(selected_count as f64);
    NestedRepoImportActionTelemetry {
        attempt_id: attempt_id.to_string(),
        surface: surface.to_string(),
        runtime_kind: runtime_kind.to_string(),
        action: action.to_string(),
        found_count: capped_found,
        found_count_bucket: bucket_nested_repo_telemetry_count(capped_found as f64).to_string(),
        selected_count: capped_selected,
        selected_count_bucket: bucket_nested_repo_telemetry_count(capped_selected as f64).to_string(),
        all_selected: raw_found > 0 && raw_selected == raw_found,
    }
}

pub fn build_nested_repo_import_result_telemetry(
    attempt_id: &str,
    surface: &str,
    runtime_kind: &str,
    mode: &str,
    found_count: i64,
    selected_count: i64,
    result: Option<&ProjectGroupImportResult>,
) -> NestedRepoImportResultTelemetry {
    let raw_found = normalize_nested_repo_telemetry_count(found_count as f64);
    let raw_selected = normalize_nested_repo_telemetry_count(selected_count as f64);
    let capped_found = cap_nested_repo_telemetry_count(found_count as f64);
    let capped_selected = cap_nested_repo_telemetry_count(selected_count as f64);
    let imported_count = cap_nested_repo_telemetry_count(result.map_or(0, |r| r.imported_count) as f64);
    let already_known_count = cap_nested_repo_telemetry_count(result.map_or(0, |r| r.already_known_count) as f64);
    let failed_count =
        cap_nested_repo_telemetry_count(result.map_or(capped_selected, |r| r.failed_count) as f64);
    let accepted = imported_count + already_known_count;
    let outcome = if accepted == 0 {
        "failed"
    } else if failed_count > 0 {
        "partial_failure"
    } else {
        "success"
    };
    let bucket = |count: i64| bucket_nested_repo_telemetry_count(count as f64).to_string();
    NestedRepoImportResultTelemetry {
        attempt_id: attempt_id.to_string(),
        surface: surface.to_string(),
        runtime_kind: runtime_kind.to_string(),
        mode: mode.to_string(),
        outcome: outcome.to_string(),
        found_count: capped_found,
        found_count_bucket: bucket(capped_found),
        selected_count: capped_selected,
        selected_count_bucket: bucket(capped_selected),
        imported_count,
        imported_count_bucket: bucket(imported_count),
        already_known_count,
        already_known_count_bucket: bucket(already_known_count),
        failed_count,
        failed_count_bucket: bucket(failed_count),
        all_selected: raw_found > 0 && raw_selected == raw_found,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ATTEMPT_ID: &str = "2fbac1e3-5094-45b4-80a6-90281e6e9e09";
    const NEXT_ATTEMPT_ID: &str = "d22bb9e0-b7f8-480a-8a2a-9b34f84f2c42";

    fn scan_result() -> NestedRepoScanResult {
        NestedRepoScanResult { repo_count: 3, selected_path_kind: "non_git_folder".to_string(), truncated: false, timed_out: false }
    }

    #[test]
    fn caps_and_buckets_repo_counts_for_low_cardinality_breakdowns() {
        assert_eq!(cap_nested_repo_telemetry_count(-1.0), 0);
        assert_eq!(cap_nested_repo_telemetry_count(2.9), 2);
        assert_eq!(cap_nested_repo_telemetry_count(f64::NAN), 0);
        assert_eq!(cap_nested_repo_telemetry_count(999.0), NESTED_REPO_TELEMETRY_MAX_REPO_COUNT);

        assert_eq!(bucket_nested_repo_telemetry_count(0.0), "0");
        assert_eq!(bucket_nested_repo_telemetry_count(1.0), "1");
        assert_eq!(bucket_nested_repo_telemetry_count(3.0), "2-3");
        assert_eq!(bucket_nested_repo_telemetry_count(7.0), "4-7");
        assert_eq!(bucket_nested_repo_telemetry_count(15.0), "8-15");
        assert_eq!(bucket_nested_repo_telemetry_count(16.0), "16+");
    }

    #[test]
    fn classifies_a_scan_that_should_show_nested_repo_review() {
        assert_eq!(
            build_nested_repo_scan_telemetry(ATTEMPT_ID, "onboarding", "local", Some(&scan_result())),
            NestedRepoScanTelemetry {
                attempt_id: ATTEMPT_ID.to_string(),
                surface: "onboarding".to_string(),
                runtime_kind: "local".to_string(),
                result: "review_shown".to_string(),
                selected_path_kind: Some("non_git_folder".to_string()),
                found_count: 3,
                found_count_bucket: "2-3".to_string(),
                truncated: false,
                timed_out: false,
            }
        );
    }

    #[test]
    fn records_import_action_selection_without_raw_path_details() {
        assert_eq!(
            build_nested_repo_import_action_telemetry(ATTEMPT_ID, "sidebar", "ssh", "import_group", 3, 2),
            NestedRepoImportActionTelemetry {
                attempt_id: ATTEMPT_ID.to_string(),
                surface: "sidebar".to_string(),
                runtime_kind: "ssh".to_string(),
                action: "import_group".to_string(),
                found_count: 3,
                found_count_bucket: "2-3".to_string(),
                selected_count: 2,
                selected_count_bucket: "2-3".to_string(),
                all_selected: false,
            }
        );
    }

    #[test]
    fn computes_all_selected_from_raw_counts_before_caps() {
        let action = build_nested_repo_import_action_telemetry(ATTEMPT_ID, "sidebar", "local", "import_group", 600, 500);
        let result = build_nested_repo_import_result_telemetry(
            ATTEMPT_ID,
            "sidebar",
            "local",
            "group",
            600,
            500,
            Some(&ProjectGroupImportResult { imported_count: 500, already_known_count: 0, failed_count: 0 }),
        );
        assert_eq!(action.found_count, 500);
        assert_eq!(action.selected_count, 500);
        assert!(!action.all_selected);
        assert!(!result.all_selected);
    }

    #[test]
    fn keeps_exact_imported_counts_on_import_result_payloads() {
        let result = build_nested_repo_import_result_telemetry(
            ATTEMPT_ID,
            "onboarding",
            "runtime",
            "group",
            4,
            4,
            Some(&ProjectGroupImportResult { imported_count: 2, already_known_count: 1, failed_count: 1 }),
        );
        assert_eq!(result.outcome, "partial_failure");
        assert_eq!(result.found_count, 4);
        assert_eq!(result.selected_count, 4);
        assert_eq!(result.imported_count, 2);
        assert_eq!(result.already_known_count, 1);
        assert_eq!(result.failed_count, 1);
        assert!(result.all_selected);
        assert_eq!(result.mode, "group");
        assert_eq!(result.runtime_kind, "runtime");
    }

    #[test]
    fn generates_non_persistent_random_attempt_ids() {
        let first = create_nested_repo_telemetry_attempt_id([1; 16]);
        let second = create_nested_repo_telemetry_attempt_id([2; 16]);
        assert_eq!(first.len(), 36);
        assert!(first.chars().all(|c| c.is_ascii_hexdigit() || c == '-'));
        assert_ne!(first, second);
    }

    #[test]
    fn threads_one_attempt_id_across_scan_action_and_result() {
        let scan = build_nested_repo_scan_telemetry(ATTEMPT_ID, "sidebar", "local", Some(&scan_result()));
        let action = build_nested_repo_import_action_telemetry(ATTEMPT_ID, "sidebar", "local", "import_separate", 3, 3);
        let result = build_nested_repo_import_result_telemetry(
            ATTEMPT_ID,
            "sidebar",
            "local",
            "separate",
            3,
            3,
            Some(&ProjectGroupImportResult { imported_count: 3, already_known_count: 0, failed_count: 0 }),
        );
        let next_scan = build_nested_repo_scan_telemetry(NEXT_ATTEMPT_ID, "sidebar", "local", Some(&scan_result()));
        assert_eq!(action.attempt_id, scan.attempt_id);
        assert_eq!(result.attempt_id, scan.attempt_id);
        assert_ne!(next_scan.attempt_id, scan.attempt_id);
    }

    #[test]
    fn prevents_zero_selection_submit_telemetry() {
        assert!(!should_emit_nested_repo_import_submit_telemetry(Some(ATTEMPT_ID), 0, false));
        assert!(!should_emit_nested_repo_import_submit_telemetry(Some(ATTEMPT_ID), 1, true));
        assert!(should_emit_nested_repo_import_submit_telemetry(Some(ATTEMPT_ID), 1, false));
    }
}
