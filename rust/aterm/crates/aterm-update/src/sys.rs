// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0

//! The two narrow `libc` calls the updater needs that have no safe std wrapper:
//! an advisory exclusive file lock for the apply critical section, and the APFS
//! atomic directory exchange used for the swap. Each is a single, documented
//! `unsafe` call — the rest of the crate is safe Rust.

use std::fs::File;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::io::AsRawFd;
use std::path::Path;

/// An advisory exclusive lock held for the lifetime of the value. Dropping it (or
/// the process exiting / `exec`ing) releases the lock — `flock` is associated with
/// the open file description, so the kernel always cleans up.
pub struct FileLock {
    _file: File,
}

impl FileLock {
    /// Acquire `LOCK_EX` on `path` (created `0600` if absent), blocking until the
    /// lock is available. Blocking is fine: this runs before the window exists and
    /// the holder either re-execs (releasing immediately) or returns in millis.
    pub fn acquire(path: &Path) -> io::Result<Self> {
        use std::os::unix::fs::OpenOptionsExt;
        let file = std::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            // A lock file is a rendezvous, not data — never clobber its contents.
            .truncate(false)
            .mode(0o600)
            .open(path)?;
        // SAFETY: `file` owns a valid fd for the duration of the call; flock takes
        // an fd + flag and returns -1/errno on failure (mapped below).
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { _file: file })
    }
}

/// Whether `a` and `b` live on the same filesystem volume (`st_dev`). Required
/// before attempting [`rename_swap`] (`RENAME_SWAP` is in-volume only).
pub fn same_volume(a: &Path, b: &Path) -> bool {
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(ma), Ok(mb)) => {
            use std::os::unix::fs::MetadataExt;
            ma.dev() == mb.dev()
        }
        _ => false,
    }
}

/// Atomically exchange the directory entries `a` and `b` via `renamex_np` with
/// `RENAME_SWAP` (APFS, same volume). After success `a` names what was at `b` and
/// vice-versa, with no intermediate window where either path is missing — the
/// swap a self-update needs. Caller must have checked [`same_volume`] first.
pub fn rename_swap(a: &Path, b: &Path) -> io::Result<()> {
    let ca = cpath(a)?;
    let cb = cpath(b)?;
    // SAFETY: both pointers are valid NUL-terminated C strings kept alive across
    // the call; RENAME_SWAP is the documented flag; -1/errno on failure.
    let rc = unsafe { libc::renamex_np(ca.as_ptr(), cb.as_ptr(), libc::RENAME_SWAP) };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Build a NUL-terminated C string from a path, rejecting embedded NULs.
fn cpath(p: &Path) -> io::Result<std::ffi::CString> {
    std::ffi::CString::new(p.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains NUL"))
}
