// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Centralized audit trail for containment gate denials (#5533).
//!
//! Every containment gate that denies an operation calls [`log_denial`]
//! before returning an error. All events share the log target
//! `containment_audit`, allowing operators to filter and aggregate
//! security events independently of general application logging.
//!
//! ## Log target
//!
//! All audit events use `target: "containment_audit"`. Configure your
//! log backend to route this target to a dedicated audit sink:
//!
//! ```text
//! RUST_LOG=containment_audit=warn
//! ```

use crate::ContainmentMode;

/// Record a containment gate denial.
///
/// Called by gate sites across all subsystems before returning a denial
/// error. The consistent log target `containment_audit` allows log
/// backends to route all security events to a dedicated audit sink.
///
/// # Arguments
///
/// * `subsystem` — The gate's domain (e.g. `"process"`, `"mcp"`, `"network"`, `"plugins"`).
/// * `operation` — What was attempted (e.g. `"spawn '/bin/bash'"`, `"tool 'run_command'"`).
/// * `mode` — The active containment mode that triggered the denial.
/// * `reason` — Why the operation was denied (e.g. `"NoFork"`, `"not in allowlist"`).
#[inline]
pub fn log_denial(subsystem: &str, operation: &str, mode: ContainmentMode, reason: &str) {
    aterm_log::__log(
        aterm_log::Level::Warn,
        "containment_audit",
        format_args!("DENIED: {subsystem}::{operation} in {mode} mode — {reason}"),
        Some(file!()),
        Some(line!()),
    );
}
