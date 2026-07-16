//! Parity dispatch for `orca_provider_backoff` vs the TS twin
//! `src/main/rate-limits/active-failure-backoff.ts`.
//!
//! F4 promotion of an E1-certified, autoformalize-TRUSTED decision core through
//! the one production `orca-dispatch` seam: additive only — no call site is cut
//! over (the shipping throttle stays TS), so this wires the Rust core in for
//! machine-checked parity without a hot-path change. The TS twin parameterizes
//! base/ceiling; the Rust core owns them as constants, so the adapter pins the
//! same `ACTIVE_FAILURE_REFETCH_MS` / `MAX_ACTIVE_FAILURE_REFETCH_MS` values.

use orca_provider_backoff::active_failure_refetch_throttle_ms;
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        "activeFailureRefetchThrottleMs" => {
            // A negative or absent streak reads as 0, matching the TS `max(0, …)`
            // guard where a non-positive streak collapses to the base wait.
            let streak = input
                .get("streak")
                .and_then(Value::as_i64)
                .map_or(0, |n| n.max(0).min(u32::MAX as i64) as u32);
            json!(active_failure_refetch_throttle_ms(streak))
        }
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}
