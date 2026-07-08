//! Parity dispatch for `orca_agents::tui_agent_startup` vs
//! `src/shared/tui-agent-startup.ts`. The JSON marshalling now lives in
//! `orca_agents::tui_agent_startup_json` so napi (main), wasm (renderer), and
//! this oracle share one boundary; this module just delegates.

use serde_json::Value;

pub fn dispatch(function: &str, input: &Value) -> Value {
    orca_agents::tui_agent_startup_json::dispatch(function, input)
}
