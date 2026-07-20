//! `orca-pty` — local PTY spawning for Orca, the native replacement for
//! `node-pty` (used by `src/main/rate-limits/*` and the runtime's `pty:spawn`).
//!
//! Backed by vendored `portable-pty`. A [`session::PtySession`] mirrors the
//! node-pty surface Orca relies on: spawn `(program, args, {cwd, env, cols,
//! rows})`, stream output via a reader, `write`, `resize`, `process_id`,
//! `kill`, `wait`.

pub mod session;

pub use session::{PtyCommand, PtySession, PtySize};
