// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0

//! Thin RAII wrapper around `libc::mmap` / `libc::munmap`.
//!
//! Replaces the `memmap2` crate with a minimal inline implementation that
//! shares the same libc dependency already used by `aterm-shm`.
//! macOS / Linux only — no Windows support required.

use std::fs::File;
use std::io;
use std::ops::{Deref, DerefMut};
use std::os::unix::io::AsRawFd;

/// Mutable memory-mapped region backed by a file descriptor.
///
/// Unmaps the region on [`Drop`]. The mapping covers the entire file at the
/// time [`map_mut`](Self::map_mut) is called.
#[derive(Debug)]
// Trust: `ptr` is valid for `len` bytes — the relational backing-length invariant.
// Under `trustc -Z trust-verify` this lets the compiler PROVE the `from_raw_parts`
// bounds in `as_slice`/`slice` (spatial HIGH-2) instead of only catching them.
#[cfg_attr(trust_verify, trust::backing)]
pub struct MmapMut {
    ptr: *mut u8,
    len: usize,
}

// SAFETY: The mapped region is exclusively owned by this struct. No other
// references alias the pointer, and the region is valid for `len` bytes
// from `ptr` until `Drop` calls `munmap`.
unsafe impl Send for MmapMut {}
// SAFETY: All public access goes through `&self` / `&mut self`, which the
// borrow checker serializes.
unsafe impl Sync for MmapMut {}

impl MmapMut {
    /// Create a read-write memory mapping of the entire file.
    ///
    /// # Safety
    ///
    /// The caller must ensure the file is not concurrently modified by
    /// another process while this mapping exists.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the file metadata cannot be read, the file
    /// is empty, or `mmap(2)` fails.
    // Trust: the caller's `# Safety` contract (no concurrent modification of the
    // backing file) IS the single-writer invariant. Under `trustc -Z trust-verify`
    // this lets `ty` PROVE the temporal (truncation) safety of the mapping instead
    // of catching it — the `# Safety` promise made machine-checked.
    #[cfg_attr(trust_verify, trust::single_writer)]
    pub unsafe fn map_mut(file: &File) -> io::Result<Self> {
        let len = file.metadata()?.len();
        if len == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "cannot mmap an empty file",
            ));
        }
        let len = usize::try_from(len).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidInput, "file size overflows usize")
        })?;

        // SAFETY: Caller guarantees exclusive access. We pass a valid fd,
        // non-zero length, MAP_SHARED for durability, PROT_READ|PROT_WRITE
        // for read-write access, and offset 0 to map the full file.
        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                file.as_raw_fd(),
                0,
            )
        };
        if ptr == libc::MAP_FAILED {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            ptr: ptr.cast::<u8>(),
            len,
        })
    }

    /// Returns a shared slice over the mapped region.
    #[must_use]
    #[inline]
    pub fn as_slice(&self) -> &[u8] {
        // SAFETY: `ptr` is a valid mmap'd region of `len` bytes, guaranteed
        // by the successful `mmap` call in `map_mut` and the absence of
        // `munmap` until `Drop`.
        unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
    }

    /// Returns a checked sub-slice `[start, start + len)` of the mapped region.
    ///
    /// Returns `None` if `start + len` overflows or exceeds the recorded
    /// mapping length, ensuring the raw `from_raw_parts` is never indexed
    /// past the bytes the kernel mapped. This is the only sound way to read
    /// a sub-range from attacker-influenced offsets/lengths: bounding against
    /// `self.len` keeps the deref within the mapped region.
    ///
    /// Note: `self.len` is the length at map time. Callers that need to guard
    /// against an external truncation of the backing file should additionally
    /// validate against the live file length before calling here.
    #[must_use]
    #[inline]
    pub fn slice(&self, start: usize, len: usize) -> Option<&[u8]> {
        let end = start.checked_add(len)?;
        if end > self.len {
            return None;
        }
        // SAFETY: `start + len <= self.len`, so the sub-range lies entirely
        // within the mapped region described by `ptr`/`self.len`, which is
        // valid until `Drop`. `self.ptr.add(start)` therefore stays in bounds.
        Some(unsafe { std::slice::from_raw_parts(self.ptr.add(start), len) })
    }

    /// Returns a mutable slice over the mapped region.
    #[must_use]
    #[inline]
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        // SAFETY: We have `&mut self`, so no other references exist.
        // The region is valid for `len` bytes (see `as_slice` rationale).
        unsafe { std::slice::from_raw_parts_mut(self.ptr, self.len) }
    }

    /// Returns the length of the mapped region in bytes.
    #[must_use]
    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns `true` if the mapped region has zero length.
    #[must_use]
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Flushes modified pages to the underlying file via `msync(2)`.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if `msync` fails.
    pub fn flush(&self) -> io::Result<()> {
        // SAFETY: `ptr` and `len` describe a valid mmap'd region.
        // MS_SYNC performs a synchronous flush.
        let ret = unsafe { libc::msync(self.ptr.cast(), self.len, libc::MS_SYNC) };
        if ret != 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}

impl Deref for MmapMut {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl DerefMut for MmapMut {
    #[inline]
    fn deref_mut(&mut self) -> &mut [u8] {
        self.as_mut_slice()
    }
}

impl Drop for MmapMut {
    fn drop(&mut self) {
        // SAFETY: `ptr` and `len` are from a successful `mmap` call.
        // After `munmap`, the pointer is invalidated and must not be used.
        unsafe {
            libc::munmap(self.ptr.cast(), self.len);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_roundtrip() {
        let dir = aterm_tempfile::tempdir().unwrap();
        let path = dir.path().join("mmap_test.bin");
        let content = b"hello mmap world";

        std::fs::write(&path, content).unwrap();
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();

        // SAFETY: exclusive access in test.
        let mmap = unsafe { MmapMut::map_mut(&file).unwrap() };
        assert_eq!(mmap.len(), content.len());
        assert_eq!(&*mmap, content);
    }

    #[test]
    fn map_mut_write_and_flush() {
        let dir = aterm_tempfile::tempdir().unwrap();
        let path = dir.path().join("mmap_write.bin");

        std::fs::write(&path, [0u8; 16]).unwrap();
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();

        // SAFETY: exclusive access in test.
        let mut mmap = unsafe { MmapMut::map_mut(&file).unwrap() };
        mmap.as_mut_slice()[0..5].copy_from_slice(b"HELLO");
        mmap.flush().unwrap();

        drop(mmap);
        drop(file);

        let data = std::fs::read(&path).unwrap();
        assert_eq!(&data[0..5], b"HELLO");
    }

    #[test]
    fn map_empty_file_fails() {
        let dir = aterm_tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.bin");

        std::fs::write(&path, []).unwrap();
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();

        // SAFETY: test context.
        let result = unsafe { MmapMut::map_mut(&file) };
        assert!(result.is_err());
    }
}
