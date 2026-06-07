//! `orca-runtime` — runtime orchestration for Orca.
//!
//! The multi-agent coordination store (messages, tasks, dispatch contexts,
//! decision gates, coordinator runs), ported from
//! `src/main/runtime/orchestration/db.ts`, on top of `orca-store`'s vendored
//! SQLite.

pub mod orchestration;

pub use orchestration::{Message, NewMessage, OrchestrationDb, Task};
