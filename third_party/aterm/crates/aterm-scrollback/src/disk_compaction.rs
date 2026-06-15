// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0
// Author: The aterm Authors

//! Compaction, clear, sync, and mmap lifecycle for [`DiskColdTier`].

use super::DiskColdTier;
use crate::disk_format::{
    HEADER_SIZE, MAGIC, PAGE_HEADER_SIZE, PageIndexEntry, VERSION, len_to_u32, len_u32_to_usize,
};
use crate::line::{deserialize_lines, serialize_lines};
use crate::mmap::MmapMut;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};

impl DiskColdTier {
    /// Rewrite surviving pages contiguously to reclaim dead space at the
    /// front of the file. Uses atomic temp-file + rename for crash safety.
    ///
    /// Called from `truncate_front_lines` when `dead_bytes() > live_bytes()`
    /// (file is >50% garbage). Compaction failure is non-fatal — the file
    /// works fine with dead space; the next rotation will retry.
    pub(super) fn compact(&mut self) -> io::Result<()> {
        if self.file.is_none() {
            return Ok(());
        }

        self.flush_and_drop_mmap()?;

        let tmp_path = self.path.with_extension("dtrm.tmp");
        let mut tmp = File::create(&tmp_path)?;

        let mut header = [0u8; HEADER_SIZE];
        header[0..4].copy_from_slice(MAGIC);
        header[4..8].copy_from_slice(&VERSION.to_le_bytes());
        tmp.write_all(&header)?;

        let mut new_offset = HEADER_SIZE as u64;
        let mut new_index = Vec::with_capacity(self.index.len());
        let trim_front = self.front_offset;
        let file = self
            .file
            .as_mut()
            .expect("invariant: file exists after is_some guard");

        for (i, entry) in self.index.iter().enumerate() {
            let (idx_entry, size) = if i == 0 && trim_front > 0 {
                Self::compact_page_trimmed(file, entry, trim_front, new_offset, &mut tmp)?
            } else {
                Self::compact_page_verbatim(file, entry, new_offset, &mut tmp)?
            };
            new_index.push(idx_entry);
            new_offset += size;
        }

        Self::write_compact_header(&mut tmp, &new_index)?;
        drop(tmp);
        std::fs::rename(&tmp_path, &self.path)?;
        self.reopen_after_compact(new_index, new_offset, trim_front > 0)
    }

    /// Decompress the first page, trim the consumed prefix, recompress (#5942).
    fn compact_page_trimmed(
        file: &mut File,
        entry: &PageIndexEntry,
        trim: usize,
        write_offset: u64,
        tmp: &mut File,
    ) -> io::Result<(PageIndexEntry, u64)> {
        let mut compressed = vec![0u8; len_u32_to_usize(entry.compressed_size)];
        file.seek(SeekFrom::Start(entry.offset + PAGE_HEADER_SIZE as u64))?;
        file.read_exact(&mut compressed)?;

        let decompressed = crate::decode_zstd_bounded(&compressed)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
        let lines = deserialize_lines(&decompressed);
        if trim > lines.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "front_offset ({trim}) exceeds deserialized line count ({})",
                    lines.len()
                ),
            ));
        }
        let trimmed = &lines[trim..];
        let recompressed = zstd::encode_all(serialize_lines(trimmed).as_slice(), 3)?;

        let mut page_header = [0u8; PAGE_HEADER_SIZE];
        page_header[0..4].copy_from_slice(&len_to_u32(recompressed.len()).to_le_bytes());
        page_header[4..8].copy_from_slice(&len_to_u32(trimmed.len()).to_le_bytes());
        tmp.write_all(&page_header)?;
        tmp.write_all(&recompressed)?;

        let size = PAGE_HEADER_SIZE as u64 + recompressed.len() as u64;
        let idx = PageIndexEntry {
            offset: write_offset,
            compressed_size: len_to_u32(recompressed.len()),
            line_count: len_to_u32(trimmed.len()),
        };
        Ok((idx, size))
    }

    /// Copy a page verbatim to the compacted file.
    fn compact_page_verbatim(
        file: &mut File,
        entry: &PageIndexEntry,
        write_offset: u64,
        tmp: &mut File,
    ) -> io::Result<(PageIndexEntry, u64)> {
        let page_size = PAGE_HEADER_SIZE as u64 + u64::from(entry.compressed_size);
        let mut buf = vec![0u8; page_size as usize];
        file.seek(SeekFrom::Start(entry.offset))?;
        file.read_exact(&mut buf)?;
        tmp.write_all(&buf)?;

        let idx = PageIndexEntry {
            offset: write_offset,
            compressed_size: entry.compressed_size,
            line_count: entry.line_count,
        };
        Ok((idx, page_size))
    }

    /// Write final page/line counts to the compacted file header.
    fn write_compact_header(tmp: &mut File, index: &[PageIndexEntry]) -> io::Result<()> {
        let physical_lines: usize = index.iter().map(|e| len_u32_to_usize(e.line_count)).sum();
        tmp.seek(SeekFrom::Start(8))?;
        tmp.write_all(&(index.len() as u64).to_le_bytes())?;
        tmp.write_all(&(physical_lines as u64).to_le_bytes())?;
        tmp.sync_data()
    }

    /// Reopen the compacted file and rebuild in-memory state.
    fn reopen_after_compact(
        &mut self,
        new_index: Vec<PageIndexEntry>,
        new_offset: u64,
        had_front_trim: bool,
    ) -> io::Result<()> {
        let file = OpenOptions::new().read(true).write(true).open(&self.path)?;
        // SAFETY: File is exclusively owned; we just wrote and renamed it.
        self.mmap = if new_offset > HEADER_SIZE as u64 {
            Some(unsafe { MmapMut::map_mut(&file)? })
        } else {
            None
        };
        self.file = Some(file);
        self.index = new_index;
        self.write_offset = new_offset;

        if had_front_trim {
            // Consumed prefix is now physically trimmed — reset front_offset (#5942).
            self.front_offset = 0;
        }

        // Rebuild cumulative_lines to match updated page line counts.
        self.cumulative_lines.clear();
        let mut cumulative = 0;
        for entry in &self.index {
            cumulative += len_u32_to_usize(entry.line_count);
            self.cumulative_lines.push(cumulative);
        }

        self.cache.get_mut().clear();
        self.reset_bytes_used();
        Ok(())
    }

    /// Clear all data.
    pub fn clear(&mut self) -> io::Result<()> {
        self.index.clear();
        self.cumulative_lines.clear();
        self.line_count = 0;
        self.front_offset = 0;
        self.cache.get_mut().clear();
        self.access_counter.set(0);
        self.write_offset = HEADER_SIZE as u64;

        // Truncate file if we have one
        if self.file.is_some() {
            self.flush_and_drop_mmap()?;
        }

        if let Some(ref mut file) = self.file {
            file.set_len(HEADER_SIZE as u64)?;
            file.seek(SeekFrom::Start(8))?;
            file.write_all(&0u64.to_le_bytes())?; // page_count
            file.write_all(&0u64.to_le_bytes())?; // line_count
            file.sync_data()?;
        }

        self.reset_bytes_used();

        Ok(())
    }

    /// Sync changes to disk.
    #[cfg(test)]
    pub fn sync(&mut self) -> io::Result<()> {
        if let Some(ref mmap) = self.mmap {
            mmap.flush()?;
        }
        if let Some(ref mut file) = self.file {
            file.sync_all()?;
        }
        Ok(())
    }

    /// Ensure mapped data is flushed before unmapping.
    ///
    /// We explicitly drop the mmap to avoid relying on field drop order.
    pub(super) fn flush_and_drop_mmap(&mut self) -> io::Result<()> {
        if let Some(ref mmap) = self.mmap {
            mmap.flush()?;
        }
        self.mmap = None;
        Ok(())
    }
}
