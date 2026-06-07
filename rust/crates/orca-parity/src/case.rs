//! Records the result of running one parity case through the Rust port.

use serde_json::Value;

/// One executed parity case: the shared input, the optional golden value
/// transcribed from the module's `.test.ts`, and what the Rust port produced.
pub struct ParityRun {
    pub module: String,
    pub case_index: usize,
    pub function: String,
    pub note: String,
    pub input: Value,
    pub expected: Option<Value>,
    pub rust_output: Value,
}
