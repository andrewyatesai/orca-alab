// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

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
/// unchanged. Both the soft and hard limit are set to the given value.
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

/// Set both soft and hard `resource` to `value` (no-op when `value` is `None`).
fn set_limit(resource: libc::c_int, value: Option<u64>) -> io::Result<()> {
    let Some(v) = value else {
        return Ok(());
    };
    let lim = libc::rlimit { rlim_cur: v as libc::rlim_t, rlim_max: v as libc::rlim_t };
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

    fn current(resource: libc::c_int) -> u64 {
        let mut lim = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
        // SAFETY: valid resource id + out-param.
        let rc = unsafe { libc::getrlimit(resource, &mut lim) };
        assert_eq!(rc, 0, "getrlimit failed");
        lim.rlim_cur as u64
    }

    #[test]
    fn apply_requires_a_trusted_capability() {
        let auth = unsafe { Authority::root_authority() };
        let weak: Cap<Sandbox> = auth.grant(Tier::Untrusted);
        let err = Limits { cpu_seconds: Some(123456), ..Default::default() }
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
        assert_eq!(d.address_space, None, "macOS must not request a finite RLIMIT_AS");
        #[cfg(not(target_os = "macos"))]
        assert!(d.address_space.is_some(), "non-macOS should bound the address space");
        assert_eq!(d.open_files, Some(8192), "the working NOFILE limit must remain");
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
        Limits { open_files: Some(target), ..Default::default() }.apply(&cap).unwrap();
        assert_eq!(current(libc::RLIMIT_NOFILE), target);
    }
}
