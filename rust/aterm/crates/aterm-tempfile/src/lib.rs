// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0

//! Zero-dependency temporary files and directories with RAII cleanup.
//!
//! Drop-in replacement for the `tempfile` crate covering the API surface
//! used in aterm: `TempDir`, `NamedTempFile`, `Builder`, and the free
//! functions `tempdir()`, `tempdir_in()`, and `tempfile()`.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Global counter for unique temp names within this process.
static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Generate a unique temporary name component.
///
/// Combines PID, monotonic counter, and OS-sourced randomness for
/// cross-process collision resistance even when timestamps coincide.
fn unique_name(prefix: &str) -> String {
    let pid = std::process::id();
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    let rand = os_random_u64();
    format!("{prefix}{pid}_{rand:016x}_{count}")
}

/// Read 8 bytes of randomness from the OS.
///
/// Falls back to nanosecond timestamp if the OS source is unavailable.
fn os_random_u64() -> u64 {
    let mut buf = [0u8; 8];
    if read_os_random(&mut buf) {
        u64::from_ne_bytes(buf)
    } else {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0)
    }
}

#[cfg(unix)]
fn read_os_random(buf: &mut [u8]) -> bool {
    use std::io::Read;
    fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(buf))
        .is_ok()
}

#[cfg(not(unix))]
fn read_os_random(_buf: &mut [u8]) -> bool {
    false
}

// ============================================================================
// TempDir
// ============================================================================

/// A temporary directory that is automatically deleted on drop.
///
/// The directory and all its contents are removed when this value is dropped,
/// unless [`keep`](TempDir::keep) is called to disarm the destructor.
#[derive(Debug)]
pub struct TempDir {
    path: PathBuf,
    disarmed: bool,
}

impl TempDir {
    /// Create a new temporary directory in the system temp directory.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be created.
    pub fn new() -> io::Result<Self> {
        Self::new_in(std::env::temp_dir())
    }

    /// Create a new temporary directory inside `dir`.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be created.
    pub fn new_in(dir: impl AsRef<Path>) -> io::Result<Self> {
        Self::with_prefix_in(".tmp", dir)
    }

    /// Create a new temporary directory with a custom prefix inside `dir`.
    fn with_prefix_in(prefix: &str, dir: impl AsRef<Path>) -> io::Result<Self> {
        let dir = dir.as_ref();
        for _ in 0..5 {
            let name = unique_name(prefix);
            let path = dir.join(name);
            match fs::create_dir(&path) {
                Ok(()) => {
                    return Ok(Self {
                        path,
                        disarmed: false,
                    });
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "failed to create unique temp directory after 5 attempts",
        ))
    }

    /// Get the path to the temporary directory.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Disarm the destructor — the directory will NOT be deleted on drop.
    ///
    /// Returns the path to the directory. The caller takes ownership of
    /// the directory's lifecycle.
    #[must_use]
    pub fn keep(mut self) -> PathBuf {
        self.disarmed = true;
        self.path.clone()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        if !self.disarmed {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

impl AsRef<Path> for TempDir {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

// ============================================================================
// NamedTempFile
// ============================================================================

/// Destructured parts of a `NamedTempFile`, used to avoid running the Drop impl.
struct Parts {
    path: PathBuf,
    file: fs::File,
}

/// A temporary file with a known path that is deleted on drop.
///
/// Implements `Write` for convenient writing. Call [`persist`](NamedTempFile::persist)
/// to atomically rename the file to a permanent location.
#[derive(Debug)]
pub struct NamedTempFile {
    path: PathBuf,
    file: fs::File,
}

impl NamedTempFile {
    /// Create a new temporary file in the system temp directory.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be created.
    pub fn new() -> io::Result<Self> {
        Self::new_in(std::env::temp_dir())
    }

    /// Create a new temporary file inside `dir`.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be created.
    pub fn new_in(dir: impl AsRef<Path>) -> io::Result<Self> {
        let name = unique_name(".tmpfile");
        let path = dir.as_ref().join(name);
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)?;
        Ok(Self { path, file })
    }

    /// Get the path to the temporary file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Get a reference to the underlying `File`.
    #[must_use]
    pub fn as_file(&self) -> &fs::File {
        &self.file
    }

    /// Get a mutable reference to the underlying `File`.
    pub fn as_file_mut(&mut self) -> &mut fs::File {
        &mut self.file
    }

    /// Atomically rename (persist) the temp file to `target`.
    ///
    /// On success the temp file destructor is disarmed and the file lives
    /// at `target`. On failure the temp file remains at its original path.
    ///
    /// # Errors
    ///
    /// Returns an error if the rename fails (e.g. cross-filesystem).
    pub fn persist(self, target: impl AsRef<Path>) -> Result<fs::File, PersistError> {
        let Parts { path, file } = self.into_parts();
        match fs::rename(&path, target.as_ref()) {
            Ok(()) => Ok(file),
            Err(error) => Err(PersistError {
                file: NamedTempFile { path, file },
                error,
            }),
        }
    }

    /// Decompose into path and file handle without running the destructor.
    fn into_parts(self) -> Parts {
        let this = std::mem::ManuallyDrop::new(self);
        // SAFETY: we read each field exactly once and never use `this` again.
        // ManuallyDrop prevents the NamedTempFile destructor (which would
        // delete the file) from running.
        unsafe {
            let path = std::ptr::read(&this.path);
            let file = std::ptr::read(&this.file);
            Parts { path, file }
        }
    }
}

impl Drop for NamedTempFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

impl Write for NamedTempFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.file.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

impl io::Read for NamedTempFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.file.read(buf)
    }
}

impl io::Seek for NamedTempFile {
    fn seek(&mut self, pos: io::SeekFrom) -> io::Result<u64> {
        self.file.seek(pos)
    }
}

/// Error returned when [`NamedTempFile::persist`] fails.
#[derive(Debug)]
pub struct PersistError {
    /// The temp file that failed to persist (still at its original path).
    pub file: NamedTempFile,
    /// The underlying I/O error.
    pub error: io::Error,
}

impl std::fmt::Display for PersistError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "failed to persist temp file: {}", self.error)
    }
}

impl std::error::Error for PersistError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}

impl From<PersistError> for io::Error {
    fn from(e: PersistError) -> Self {
        e.error
    }
}

// ============================================================================
// Builder
// ============================================================================

/// Builder for creating temporary files and directories with custom options.
#[derive(Debug, Default)]
pub struct Builder {
    prefix: Option<String>,
}

impl Builder {
    /// Create a new builder with default options.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the prefix for the temporary name.
    #[must_use]
    pub fn prefix(mut self, prefix: &str) -> Self {
        self.prefix = Some(prefix.to_owned());
        self
    }

    /// Create a temporary directory using the configured options.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be created.
    pub fn tempdir(&self) -> io::Result<TempDir> {
        let prefix = self.prefix.as_deref().unwrap_or(".tmp");
        TempDir::with_prefix_in(prefix, std::env::temp_dir())
    }

    /// Create a temporary directory inside `dir` using the configured options.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be created.
    pub fn tempdir_in(&self, dir: impl AsRef<Path>) -> io::Result<TempDir> {
        let prefix = self.prefix.as_deref().unwrap_or(".tmp");
        TempDir::with_prefix_in(prefix, dir)
    }
}

// ============================================================================
// Free functions
// ============================================================================

/// Create a temporary directory in the system temp directory.
///
/// # Errors
///
/// Returns an error if the directory cannot be created.
pub fn tempdir() -> io::Result<TempDir> {
    TempDir::new()
}

/// Create a temporary directory inside `dir`.
///
/// # Errors
///
/// Returns an error if the directory cannot be created.
pub fn tempdir_in(dir: impl AsRef<Path>) -> io::Result<TempDir> {
    TempDir::new_in(dir)
}

/// Create an anonymous temporary file in the system temp directory.
///
/// The file has no path entry after creation (unlike [`NamedTempFile`]).
/// It is automatically deleted when the returned `File` handle is dropped.
///
/// # Errors
///
/// Returns an error if the file cannot be created.
pub fn tempfile() -> io::Result<fs::File> {
    let name = unique_name(".anon");
    let path = std::env::temp_dir().join(name);
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&path)?;
    // Immediately unlink the file so it's deleted when the handle closes.
    #[cfg(unix)]
    {
        let _ = fs::remove_file(&path);
    }
    #[cfg(not(unix))]
    {
        // On Windows, we can't unlink an open file. The file will be cleaned
        // up by the OS temp directory cleanup. This matches tempfile crate behavior.
    }
    Ok(file)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn tempdir_creates_and_cleans_up() {
        let path;
        {
            let dir = tempdir().expect("create tempdir");
            path = dir.path().to_path_buf();
            assert!(path.exists(), "tempdir should exist");
            assert!(path.is_dir(), "tempdir should be a directory");
        }
        assert!(!path.exists(), "tempdir should be cleaned up on drop");
    }

    #[test]
    fn tempdir_in_creates_in_specified_dir() {
        let parent = tempdir().expect("create parent");
        let child = tempdir_in(parent.path()).expect("create child");
        assert!(child.path().starts_with(parent.path()));
    }

    #[test]
    fn tempdir_keep_prevents_cleanup() {
        let path;
        {
            let dir = tempdir().expect("create tempdir");
            path = dir.keep();
        }
        assert!(path.exists(), "kept tempdir should still exist");
        fs::remove_dir_all(&path).expect("manual cleanup");
    }

    #[test]
    fn named_tempfile_creates_and_cleans_up() {
        let path;
        {
            let mut f = NamedTempFile::new().expect("create");
            path = f.path().to_path_buf();
            assert!(path.exists());
            f.write_all(b"hello").expect("write");
        }
        assert!(!path.exists(), "tempfile should be cleaned up on drop");
    }

    #[test]
    fn named_tempfile_persist() {
        let dir = tempdir().expect("create dir");
        let target = dir.path().join("persisted.txt");
        let mut f = NamedTempFile::new().expect("create");
        f.write_all(b"persisted").expect("write");
        let original_path = f.path().to_path_buf();
        f.persist(&target).expect("persist");
        assert!(target.exists(), "target should exist after persist");
        assert!(!original_path.exists(), "original should be gone");
        let mut content = String::new();
        fs::File::open(&target)
            .expect("open")
            .read_to_string(&mut content)
            .expect("read");
        assert_eq!(content, "persisted");
    }

    #[test]
    fn builder_prefix() {
        let dir = Builder::new()
            .prefix("myprefix_")
            .tempdir()
            .expect("create");
        let name = dir.path().file_name().unwrap().to_str().unwrap();
        assert!(name.starts_with("myprefix_"), "name: {name}");
    }

    #[test]
    fn tempfile_anonymous() {
        let mut f = tempfile().expect("create");
        f.write_all(b"anon").expect("write");
    }

    #[test]
    fn unique_names_are_unique() {
        let a = unique_name("test");
        let b = unique_name("test");
        assert_ne!(a, b);
    }
}
