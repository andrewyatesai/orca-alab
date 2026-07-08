//! Parity dispatch for the terminal quick-command helpers vs
//! `src/shared/terminal-quick-commands.ts`. Delegates to the shared JSON boundary
//! that the napi addon + wasm also run, so the oracle can never drift from them.

use serde_json::Value;

pub fn dispatch(function: &str, input: &Value) -> Value {
    orca_agents::terminal_quick_command_json::dispatch(function, input)
}
