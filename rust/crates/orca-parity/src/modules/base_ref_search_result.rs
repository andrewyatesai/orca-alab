//! Parity dispatch for `orca_core::base_ref_search_result` vs
//! `src/shared/base-ref-search-result.ts`.

use orca_core::base_ref_search_result::{
    derive_legacy_local_branch_name, legacy_base_ref_search_result,
};
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "deriveLegacyLocalBranchName" => match input.as_str() {
            Some(ref_name) => Value::String(derive_legacy_local_branch_name(ref_name)),
            None => json!({ "__parity_error__": "expected string input" }),
        },
        "legacyBaseRefSearchResult" => match input.as_str() {
            Some(ref_name) => {
                // Match `JSON.stringify` of the TS `BaseRefSearchResult` object.
                let result = legacy_base_ref_search_result(ref_name);
                json!({
                    "refName": result.ref_name,
                    "localBranchName": result.local_branch_name,
                })
            }
            None => json!({ "__parity_error__": "expected string input" }),
        },
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}
