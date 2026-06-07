//! Parity dispatch for `orca_core::hosted_review_queue` vs
//! `src/shared/hosted-review-queue.ts`.
//!
//! The three pure classifiers each take a single argument, so the vector input
//! is that argument directly. The Rust summary struct is reconstructed from the
//! JSON by hand (the lib types don't derive Deserialize); only the fields the
//! classifiers read are populated. Returns are plain strings / bools.

use orca_core::hosted_review_queue::{
    hosted_review_identity_key, review_needs_response, review_ready_to_merge, HostedReviewIdentity,
    HostedReviewQueueSummary, HostedReviewUser, ThreadSummary,
};
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "hostedReviewIdentityKey" => {
            Value::String(hosted_review_identity_key(&identity_from_json(input)))
        }
        "reviewNeedsResponse" => Value::Bool(review_needs_response(&summary_from_json(input))),
        "reviewReadyToMerge" => Value::Bool(review_ready_to_merge(&summary_from_json(input))),
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

fn string_field(input: &Value, key: &str) -> Option<String> {
    input.get(key).and_then(Value::as_str).map(str::to_string)
}

fn identity_from_json(value: &Value) -> HostedReviewIdentity {
    HostedReviewIdentity {
        provider: string_field(value, "provider").unwrap_or_default(),
        host: string_field(value, "host").unwrap_or_default(),
        owner: string_field(value, "owner").unwrap_or_default(),
        repo: string_field(value, "repo").unwrap_or_default(),
        number: value.get("number").and_then(Value::as_u64).unwrap_or_default(),
    }
}

fn user_from_json(value: &Value) -> HostedReviewUser {
    HostedReviewUser {
        login: value.get("login").and_then(Value::as_str).map(str::to_string),
        is_bot: value.get("isBot").and_then(Value::as_bool).unwrap_or(false),
    }
}

fn summary_from_json(input: &Value) -> HostedReviewQueueSummary {
    HostedReviewQueueSummary {
        identity: input.get("identity").map(identity_from_json).unwrap_or_default(),
        state: string_field(input, "state").unwrap_or_default(),
        // `author` is `HostedReviewUser | null`; a JSON null collapses to None.
        author: input.get("author").filter(|value| value.is_object()).map(user_from_json),
        requested_reviewer_logins: input
            .get("requestedReviewerLogins")
            .and_then(Value::as_array)
            .map(|logins| logins.iter().filter_map(Value::as_str).map(str::to_string).collect()),
        updated_at: string_field(input, "updatedAt").unwrap_or_default(),
        last_viewed_at: input.get("lastViewedAt").and_then(Value::as_i64),
        mergeable: string_field(input, "mergeable"),
        checks_status: string_field(input, "checksStatus"),
        thread_summary: input.get("threadSummary").filter(|value| value.is_object()).map(|value| {
            ThreadSummary {
                unresolved_count: value.get("unresolvedCount").and_then(Value::as_u64).unwrap_or(0),
            }
        }),
        draft: input.get("draft").and_then(Value::as_bool).unwrap_or(false),
        merge_state_status: string_field(input, "mergeStateStatus"),
        review_decision: string_field(input, "reviewDecision"),
    }
}
