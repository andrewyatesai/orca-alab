// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

#![deny(unsafe_op_in_unsafe_fn)]

//! 4-mode containment POLICY DATA MODEL for AI agent isolation.
//!
//! ## Honest scope (`ATERM_DESIGN` §0.1)
//!
//! This crate is a **policy data model plus a spawn-seam actuator**. It maps a
//! [`ContainmentMode`] to a [`Capabilities`] set, records the chosen mode at the
//! spawn seam, and — as of this increment — actuates a REAL OS sandbox (macOS
//! Seatbelt `(deny network*)` PLUS a conservative `(deny file-read* file-write*)`
//! over the user's secret-credential directories, via `sandbox-exec`) for
//! `Containment` mode; GENERAL OS filesystem scoping remains a follow-up. The
//! mode→capability MAPPING is
//! DESIGNED for non-escalation/monotonicity and encodes those as Kani proof
//! harnesses ([`kani_proofs`]); that is a property of the mapping, not a proof
//! that the operating system enforces anything. (The harnesses are opt-in — see
//! `scripts/verify-kani-proofs.sh` — and a TLA+ model is the intended formal
//! spec but is NOT yet in-tree.)
//!
//! What is actuated TODAY (see [`actuator`]):
//! - the spawn seam consults [`actuator::decide`] before forking the shell;
//! - the chosen mode and the OS-sandbox posture are written to the audit log;
//! - resource limits (`setrlimit`) are installed fail-closed by `aterm-sandbox`
//!   / `aterm-pty` in the child before exec;
//! - **OS NETWORK + SECRET-FS sandbox (macOS).** In `Containment` mode the spawn
//!   is wrapped with `/usr/bin/sandbox-exec -p <SBPL>` applying the per-user
//!   profile from [`sbpl::profile_for`] — `(version 1)(allow default)(deny
//!   network*)` PLUS a conservative `(deny file-read* file-write* …)` over the
//!   secret-credential set under `$HOME` (`.ssh`, `.aws`, `.gnupg`, `.config/gh`,
//!   `.config/aterm`, `.netrc`). So the kernel Seatbelt DENIES all network AND
//!   read/write of those credential stores to the child shell, while the rest of
//!   the filesystem stays usable so a normal `$SHELL` works.
//!   [`actuator::os_sandbox_actuated`] is `true` on macOS and
//!   [`actuator::network_sandbox_actuated`] reports it per-mode; both the network
//!   deny and the secret deny are verified by the actuator's enforcement-proof
//!   tests. The launcher fails CLOSED if the wrapper is missing (it refuses to
//!   spawn an unsandboxed shell when the policy demands the sandbox).
//!
//! What is still **deferred** (honest, NOT yet a guarantee):
//! - **GENERAL OS FILESYSTEM scoping.** Beyond the conservative secret set above,
//!   the Seatbelt profile is `(allow default)` for the filesystem (a blanket `(deny
//!   file-*)` base tight enough to matter also breaks a normal `$SHELL`); scoping
//!   the WHOLE filesystem per [`FsCapability`] is an explicit FOLLOW-UP. The audit
//!   log and `os_sandbox_actuated`/`network_sandbox_actuated` say exactly this —
//!   network enforced, secret-dir read/write enforced, general filesystem not yet
//!   scoped.
//! - **Network ENFORCEMENT off macOS** (a Linux seccomp/Landlock lane) and
//!   **allowlist-mode** network scoping (`Safety`) — both follow-ups; there
//!   `os_sandbox_actuated` is `false` and the actuator logs the unconfined posture
//!   explicitly, so it is an audited choice, never a silent claim.
//!
//! aterm operates in one of four containment modes, set once by the launcher:
//!
//! | Mode | Trust Level | Description (policy intent) |
//! |------|------------|-------------|
//! | **Master** | Full | Developer mode — all capabilities unrestricted |
//! | **User** | Normal | Standard safeguards — output shadow-scanned |
//! | **Safety** | Reduced | Allowlisted operations only |
//! | **Containment** | Hostile | Most restrictive POLICY (no network, filtered I/O) — the NO-NETWORK part AND a conservative SECRET-directory read/write deny (`~/.ssh`, `~/.aws`, `~/.gnupg`, `~/.config/gh`, `~/.config/aterm`, `~/.netrc`) are OS-enforced on macOS (Seatbelt `deny network*` + `deny file-read*/file-write*` via `sandbox-exec`); GENERAL OS filesystem scoping is the deferred follow-up |
//!
//! ## Core Axiom
//!
//! **ALL DATA IS CODE.** Any byte crossing a trust boundary is a potential
//! instruction to an AI agent. The containment system treats all external
//! data as untrusted by default.
//!
//! ## Safety Properties (of the POLICY MAPPING — Kani harnesses; TLA+ model planned)
//!
//! These are properties of the mode→capability mapping, encoded as Kani proof
//! harnesses in [`kani_proofs`] (a `tla/Containment.tla` model is the intended
//! formal spec but is not yet in-tree). They are properties of the mapping data
//! model, NOT of any OS enforcement:
//!
//! - **`NonEscalation`** — mode never increases in capability
//! - **`CapabilitiesMatchMode`** — capabilities always consistent with mode
//! - **`ModeImmutableAfterInit`** — model-level launcher-ownership invariant;
//!   runtime immutability comes from `OnceLock` plus `NonEscalation`
//! - **`ContainmentMinimal`** — Containment mode is the minimal POLICY (every
//!   capability value at its floor) — minimality of the data model, not OS isolation
//! - **`MonotonicCapabilities`** — capabilities only decrease over time
//!
//! ## Usage
//!
//! ```rust
//! use aterm_containment::{ContainmentMode, ContainmentPolicy, init_mode};
//!
//! // At startup (called once by launcher):
//! init_mode(ContainmentMode::User).expect("mode already set");
//!
//! // Anywhere in aterm:
//! let mode = aterm_containment::current_mode();
//! let caps = ContainmentPolicy::capabilities(mode);
//! ```
//!
//! The intended formal spec is a `tla/Containment.tla` model (planned, NOT yet
//! in-tree); the in-tree checks are the [`kani_proofs`] harnesses.

#![deny(missing_docs)]
#![deny(clippy::all)]
#![warn(clippy::pedantic)]
#![cfg_attr(test, allow(clippy::all, clippy::pedantic))]
#![allow(
    clippy::module_name_repetitions,
    reason = "containment::ContainmentMode is clear"
)]
#![allow(
    clippy::missing_errors_doc,
    reason = "error semantics are clear from return types"
)]
#![allow(
    clippy::inline_always,
    reason = "containment hot paths require cross-crate inlining for zero-cost in Master/User modes (#5559)"
)]

pub mod actuator;
pub(crate) mod allowlist;
pub(crate) mod audit;
pub(crate) mod capability;
#[cfg(kani)]
mod kani_proofs;
pub(crate) mod mode;
pub(crate) mod output_filter;
pub(crate) mod policy;
pub mod sbpl;

pub use actuator::{
    SpawnDecision, decide as decide_spawn, network_sandbox_actuated, os_sandbox_actuated,
};
#[cfg(unix)]
pub use allowlist::verify_executable_fd;
pub use allowlist::{
    AllowlistConfig, AllowlistError, init_allowlist, is_mcp_allowed, is_network_allowed,
    is_plugin_allowed, is_process_allowed,
};
pub use audit::log_denial;
pub use capability::{
    CommandCapability, FsCapability, InputCapability, McpCapability, NetworkCapability,
    OutputCapability, PluginCapability, ProcessCapability,
};
pub use mode::{ContainmentMode, ParseModeError};
pub use output_filter::OutputSanitizer;
pub use policy::{Capabilities, ContainmentPolicy};
pub use sbpl::{NETWORK_DENY_PROFILE, SANDBOX_EXEC_PATH, profile_for as sbpl_profile_for};

use std::sync::OnceLock;

/// Global containment mode, set once at startup.
static MODE: OnceLock<ContainmentMode> = OnceLock::new();

/// Error returned when mode initialization fails.
#[derive(Debug, Clone, aterm_error::Error)]
#[non_exhaustive]
pub enum InitError {
    /// Mode was already initialized (cannot be changed).
    #[error("containment mode already set to {existing}, cannot change to {attempted}")]
    AlreadyInitialized {
        /// The mode that was previously set.
        existing: ContainmentMode,
        /// The mode that was attempted.
        attempted: ContainmentMode,
    },
}

/// Initialize the containment mode for this process.
///
/// Must be called exactly once at startup, before any subsystem queries the
/// mode. The mode is immutable after this call — subsequent calls return
/// [`InitError::AlreadyInitialized`].
///
/// This function establishes runtime immutability directly through
/// `OnceLock` single-init semantics. In the TLA+ model,
/// `ModeImmutableAfterInit` records launcher ownership of the initialized
/// mode, and `NonEscalation` captures the security effect of staying at or
/// below that starting mode.
///
/// # Errors
///
/// Returns `InitError::AlreadyInitialized` if called more than once.
///
/// # Panics
///
/// Panics (unreachable) if `OnceLock::set` fails but `OnceLock::get`
/// returns `None`. This cannot happen with a correctly functioning
/// `OnceLock`.
pub fn init_mode(mode: ContainmentMode) -> Result<(), InitError> {
    MODE.set(mode).map_err(|_| {
        let existing = *MODE.get().expect("set failed but value exists");
        InitError::AlreadyInitialized {
            existing,
            attempted: mode,
        }
    })
}

/// Get the current containment mode.
///
/// # Panics
///
/// Panics if [`init_mode`] has not been called. This is a programmer
/// error — the launcher must set the mode before any subsystem runs.
#[inline(always)]
#[must_use]
pub fn current_mode() -> ContainmentMode {
    *MODE
        .get()
        .expect("containment mode not initialized — call init_mode() at startup")
}

/// Get the current containment mode, if initialized.
///
/// Returns `None` if [`init_mode`] has not been called yet.
#[inline(always)]
#[must_use]
pub fn try_current_mode() -> Option<ContainmentMode> {
    MODE.get().copied()
}

/// Get the current containment mode, defaulting to [`ContainmentMode::Containment`]
/// if not initialized.
///
/// **Fail-closed behavior:** if [`init_mode`] was never called, this returns
/// the most restrictive mode (`Containment`), which denies all operations.
/// Library consumers must explicitly call [`init_mode`] at startup to get
/// the access level they need.
#[inline(always)]
#[must_use]
pub fn mode_or_containment() -> ContainmentMode {
    try_current_mode().unwrap_or(ContainmentMode::Containment)
}

#[cfg(test)]
fn current_capabilities() -> Capabilities {
    ContainmentPolicy::capabilities(current_mode())
}

/// Initialize mode from environment variable `ATERM_CONTAINMENT_MODE`.
///
/// Falls back to the provided default if the env var is not set.
/// Returns the resolved mode on success.
///
/// # Errors
///
/// Returns error if the env var contains an invalid value or mode was
/// already initialized.
pub fn init_mode_from_env(
    default: ContainmentMode,
) -> Result<ContainmentMode, InitModeFromEnvError> {
    let mode = match std::env::var("ATERM_CONTAINMENT_MODE") {
        Ok(val) => val
            .parse::<ContainmentMode>()
            .map_err(InitModeFromEnvError::Parse)?,
        Err(_) => default,
    };
    init_mode(mode).map_err(InitModeFromEnvError::Init)?;
    Ok(mode)
}

/// Error from [`init_mode_from_env`].
#[derive(Debug, aterm_error::Error)]
#[non_exhaustive]
pub enum InitModeFromEnvError {
    /// Invalid mode string in environment variable.
    #[error("invalid ATERM_CONTAINMENT_MODE: {0}")]
    Parse(#[from] ParseModeError),
    /// Mode already initialized.
    #[error("{0}")]
    Init(#[from] InitError),
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: Because OnceLock is process-global, we can't test init_mode
    // isolation across tests in the same binary. Policy tests are pure
    // and don't touch global state — those are the primary verification.

    #[test]
    fn test_policy_without_init() {
        // Policy functions are pure — they don't require init_mode.
        let caps = ContainmentPolicy::capabilities(ContainmentMode::Safety);
        assert_eq!(caps.network, NetworkCapability::Allowlist);
    }

    #[test]
    fn test_try_current_mode_does_not_panic() {
        // try_current_mode never panics, even before init.
        let _ = try_current_mode();
    }

    #[test]
    fn test_init_mode_succeeds_or_already_set() {
        // Try to initialize. If another test already set it, that's fine.
        let result = init_mode(ContainmentMode::User);
        match result {
            Ok(()) => {
                assert_eq!(current_mode(), ContainmentMode::User);
            }
            Err(InitError::AlreadyInitialized { .. }) => {
                // Another test set it first — verify it's readable.
                let _ = current_mode();
            }
        }
    }

    #[test]
    fn test_capabilities_for_all_modes() {
        // Pure policy tests — no global state needed.
        for mode in [
            ContainmentMode::Master,
            ContainmentMode::User,
            ContainmentMode::Safety,
            ContainmentMode::Containment,
        ] {
            let caps = ContainmentPolicy::capabilities(mode);
            let _ = (
                caps.network,
                caps.fs,
                caps.process,
                caps.mcp,
                caps.plugins,
                caps.output,
                caps.input,
            );
        }
    }

    /// Verify InitError::AlreadyInitialized error message includes both modes.
    #[test]
    fn test_init_error_message_includes_modes() {
        let err = InitError::AlreadyInitialized {
            existing: ContainmentMode::Containment,
            attempted: ContainmentMode::Master,
        };
        let msg = err.to_string();
        assert!(
            msg.contains("Containment"),
            "error should mention existing mode: {msg}"
        );
        assert!(
            msg.contains("Master"),
            "error should mention attempted mode: {msg}"
        );
    }

    /// Verify InitModeFromEnvError variants have useful messages.
    #[test]
    fn test_init_mode_from_env_error_variants() {
        // Parse error wraps ParseModeError
        let parse_err = InitModeFromEnvError::Parse(ParseModeError("bogus".to_string()));
        let msg = parse_err.to_string();
        assert!(
            msg.contains("ATERM_CONTAINMENT_MODE"),
            "parse error should reference env var: {msg}"
        );
        assert!(
            msg.contains("bogus"),
            "parse error should include bad value: {msg}"
        );

        // Init error wraps InitError
        let init_err = InitModeFromEnvError::Init(InitError::AlreadyInitialized {
            existing: ContainmentMode::User,
            attempted: ContainmentMode::Safety,
        });
        let msg = init_err.to_string();
        assert!(
            msg.contains("already set"),
            "init error should explain double-init: {msg}"
        );
    }

    /// Verify current_capabilities returns Capabilities matching the mode.
    #[test]
    fn test_current_capabilities_matches_policy() {
        // If mode was initialized by another test, verify consistency
        if let Some(mode) = try_current_mode() {
            let caps = current_capabilities();
            let expected = ContainmentPolicy::capabilities(mode);
            assert_eq!(
                caps, expected,
                "current_capabilities() != policy for {mode}"
            );
        }
    }

    /// Verify mode_or_containment returns mode if set, Containment otherwise.
    #[test]
    fn test_mode_or_containment_returns_mode_or_default() {
        let result = mode_or_containment();
        if let Some(mode) = try_current_mode() {
            assert_eq!(result, mode, "should return initialized mode");
        } else {
            assert_eq!(
                result,
                ContainmentMode::Containment,
                "should default to Containment when uninitialized"
            );
        }
    }
}
