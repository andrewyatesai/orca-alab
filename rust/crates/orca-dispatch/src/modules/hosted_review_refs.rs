//! Parity dispatch for `orca_core::hosted_review_refs` vs
//! `src/shared/hosted-review-refs.ts`.

use orca_core::hosted_review_refs::{
    normalize_hosted_review_base_ref, normalize_hosted_review_head_ref,
};
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    // Vectors only carry string refs; both functions take a single string arg.
    let reference = input.as_str().unwrap_or_default();
    match function {
        "normalizeHostedReviewHeadRef" => {
            Value::String(normalize_hosted_review_head_ref(reference))
        }
        "normalizeHostedReviewBaseRef" => {
            Value::String(normalize_hosted_review_base_ref(reference))
        }
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}
