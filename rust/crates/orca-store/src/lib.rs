//! `orca-store` — persistence for Orca.
//!
//! A thin synchronous SQLite adapter, the native replacement for
//! `src/main/sqlite/sync-database.ts` (which wraps Electron's `node:sqlite`).
//! Backed by vendored, bundled SQLite (the C amalgamation is compiled from
//! `rust/vendor/`), so there is no system SQLite dependency and builds stay
//! offline.

pub mod database;

pub use database::{Database, OpenOptions, StoreError};
