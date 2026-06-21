// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0

//! Disk-backed cold tier storage using memory-mapped files.
//!
//! File format defined in [`disk_format`](super::disk_format).
//! Pages are loaded on demand and cached in an LRU cache for repeated access.
//! The index is rebuilt on load by scanning page headers.

pub use super::disk_format::DiskColdConfig;
use super::disk_format::{
    DEFAULT_CACHE_SIZE, HEADER_SIZE, MAGIC, PAGE_HEADER_SIZE, PageIndexEntry, VERSION,
    len_u32_to_usize, len_u64_to_usize,
};
use super::line::Line;
use crate::mmap::MmapMut;
use std::cell::Cell;
use std::cell::RefCell;
#[cfg(not(kani))]
use std::collections::HashMap;
// Under Kani, BTreeMap avoids unsupported CCRandomGenerateBytes FFI from HashMap's hasher seed.
#[cfg(kani)]
use std::collections::BTreeMap as HashMap;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

struct CacheEntry {
    lines: Vec<Line>,
    last_access: u64,
}

/// Disk-backed cold tier storage.
///
/// Stores Zstd-compressed pages in a memory-mapped file with lazy loading.
///
/// The LRU page cache uses interior mutability (`RefCell`/`Cell`) so that
/// `get_line` can take `&self`. This mirrors the pattern used by
/// [`ColdTier`](super::ColdTier) and allows FFI functions that only read
/// terminal state to accept `*const AtermTerminal`.
#[derive(Debug)]
pub struct DiskColdTier {
    /// Storage file.
    file: Option<File>,
    /// Memory map of the file (for reading).
    mmap: Option<MmapMut>,
    /// Path to the storage file.
    path: PathBuf,
    /// Page index (kept in memory for fast lookup).
    index: Vec<PageIndexEntry>,
    /// Total line count.
    line_count: usize,
    /// Cumulative line counts for binary search.
    cumulative_lines: Vec<usize>,
    /// LRU cache of decompressed pages (interior mutability for `&self` reads).
    cache: RefCell<HashMap<usize, CacheEntry>>,
    /// Cache size limit.
    cache_size: usize,
    /// Access counter for LRU (interior mutability for `&self` reads).
    access_counter: Cell<u64>,
    /// Next write offset in file.
    write_offset: u64,
    /// Running total for `memory_used()`.
    bytes_used: Cell<usize>,
    /// Lines logically consumed from the first page. Avoids decompression
    /// during line-limit truncation — pages are dropped when fully consumed.
    front_offset: usize,
}

impl DiskColdTier {
    /// Create a new in-memory cold tier (no disk backing).
    #[must_use]
    pub fn new() -> Self {
        Self {
            file: None,
            mmap: None,
            path: PathBuf::new(),
            index: Vec::new(),
            line_count: 0,
            cumulative_lines: Vec::new(),
            cache: RefCell::new(HashMap::new()),
            cache_size: DEFAULT_CACHE_SIZE,
            access_counter: Cell::new(0),
            write_offset: HEADER_SIZE as u64,
            bytes_used: Cell::new(0),
            front_offset: 0,
        }
        .with_computed_bytes_used()
    }

    /// Create a disk-backed cold tier; loads existing file or creates new.
    ///
    /// Cleans up orphan `.dtrm.tmp` files left by a crash during compaction
    /// before opening the main store (#5964).
    pub fn with_config(config: DiskColdConfig) -> io::Result<Self> {
        let path = config.path;
        let cache_size = config.cache_size;

        // Remove orphan compaction temp files if present. A crash between
        // `File::create(tmp)` and `fs::rename(tmp, main)` in `compact()`
        // leaves incomplete temp files that must be discarded.
        let tmp_path = path.with_extension("dtrm.tmp");
        if tmp_path.exists() {
            let _ = std::fs::remove_file(&tmp_path);
        }
        // Also clean up `.dtrm.compact` orphans from the compaction output path.
        let compact_path = path.with_extension("dtrm.compact");
        if compact_path.exists() {
            let _ = std::fs::remove_file(&compact_path);
        }

        if path.exists() {
            Self::load(&path, cache_size)
        } else {
            Self::create(&path, cache_size)
        }
    }

    fn create(path: &Path, cache_size: usize) -> io::Result<Self> {
        if let Some(parent) = path.parent() {
            crate::storage::create_dir_restricted(parent)?;
        }

        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)?;

        // Write header
        let mut header = [0u8; HEADER_SIZE];
        header[0..4].copy_from_slice(MAGIC);
        header[4..8].copy_from_slice(&VERSION.to_le_bytes());
        // page_count and line_count start at 0
        file.write_all(&header)?;
        file.sync_data()?;

        Ok(Self {
            file: Some(file),
            mmap: None,
            path: path.to_path_buf(),
            index: Vec::new(),
            line_count: 0,
            cumulative_lines: Vec::new(),
            cache: RefCell::new(HashMap::new()),
            cache_size,
            access_counter: Cell::new(0),
            write_offset: HEADER_SIZE as u64,
            bytes_used: Cell::new(0),
            front_offset: 0,
        }
        .with_computed_bytes_used())
    }

    /// Load an existing storage file.
    fn load(path: &Path, cache_size: usize) -> io::Result<Self> {
        let mut file = OpenOptions::new().read(true).write(true).open(path)?;
        let page_count = Self::validate_header(&mut file)?;
        let file_len = file.metadata()?.len();
        let (index, cumulative_lines, line_count, write_offset) =
            Self::scan_pages(&mut file, file_len, page_count)?;

        // SAFETY: File is exclusively owned; no external process modifies it.
        let mmap = if file_len > HEADER_SIZE as u64 {
            Some(unsafe { MmapMut::map_mut(&file)? })
        } else {
            None
        };

        Ok(Self {
            file: Some(file),
            mmap,
            path: path.to_path_buf(),
            index,
            line_count,
            cumulative_lines,
            cache: RefCell::new(HashMap::new()),
            cache_size,
            access_counter: Cell::new(0),
            write_offset,
            bytes_used: Cell::new(0),
            front_offset: 0,
        }
        .with_computed_bytes_used())
    }

    /// Validate file header; returns page count on success.
    fn validate_header(file: &mut File) -> io::Result<usize> {
        let mut header = [0u8; HEADER_SIZE];
        file.read_exact(&mut header)?;
        if &header[0..4] != MAGIC {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid magic bytes",
            ));
        }
        let version = u32::from_le_bytes(
            header[4..8]
                .try_into()
                .expect("invariant: 4-byte slice fits [u8; 4]"),
        );
        if version != VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unsupported version: {version}"),
            ));
        }
        len_u64_to_usize(u64::from_le_bytes(
            header[8..16]
                .try_into()
                .expect("invariant: 8-byte slice fits [u8; 8]"),
        ))
    }

    /// Scan page headers to rebuild the in-memory index.
    fn scan_pages(
        file: &mut File,
        file_len: u64,
        capacity: usize,
    ) -> io::Result<(Vec<PageIndexEntry>, Vec<usize>, usize, u64)> {
        let mut index = Vec::with_capacity(capacity);
        let mut cumulative_lines = Vec::with_capacity(capacity);
        let mut cumulative = 0usize;
        let mut offset = HEADER_SIZE as u64;
        let mut buf = [0u8; PAGE_HEADER_SIZE];

        while offset + PAGE_HEADER_SIZE as u64 <= file_len {
            file.seek(SeekFrom::Start(offset))?;
            if file.read_exact(&mut buf).is_err() {
                break;
            }
            let compressed_size = u32::from_le_bytes(
                buf[0..4]
                    .try_into()
                    .expect("invariant: 4-byte slice fits [u8; 4]"),
            );
            let line_count = u32::from_le_bytes(
                buf[4..8]
                    .try_into()
                    .expect("invariant: 4-byte slice fits [u8; 4]"),
            );
            if compressed_size == 0 {
                break;
            }
            // Validate page data fits within file (crash recovery: #5917).
            // A crash mid-write leaves a partial page at the end — discard it.
            let page_end = offset
                .saturating_add(PAGE_HEADER_SIZE as u64)
                .saturating_add(u64::from(compressed_size));
            if page_end > file_len {
                break;
            }
            index.push(PageIndexEntry {
                offset,
                compressed_size,
                line_count,
            });
            cumulative += len_u32_to_usize(line_count);
            cumulative_lines.push(cumulative);
            offset = offset
                .saturating_add(PAGE_HEADER_SIZE as u64)
                .saturating_add(u64::from(compressed_size));
        }

        #[cfg(debug_assertions)]
        {
            let total: usize = index.iter().map(|e| e.line_count as usize).sum();
            debug_assert_eq!(cumulative, total, "line count matches index");
        }

        Ok((index, cumulative_lines, cumulative, offset))
    }

    /// Get the total number of lines.
    #[must_use]
    #[inline]
    pub fn line_count(&self) -> usize {
        self.line_count
    }

    /// Get the total compressed size on disk.
    #[must_use]
    pub fn compressed_size(&self) -> usize {
        self.index
            .iter()
            .map(|e| len_u32_to_usize(e.compressed_size))
            .sum()
    }

    /// Estimate in-memory usage (bytes). Excludes mmap pages.
    #[must_use]
    pub fn memory_used(&self) -> usize {
        self.bytes_used.get()
    }

    /// Bytes of dead (unreclaimable) space at the front of the file.
    ///
    /// After `truncate_front_lines` drops pages, the space between the file
    /// header and the first surviving page is dead — it cannot be reclaimed
    /// by `ftruncate` (which only trims from the end).
    fn dead_bytes(&self) -> u64 {
        self.index
            .first()
            .map_or(0, |e| e.offset.saturating_sub(HEADER_SIZE as u64))
    }

    /// Bytes of live compressed data in the file (surviving pages).
    fn live_bytes(&self) -> u64 {
        self.write_offset
            .saturating_sub(self.dead_bytes())
            .saturating_sub(HEADER_SIZE as u64)
    }

    #[cfg(any(test, debug_assertions))]
    #[must_use]
    pub(crate) fn recompute_memory_used(&self) -> usize {
        self.calculate_memory_used()
    }
}

/// Flush and unmap before closing the backing file.
impl Drop for DiskColdTier {
    fn drop(&mut self) {
        let _ = self.flush_and_drop_mmap();
        if let Some(ref mut file) = self.file {
            let _ = file.sync_all();
        }
    }
}

impl Default for DiskColdTier {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for CacheEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CacheEntry")
            .field("lines_count", &self.lines.len())
            .field("last_access", &self.last_access)
            .finish()
    }
}

#[cfg(test)]
#[path = "disk_tests.rs"]
mod tests;

#[path = "disk_memory.rs"]
mod memory;

#[path = "disk_write.rs"]
mod write;

#[path = "disk_read.rs"]
mod read;

#[path = "disk_front_truncation.rs"]
mod front_truncation;

#[path = "disk_compaction.rs"]
mod compaction;

#[cfg(kani)]
#[path = "disk_kani.rs"]
mod proofs;
