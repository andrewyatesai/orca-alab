// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0

//! Staging directory resolution under `~/Library/Application Support/aterm`.
//!
//! Mirrors `aterm-gui`'s `control_auth::{socket_dir, ensure_private_dir}` (we
//! cannot depend on `aterm-gui` — it depends on us), reusing the *same* ownership
//! predicate ([`aterm_types::fs_restricted::dir_safe_for_private_write`]) so the
//! two cannot drift on what "private" means: owned by us, mode `0700`, never
//! group/other-writable.

use std::io;
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// Layout of the staging area, all under `…/aterm/Updates/`.
pub struct Staging {
    /// The `Updates` root.
    pub root: PathBuf,
    /// flock target guarding the apply critical section.
    pub apply_lock: PathBuf,
    /// flock target serializing the staging critical section (download + extract +
    /// publish) across processes. Distinct from `apply_lock` so a long download
    /// never blocks a starting instance's apply path.
    pub stage_lock: PathBuf,
    /// Scratch dir for in-progress downloads.
    pub download: PathBuf,
    /// The verified, extracted bundle awaiting application.
    pub staged_app: PathBuf,
    /// The "ready" marker — written last; its presence is the sole ready signal.
    pub ready: PathBuf,
}

impl Staging {
    /// Resolve (and create, `0700`, ownership-verified) the staging layout.
    /// Returns `None` if `HOME` is unset or the directory cannot be made private.
    pub fn resolve() -> Option<Self> {
        let home = std::env::var_os("HOME")?;
        let base = PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("aterm");
        let root = base.join("Updates");
        ensure_private_dir(&root).ok()?;
        let download = root.join("download");
        ensure_private_dir(&download).ok()?;
        Some(Self {
            apply_lock: root.join("apply.lock"),
            stage_lock: root.join("stage.lock"),
            download,
            staged_app: root.join("staged").join("aterm.app"),
            ready: root.join("ready.toml"),
            root,
        })
    }

    /// The `staged/` parent of [`Self::staged_app`].
    pub fn staged_dir(&self) -> PathBuf {
        self.root.join("staged")
    }

    /// Remove any staged bundle + ready marker (called when staging is stale or a
    /// verification fails — a tampered/old staged copy is worthless). Also GCs the
    /// download scratch so a crashed download's `.part`/`.dmg` can't accumulate.
    pub fn clear(&self) {
        let _ = std::fs::remove_file(&self.ready);
        let _ = std::fs::remove_dir_all(self.staged_dir());
        if let Ok(entries) = std::fs::read_dir(&self.download) {
            for e in entries.flatten() {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}

/// Our effective uid.
fn our_uid() -> u32 {
    // SAFETY: getuid() is always-safe (no args, cannot fail).
    unsafe { libc::getuid() }
}

/// Create `dir` (and parents), force mode `0700`, then verify it is owned by us
/// and not group/other-writable — refusing a foreign-owned or shared directory
/// (fail closed) exactly as `control_auth::ensure_private_dir` does.
pub fn ensure_private_dir(dir: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    let meta = std::fs::metadata(dir)?;
    if aterm_types::fs_restricted::dir_safe_for_private_write(our_uid(), meta.uid(), meta.mode()) {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "{}: update directory must be owned by uid {} and not group/other-writable",
                dir.display(),
                our_uid()
            ),
        ))
    }
}
