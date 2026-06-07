//! `orca-session` — a live terminal session.
//!
//! Composes a PTY (`orca-pty`) with a headless terminal (`orca-terminal`): a
//! background thread streams the child's output into the terminal grid, while
//! the foreground exposes the grid for rendering and accepts input/resize.
//! This is the unit the UI (and the FFI/Swift shell) drives.

pub mod session;

pub use orca_pty::{PtyCommand, PtySize};
pub use session::TerminalSession;
