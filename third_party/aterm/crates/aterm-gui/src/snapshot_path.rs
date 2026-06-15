// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! Where a SIGUSR1 screen snapshot may land, and how it is written.
//!
//! The snapshot carries everything on screen, so it gets the same posture as
//! the control socket's `image` verb (see `control_auth`): by default the
//! PNG/.txt/.done files land in the per-user `0700` control directory and are
//! written `0600`. `$ATERM_SNAPSHOT_PATH` still overrides for users who
//! explicitly opt into a path, but an override whose directory another user
//! owns or can write into (e.g. `/tmp`, the historical default) is refused —
//! that user could read the screen contents or swap the target for a symlink
//! between our check and our write. The owned-and-unshared decision itself is
//! engine-side ([`aterm_types::fs_restricted::dir_safe_for_private_write`]);
//! this module only stats and writes.

use std::path::{Path, PathBuf};

use crate::control_auth;

/// Default snapshot filename inside the per-user control directory.
pub const SNAPSHOT_FILE: &str = "aterm_snapshot.png";

/// Resolve the path the snapshot PNG may be written to (`.txt`/`.done` are
/// siblings), or `None` — with the refusal already logged — when no safe
/// destination exists.
#[must_use]
pub fn resolve() -> Option<String> {
    if let Some(over) = std::env::var_os("ATERM_SNAPSHOT_PATH") {
        let requested = PathBuf::from(over);
        return match validate_override(&requested) {
            Some(p) => Some(p.to_string_lossy().into_owned()),
            None => {
                eprintln!(
                    "aterm-gui: refusing ATERM_SNAPSHOT_PATH {}: its directory must exist, \
                     be owned by uid {}, and not be group/other-writable; snapshot skipped",
                    requested.display(),
                    control_auth::our_uid()
                );
                None
            }
        };
    }
    match control_auth::socket_dir() {
        Some(dir) => Some(dir.join(SNAPSHOT_FILE).to_string_lossy().into_owned()),
        None => {
            eprintln!(
                "aterm-gui: no per-user runtime dir (set XDG_RUNTIME_DIR, HOME, or \
                 ATERM_SNAPSHOT_PATH); snapshot skipped"
            );
            None
        }
    }
}

/// Validate an explicit `$ATERM_SNAPSHOT_PATH` override: the parent directory
/// (symlinks resolved, so the check binds to the real directory) must satisfy
/// the engine-side private-write predicate for our euid. Returns the
/// canonical-parent form of the path — the directory checked IS the directory
/// written to — or `None` when missing/unsafe.
fn validate_override(requested: &Path) -> Option<PathBuf> {
    use std::os::unix::fs::MetadataExt;
    let file_name = requested.file_name()?;
    let parent = match requested.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let canon = std::fs::canonicalize(parent).ok()?;
    let meta = std::fs::metadata(&canon).ok()?;
    let safe = aterm_types::fs_restricted::dir_safe_for_private_write(
        control_auth::our_uid(),
        meta.uid(),
        meta.mode(),
    );
    if safe { Some(canon.join(file_name)) } else { None }
}

/// Write `bytes` to `path` at mode `0600`, truncating any prior file. Mirrors
/// `control_auth::provision_token`: restrictive perms BEFORE content lands,
/// and the mode forced even when the file pre-existed (`OpenOptions::mode`
/// only applies on creation).
pub fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true).mode(0o600);
    // O_NOFOLLOW: refuse to open (and thus write or chmod THROUGH) a symlink
    // planted at the final component — otherwise a same-uid client could
    // redirect this write to clobber an arbitrary file and force it to 0600.
    opts.custom_flags(libc::O_NOFOLLOW);
    let f = opts.open(path)?;
    // Force 0600 even when the file pre-existed (`mode()` only applies on
    // creation) — via the OPEN fd (`fchmod`), never a path-based
    // `set_permissions` that would re-resolve (and follow) the path.
    f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    let mut f = f;
    f.write_all(bytes)?;
    f.flush()
}

/// Write `bytes` to `file_name` INSIDE `dir`, opening `dir` itself
/// `O_DIRECTORY|O_NOFOLLOW` and `openat`-ing the final component
/// `O_NOFOLLOW|O_CREAT|O_TRUNC` at mode `0600`.
///
/// TOCTOU-1: this is the close of the check→write window. The confinement check
/// (`control_auth::confine_image_path`) runs on the control thread and yields a
/// canonical `dir` + single `file_name`; the WRITE here, on the main thread,
/// opens THAT directory by path once (refusing a symlinked dir via O_NOFOLLOW
/// and requiring it to be a real directory via O_DIRECTORY) and creates the file
/// RELATIVE to the resulting fd. There is therefore no multi-segment path string
/// re-resolved at write time, so an intermediate-dir symlink swapped in after the
/// check cannot redirect the write. `file_name` must be a single component (the
/// confiner guarantees this; we assert it defensively).
pub fn write_private_at(dir: &Path, file_name: &std::ffi::OsString, bytes: &[u8]) -> std::io::Result<()> {
    use std::ffi::CString;
    use std::io::Write as _;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::io::{AsRawFd, FromRawFd};

    // Defensive: `file_name` must be a single path component, no separators.
    if Path::new(file_name).components().count() != 1 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "image filename must be a single component",
        ));
    }

    // Open the confining directory itself, refusing a symlinked directory and
    // requiring it to actually be a directory. The resulting fd is the anchor
    // for the relative create — it cannot be repointed by a later path swap.
    let dir_c = CString::new(dir.as_os_str().as_bytes())
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "dir has NUL"))?;
    // SAFETY: `dir_c` is a valid NUL-terminated path; flags are valid open flags.
    let dir_fd = unsafe {
        libc::open(dir_c.as_ptr(), libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
    };
    if dir_fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // Own the dir fd so it is closed on every path out of this function.
    // SAFETY: `dir_fd` is a freshly-opened, owned fd.
    let dir_file = unsafe { std::fs::File::from_raw_fd(dir_fd) };

    let name_c = CString::new(file_name.as_bytes())
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "filename has NUL"))?;
    // openat the final component relative to the directory fd: O_NOFOLLOW so a
    // symlink at the name is refused; O_CREAT|O_TRUNC|O_WRONLY at 0600.
    // SAFETY: `dir_file` owns a valid dir fd; `name_c` is a valid NUL-terminated
    // single component; flags + mode are valid for openat.
    let file_fd = unsafe {
        libc::openat(
            dir_file.as_raw_fd(),
            name_c.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600 as libc::c_uint,
        )
    };
    if file_fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: `file_fd` is a freshly-opened, owned fd.
    let mut f = unsafe { std::fs::File::from_raw_fd(file_fd) };
    // Force 0600 even if the file pre-existed (O_CREAT mode only applies on
    // creation) — via the OPEN fd, never a path re-resolve.
    f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    f.write_all(bytes)?;
    f.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control_auth::ensure_private_dir;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn override_into_private_dir_is_allowed() {
        let dir = std::env::temp_dir().join(format!("aterm-snap-ok-{}", std::process::id()));
        ensure_private_dir(&dir).unwrap();
        let ok = validate_override(&dir.join("shot.png")).expect("0700 own dir allowed");
        assert!(ok.ends_with("shot.png"));
        // The returned path is canonical-parent based: its parent exists.
        assert!(ok.parent().unwrap().is_dir());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn override_into_tmp_is_refused() {
        // /tmp is root-owned and world-writable — the historical leak target.
        assert!(validate_override(Path::new("/tmp/aterm_snapshot.png")).is_none());
    }

    #[test]
    fn override_into_group_writable_dir_is_refused() {
        let dir = std::env::temp_dir().join(format!("aterm-snap-gw-{}", std::process::id()));
        ensure_private_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o770)).unwrap();
        assert!(validate_override(&dir.join("shot.png")).is_none());
        // Tightening the dir back to 0700 makes the same override valid.
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(validate_override(&dir.join("shot.png")).is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn override_with_missing_dir_is_refused() {
        let dir = std::env::temp_dir().join(format!("aterm-snap-none-{}", std::process::id()));
        assert!(validate_override(&dir.join("shot.png")).is_none());
    }

    #[test]
    fn write_private_at_creates_inside_dir_via_openat() {
        let dir = std::env::temp_dir().join(format!("aterm-snap-at-{}", std::process::id()));
        ensure_private_dir(&dir).unwrap();
        let name = std::ffi::OsString::from("shot.png");
        write_private_at(&dir, &name, b"png-bytes").unwrap();
        let path = dir.join(&name);
        assert_eq!(std::fs::read(&path).unwrap(), b"png-bytes");
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        // Overwrite truncates and re-tightens.
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        write_private_at(&dir, &name, b"x").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"x");
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_private_at_refuses_symlinked_final_component() {
        // A symlink planted at the final name must NOT be followed (O_NOFOLLOW):
        // the write must fail rather than clobber the link target.
        use std::os::unix::fs::symlink;
        let dir = std::env::temp_dir().join(format!("aterm-snap-at-sym-{}", std::process::id()));
        ensure_private_dir(&dir).unwrap();
        let victim = dir.join("victim.txt");
        std::fs::write(&victim, b"original").unwrap();
        symlink(&victim, dir.join("evil.png")).unwrap();
        let name = std::ffi::OsString::from("evil.png");
        assert!(
            write_private_at(&dir, &name, b"attack").is_err(),
            "writing through a symlinked final component must be refused",
        );
        // The victim is untouched.
        assert_eq!(std::fs::read(&victim).unwrap(), b"original");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_private_at_rejects_multi_component_name() {
        let dir = std::env::temp_dir().join(format!("aterm-snap-at-multi-{}", std::process::id()));
        ensure_private_dir(&dir).unwrap();
        let name = std::ffi::OsString::from("sub/shot.png");
        assert!(write_private_at(&dir, &name, b"x").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_private_creates_0600_and_forces_mode_on_overwrite() {
        let dir = std::env::temp_dir().join(format!("aterm-snap-wr-{}", std::process::id()));
        ensure_private_dir(&dir).unwrap();
        let path = dir.join("snap.bin");
        write_private(&path, b"first").unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        assert_eq!(std::fs::read(&path).unwrap(), b"first");
        // A pre-existing loose file is truncated AND tightened to 0600.
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        write_private(&path, b"x").unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        assert_eq!(std::fs::read(&path).unwrap(), b"x");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
