// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Andrew Yates

//! Resource sandbox for spawned children (ATERM_DESIGN WS-G).
//!
//! aterm spawns a child `$SHELL`; this confines that child's resource use with
//! POSIX `setrlimit` bounds, applied in the child after `fork` and before `exec`
//! (so the limits are inherited by the shell and everything it runs). Installing
//! a sandbox is a privileged effect, so [`Limits::apply`] requires a
//! [`Cap<Sandbox>`] from `aterm-cap` (capability-gated; the cap cannot be
//! struct-literal-forged outside `aterm-cap` — see that crate for the exact,
//! honest scope of the guarantee, and §5.4 for the stronger sealed mint).
//!
//! This is the portable resource-limit layer. A macOS Seatbelt / Endpoint
//! Security profile (filesystem/network scoping) is a separate, platform-specific
//! lane on top of it and is not implemented here.
//!
//! STATUS (per §0.1): the cap gate and `setrlimit` application are tested (the
//! application is verified by reading the limit back); not yet Trust-proven.

use std::io;

use aterm_cap::{Cap, Tier};

/// The effect a capability authorizes here: installing a resource sandbox.
pub enum Sandbox {}

/// POSIX resource limits to apply. `None` leaves the corresponding limit
/// unchanged. Each value is installed as the **soft** limit; the inherited
/// **hard** ceiling is PRESERVED (never lowered) so the spawned `$SHELL` can
/// still raise its own soft limit from its rc — see [`set_limit`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Limits {
    /// CPU seconds (`RLIMIT_CPU`).
    pub cpu_seconds: Option<u64>,
    /// Address space / virtual memory in bytes (`RLIMIT_AS`).
    pub address_space: Option<u64>,
    /// Max written file size in bytes (`RLIMIT_FSIZE`).
    pub file_size: Option<u64>,
    /// Max open file descriptors (`RLIMIT_NOFILE`).
    pub open_files: Option<u64>,
}

impl Limits {
    /// A generous default for an interactive shell: cap address space and fds, but
    /// leave CPU and file size unbounded (an interactive shell legitimately runs
    /// long and writes large files).
    #[must_use]
    pub fn shell_default() -> Self {
        Limits {
            cpu_seconds: None,
            // macOS rejects ANY finite `RLIMIT_AS` (only `RLIM_INFINITY` is
            // accepted — `setrlimit` returns `EINVAL` otherwise), so leave the
            // address space unbounded there and rely on the other limits.
            address_space: if cfg!(target_os = "macos") {
                None
            } else {
                Some(16 * 1024 * 1024 * 1024) // 16 GiB
            },
            file_size: None,
            open_files: Some(8192),
        }
    }

    /// The PERMISSIVE limits for the daily-driver modes (Master / User): every
    /// limit `None`, so the spawned shell INHERITS the launching login shell's
    /// `rlimit`s unchanged — a terminal must not constrain the programs you run more
    /// than the shell that started it would. In particular it imposes NO `RLIMIT_AS`:
    /// that caps VIRTUAL address space, which CUDA/ML runtimes, the JVM, Go, and the
    /// sanitizers all RESERVE far in excess of resident use, so any finite cap breaks
    /// legitimate programs while bounding nothing real. Confinement in these modes is
    /// the capability gate (what the shell may DO), not a blanket memory cap. The
    /// hardened [`Self::shell_default`] caps (opted into via Safety / Containment)
    /// are unchanged.
    #[must_use]
    pub fn inherit() -> Self {
        Limits::default()
    }

    /// Apply these limits to the CURRENT process. Call in the child, after
    /// `fork`, before `exec`. Requires a `Trusted`+ [`Cap<Sandbox>`].
    ///
    /// The cap gate hard-fails, but the individual `setrlimit` calls are applied
    /// BEST-EFFORT: a resource the OS does not support (e.g. `RLIMIT_AS` on
    /// macOS) must NOT prevent the limits that DO work (`RLIMIT_NOFILE`) from
    /// being installed. Every limit is attempted; the first per-limit error is
    /// returned only after all have been tried, so one unsupported resource can
    /// never silently leave the child unconfined.
    ///
    /// The returned `Result` MUST NOT be discarded by a forking spawn seam: an
    /// `Err` here means confinement did not fully install, and the caller is
    /// required to fail closed (do NOT exec an unconfined child). `aterm-pty`'s
    /// child does exactly this — it `_exit(126)`s before `execve` on `Err`
    /// (ATERM_DESIGN §5.6, exit-before-exec).
    ///
    /// # Errors
    /// `PermissionDenied` if the capability tier is too low; otherwise the first
    /// `setrlimit` OS error encountered (after attempting every limit).
    ///
    /// SPEC: this is the real implementation of the `Apply` action of the external
    /// `Sandbox.tla` model (TRUST_NATIVE_TLA Phase 2, CONFINEMENT family). The spec's
    /// `AllSupportedApplied` invariant — once apply has run, EVERY restriction the
    /// policy *requested* that the OS *supports* is actually installed — is exactly
    /// the macOS no-op regression this best-effort-per-limit loop fixes: a requested
    /// limit the OS supports is never silently skipped because an earlier unsupported
    /// one (e.g. `RLIMIT_AS` on macOS) errored. Tier-1 conformance drives this method
    /// and projects `<<requested, supported, applied, done>>`
    /// (`tests/conformance_sandbox.rs`).
    // PROJECTION (TRUST_VACUITY_GATE §2.2 / finding 2): `Apply` projects the real
    // best-effort-per-limit apply loop onto the spec's `<<requested, supported,
    // applied, done>>` — the projection `conformance_sandbox.rs` drives in Tier-1.
    // The L2 obligation requires the projection NAME be present (Trust does not
    // execute it); `aterm_sandbox::Sandbox::project_apply` is that witness.
    #[cfg_attr(
        any(test, feature = "spec-anchors"),
        aterm_spec::refines(
            machine = "sandbox",
            action = "Apply",
            project = "aterm_sandbox::Sandbox::project_apply"
        )
    )]
    pub fn apply(&self, cap: &Cap<Sandbox>) -> io::Result<()> {
        aterm_cap::require(cap, Tier::Trusted)
            .map_err(|e| io::Error::new(io::ErrorKind::PermissionDenied, e.to_string()))?;
        let mut first_err: Option<io::Error> = None;
        for (resource, value) in [
            (libc::RLIMIT_CPU, self.cpu_seconds),
            (libc::RLIMIT_AS, self.address_space),
            (libc::RLIMIT_FSIZE, self.file_size),
            (libc::RLIMIT_NOFILE, self.open_files),
        ] {
            if let Err(e) = set_limit(resource, value) {
                first_err.get_or_insert(e);
            }
        }
        match first_err {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}

/// The per-restriction APPLY rule of `Limits::apply`, factored out as a pure
/// function so the fail-closed "requested ∧ supported ⇒ applied" discipline is
/// testable WITHOUT mutating the process-wide rlimits (and is the seam the
/// `Sandbox.tla` Tier-1 conformance projects).
///
/// This is the body of the spec's (correct, `Buggy=FALSE`) `Apply` action, slot by
/// slot: `applied[n]' = applied[n] ∨ (requested[n] ∧ supported[n])`. The real
/// [`Limits::apply`] loop attempts every *requested* limit (a `Some(_)` field) and
/// the OS accepts it iff that resource is *supported*; an unsupported one is skipped
/// best-effort and never blocks the supported ones (the macOS `RLIMIT_AS` no-op the
/// spec's `AllSupportedApplied` invariant forbids). `applied` here is the prior
/// applied set (all-FALSE before the first apply) so the rule is monotone/idempotent,
/// exactly as the spec models it.
#[must_use]
pub fn apply_step(requested: &[bool], supported: &[bool], applied: &[bool]) -> Vec<bool> {
    let k = requested.len();
    assert_eq!(supported.len(), k);
    assert_eq!(applied.len(), k);
    (0..k)
        .map(|n| applied[n] || (requested[n] && supported[n]))
        .collect()
}

/// The type of a `RLIMIT_*` resource selector, which differs by platform: glibc
/// Linux types `setrlimit`'s first argument (and its `RLIMIT_*` constants) as
/// `__rlimit_resource_t` (a `u32`), while macOS/BSD and musl use `c_int`. Aliasing
/// it keeps [`set_limit`] portable — the `RLIMIT_*` constants already have this
/// per-platform type, so they pass through without a cast.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
type RlimitResource = libc::__rlimit_resource_t;
#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
type RlimitResource = libc::c_int;

/// Set the SOFT `resource` limit to `value` while PRESERVING the inherited hard
/// ceiling (no-op when `value` is `None`).
///
/// We deliberately do NOT lower the hard limit. The spawned `$SHELL` (User mode
/// runs it transparently) must stay able to RAISE its own soft limit from its rc
/// — e.g. a `.zshrc` line `ulimit -n 65536` — exactly as under any other
/// terminal. Clamping the hard limit to the requested value broke that: a soft
/// request above the (also-lowered) hard limit aborts shell startup with
/// `ulimit: value exceeds hard limit`. Treating the value as a soft default the
/// child can raise up to the inherited hard ceiling is the correct semantics for
/// the non-actuated User sandbox. The soft value is clamped to the hard ceiling
/// so `setrlimit` can never `EINVAL` on `rlim_cur > rlim_max`.
fn set_limit(resource: RlimitResource, value: Option<u64>) -> io::Result<()> {
    let Some(v) = value else {
        return Ok(());
    };
    // Read the inherited limits so the hard ceiling is preserved. This is a bare
    // `getrlimit` syscall (no allocation), so it stays async-signal-safe in the
    // post-fork child where `apply` runs. If the read fails, fall back to the old
    // behavior (set both) rather than leaving the limit unconfined.
    let mut cur = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: valid resource id + a valid out-param for the call's duration.
    let hard = if unsafe { libc::getrlimit(resource, &mut cur) } == 0 {
        cur.rlim_max
    } else {
        v as libc::rlim_t
    };
    let lim = libc::rlimit {
        rlim_cur: core::cmp::min(v as libc::rlim_t, hard),
        rlim_max: hard,
    };
    // SAFETY: `resource` is a valid RLIMIT_* constant and `&lim` is a valid,
    // fully-initialized `rlimit` for the duration of the call.
    let rc = unsafe { libc::setrlimit(resource, &lim) };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aterm_cap::Authority;

    // `RlimitResource` (not a bare `c_int`): on Linux the libc RLIMIT_* constants
    // and `getrlimit`'s first arg are `__rlimit_resource_t` (u32), so a `c_int`
    // parameter mismatches and the test build does not compile there.
    fn current(resource: RlimitResource) -> u64 {
        let mut lim = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        // SAFETY: valid resource id + out-param.
        let rc = unsafe { libc::getrlimit(resource, &mut lim) };
        assert_eq!(rc, 0, "getrlimit failed");
        lim.rlim_cur
    }

    /// The current HARD ceiling (`rlim_max`) of `resource`.
    // `RlimitResource` (not a bare `c_int`): the libc RLIMIT_* constants and
    // `getrlimit`'s arg are `__rlimit_resource_t` (u32) on Linux, so a `c_int`
    // parameter mismatches and the test build does not compile there.
    fn current_hard(resource: RlimitResource) -> u64 {
        let mut lim = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        // SAFETY: valid resource id + out-param.
        let rc = unsafe { libc::getrlimit(resource, &mut lim) };
        assert_eq!(rc, 0, "getrlimit failed");
        lim.rlim_max
    }

    #[test]
    fn apply_requires_a_trusted_capability() {
        let auth = unsafe { Authority::root_authority() };
        let weak: Cap<Sandbox> = auth.grant(Tier::Untrusted);
        let err = Limits {
            cpu_seconds: Some(123456),
            ..Default::default()
        }
        .apply(&weak)
        .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn none_limits_are_a_no_op() {
        let auth = unsafe { Authority::root_authority() };
        let cap: Cap<Sandbox> = auth.grant(Tier::Trusted);
        // All None -> Ok and nothing changed.
        let before = current(libc::RLIMIT_CPU);
        Limits::default().apply(&cap).unwrap();
        assert_eq!(current(libc::RLIMIT_CPU), before);
    }

    #[test]
    fn unsupported_limit_does_not_block_the_working_ones() {
        // Regression: `RLIMIT_AS` EINVALs on macOS; the old `?`-early-return
        // there skipped the `RLIMIT_NOFILE` that DOES work, so the child got
        // ZERO confinement. Applying an (often-unsupported) huge AS alongside a
        // small NOFILE must STILL install NOFILE — best-effort per limit.
        let auth = unsafe { Authority::root_authority() };
        let cap: Cap<Sandbox> = auth.grant(Tier::Trusted);
        let target = 256u64;
        // 64 TiB AS: macOS rejects it (EINVAL), Linux accepts it; either way
        // NOFILE must land. (Discard the Result — on macOS apply() now reports
        // the AS error AFTER applying NOFILE.)
        let _ = Limits {
            address_space: Some(64 * 1024 * 1024 * 1024 * 1024),
            open_files: Some(target),
            ..Default::default()
        }
        .apply(&cap);
        assert_eq!(
            current(libc::RLIMIT_NOFILE),
            target,
            "NOFILE must be applied even when an earlier limit is unsupported"
        );
    }

    #[test]
    fn shell_default_omits_unsupported_address_space_on_macos() {
        // The REAL production value: macOS must NOT request RLIMIT_AS (it would
        // EINVAL and — before the best-effort fix — abort the whole apply, so
        // the child was unconfined). Construction-only: no setrlimit, no
        // process-wide fd-limit side effect / test-ordering hazard.
        let d = Limits::shell_default();
        #[cfg(target_os = "macos")]
        assert_eq!(
            d.address_space, None,
            "macOS must not request a finite RLIMIT_AS"
        );
        #[cfg(not(target_os = "macos"))]
        assert!(
            d.address_space.is_some(),
            "non-macOS should bound the address space"
        );
        assert_eq!(
            d.open_files,
            Some(8192),
            "the working NOFILE limit must remain"
        );
    }

    #[test]
    fn apply_actually_sets_the_limit() {
        // Lower RLIMIT_NOFILE to a value still far above what a test needs, then
        // read it back to prove `apply` performed the syscall. Lowering NOFILE is
        // safe (we only need a handful of fds) and is reversible up to the hard
        // limit if anything later raises it.
        let auth = unsafe { Authority::root_authority() };
        let cap: Cap<Sandbox> = auth.grant(Tier::Certified); // >= Trusted
        let target = 256u64;
        Limits {
            open_files: Some(target),
            ..Default::default()
        }
        .apply(&cap)
        .unwrap();
        assert_eq!(current(libc::RLIMIT_NOFILE), target);
    }

    #[test]
    fn apply_preserves_the_hard_ceiling_so_a_shell_can_raise_its_soft_limit() {
        // REGRESSION GUARD — `/Users/.../.zshrc:ulimit:N: value exceeds hard limit`.
        // The User-mode sandbox runs the user's $SHELL transparently, so it must
        // install its limit as a SOFT default and LEAVE THE HARD CEILING ALONE.
        // The old code set both soft AND hard to the requested value, clamping the
        // hard NOFILE to 8192; a `.zshrc` doing `ulimit -n 65536` then aborted shell
        // startup. This guard fails if anyone reintroduces a hard-limit clamp: it
        // proves (1) the hard ceiling is unchanged by apply, and (2) the soft limit
        // is still raisable above the applied value, up to that preserved ceiling —
        // exactly the `.zshrc` case. The earlier tests only checked the soft limit,
        // which is why the regression slipped through.
        let auth = unsafe { Authority::root_authority() };
        let cap: Cap<Sandbox> = auth.grant(Tier::Trusted);

        let hard_before = current_hard(libc::RLIMIT_NOFILE);
        let applied = 256u64;
        assert!(
            hard_before > applied,
            "test precondition: inherited hard NOFILE ({hard_before}) must exceed the applied soft default"
        );

        Limits {
            open_files: Some(applied),
            ..Default::default()
        }
        .apply(&cap)
        .unwrap();

        assert_eq!(
            current_hard(libc::RLIMIT_NOFILE),
            hard_before,
            "apply must NOT lower the hard NOFILE ceiling (the `ulimit: value exceeds hard limit` regression)"
        );

        // Emulate the shell rc raising its soft limit above the applied default —
        // this must SUCCEED now that the ceiling is preserved.
        let raise = core::cmp::min(applied * 4, hard_before);
        set_limit(libc::RLIMIT_NOFILE, Some(raise)).expect("raising the soft limit must succeed");
        assert!(
            current(libc::RLIMIT_NOFILE) >= applied,
            "soft limit must be raisable above the applied sandbox default"
        );
    }
}
