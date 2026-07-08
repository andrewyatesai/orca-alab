//! Parity dispatch for `orca_core::gitlab_pipeline_checks` vs
//! `src/shared/gitlab-pipeline-checks.ts`.

use orca_core::gitlab_pipeline_checks::{
    gitlab_pipeline_jobs_to_pr_checks, map_gitlab_pipeline_job_status_to_check_status,
    map_gitlab_pipeline_job_status_to_conclusion, GitLabPipelineJob, PrCheckConclusion,
    PrCheckDetail, PrCheckStatus,
};
use serde_json::{json, Map, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "mapGitLabPipelineJobStatusToCheckStatus" => match input.as_str() {
            Some(status) => Value::String(
                check_status_id(map_gitlab_pipeline_job_status_to_check_status(status)).to_string(),
            ),
            None => json!({ "__parity_error__": "expected string status" }),
        },
        "mapGitLabPipelineJobStatusToConclusion" => match input.as_str() {
            Some(status) => conclusion_to_value(map_gitlab_pipeline_job_status_to_conclusion(status)),
            None => json!({ "__parity_error__": "expected string status" }),
        },
        "gitLabPipelineJobsToPRChecks" => match input.as_array() {
            Some(jobs) => {
                let parsed: Vec<GitLabPipelineJob> = jobs.iter().map(job_from_json).collect();
                Value::Array(
                    gitlab_pipeline_jobs_to_pr_checks(&parsed)
                        .iter()
                        .map(pr_check_to_value)
                        .collect(),
                )
            }
            None => json!({ "__parity_error__": "expected array of jobs" }),
        },
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

/// The Rust `GitLabPipelineJob` only carries the four fields this mapping reads;
/// id/duration in the vectors are ignored (the TS port reads them too).
fn job_from_json(value: &Value) -> GitLabPipelineJob {
    GitLabPipelineJob {
        name: str_field(value, "name"),
        stage: str_field(value, "stage"),
        status: str_field(value, "status"),
        web_url: str_field(value, "webUrl"),
    }
}

fn str_field(value: &Value, key: &str) -> String {
    value.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}

/// Match the TS `PRCheckDetail['status']` string ids.
fn check_status_id(status: PrCheckStatus) -> &'static str {
    match status {
        PrCheckStatus::Queued => "queued",
        PrCheckStatus::InProgress => "in_progress",
        PrCheckStatus::Completed => "completed",
    }
}

/// Match the TS `PRCheckDetail['conclusion']` string ids.
fn conclusion_id(conclusion: PrCheckConclusion) -> &'static str {
    match conclusion {
        PrCheckConclusion::Success => "success",
        PrCheckConclusion::Failure => "failure",
        PrCheckConclusion::Cancelled => "cancelled",
        PrCheckConclusion::TimedOut => "timed_out",
        PrCheckConclusion::Neutral => "neutral",
        PrCheckConclusion::Skipped => "skipped",
        PrCheckConclusion::Pending => "pending",
    }
}

/// TS returns `null` (not absent) when no conclusion maps.
fn conclusion_to_value(conclusion: Option<PrCheckConclusion>) -> Value {
    match conclusion {
        Some(c) => Value::String(conclusion_id(c).to_string()),
        None => Value::Null,
    }
}

fn pr_check_to_value(check: &PrCheckDetail) -> Value {
    let mut map = Map::new();
    map.insert("name".to_string(), Value::String(check.name.clone()));
    map.insert(
        "status".to_string(),
        Value::String(check_status_id(check.status).to_string()),
    );
    // conclusion and url are required PRCheckDetail fields that carry null
    // (not omitted) when unmapped/absent; checkRunId/workflowRunId are never
    // set by this mapping, so they stay absent.
    map.insert("conclusion".to_string(), conclusion_to_value(check.conclusion));
    map.insert(
        "url".to_string(),
        match &check.url {
            Some(url) => Value::String(url.clone()),
            None => Value::Null,
        },
    );
    Value::Object(map)
}
