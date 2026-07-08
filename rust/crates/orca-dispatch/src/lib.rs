//! Shippable per-module dispatch registry.
//!
//! Owns the aggregate `dispatch(module, function, input)` over every ported
//! pure-domain module (each delegating to its real domain crate). `orca-parity`
//! re-exports this so the differential vector corpus stays the acceptance gate,
//! while production reaches the SAME registry through a single napi export
//! (Electron main) and a single wasm export (renderer/relay) — so any ported
//! module ships via a thin TS wrapper with no further per-module Rust work.
pub mod modules;

pub use modules::dispatch;

// Semantic-JSON equality used by the moved modules' in-file golden self-tests
// (test-only; the shipped surface is just `dispatch`).
#[cfg(test)]
mod compare;
