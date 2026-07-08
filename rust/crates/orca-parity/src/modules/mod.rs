//! Parity's dispatch entry point.
//!
//! The full per-module registry now lives in the shippable `orca-dispatch`
//! crate (one registry shared by production napi/wasm + this harness). Two
//! modules stay local because they are parity-only oracles with NO production
//! dispatch consumer, and each would bloat the shipped artifacts: `orchestration
//! -store` needs rusqlite/orca-runtime, and `nacl-box` pulls the crypto stack
//! (curve25519/salsa20/poly1305) into the relay wasm for no caller (E2EE ships
//! via the dedicated orca-crypto-wasm). So this layers those two over the
//! aggregate dispatch and otherwise defers to `orca_dispatch`.
use serde_json::Value;

pub mod nacl_box;
pub mod orchestration_store;

/// Returns `None` when no Rust dispatch is registered for `module`.
pub fn dispatch(module: &str, function: &str, input: &Value) -> Option<Value> {
    match module {
        "orchestration-store" => Some(orchestration_store::dispatch(function, input)),
        "nacl-box" => Some(nacl_box::dispatch(function, input)),
        _ => orca_dispatch::dispatch(module, function, input),
    }
}
