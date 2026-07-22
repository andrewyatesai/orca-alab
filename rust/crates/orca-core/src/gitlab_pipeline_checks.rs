//! GitLab pipeline → unified PR-check mapping, ported from
//! `src/shared/gitlab-pipeline-checks.ts`.
//!
//! Maps raw GitLab job statuses onto the provider-neutral `PRCheckDetail`
//! status/conclusion shape the Checks panel renders, so GitLab pipelines and
//! GitHub check runs share one surface.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrCheckStatus {
    Queued,
    InProgress,
    Completed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrCheckConclusion {
    Success,
    Failure,
    Cancelled,
    TimedOut,
    Neutral,
    Skipped,
    Pending,
    /// A blocking manual gate — terminal-not-green must not read as a pass
    /// (parity with GitHub's ACTION_REQUIRED and the pipeline-level
    /// manual→failure mapping in `src/main/gitlab/mappers.ts`).
    ActionRequired,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrCheckDetail {
    pub name: String,
    pub status: PrCheckStatus,
    pub conclusion: Option<PrCheckConclusion>,
    pub url: Option<String>,
    /// The GitLab job id, surfaced as `checkRunId` so the Checks panel can
    /// fetch job details (trace) for the row (#7732).
    pub check_run_id: Option<i64>,
}

/// The GitLab job fields this mapping reads (the full record also carries
/// duration, which the Checks panel doesn't need here).
#[derive(Clone, Debug)]
pub struct GitLabPipelineJob {
    pub id: Option<i64>,
    pub name: String,
    pub stage: String,
    pub status: String,
    pub web_url: String,
    /// GitLab's `allow_failure`: true = optional job, false = a blocking gate.
    pub allow_failure: bool,
}

pub fn map_gitlab_pipeline_job_status_to_check_status(status: &str) -> PrCheckStatus {
    match status.to_lowercase().as_str() {
        "created" | "pending" | "scheduled" | "waiting_for_callback" | "waiting_for_resource"
        | "preparing" => PrCheckStatus::Queued,
        "running" => PrCheckStatus::InProgress,
        _ => PrCheckStatus::Completed,
    }
}

pub fn map_gitlab_pipeline_job_status_to_conclusion(
    status: &str,
    allow_failure: bool,
) -> Option<PrCheckConclusion> {
    match status.to_lowercase().as_str() {
        "success" => Some(PrCheckConclusion::Success),
        "failed" => Some(PrCheckConclusion::Failure),
        "canceled" | "canceling" => Some(PrCheckConclusion::Cancelled),
        "skipped" => Some(PrCheckConclusion::Skipped),
        // Manual jobs wait for a human trigger (never Pending: the Checks tab
        // would look stuck forever). Optional ones (allow_failure) stay Neutral;
        // a blocking one gates the whole pipeline, so it must read as
        // ActionRequired — the MR badge already shows that state red via the
        // pipeline-level manual/blocked→failure mapping. "blocked" is the
        // pipeline-level spelling, matched in case it ever surfaces job-level.
        "manual" | "blocked" => Some(if allow_failure {
            PrCheckConclusion::Neutral
        } else {
            PrCheckConclusion::ActionRequired
        }),
        "created" | "pending" | "running" | "waiting_for_callback" | "waiting_for_resource"
        | "preparing" | "scheduled" => Some(PrCheckConclusion::Pending),
        _ => None,
    }
}

pub fn gitlab_pipeline_jobs_to_pr_checks(jobs: &[GitLabPipelineJob]) -> Vec<PrCheckDetail> {
    jobs.iter()
        .map(|job| PrCheckDetail {
            name: if job.stage.is_empty() {
                job.name.clone()
            } else {
                format!("{}: {}", job.stage, job.name)
            },
            status: map_gitlab_pipeline_job_status_to_check_status(&job.status),
            conclusion: map_gitlab_pipeline_job_status_to_conclusion(&job.status, job.allow_failure),
            url: if job.web_url.is_empty() { None } else { Some(job.web_url.clone()) },
            check_run_id: job.id,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use PrCheckConclusion::{ActionRequired, Cancelled, Failure, Neutral, Pending, Success};
    use PrCheckStatus::{Completed, InProgress, Queued};

    fn job(id: i64, name: &str, stage: &str, status: &str, web_url: &str) -> GitLabPipelineJob {
        GitLabPipelineJob {
            id: Some(id),
            name: name.to_string(),
            stage: stage.to_string(),
            status: status.to_string(),
            web_url: web_url.to_string(),
            allow_failure: false,
        }
    }

    fn optional_job(id: i64, name: &str, stage: &str, status: &str) -> GitLabPipelineJob {
        GitLabPipelineJob { allow_failure: true, ..job(id, name, stage, status, "") }
    }

    #[test]
    fn maps_gitlab_pipeline_jobs_into_right_panel_check_rows() {
        let jobs = vec![
            job(1, "unit", "test", "failed", "https://gitlab.com/acme/orca/-/jobs/1"),
            job(2, "deploy", "deploy", "manual", ""),
            job(3, "delayed deploy", "deploy", "scheduled", "https://gitlab.com/acme/orca/-/jobs/3"),
            job(4, "external callback", "integration", "waiting_for_callback", "https://gitlab.com/acme/orca/-/jobs/4"),
        ];

        assert_eq!(
            gitlab_pipeline_jobs_to_pr_checks(&jobs),
            vec![
                PrCheckDetail {
                    name: "test: unit".to_string(),
                    status: Completed,
                    conclusion: Some(Failure),
                    url: Some("https://gitlab.com/acme/orca/-/jobs/1".to_string()),
                    check_run_id: Some(1),
                },
                PrCheckDetail {
                    name: "deploy: deploy".to_string(),
                    status: Completed,
                    conclusion: Some(ActionRequired),
                    url: None,
                    check_run_id: Some(2),
                },
                PrCheckDetail {
                    name: "deploy: delayed deploy".to_string(),
                    status: Queued,
                    conclusion: Some(Pending),
                    url: Some("https://gitlab.com/acme/orca/-/jobs/3".to_string()),
                    check_run_id: Some(3),
                },
                PrCheckDetail {
                    name: "integration: external callback".to_string(),
                    status: Queued,
                    conclusion: Some(Pending),
                    url: Some("https://gitlab.com/acme/orca/-/jobs/4".to_string()),
                    check_run_id: Some(4),
                },
            ]
        );
    }

    #[test]
    fn maps_individual_statuses_case_insensitively() {
        assert_eq!(map_gitlab_pipeline_job_status_to_check_status("RUNNING"), InProgress);
        assert_eq!(map_gitlab_pipeline_job_status_to_check_status("preparing"), Queued);
        assert_eq!(map_gitlab_pipeline_job_status_to_check_status("success"), Completed);
        assert_eq!(map_gitlab_pipeline_job_status_to_conclusion("Success", false), Some(Success));
        assert_eq!(
            map_gitlab_pipeline_job_status_to_conclusion("canceling", false),
            Some(Cancelled)
        );
        // Unknown status: completed run with no conclusion.
        assert_eq!(map_gitlab_pipeline_job_status_to_check_status("bogus"), Completed);
        assert_eq!(map_gitlab_pipeline_job_status_to_conclusion("bogus", false), None);
    }

    #[test]
    fn blocking_manual_gate_reads_action_required_but_optional_manual_stays_neutral() {
        // A pipeline gated on a required manual job shows a red MR badge
        // (pipeline manual/blocked → failure); the job row must agree.
        assert_eq!(
            map_gitlab_pipeline_job_status_to_conclusion("manual", false),
            Some(ActionRequired)
        );
        assert_eq!(
            map_gitlab_pipeline_job_status_to_conclusion("blocked", false),
            Some(ActionRequired)
        );
        // Optional manual jobs (allow_failure) never block the pipeline and
        // must not read as failing checks.
        assert_eq!(map_gitlab_pipeline_job_status_to_conclusion("manual", true), Some(Neutral));
        assert_eq!(map_gitlab_pipeline_job_status_to_conclusion("blocked", true), Some(Neutral));

        let checks = gitlab_pipeline_jobs_to_pr_checks(&[
            job(7, "release gate", "deploy", "manual", ""),
            optional_job(8, "optional deploy", "deploy", "manual"),
        ]);
        assert_eq!(checks[0].conclusion, Some(ActionRequired));
        assert_eq!(checks[1].conclusion, Some(Neutral));
    }

    #[test]
    fn omits_stage_prefix_when_stage_is_empty() {
        let checks = gitlab_pipeline_jobs_to_pr_checks(&[job(9, "lint", "", "success", "")]);
        assert_eq!(checks[0].name, "lint");
    }

    #[test]
    fn carries_the_job_id_as_check_run_id_and_tolerates_absent_ids() {
        let with_id = gitlab_pipeline_jobs_to_pr_checks(&[job(42, "unit", "test", "failed", "")]);
        assert_eq!(with_id[0].check_run_id, Some(42));
        let without_id = gitlab_pipeline_jobs_to_pr_checks(&[GitLabPipelineJob {
            id: None,
            name: "unit".to_string(),
            stage: "test".to_string(),
            status: "failed".to_string(),
            web_url: String::new(),
            allow_failure: false,
        }]);
        assert_eq!(without_id[0].check_run_id, None);
    }
}
