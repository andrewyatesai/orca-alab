// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Memory management and back-removal operations for [`DiskColdTier`].
//!
//! Contains `bytes_used` tracking, page decompression, LRU cache, and
//! newest-end (back) removal. Extracted to keep `disk.rs` under the
//! 500-line file limit.

use super::{CacheEntry, DiskColdTier, HashMap, Line, PageIndexEntry};
use crate::disk_format::{HEADER_SIZE, PAGE_HEADER_SIZE, len_to_u32, len_u32_to_usize};
use crate::line::deserialize_lines;
use crate::mmap::MmapMut;
use std::io::{self, Seek, SeekFrom, Write};

impl DiskColdTier {
    pub(crate) fn with_computed_bytes_used(self) -> Self {
        self.reset_bytes_used();
        self
    }

    pub(crate) fn reset_bytes_used(&self) {
        self.bytes_used.set(self.calculate_memory_used());
    }

    pub(crate) fn calculate_memory_used(&self) -> usize {
        // Saturating arithmetic throughout: this is a diagnostic byte counter, so
        // a pathological capacity must clamp to `usize::MAX` (an "enormous"
        // reading) rather than debug-panic or release-wrap to a bogus small one.
        // Trust `-Z trust-verify` proves each operation here panic-free.
        let base = std::mem::size_of::<Self>();
        let path_mem = self.path.capacity();
        let index_mem = self
            .index
            .capacity()
            .saturating_mul(std::mem::size_of::<PageIndexEntry>());
        let cumulative_mem = self
            .cumulative_lines
            .capacity()
            .saturating_mul(std::mem::size_of::<usize>());
        let cache = self.cache.borrow();
        let cache_struct_mem = Self::cache_bucket_count(&cache)
            .saturating_mul(std::mem::size_of::<usize>().saturating_add(std::mem::size_of::<CacheEntry>()));
        let line_struct_size = std::mem::size_of::<Line>();
        let cache_lines_mem: usize = cache
            .values()
            .map(|entry| {
                let lines_mem = entry.lines.capacity().saturating_mul(line_struct_size);
                let contents_mem: usize = entry
                    .lines
                    .iter()
                    .map(|line| line.memory_used().saturating_sub(line_struct_size))
                    .fold(0usize, usize::saturating_add);
                lines_mem.saturating_add(contents_mem)
            })
            .fold(0usize, usize::saturating_add);
        base.saturating_add(path_mem)
            .saturating_add(index_mem)
            .saturating_add(cumulative_mem)
            .saturating_add(cache_struct_mem)
            .saturating_add(cache_lines_mem)
    }

    #[cfg(not(kani))]
    fn cache_bucket_count(cache: &HashMap<usize, CacheEntry>) -> usize {
        cache.capacity()
    }

    #[cfg(kani)]
    fn cache_bucket_count(cache: &HashMap<usize, CacheEntry>) -> usize {
        cache.len()
    }

    /// Decompress a page from disk via mmap.
    pub(super) fn decompress_page(
        &self,
        page_idx: usize,
    ) -> Result<Vec<Line>, crate::ScrollbackError> {
        let Some(entry) = self.index.get(page_idx) else {
            return Err(crate::ScrollbackError::Io(io::Error::new(
                io::ErrorKind::NotFound,
                format!("page index {page_idx} out of range"),
            )));
        };

        let compressed = if let Some(ref mmap) = self.mmap {
            let offset_usize = usize::try_from(entry.offset).map_err(|_| {
                crate::ScrollbackError::Io(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "page offset overflows usize",
                ))
            })?;
            // Checked offset arithmetic: a malformed (attacker-influenced)
            // `PageIndexEntry` could carry a huge offset/compressed_size, so
            // every addition must reject overflow rather than wrap.
            let compressed_len = len_u32_to_usize(entry.compressed_size);
            let data_start = offset_usize.checked_add(PAGE_HEADER_SIZE).ok_or_else(|| {
                crate::ScrollbackError::Io(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "page data_start overflow",
                ))
            })?;
            let data_end = data_start.checked_add(compressed_len).ok_or_else(|| {
                crate::ScrollbackError::Io(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "page data_end overflow",
                ))
            })?;

            // Defense-in-depth against another process truncating the backing
            // file: the mmap length is fixed at map time, so a shrunk file
            // leaves the tail of the mapping pointing past EOF (SIGBUS on
            // deref). Re-read the live file length and reject reads that fall
            // outside the current file before touching the mapping.
            //
            // This is best-effort: a truncation racing between this metadata
            // read and the actual deref of the returned slice (during
            // decompression, below) can still fault. Closing that window fully
            // requires pread()/read_at or a scoped SIGBUS handler; the map_mut
            // SAFETY contract already forbids concurrent external modification,
            // so this check is hardening beyond contract, not the primary
            // guarantee.
            if let Some(meta) = self.file.as_ref().and_then(|f| f.metadata().ok()) {
                // Fail CLOSED: if the live length can't be represented (cannot
                // happen on 64-bit, where u64->usize is infallible), refuse the
                // read rather than skipping the truncation check.
                let live_len = usize::try_from(meta.len()).map_err(|_| {
                    crate::ScrollbackError::Io(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "live file length does not fit in usize",
                    ))
                })?;
                if data_end > live_len {
                    return Err(crate::ScrollbackError::Io(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        format!(
                            "page {page_idx} range {data_start}..{data_end} exceeds live file len {live_len} (file truncated?)"
                        ),
                    )));
                }
            }

            // Route through the checked accessor so the raw `from_raw_parts`
            // is never indexed past the recorded mapping length. `slice`
            // validates `data_start + compressed_len <= mmap.len()`.
            mmap.slice(data_start, compressed_len).ok_or_else(|| {
                crate::ScrollbackError::Io(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    format!(
                        "page {page_idx} range {data_start}..{data_end} exceeds mmap len {}",
                        mmap.len()
                    ),
                ))
            })?
        } else {
            return Err(crate::ScrollbackError::Io(io::Error::new(
                io::ErrorKind::NotFound,
                "no memory map available for disk read",
            )));
        };

        let decompressed = crate::decode_zstd_bounded(compressed)?;
        Ok(deserialize_lines(&decompressed))
    }

    /// Add a page to the LRU cache, evicting if necessary.
    pub(super) fn cache_page(&self, page_idx: usize, lines: Vec<Line>) {
        if self.cache_size == 0 {
            return;
        }
        {
            let mut cache = self.cache.borrow_mut();
            while cache.len() >= self.cache_size {
                let lru_key = cache
                    .iter()
                    .min_by_key(|(_, e)| e.last_access)
                    .map(|(k, _)| *k);
                if let Some(key) = lru_key {
                    cache.remove(&key);
                } else {
                    break;
                }
            }
            let counter = self.access_counter.get() + 1;
            self.access_counter.set(counter);
            cache.insert(
                page_idx,
                CacheEntry {
                    lines,
                    last_access: counter,
                },
            );
        }
        self.reset_bytes_used();
    }

    // ------------------------------------------------------------------
    // Back-removal (newest-end) operations
    // ------------------------------------------------------------------

    /// Count whole pages consumable from the back and remaining boundary trim.
    ///
    /// Returns `(whole_pages, boundary_trim)` where `boundary_trim` is the
    /// number of lines to trim from the boundary page's back.
    fn count_back_pages(&self, n: usize) -> (usize, usize) {
        let mut whole_pages = 0;
        let mut remaining = n;
        for entry in self.index.iter().rev() {
            let page_lines = len_u32_to_usize(entry.line_count);
            let actual_idx = self.index.len() - 1 - whole_pages;
            let available = if actual_idx == 0 {
                page_lines.saturating_sub(self.front_offset)
            } else {
                page_lines
            };
            if remaining >= available {
                remaining -= available;
                whole_pages += 1;
            } else {
                break;
            }
        }
        (whole_pages, remaining)
    }

    /// Pre-validate that `truncate_back_lines(n)` will succeed.
    ///
    /// Tries to decompress the boundary page (if any) without modifying state.
    /// Call this before committing cross-tier removal to ensure error safety.
    pub fn pre_validate_truncate_back(&self, n: usize) -> Result<(), crate::ScrollbackError> {
        if n == 0 || n >= self.line_count {
            return Ok(());
        }
        let (whole_pages, boundary_trim) = self.count_back_pages(n);
        if boundary_trim > 0 && whole_pages < self.index.len() {
            let boundary_idx = self.index.len() - 1 - whole_pages;
            self.decompress_page(boundary_idx)?;
        }
        Ok(())
    }

    /// Remove the newest `n` lines from the back of the cold tier.
    ///
    /// Drops whole pages from the back without decompression. For the
    /// boundary page (partially within the remove range), decompresses it,
    /// trims the consumed lines from the back, re-compresses, and rewrites
    /// the page at its original file offset.
    ///
    /// Error safety (#4638): the boundary page is decompressed before any state
    /// is modified. On decompression failure, state is unchanged.
    ///
    /// # Panics
    ///
    /// Debug-asserts that `n <= self.line_count`.
    pub fn truncate_back_lines(&mut self, n: usize) -> Result<(), crate::ScrollbackError> {
        if n == 0 {
            return Ok(());
        }
        debug_assert!(
            n <= self.line_count,
            "truncate_back_lines({n}) exceeds line_count({})",
            self.line_count
        );

        let (whole_pages, boundary_trim) = self.count_back_pages(n);

        // Pre-decompress boundary page if needed (before modifying state).
        let boundary_data = if boundary_trim > 0 {
            let boundary_idx = self.index.len() - 1 - whole_pages;
            let lines = self.decompress_page(boundary_idx)?;
            debug_assert!(
                lines.len() >= boundary_trim,
                "decompress returned {} lines but boundary_trim is {}",
                lines.len(),
                boundary_trim,
            );
            let keep = lines.len().saturating_sub(boundary_trim);
            if keep == 0 {
                None
            } else {
                let serialized = crate::line::serialize_lines(&lines[..keep]);
                let compressed = zstd::encode_all(serialized.as_slice(), 3)
                    .map_err(crate::ScrollbackError::Io)?;
                Some((compressed, keep))
            }
        } else {
            None
        };

        // Commit — remove whole pages from back.
        for _ in 0..whole_pages {
            self.index.pop();
            self.cumulative_lines.pop();
        }

        if boundary_trim > 0 && !self.index.is_empty() {
            self.rewrite_boundary_page_back(boundary_data)?;
        } else {
            self.recalculate_write_offset();
        }

        if n > self.line_count {
            aterm_log::warn!(
                "disk truncate_back_lines({n}) exceeds line_count({}), saturating",
                self.line_count
            );
        }
        self.line_count = self.line_count.saturating_sub(n);

        // Reset front_offset when all pages are gone. Without this, a stale
        // front_offset would incorrectly skip lines from the first page if
        // new pages are appended later. Matches warm/cold tier cleanup.
        if self.index.is_empty() {
            self.front_offset = 0;
        }

        self.finalize_back_truncation()?;
        Ok(())
    }

    /// Rewrite the last index entry with trimmed boundary page data.
    fn rewrite_boundary_page_back(
        &mut self,
        boundary_data: Option<(Vec<u8>, usize)>,
    ) -> Result<(), crate::ScrollbackError> {
        let boundary_entry = self.index.pop().expect("pre-validated non-empty index");
        self.cumulative_lines.pop();

        if let Some((ref compressed, line_count)) = boundary_data {
            if self.mmap.is_some() {
                self.flush_and_drop_mmap()
                    .map_err(crate::ScrollbackError::Io)?;
            }
            if let Some(ref mut file) = self.file {
                file.seek(SeekFrom::Start(boundary_entry.offset))
                    .map_err(crate::ScrollbackError::Io)?;
                let mut page_header = [0u8; PAGE_HEADER_SIZE];
                page_header[0..4].copy_from_slice(&len_to_u32(compressed.len()).to_le_bytes());
                page_header[4..8].copy_from_slice(&len_to_u32(line_count).to_le_bytes());
                file.write_all(&page_header)
                    .map_err(crate::ScrollbackError::Io)?;
                file.write_all(compressed)
                    .map_err(crate::ScrollbackError::Io)?;
                file.sync_data().map_err(crate::ScrollbackError::Io)?;
            }
            let entry = PageIndexEntry {
                offset: boundary_entry.offset,
                compressed_size: len_to_u32(compressed.len()),
                line_count: len_to_u32(line_count),
            };
            self.index.push(entry);
            let cumulative = self.cumulative_lines.last().copied().unwrap_or(0) + line_count;
            self.cumulative_lines.push(cumulative);
            self.write_offset =
                boundary_entry.offset + PAGE_HEADER_SIZE as u64 + compressed.len() as u64;
        } else {
            self.recalculate_write_offset();
        }
        Ok(())
    }

    /// Recalculate write_offset from the last index entry.
    fn recalculate_write_offset(&mut self) {
        if self.index.is_empty() {
            self.write_offset = HEADER_SIZE as u64;
        } else {
            let last = self.index.last().expect("non-empty index");
            self.write_offset = last.offset
                + PAGE_HEADER_SIZE as u64
                + len_u32_to_usize(last.compressed_size) as u64;
        }
    }

    /// Update file header and truncate after back-removal.
    fn finalize_back_truncation(&mut self) -> Result<(), crate::ScrollbackError> {
        if self.file.is_some() {
            if self.mmap.is_some() {
                self.flush_and_drop_mmap()
                    .map_err(crate::ScrollbackError::Io)?;
            }
            if let Some(ref mut file) = self.file {
                file.seek(SeekFrom::Start(8))
                    .map_err(crate::ScrollbackError::Io)?;
                file.write_all(&(self.index.len() as u64).to_le_bytes())
                    .map_err(crate::ScrollbackError::Io)?;
                let physical_lines: usize = self
                    .index
                    .iter()
                    .map(|e| len_u32_to_usize(e.line_count))
                    .sum();
                file.write_all(&(physical_lines as u64).to_le_bytes())
                    .map_err(crate::ScrollbackError::Io)?;
                file.sync_data().map_err(crate::ScrollbackError::Io)?;
                file.set_len(self.write_offset)
                    .map_err(crate::ScrollbackError::Io)?;
                file.sync_data().map_err(crate::ScrollbackError::Io)?;
                // Refresh mmap after file resize.
                // SAFETY: File is exclusively owned; we just updated it.
                if self.write_offset > HEADER_SIZE as u64 {
                    self.mmap = Some(unsafe {
                        MmapMut::map_mut(&*file).map_err(crate::ScrollbackError::Io)?
                    });
                } else {
                    self.mmap = None;
                }
            }
        }
        self.cache.get_mut().clear();
        self.reset_bytes_used();
        Ok(())
    }
}
