//! Runtime protocol compatibility constants, ported from
//! `src/shared/protocol-version.ts`.
//!
//! Desktop, headless server, CLI, and mobile builds may drift in app version
//! but must agree on this protocol range before runtime RPCs are allowed.

pub const RUNTIME_PROTOCOL_VERSION: i64 = 3;
pub const MIN_COMPATIBLE_RUNTIME_CLIENT_VERSION: i64 = 2;
pub const MIN_COMPATIBLE_RUNTIME_SERVER_VERSION: i64 = 2;

/// Mobile-facing aliases (COMPAT: mobile builds that still read desktop/mobile names).
pub const DESKTOP_PROTOCOL_VERSION: i64 = RUNTIME_PROTOCOL_VERSION;
pub const MIN_COMPATIBLE_MOBILE_VERSION: i64 = MIN_COMPATIBLE_RUNTIME_CLIENT_VERSION;

/// Capability strings advertised by a runtime server.
pub const RUNTIME_CAPABILITIES: &[&str] = &[
    "runtime.status.compat.v1",
    "runtime.environments.v1",
    "browser.screencast.v1",
    "terminal.binary-stream.v1",
    "terminal.multiplex.v1",
    "workspace-ports.v1",
    "mobile.tasks.v1",
];
