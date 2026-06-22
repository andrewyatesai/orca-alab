// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0

//! `aterm-update` — silent, signature-pinned in-app self-update for the macOS
//! `aterm.app`.
//!
//! Two entry points, both no-ops unless the running process is a real installed
//! `.app` bundle and the updater is configured + enabled:
//!
//! * [`apply_staged_if_ready`] — call **very early in `main()`**, before any
//!   window/thread. If a previous run staged a verified, *newer* build, this
//!   atomically swaps it into place and re-execs the new binary (the swap is
//!   invisible: same PID/tty/parent). Otherwise it returns and the current build
//!   keeps running.
//! * [`spawn_background_check`] — call once the GUI is up. Spawns a detached
//!   thread that talks to the private GitHub Release, downloads the newer DMG,
//!   verifies it, and stages it for the *next* launch. It never touches the UI
//!   and never blocks the event loop.
//!
//! # Trust model (no cryptographic dependency)
//!
//! An update is accepted only if ALL hold:
//! 1. **No downgrade** — the candidate's build number (git commit depth) is
//!    strictly greater than the running [`build`](apply_staged_if_ready) number.
//! 2. **Integrity** — the downloaded DMG's SHA-256 equals the manifest's.
//! 3. **Authenticity** — the `.app` inside passes `codesign --verify`, passes
//!    `spctl -a -t exec` (Apple notarization, read offline from the stapled
//!    ticket), AND is signed by the [`PINNED_TEAM_ID`] compiled into this binary.
//!
//! (3) is the real anchor: an attacker who controls the GitHub repo still cannot
//! produce a Developer-ID-signed, Apple-notarized bundle for our Team ID. (1)
//! blocks downgrade/replay, (2) blocks corruption. No crypto crate is pulled in —
//! everything shells out to `codesign`/`spctl`/`hdiutil`/`ditto`/`curl`/`shasum`,
//! mirroring `apps/aterm-mac/*.sh`.

#[cfg(target_os = "macos")]
mod bundle;
#[cfg(target_os = "macos")]
mod github;
#[cfg(target_os = "macos")]
mod install;
#[cfg(target_os = "macos")]
mod manifest;
#[cfg(target_os = "macos")]
mod paths;
#[cfg(target_os = "macos")]
mod status;
#[cfg(target_os = "macos")]
mod sys;
#[cfg(target_os = "macos")]
mod token;
#[cfg(target_os = "macos")]
mod verify;

/// GitHub owner of the release repository (the `OWNER` in
/// `github.com/OWNER/REPO`). Pinned at compile time.
pub const OWNER: &str = "andrewyatesai";

/// GitHub repository name the updater pulls releases from.
pub const REPO: &str = "aterm";

/// The Apple Developer **Team ID** the downloaded bundle MUST be signed by, baked
/// in at compile time from `ATERM_EXPECTED_TEAM_ID`. Empty (the default when the
/// env var is unset at build time) **disables the updater entirely**, fail-closed:
/// with no pin there is no authenticity anchor, so we never swap anything.
///
/// CI / the owner's release build exports `ATERM_EXPECTED_TEAM_ID=<TEAMID>` (the
/// same 10-char ID embedded in the Developer-ID signature) so shipped builds can
/// self-update; a plain `cargo build` leaves it empty and the updater is inert.
pub const PINNED_TEAM_ID: &str = match option_env!("ATERM_EXPECTED_TEAM_ID") {
    Some(t) => t,
    None => "",
};

/// Outcome of an [`apply_staged_if_ready`] call. Every variant is non-fatal: the
/// caller continues launching the current build (the one variant that *would*
/// replace it, [`ApplyOutcome::ReExecFailed`], only happens after a swap that was
/// rolled back). On a successful apply the function never returns — it `exec`s the
/// new binary — so there is no "applied" variant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyOutcome {
    /// Not an installed `.app` launch (dev build / `cargo run` / translocated /
    /// run from a mounted DMG), or the updater is disabled. No-op.
    NotApplicable,
    /// Nothing staged, or what's staged is not strictly newer. No-op.
    NoUpdate,
    /// A newer build is staged but could not be applied right now (e.g. the
    /// install location is not writable, or apply-time re-verification failed).
    /// The staged build is left in place for a future attempt; carries a reason.
    Deferred(String),
    /// The bundle was swapped but re-exec into the new binary failed; the swap
    /// was rolled back and the caller should keep running the old build.
    ReExecFailed(String),
}

/// Whether the updater is configured to run at all: macOS-only, a Team ID must be
/// pinned, and the user must not have opted out via `ATERM_NO_AUTO_UPDATE`. On
/// non-macOS targets both entry points are unconditional no-ops, so this is false.
#[must_use]
pub fn enabled() -> bool {
    cfg!(target_os = "macos")
        && !PINNED_TEAM_ID.is_empty()
        && std::env::var_os("ATERM_NO_AUTO_UPDATE").is_none()
}

/// Apply a staged update if one is ready and strictly newer than `current_build`
/// (the running build number, i.e. `git rev-list --count HEAD`). On success this
/// **does not return** — it re-execs the freshly swapped-in binary. See the
/// module docs for the full ordered sequence and the crate-level trust model.
#[cfg(target_os = "macos")]
#[must_use]
pub fn apply_staged_if_ready(current_build: u64) -> ApplyOutcome {
    install::apply_staged_if_ready(current_build)
}

/// Non-macOS no-op: there is no `.app` bundle to swap.
#[cfg(not(target_os = "macos"))]
#[must_use]
pub fn apply_staged_if_ready(_current_build: u64) -> ApplyOutcome {
    ApplyOutcome::NotApplicable
}

/// Spawn the background update check + stage on a detached thread. Returns
/// immediately; the work (network + disk I/O) happens off the event loop and is a
/// no-op when the updater is disabled or this is not an installed `.app`.
#[cfg(target_os = "macos")]
pub fn spawn_background_check(current_build: u64, current_version: &'static str) {
    if !enabled() {
        return;
    }
    std::thread::Builder::new()
        .name("aterm-update".into())
        .spawn(move || {
            // Re-check periodically so a long-running session (a terminal open for
            // days) still picks up releases without a relaunch. Still silent — it
            // only stages; the staged build applies on the next launch. Interval is
            // `ATERM_UPDATE_INTERVAL_SECS` (default 6h); 0 means check once and stop.
            let interval = std::env::var("ATERM_UPDATE_INTERVAL_SECS")
                .ok()
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(6 * 60 * 60);
            loop {
                match github::check_and_stage(current_build, current_version) {
                    Ok(Some(v)) => log(&format!("staged update {v} — applies on next launch")),
                    Ok(None) => {}
                    Err(e) => warn(&format!("update check failed: {e}")),
                }
                if interval == 0 {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_secs(interval));
            }
        })
        .ok();
}

/// Non-macOS no-op.
#[cfg(not(target_os = "macos"))]
pub fn spawn_background_check(_current_build: u64, _current_version: &'static str) {}

/// Emit an informational updater line to stderr (captured by the GUI's logger).
/// Kept deliberately low-volume: silent operation means most runs print nothing.
/// Routed through `aterm_log` (the global logger `aterm-gui` installs before the
/// updater runs), so it lands in the app log FILE — visible for a Finder-launched
/// `.app`, unlike stderr. A no-op if no logger is installed (e.g. a dev harness).
#[cfg(target_os = "macos")]
pub(crate) fn log(msg: &str) {
    aterm_log::info!("aterm-update: {msg}");
}

/// Emit a non-fatal updater warning to the app log (see [`log`]).
#[cfg(target_os = "macos")]
pub(crate) fn warn(msg: &str) {
    aterm_log::warn!("aterm-update: {msg}");
}
