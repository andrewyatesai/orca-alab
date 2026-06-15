// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Spawn-seam containment actuator — the bridge from the policy DATA MODEL
//! (mode → [`Capabilities`](crate::Capabilities)) to a real, logged decision at
//! the one place aterm forks a child shell.
//!
//! ## What this is, honestly (`ATERM_DESIGN` §0.1 / §5.6)
//!
//! The rest of this crate is a *policy data model*: it maps a [`ContainmentMode`]
//! to a [`Capabilities`](crate::Capabilities) set and is TLA+-checked for monotonicity/non-escalation
//! of THAT MAPPING. It does NOT, by itself, make the operating system enforce
//! anything — before this module nothing consulted it at the spawn seam.
//!
//! This module is the actuation seam. Given the resolved mode it produces a
//! [`SpawnDecision`] that the GUI launcher consults BEFORE handing the PTY seam a
//! spawn capability. Two things are actuated TODAY:
//!
//! 1. **Process-capability gate (actuated).** The [`ProcessCapability`] for the
//!    mode is checked. `Full`/`Restricted`/`NoFork` all permit the *initial*
//!    interactive shell (`NoFork` means "no fork — exec only, for the initial
//!    shell"), so a normal `$SHELL` still spawns; but the decision, the mode, and
//!    the fact that it is permitted are recorded via the containment audit log.
//! 2. **Resource limits (actuated, elsewhere).** `aterm-sandbox` installs
//!    `setrlimit` bounds in the child before exec, fail-closed (`aterm-pty`).
//!
//! What is **NOT** actuated (deferred, deny-and-log honest):
//!
//! - **OS filesystem/network sandbox (macOS Seatbelt SBPL / Endpoint Security).**
//!   A real `sandbox_init` profile that denies network in `Containment` mode and
//!   scopes the filesystem per [`FsCapability`](crate::FsCapability) is NOT implemented here, because
//!   it cannot be VERIFIED in the headless CI this was built in. Rather than ship
//!   an unverified actuator and claim "isolation", [`os_sandbox_actuated`] returns
//!   `false` and [`decide`] LOGS that the OS sandbox is not in force for the
//!   chosen mode. An unconfined posture is therefore an explicit, audited choice —
//!   never a silent false guarantee.
//!
//! See `tla/Containment.tla` for the policy model and `ATERM_DESIGN` §5.6 for the
//! fail-closed precondition this seam is the first concrete step toward.

use crate::audit::log_denial;
use crate::capability::ProcessCapability;
use crate::mode::ContainmentMode;
use crate::policy::ContainmentPolicy;

/// Audit subsystem label for spawn-seam containment events.
const SUBSYSTEM: &str = "spawn";

/// Whether a REAL operating-system sandbox (Seatbelt/Endpoint Security on macOS,
/// seccomp/Landlock on Linux) is actuated by this build at the spawn seam.
///
/// This is intentionally `false`: the OS actuator is ROADMAP work (`ATERM_DESIGN`
/// §5.6) and was NOT implemented/verified here. Callers and docs must not claim
/// OS isolation while this returns `false`. The honest posture in force today is
/// the process-capability gate (this module) plus `setrlimit` resource bounds
/// (`aterm-sandbox`, applied fail-closed by `aterm-pty`).
#[must_use]
pub const fn os_sandbox_actuated() -> bool {
    false
}

/// The actuated decision for the single spawn seam, given a containment mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SpawnDecision {
    /// Spawning the initial shell is permitted. `os_sandbox` records whether a
    /// real OS sandbox backs this mode (today always `false` — see
    /// [`os_sandbox_actuated`]); when `false`, an explicit audit line was logged
    /// so an unconfined posture is never silent.
    Permit {
        /// The mode this decision was made for.
        mode: ContainmentMode,
        /// Whether a real OS sandbox is in force (see [`os_sandbox_actuated`]).
        os_sandbox: bool,
    },
    /// Spawning is denied for this mode. A denial was logged to the containment
    /// audit trail. (No current mode reaches this — the initial shell is allowed
    /// in every mode, `NoFork` included — but the variant exists so a future,
    /// stricter policy denies fail-closed rather than silently permitting.)
    Deny {
        /// The mode this decision was made for.
        mode: ContainmentMode,
    },
}

impl SpawnDecision {
    /// Whether this decision permits the spawn.
    #[must_use]
    pub const fn is_permitted(self) -> bool {
        matches!(self, SpawnDecision::Permit { .. })
    }
}

/// Decide — and AUDIT — whether the single PTY spawn seam may run for `mode`.
///
/// This is the seam wiring required by `ATERM_DESIGN` §5.6: the spawn is now gated
/// on the containment decision, the chosen mode is logged, and — because no real
/// OS sandbox is actuated yet — the unconfined posture is logged EXPLICITLY so it
/// is an auditable choice, not a silent gap.
///
/// The initial interactive shell is permitted in every mode (including
/// `Containment`/`NoFork`, whose contract is "exec only, for the initial
/// shell"). A hypothetical future `ProcessCapability` below `NoFork` would
/// fail closed via [`SpawnDecision::Deny`].
#[must_use]
pub fn decide(mode: ContainmentMode) -> SpawnDecision {
    let process_cap = ContainmentPolicy::process(mode);
    // Audit the decision and the OS-sandbox posture for the chosen mode. We log
    // through the same containment audit target so operators see one stream.
    if !os_sandbox_actuated() {
        // Not a denial of the spawn, but an explicit record that the OS-level
        // confinement for this mode is NOT in force — the honest, non-silent
        // statement of the gap (per §0.1). Routed through the audit log so it is
        // greppable alongside real denials.
        log_denial(
            SUBSYSTEM,
            "os-sandbox actuation",
            mode,
            "OS sandbox not actuated (ROADMAP §5.6); relying on rlimits + process-cap gate",
        );
    }
    match process_cap {
        // Every currently-defined capability permits the INITIAL shell.
        ProcessCapability::Full
        | ProcessCapability::Restricted
        | ProcessCapability::NoFork => {
            SpawnDecision::Permit { mode, os_sandbox: os_sandbox_actuated() }
        }
        // Defensive default: any future, more-restrictive variant fails closed.
        #[allow(unreachable_patterns)]
        _ => {
            log_denial(SUBSYSTEM, "spawn initial shell", mode, "process capability denies fork/exec");
            SpawnDecision::Deny { mode }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_sandbox_is_honestly_not_actuated() {
        // The whole point of SEC-1's honest downgrade: we do NOT claim OS
        // isolation. If a real actuator is ever wired, flip this and the docs.
        assert!(!os_sandbox_actuated());
    }

    #[test]
    fn every_mode_permits_the_initial_shell() {
        for mode in [
            ContainmentMode::Master,
            ContainmentMode::User,
            ContainmentMode::Safety,
            ContainmentMode::Containment,
        ] {
            let d = decide(mode);
            assert!(
                d.is_permitted(),
                "the initial shell must be permitted in {mode} mode, got {d:?}"
            );
            // And the permit honestly reports os_sandbox == false today.
            assert_eq!(
                d,
                SpawnDecision::Permit { mode, os_sandbox: false },
                "permit must record the (un)actuated OS-sandbox posture"
            );
        }
    }

    #[test]
    fn decision_is_permitted_helper_matches_variant() {
        assert!(SpawnDecision::Permit {
            mode: ContainmentMode::User,
            os_sandbox: false
        }
        .is_permitted());
        assert!(!SpawnDecision::Deny { mode: ContainmentMode::Containment }.is_permitted());
    }
}
