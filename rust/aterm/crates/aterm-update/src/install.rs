// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0

//! Staging (mount → extract → verify → publish) and application (lock →
//! re-verify → atomic swap → re-exec) of an update. The ordering here is the
//! security-critical part; see the per-step comments and the crate-level trust
//! model.

use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::manifest::{Manifest, Ready};
use crate::paths::{Staging, ensure_private_dir};
use crate::sys::{FileLock, rename_swap, same_volume};
use crate::{ApplyOutcome, PINNED_TEAM_ID, bundle, verify};

/// A mounted DMG that detaches itself on drop (best-effort), so no error path
/// leaks a `/Volumes` mount.
struct Mounted {
    mountpoint: PathBuf,
}

impl Mounted {
    /// `hdiutil attach -nobrowse -readonly -noautoopen <dmg>` and parse the
    /// `/Volumes/…` mount point out of its table (which may contain spaces).
    fn attach(dmg: &Path) -> Result<Self, String> {
        let out = Command::new("/usr/bin/hdiutil")
            .args(["attach", "-nobrowse", "-readonly", "-noautoopen"])
            .arg(dmg)
            .output()
            .map_err(|e| format!("spawn hdiutil attach: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "hdiutil attach failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        let stdout = String::from_utf8_lossy(&out.stdout);
        // The mount point is the path from "/Volumes/" to end-of-line; taking the
        // whole tail preserves volume names that contain spaces ("aterm 0.2.0").
        let mountpoint = stdout
            .lines()
            .find_map(|l| l.find("/Volumes/").map(|i| l[i..].trim_end().to_string()))
            .ok_or("hdiutil attach: no /Volumes mount point in output")?;
        Ok(Self {
            mountpoint: PathBuf::from(mountpoint),
        })
    }
}

impl Drop for Mounted {
    fn drop(&mut self) {
        let _ = Command::new("/usr/bin/hdiutil")
            .arg("detach")
            .arg(&self.mountpoint)
            .output();
    }
}

/// Stage a verified copy of the bundle from a downloaded (sha256-checked) DMG:
/// mount, `ditto`-extract the `.app`, verify it (codesign/Team-ID/spctl), then
/// publish `staged/aterm.app` + write `ready.toml` LAST. The ready marker's
/// presence is the sole "ready" signal, so writing it last (atomic rename) means
/// a reader never sees a half-staged bundle.
pub fn stage_from_dmg(
    staging: &Staging,
    dmg: &Path,
    manifest: &Manifest,
    expected_team: &str,
) -> Result<(), String> {
    ensure_private_dir(&staging.staged_dir()).map_err(|e| format!("staged dir: {e}"))?;
    let mounted = Mounted::attach(dmg)?;
    let src = mounted.mountpoint.join("aterm.app");
    if !src.is_dir() {
        return Err(format!("{} not found on mounted DMG", src.display()));
    }

    let incoming = staging.staged_dir().join("aterm.app.incoming");
    let _ = std::fs::remove_dir_all(&incoming);
    // `ditto` (not `cp -R`) preserves extended attributes + the _CodeSignature
    // layout, so the copied bundle's signature stays valid.
    let status = Command::new("/usr/bin/ditto")
        .arg(&src)
        .arg(&incoming)
        .status()
        .map_err(|e| format!("spawn ditto: {e}"))?;
    if !status.success() {
        let _ = std::fs::remove_dir_all(&incoming);
        return Err(format!("ditto extract failed ({status})"));
    }
    // detach the DMG now; everything we need is in `incoming`.
    drop(mounted);

    // Verify the extracted bundle before publishing it.
    if let Err(e) = verify::verify_bundle(&incoming, expected_team) {
        let _ = std::fs::remove_dir_all(&incoming);
        return Err(format!("staged bundle failed verification: {e}"));
    }
    let team = verify::team_id(&incoming).unwrap_or_else(|_| expected_team.to_string());

    // Publish atomically: swap the verified bundle into place, then the marker.
    let _ = std::fs::remove_dir_all(&staging.staged_app);
    std::fs::rename(&incoming, &staging.staged_app)
        .map_err(|e| format!("publish staged bundle: {e}"))?;

    let ready = Ready {
        build_number: manifest.build_number,
        version: manifest.version.clone(),
        dmg_sha256: manifest.sha256.to_ascii_lowercase(),
        team_id: team,
        staged_at: now_rfc3339(),
    };
    let tmp = staging.root.join("ready.toml.tmp");
    std::fs::write(&tmp, ready.to_toml()?).map_err(|e| format!("write ready marker: {e}"))?;
    std::fs::rename(&tmp, &staging.ready).map_err(|e| format!("commit ready marker: {e}"))?;
    Ok(())
}

/// Apply a staged update if it is ready and strictly newer. On success this
/// re-execs and never returns. See module + crate docs for the full contract.
pub fn apply_staged_if_ready(current_build: u64) -> ApplyOutcome {
    // 1. Loop guard: we are the post-swap re-exec — never try to swap again, and
    //    clean up what the swap deferred to us: the staging dir (which, for a
    //    same-volume swap, now holds the OLD bundle) plus any transient swap
    //    leftovers (the OLD bundle from a cross-volume swap, named aterm.app.new-*).
    if std::env::var_os("ATERM_UPDATE_REEXEC").is_some() {
        // SAFETY of remove_var: single-threaded — this runs before any thread
        // spawn in main(). Clearing it stops child shells from inheriting it.
        unsafe { std::env::remove_var("ATERM_UPDATE_REEXEC") };
        if let Some(s) = Staging::resolve() {
            let _ = std::fs::remove_dir_all(s.staged_dir());
            let _ = std::fs::remove_file(&s.ready);
        }
        if let Some(b) = bundle::resolve() {
            sweep_swap_leftovers(&b.app_root);
        }
        return ApplyOutcome::NotApplicable;
    }
    if !crate::enabled() {
        return ApplyOutcome::NotApplicable;
    }
    // 2. Must be a real installed bundle.
    let Some(b) = bundle::resolve() else {
        return ApplyOutcome::NotApplicable;
    };
    let Some(staging) = Staging::resolve() else {
        return ApplyOutcome::NotApplicable;
    };

    // 3. Quick pre-lock gate: is anything newer even staged? A present-but-corrupt
    //    marker is discarded so it can't permanently wedge updates.
    match read_ready(&staging, current_build) {
        ReadyState::Newer(_) => {}
        ReadyState::NotNewer => {
            staging.clear();
            return ApplyOutcome::NoUpdate;
        }
        ReadyState::Corrupt => {
            crate::warn("ready.toml is unreadable; discarding staged update");
            staging.clear();
            return ApplyOutcome::NoUpdate;
        }
        ReadyState::Absent => return ApplyOutcome::NoUpdate,
    }

    // 4. Serialize the swap across concurrent launches.
    let _lock = match FileLock::acquire(&staging.apply_lock) {
        Ok(l) => l,
        Err(e) => return ApplyOutcome::Deferred(format!("lock: {e}")),
    };
    // Under the lock no other swap is in flight, so it is safe to clear orphaned
    // transient swap copies from a previously interrupted/completed swap.
    sweep_swap_leftovers(&b.app_root);
    // Re-read under the lock: another instance may have just applied + cleared.
    let ready = match read_ready(&staging, current_build) {
        ReadyState::Newer(r) => r,
        ReadyState::Corrupt => {
            staging.clear();
            return ApplyOutcome::NoUpdate;
        }
        ReadyState::NotNewer | ReadyState::Absent => return ApplyOutcome::NoUpdate,
    };
    if !staging.staged_app.is_dir() {
        staging.clear();
        return ApplyOutcome::NoUpdate;
    }

    // 5. Can we even write the install location? Checked BEFORE the (more
    //    expensive) re-verification so a persistently non-writable install (e.g.
    //    an admin-owned /Applications) doesn't re-verify the staged bundle on every
    //    single launch — there's nothing we could do with it anyway.
    if !bundle::parent_writable(&b.app_root) {
        crate::status::record(
            &staging,
            current_build,
            "deferred: install location not writable",
        );
        return ApplyOutcome::Deferred(format!(
            "install location not writable: {}",
            b.app_root.display()
        ));
    }

    // 6. Apply-time re-verification (TOCTOU defence).
    if let Err(e) = verify::verify_bundle(&staging.staged_app, PINNED_TEAM_ID) {
        crate::warn(&format!(
            "staged bundle re-verification failed: {e}; discarding"
        ));
        staging.clear();
        crate::status::record(
            &staging,
            current_build,
            "deferred: staged bundle failed re-verification (discarded)",
        );
        return ApplyOutcome::Deferred(format!("re-verify: {e}"));
    }

    // 7. Atomic swap. `swap_in` returns the path now holding the OLD bundle (our
    //    rollback source); the OLD bundle is NEVER destroyed before re-exec, so an
    //    exec failure is fully recoverable.
    let rollback = match swap_in(&staging.staged_app, &b.app_root) {
        Ok(p) => p,
        Err(e) => return ApplyOutcome::Deferred(format!("swap: {e}")),
    };
    // Don't let it re-apply; the OLD bundle stays at `rollback` for the re-exec.
    let _ = std::fs::remove_file(&staging.ready);

    // 8. Re-exec into the new binary. Release the lock first (drop the fd).
    drop(_lock);
    crate::log(&format!("applied update {} → re-launching", ready.version));
    // `b.exe` is the canonical path we launched from; after the in-place swap it
    // resolves to the NEW binary at the same location.
    let new_exe = &b.exe;
    let err = Command::new(new_exe)
        .args(std::env::args_os().skip(1))
        .env("ATERM_UPDATE_REEXEC", "1")
        .exec(); // never returns on success
    // exec failed — restore the OLD bundle from the rollback source so the user
    // keeps a runnable app.
    crate::warn(&format!(
        "re-exec of {} failed: {err}; rolling back to the previous build",
        new_exe.display()
    ));
    if let Err(e) = restore_rollback(&rollback, &b.app_root) {
        crate::warn(&format!("rollback failed: {e}"));
    }
    crate::status::record(
        &staging,
        current_build,
        "re-exec of new build failed (rolled back)",
    );
    ApplyOutcome::ReExecFailed(err.to_string())
}

/// Tri-state read of the staging marker, distinguishing a missing marker from a
/// present-but-unparseable one (the latter is discarded rather than wedging
/// updates forever) and folding in the strict downgrade gate.
enum ReadyState {
    Newer(Ready),
    NotNewer,
    Corrupt,
    Absent,
}

fn read_ready(staging: &Staging, current_build: u64) -> ReadyState {
    match Ready::read(&staging.ready) {
        Some(r) if r.build_number > current_build => ReadyState::Newer(r),
        Some(_) => ReadyState::NotNewer,
        None if staging.ready.exists() => ReadyState::Corrupt,
        None => ReadyState::Absent,
    }
}

/// Swap the verified `staged` bundle into `installed` atomically, returning the
/// path that now holds the OLD bundle (the rollback source, cleaned after a
/// successful re-exec). The live bundle at `installed` is never left missing: the
/// only mutation of `installed` is a single atomic `RENAME_SWAP`.
///
/// Same volume: exchange `staged` ↔ `installed` directly (OLD ends at `staged`).
/// Cross volume: first `ditto` the NEW bundle to a sibling of `installed` on the
/// destination volume (the live bundle is untouched), then exchange that sibling
/// ↔ `installed` (OLD ends at the sibling). If the destination volume cannot do an
/// atomic exchange (non-APFS), we DEFER rather than risk a rename-aside window —
/// the current build keeps running and the update simply isn't applied there.
fn swap_in(staged: &Path, installed: &Path) -> Result<std::path::PathBuf, String> {
    let (new_on_vol, drop_staged) = if same_volume(staged, installed) {
        (staged.to_path_buf(), false)
    } else {
        let incoming = sibling(installed, "new");
        let _ = std::fs::remove_dir_all(&incoming);
        let status = Command::new("/usr/bin/ditto")
            .arg(staged)
            .arg(&incoming)
            .status()
            .map_err(|e| format!("spawn ditto: {e}"))?;
        if !status.success() {
            let _ = std::fs::remove_dir_all(&incoming);
            return Err(format!("ditto to destination volume failed ({status})"));
        }
        (incoming, true)
    };
    // Atomic exchange (both paths are now on the same volume as `installed`).
    if let Err(e) = rename_swap(&new_on_vol, installed) {
        if drop_staged {
            let _ = std::fs::remove_dir_all(&new_on_vol);
        }
        return Err(format!(
            "destination volume does not support an atomic swap ({e}); update deferred"
        ));
    }
    if drop_staged {
        // The original NEW copy on the staging volume is now redundant.
        let _ = std::fs::remove_dir_all(staged);
    }
    Ok(new_on_vol) // holds the OLD bundle
}

/// Exec-failure rollback: put the OLD bundle (at `rollback`) back at `installed`,
/// which currently holds the NEW (failed-to-exec) bundle. Both are on the same
/// volume, so this is the inverse atomic exchange.
fn restore_rollback(rollback: &Path, installed: &Path) -> Result<(), String> {
    rename_swap(rollback, installed).map_err(|e| format!("RENAME_SWAP back: {e}"))
}

/// A `aterm.app.<tag>-<pid>` sibling of `installed`, used for transient swap copies.
fn sibling(installed: &Path, tag: &str) -> std::path::PathBuf {
    installed.with_file_name(format!("aterm.app.{tag}-{}", std::process::id()))
}

/// Remove transient swap copies (`aterm.app.new-*`) left in the install parent by
/// a completed or interrupted swap. Called only while holding the apply lock (no
/// concurrent swap can be mid-flight) or from the post-re-exec guard. Never
/// touches the live bundle.
fn sweep_swap_leftovers(app_root: &Path) {
    let Some(parent) = app_root.parent() else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path == *app_root {
            continue;
        }
        if entry
            .file_name()
            .to_string_lossy()
            .starts_with("aterm.app.new-")
        {
            let _ = std::fs::remove_dir_all(&path);
        }
    }
}

/// Best-effort RFC3339 UTC timestamp (`date -u`), for the human-readable
/// `staged_at`/status fields. Falls back to the empty string.
pub(crate) fn now_rfc3339() -> String {
    Command::new("/bin/date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static SEQ: AtomicU32 = AtomicU32::new(0);

    fn temp_staging() -> (Staging, std::path::PathBuf) {
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("aterm-rr-{}-{n}", std::process::id()));
        std::fs::create_dir_all(root.join("staged")).unwrap();
        std::fs::create_dir_all(root.join("download")).unwrap();
        let s = Staging {
            apply_lock: root.join("apply.lock"),
            stage_lock: root.join("stage.lock"),
            download: root.join("download"),
            staged_app: root.join("staged").join("aterm.app"),
            ready: root.join("ready.toml"),
            status: root.join("status.toml"),
            root: root.clone(),
        };
        (s, root)
    }

    fn write_ready(s: &Staging, build: u64) {
        let r = Ready {
            build_number: build,
            version: format!("0.0.{build}"),
            dmg_sha256: "x".into(),
            team_id: "T".into(),
            staged_at: String::new(),
        };
        std::fs::write(&s.ready, r.to_toml().unwrap()).unwrap();
    }

    #[test]
    fn read_ready_classifies_all_states() {
        let (s, root) = temp_staging();

        // Absent: no marker file.
        assert!(matches!(read_ready(&s, 10), ReadyState::Absent));

        // Corrupt: present but unparseable → must be discardable, not Absent.
        std::fs::write(&s.ready, "this is not valid toml {{{").unwrap();
        assert!(matches!(read_ready(&s, 10), ReadyState::Corrupt));

        // Newer: staged build strictly greater than running.
        write_ready(&s, 20);
        assert!(matches!(read_ready(&s, 10), ReadyState::Newer(_)));

        // NotNewer: equal or lower than running (downgrade gate).
        assert!(matches!(read_ready(&s, 20), ReadyState::NotNewer));
        assert!(matches!(read_ready(&s, 21), ReadyState::NotNewer));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn clear_removes_marker_staged_and_download_scratch() {
        let (s, root) = temp_staging();
        write_ready(&s, 5);
        std::fs::create_dir_all(&s.staged_app).unwrap();
        std::fs::write(s.download.join("aterm-0.0.5.dmg.part"), b"partial").unwrap();
        s.clear();
        assert!(!s.ready.exists());
        assert!(!s.staged_app.exists());
        assert!(std::fs::read_dir(&s.download).unwrap().next().is_none());
        let _ = std::fs::remove_dir_all(root);
    }
}
