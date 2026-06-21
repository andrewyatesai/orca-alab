// Copyright 2026 The aterm Authors
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for `DiskColdTier` — extracted from `disk.rs` (#2100).

use super::super::line::serialize_lines;
use super::*;
use crate::ScrollbackError;
use aterm_tempfile::tempdir;
use std::fs::File;
use std::io::Read;

impl DiskColdTier {
    /// Get the number of pages.
    #[must_use]
    pub fn page_count(&self) -> usize {
        self.index.len()
    }

    /// Check if empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    /// Check if disk-backed.
    #[must_use]
    pub fn is_disk_backed(&self) -> bool {
        self.file.is_some()
    }

    /// Test-only: corrupt the last index entry so it points past the mapped
    /// region, simulating a malformed/attacker-influenced `PageIndexEntry` or
    /// an out-of-band file truncation.
    #[cfg(test)]
    fn corrupt_last_entry_range(&mut self, offset: u64, compressed_size: u32) {
        if let Some(entry) = self.index.last_mut() {
            entry.offset = offset;
            entry.compressed_size = compressed_size;
        }
    }

    /// Test-only: invoke the page decompression path directly.
    #[cfg(test)]
    fn decompress_page_for_test(
        &self,
        page_idx: usize,
    ) -> Result<Vec<Line>, ScrollbackError> {
        self.decompress_page(page_idx)
    }
}

fn create_test_page(line_count: usize, prefix: &str) -> (Vec<u8>, usize) {
    let lines: Vec<Line> = (0..line_count)
        .map(|i| Line::from(&*format!("{prefix}-Line{i}")))
        .collect();
    let serialized = serialize_lines(&lines);
    let compressed = zstd::encode_all(serialized.as_slice(), 3).unwrap();
    (compressed, line_count)
}

fn read_header_counts(path: &Path) -> (u64, u64) {
    let mut file = File::open(path).unwrap();
    let mut header = [0u8; HEADER_SIZE];
    file.read_exact(&mut header).unwrap();
    let page_count = u64::from_le_bytes(header[8..16].try_into().unwrap());
    let line_count = u64::from_le_bytes(header[16..24].try_into().unwrap());
    (page_count, line_count)
}

/// Regression: #1004 - test failed when get_line() assertions were added.
/// In-memory mode is metadata-only by design; data is not stored.
#[test]
fn disk_cold_in_memory() {
    let mut cold = DiskColdTier::new();
    assert!(cold.is_empty());
    assert!(!cold.is_disk_backed());

    // Push first page
    let (compressed, line_count) = create_test_page(10, "Page0");
    cold.push_compressed(&compressed, line_count).unwrap();

    // In-memory mode is metadata-only - data not stored, so get_line() won't work
    // Only verify metadata tracking
    assert!(!cold.is_empty(), "should not be empty after push");
    assert_eq!(cold.line_count(), 10);
    assert_eq!(cold.page_count(), 1);
    let err = cold
        .get_line(0)
        .expect_err("metadata-only in-memory mode must surface read failure");
    assert!(
        matches!(err, ScrollbackError::Io(_)),
        "expected I/O error, got: {err:?}"
    );

    // Push second page - verify metadata accumulates correctly
    let (compressed2, line_count2) = create_test_page(15, "Page1");
    cold.push_compressed(&compressed2, line_count2).unwrap();
    assert_eq!(cold.line_count(), 25, "line count should accumulate");
    assert_eq!(cold.page_count(), 2, "page count should increment");
}

#[test]
fn disk_cold_file_create() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("scrollback/cold/cold.dtrm");

    let config = DiskColdConfig::new(&path);
    let mut cold = DiskColdTier::with_config(config).unwrap();

    assert!(cold.is_disk_backed());
    assert!(path.exists());

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let storage_dir_mode = path
            .parent()
            .expect("cold storage directory")
            .metadata()
            .expect("cold storage metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(
            storage_dir_mode, 0o700,
            "cold storage directory should be 0o700, got 0o{storage_dir_mode:03o}"
        );
    }

    let (compressed, line_count) = create_test_page(10, "Page0");
    cold.push_compressed(&compressed, line_count).unwrap();

    assert_eq!(cold.line_count(), 10);
}

#[test]
fn disk_cold_file_reload() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("cold.dtrm");

    // Create and populate
    {
        let config = DiskColdConfig::new(&path);
        let mut cold = DiskColdTier::with_config(config).unwrap();

        let (compressed1, count1) = create_test_page(5, "Page0");
        cold.push_compressed(&compressed1, count1).unwrap();

        let (compressed2, count2) = create_test_page(5, "Page1");
        cold.push_compressed(&compressed2, count2).unwrap();

        cold.sync().unwrap();
    }

    // Reload and verify
    {
        let config = DiskColdConfig::new(&path);
        let cold = DiskColdTier::with_config(config).unwrap();

        assert_eq!(cold.line_count(), 10);
        assert_eq!(cold.page_count(), 2);
    }
}

#[test]
fn disk_cold_get_line() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("cold.dtrm");

    let config = DiskColdConfig::new(&path);
    let mut cold = DiskColdTier::with_config(config).unwrap();

    // Add multiple pages
    for page_num in 0..3 {
        let (compressed, line_count) = create_test_page(5, &format!("Page{page_num}"));
        cold.push_compressed(&compressed, line_count).unwrap();
    }

    assert_eq!(cold.line_count(), 15);

    // Test line retrieval across pages
    assert_eq!(
        cold.get_line(0)
            .expect("no error")
            .expect("line present")
            .to_string(),
        "Page0-Line0"
    );
    assert_eq!(
        cold.get_line(4)
            .expect("no error")
            .expect("line present")
            .to_string(),
        "Page0-Line4"
    );
    assert_eq!(
        cold.get_line(5)
            .expect("no error")
            .expect("line present")
            .to_string(),
        "Page1-Line0"
    );
    assert_eq!(
        cold.get_line(10)
            .expect("no error")
            .expect("line present")
            .to_string(),
        "Page2-Line0"
    );
    assert_eq!(
        cold.get_line(14)
            .expect("no error")
            .expect("line present")
            .to_string(),
        "Page2-Line4"
    );
    assert!(cold.get_line(15).expect("no error").is_none());
}

#[test]
fn disk_cold_lru_cache() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("cold.dtrm");

    let config = DiskColdConfig::new(&path).with_cache_size(2);
    let mut cold = DiskColdTier::with_config(config).unwrap();

    // Add 5 pages
    for page_num in 0..5 {
        let (compressed, line_count) = create_test_page(10, &format!("Page{page_num}"));
        cold.push_compressed(&compressed, line_count).unwrap();
    }

    // Access pages 0, 1, 2 - cache should only hold 2
    cold.get_line(0).expect("no error");
    cold.get_line(10).expect("no error");
    cold.get_line(20).expect("no error");

    // Cache should have evicted page 0
    assert!(cold.cache.borrow().len() <= 2);
}

#[test]
fn disk_cold_clear() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("cold.dtrm");

    let config = DiskColdConfig::new(&path);
    let mut cold = DiskColdTier::with_config(config).unwrap();

    let (compressed, line_count) = create_test_page(10, "Page0");
    cold.push_compressed(&compressed, line_count).unwrap();

    assert_eq!(cold.line_count(), 10);
    // Verify mmap is usable (not just present) before clear
    assert_eq!(
        cold.get_line(0)
            .expect("no error")
            .expect("line present")
            .to_string(),
        "Page0-Line0"
    );
    let file_len_before = cold
        .file
        .as_ref()
        .expect("file after push")
        .metadata()
        .unwrap()
        .len();
    let mmap_len_before = cold
        .mmap
        .as_ref()
        .expect("mmap must be established after push")
        .len() as u64;
    assert!(file_len_before > HEADER_SIZE as u64);
    assert_eq!(mmap_len_before, file_len_before);

    cold.clear().unwrap();

    assert_eq!(cold.line_count(), 0);
    assert_eq!(cold.page_count(), 0);
    assert!(cold.cache.borrow().is_empty());
    assert!(cold.mmap.is_none());
    assert_eq!(cold.access_counter.get(), 0);
    assert_eq!(cold.write_offset, HEADER_SIZE as u64);
    let file_len_after = cold.file.as_ref().unwrap().metadata().unwrap().len();
    assert_eq!(file_len_after, HEADER_SIZE as u64);
}

#[test]
fn disk_cold_clear_remap() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("cold.dtrm");

    let config = DiskColdConfig::new(&path);
    let mut cold = DiskColdTier::with_config(config).unwrap();

    let (compressed, line_count) = create_test_page(10, "Page0");
    cold.push_compressed(&compressed, line_count).unwrap();
    cold.clear().unwrap();
    assert!(cold.mmap.is_none());

    let (compressed2, line_count2) = create_test_page(5, "Page1");
    cold.push_compressed(&compressed2, line_count2).unwrap();

    assert_eq!(
        cold.get_line(0)
            .expect("no error")
            .expect("line present")
            .to_string(),
        "Page1-Line0"
    );
    let file_len = cold.file.as_ref().unwrap().metadata().unwrap().len();
    let mmap_len = cold
        .mmap
        .as_ref()
        .expect("mmap must be re-established after clear+push")
        .len() as u64;
    assert_eq!(mmap_len, file_len);
}

#[test]
fn disk_cold_clear_persists_empty_header() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("cold.dtrm");

    {
        let config = DiskColdConfig::new(&path);
        let mut cold = DiskColdTier::with_config(config).unwrap();

        let (compressed, line_count) = create_test_page(10, "Page0");
        cold.push_compressed(&compressed, line_count).unwrap();
        cold.clear().unwrap();
        let (page_count, line_count) = read_header_counts(&path);
        assert_eq!(page_count, 0);
        assert_eq!(line_count, 0);
        let file_len = std::fs::metadata(&path).unwrap().len();
        assert_eq!(file_len, HEADER_SIZE as u64);
    }

    let config = DiskColdConfig::new(&path);
    let cold = DiskColdTier::with_config(config).unwrap();

    assert_eq!(cold.line_count(), 0);
    assert_eq!(cold.page_count(), 0);
    assert!(cold.is_empty());
}

#[test]
fn disk_cold_drop_releases_mmap() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("cold.dtrm");

    {
        let config = DiskColdConfig::new(&path);
        let mut cold = DiskColdTier::with_config(config).unwrap();

        let (compressed, line_count) = create_test_page(10, "Page0");
        cold.push_compressed(&compressed, line_count).unwrap();
        // Verify mmap content is readable before drop
        assert_eq!(
            cold.get_line(0)
                .expect("no error")
                .expect("line present")
                .to_string(),
            "Page0-Line0"
        );
        let file_len = cold
            .file
            .as_ref()
            .expect("file before drop")
            .metadata()
            .unwrap()
            .len();
        let mmap_len = cold
            .mmap
            .as_ref()
            .expect("mmap must exist before drop")
            .len() as u64;
        assert_eq!(mmap_len, file_len);
    }

    std::fs::remove_file(&path).unwrap();
}

#[test]
fn disk_cold_mmap_len_tracks_file_len() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("cold.dtrm");

    let config = DiskColdConfig::new(&path);
    let mut cold = DiskColdTier::with_config(config).unwrap();

    let (compressed, line_count) = create_test_page(10, "Page0");
    cold.push_compressed(&compressed, line_count).unwrap();

    let file_len = cold
        .file
        .as_ref()
        .expect("file after push")
        .metadata()
        .unwrap()
        .len();
    let mmap_len = cold.mmap.as_ref().expect("mmap after first push").len() as u64;
    assert_eq!(mmap_len, file_len);
    // Verify mmap is usable, not just sized correctly
    assert_eq!(
        cold.get_line(0)
            .expect("no error")
            .expect("line present")
            .to_string(),
        "Page0-Line0"
    );

    let (compressed2, line_count2) = create_test_page(5, "Page1");
    cold.push_compressed(&compressed2, line_count2).unwrap();

    let file_len2 = cold
        .file
        .as_ref()
        .expect("file after second push")
        .metadata()
        .unwrap()
        .len();
    let mmap_len2 = cold.mmap.as_ref().expect("mmap after second push").len() as u64;
    assert_eq!(mmap_len2, file_len2);
    assert!(file_len2 > file_len, "file should grow after second push");
    // Verify content from both pages is readable through mmap
    assert_eq!(
        cold.get_line(0)
            .expect("no error")
            .expect("line present")
            .to_string(),
        "Page0-Line0"
    );
    assert_eq!(
        cold.get_line(10)
            .expect("no error")
            .expect("line present")
            .to_string(),
        "Page1-Line0"
    );
}

#[test]
fn disk_cold_reload_and_read() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("cold.dtrm");

    // Create and populate
    {
        let config = DiskColdConfig::new(&path);
        let mut cold = DiskColdTier::with_config(config).unwrap();

        for page_num in 0..3 {
            let (compressed, line_count) = create_test_page(5, &format!("Page{page_num}"));
            cold.push_compressed(&compressed, line_count).unwrap();
        }

        cold.sync().unwrap();
    }

    // Reload and read lines
    {
        let config = DiskColdConfig::new(&path);
        let cold = DiskColdTier::with_config(config).unwrap();

        assert_eq!(cold.line_count(), 15);
        assert_eq!(
            cold.get_line(0)
                .expect("no error")
                .expect("line present")
                .to_string(),
            "Page0-Line0"
        );
        assert_eq!(
            cold.get_line(7)
                .expect("no error")
                .expect("line present")
                .to_string(),
            "Page1-Line2"
        );
        assert_eq!(
            cold.get_line(14)
                .expect("no error")
                .expect("line present")
                .to_string(),
            "Page2-Line4"
        );
    }
}

#[test]
fn disk_cold_cache_size_zero_no_infinite_loop() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("cold.dtrm");

    // with_cache_size(0) should be clamped to 1
    let config = DiskColdConfig::new(&path).with_cache_size(0);
    let mut cold = DiskColdTier::with_config(config).unwrap();

    let (compressed, line_count) = create_test_page(5, "Page0");
    cold.push_compressed(&compressed, line_count).unwrap();

    // Reading triggers cache_page — must not hang
    let line = cold.get_line(0).expect("no error").expect("line present");
    assert_eq!(line.to_string(), "Page0-Line0");

    // Cache holds exactly 1 entry (clamped from 0)
    assert_eq!(cold.cache.borrow().len(), 1);
}

// =========================================================================
// DiskColdTier::truncate_front_lines tests (#5911)
// =========================================================================

/// truncate_front_lines within a single page uses front_offset.
#[test]
fn disk_cold_truncate_front_lines_partial_page() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("trunc-partial.dtrm");
    let config = DiskColdConfig::new(&path);
    let mut cold = DiskColdTier::with_config(config).unwrap();

    let (compressed, count) = create_test_page(5, "P0");
    cold.push_compressed(&compressed, count).unwrap();
    assert_eq!(cold.line_count(), 5);

    // Remove 2 oldest lines.
    cold.truncate_front_lines(2);
    assert_eq!(cold.line_count(), 3);
    assert_eq!(cold.page_count(), 1, "page not yet fully consumed");

    // First available line should be P0-Line2.
    let line = cold.get_line(0).unwrap().unwrap();
    assert_eq!(line.to_string(), "P0-Line2");
    let line = cold.get_line(2).unwrap().unwrap();
    assert_eq!(line.to_string(), "P0-Line4");
}

/// truncate_front_lines crossing a page boundary drops the consumed page.
#[test]
fn disk_cold_truncate_front_lines_crosses_page() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("trunc-cross.dtrm");
    let config = DiskColdConfig::new(&path);
    let mut cold = DiskColdTier::with_config(config).unwrap();

    let (c1, n1) = create_test_page(5, "P0");
    let (c2, n2) = create_test_page(5, "P1");
    cold.push_compressed(&c1, n1).unwrap();
    cold.push_compressed(&c2, n2).unwrap();
    assert_eq!(cold.line_count(), 10);
    assert_eq!(cold.page_count(), 2);

    // Remove 7 lines: consumes all of page 0 (5 lines) + 2 from page 1.
    cold.truncate_front_lines(7);
    assert_eq!(cold.line_count(), 3);
    assert_eq!(cold.page_count(), 1, "consumed page should be dropped");

    // First available line should be P1-Line2.
    let line = cold.get_line(0).unwrap().unwrap();
    assert_eq!(line.to_string(), "P1-Line2");
    let last = cold.get_line(2).unwrap().unwrap();
    assert_eq!(last.to_string(), "P1-Line4");
}

/// truncate_front_lines on exact page boundary drops the page cleanly.
#[test]
fn disk_cold_truncate_front_lines_exact_boundary() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("trunc-exact.dtrm");
    let config = DiskColdConfig::new(&path);
    let mut cold = DiskColdTier::with_config(config).unwrap();

    let (c1, n1) = create_test_page(5, "P0");
    let (c2, n2) = create_test_page(5, "P1");
    cold.push_compressed(&c1, n1).unwrap();
    cold.push_compressed(&c2, n2).unwrap();

    // Remove exactly 5 (one full page).
    cold.truncate_front_lines(5);
    assert_eq!(cold.line_count(), 5);
    assert_eq!(cold.page_count(), 1, "first page should be dropped");

    let line = cold.get_line(0).unwrap().unwrap();
    assert_eq!(line.to_string(), "P1-Line0");
}

/// truncate_front_lines removing ALL lines leaves DiskColdTier empty.
#[test]
fn disk_cold_truncate_front_lines_all() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("trunc-all.dtrm");
    let config = DiskColdConfig::new(&path);
    let mut cold = DiskColdTier::with_config(config).unwrap();

    let (c1, n1) = create_test_page(5, "P0");
    let (c2, n2) = create_test_page(5, "P1");
    cold.push_compressed(&c1, n1).unwrap();
    cold.push_compressed(&c2, n2).unwrap();
    assert_eq!(cold.line_count(), 10);

    cold.truncate_front_lines(10);
    assert_eq!(cold.line_count(), 0);
    assert_eq!(cold.page_count(), 0, "all pages should be dropped");
    assert!(
        cold.get_line(0).unwrap().is_none(),
        "empty tier returns None"
    );
}

/// Push after truncate_front_lines keeps cumulative_lines consistent.
#[test]
fn disk_cold_truncate_then_push() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("trunc-push.dtrm");
    let config = DiskColdConfig::new(&path);
    let mut cold = DiskColdTier::with_config(config).unwrap();

    let (c1, n1) = create_test_page(5, "P0");
    let (c2, n2) = create_test_page(5, "P1");
    cold.push_compressed(&c1, n1).unwrap();
    cold.push_compressed(&c2, n2).unwrap();

    // Truncate first page + 2 lines of second.
    cold.truncate_front_lines(7);
    assert_eq!(cold.line_count(), 3);

    // Push a new page.
    let (c3, n3) = create_test_page(4, "P2");
    cold.push_compressed(&c3, n3).unwrap();
    assert_eq!(cold.line_count(), 7, "3 surviving + 4 new = 7");
    assert_eq!(cold.page_count(), 2, "1 surviving + 1 new = 2");

    // Verify data: first 3 lines from P1, then 4 from P2.
    let line0 = cold.get_line(0).unwrap().unwrap();
    assert_eq!(line0.to_string(), "P1-Line2");
    let line3 = cold.get_line(3).unwrap().unwrap();
    assert_eq!(line3.to_string(), "P2-Line0");
    let line6 = cold.get_line(6).unwrap().unwrap();
    assert_eq!(line6.to_string(), "P2-Line3");
}

/// Repeated small truncations across multiple page boundaries.
#[test]
fn disk_cold_truncate_front_lines_incremental() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("trunc-incr.dtrm");
    let config = DiskColdConfig::new(&path);
    let mut cold = DiskColdTier::with_config(config).unwrap();

    // 4 pages of 3 lines each = 12 lines total.
    for i in 0..4 {
        let (c, n) = create_test_page(3, &format!("P{i}"));
        cold.push_compressed(&c, n).unwrap();
    }
    assert_eq!(cold.line_count(), 12);
    assert_eq!(cold.page_count(), 4);

    // Remove 2 (partial first page).
    cold.truncate_front_lines(2);
    assert_eq!(cold.line_count(), 10);
    assert_eq!(cold.page_count(), 4);
    let line = cold.get_line(0).unwrap().unwrap();
    assert_eq!(line.to_string(), "P0-Line2");

    // Remove 1 more (completes first page).
    cold.truncate_front_lines(1);
    assert_eq!(cold.line_count(), 9);
    assert_eq!(cold.page_count(), 3, "first page now consumed");
    let line = cold.get_line(0).unwrap().unwrap();
    assert_eq!(line.to_string(), "P1-Line0");

    // Remove 4 (crosses page 1 into page 2).
    cold.truncate_front_lines(4);
    assert_eq!(cold.line_count(), 5);
    assert_eq!(cold.page_count(), 2);
    let line = cold.get_line(0).unwrap().unwrap();
    assert_eq!(line.to_string(), "P2-Line1");
}

// =========================================================================
// Crash recovery tests (#5917)
// =========================================================================

/// Simulate a crash mid-write: file has a complete page followed by a partial
/// page (header written, compressed data truncated). On reload, the partial
/// page should be discarded and the complete page should be accessible.
#[test]
fn disk_cold_crash_recovery_partial_page_discarded() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("crash-partial.dtrm");

    // Write one complete page normally.
    {
        let config = DiskColdConfig::new(&path);
        let mut cold = DiskColdTier::with_config(config).unwrap();
        let (compressed, count) = create_test_page(5, "Good");
        cold.push_compressed(&compressed, count).unwrap();
        cold.sync().unwrap();
    }

    // Simulate a crash: append a page header claiming 1000 bytes of compressed
    // data, but only write 10 bytes. This mimics a process killed mid-write.
    {
        use std::io::{Seek, Write};
        let mut file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        file.seek(std::io::SeekFrom::End(0)).unwrap();
        let fake_compressed_size: u32 = 1000;
        let fake_line_count: u32 = 10;
        file.write_all(&fake_compressed_size.to_le_bytes()).unwrap();
        file.write_all(&fake_line_count.to_le_bytes()).unwrap();
        // Only write 10 bytes of "compressed" data instead of 1000.
        file.write_all(&[0xAB; 10]).unwrap();
        file.flush().unwrap();
    }

    // Reload: partial page should be discarded, complete page intact.
    {
        let config = DiskColdConfig::new(&path);
        let cold = DiskColdTier::with_config(config).unwrap();
        assert_eq!(cold.page_count(), 1, "partial page must be discarded");
        assert_eq!(cold.line_count(), 5, "only complete page's lines survive");
        let line = cold.get_line(0).unwrap().unwrap();
        assert_eq!(line.to_string(), "Good-Line0");
        let last = cold.get_line(4).unwrap().unwrap();
        assert_eq!(last.to_string(), "Good-Line4");
    }
}

/// A file with only a partial page header (< PAGE_HEADER_SIZE bytes after the
/// last complete page) should be handled gracefully — zero pages loaded.
#[test]
fn disk_cold_crash_recovery_truncated_header() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("crash-trunc-hdr.dtrm");

    // Create an empty file with just the file header.
    {
        let config = DiskColdConfig::new(&path);
        let _cold = DiskColdTier::with_config(config).unwrap();
    }

    // Append a few bytes (less than PAGE_HEADER_SIZE) to simulate a crash
    // during the very start of a page write.
    {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        file.write_all(&[0x01, 0x02, 0x03]).unwrap();
        file.flush().unwrap();
    }

    // Reload: no pages should be loaded.
    {
        let config = DiskColdConfig::new(&path);
        let cold = DiskColdTier::with_config(config).unwrap();
        assert_eq!(cold.page_count(), 0, "truncated header must be ignored");
        assert_eq!(cold.line_count(), 0);
    }
}

/// Verify that push_compressed uses write-ahead ordering: page data is synced
/// before header counters. After a normal write, header counters match the
/// scanned page data.
#[test]
fn disk_cold_push_compressed_header_consistent() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("consistent.dtrm");

    {
        let config = DiskColdConfig::new(&path);
        let mut cold = DiskColdTier::with_config(config).unwrap();
        let (c1, n1) = create_test_page(5, "P0");
        let (c2, n2) = create_test_page(3, "P1");
        cold.push_compressed(&c1, n1).unwrap();
        cold.push_compressed(&c2, n2).unwrap();
        cold.sync().unwrap();
    }

    // Verify header counts match actual page data.
    let (header_pages, header_lines) = read_header_counts(&path);
    assert_eq!(header_pages, 2, "header page count must match");
    assert_eq!(header_lines, 8, "header line count must match (5+3)");

    // Reload and verify data integrity.
    let config = DiskColdConfig::new(&path);
    let cold = DiskColdTier::with_config(config).unwrap();
    assert_eq!(cold.page_count(), 2);
    assert_eq!(cold.line_count(), 8);
    assert_eq!(cold.get_line(0).unwrap().unwrap().to_string(), "P0-Line0");
    assert_eq!(cold.get_line(5).unwrap().unwrap().to_string(), "P1-Line0");
}

// Compaction tests extracted to disk_compaction_tests.rs
#[path = "disk_compaction_tests.rs"]
mod compaction;

/// Regression: #5923 — `create()` must sync header to disk immediately.
///
/// Before the fix, `create()` called `file.flush()` which is a no-op on
/// `std::fs::File`. After the fix, `sync_data()` ensures the header is
/// durable on disk without waiting for `Drop`.
#[test]
fn disk_cold_create_header_durable_on_disk() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("cold.dtrm");

    let config = DiskColdConfig::new(&path);
    let _cold = DiskColdTier::with_config(config).unwrap();

    // Read the on-disk header directly (bypassing the tier API).
    // sync_data() in create() ensures this is visible immediately.
    let (page_count, line_count) = read_header_counts(&path);
    assert_eq!(page_count, 0, "freshly created header should have 0 pages");
    assert_eq!(line_count, 0, "freshly created header should have 0 lines");
}

/// Regression: #5923 — `clear()` must sync zeroed header to disk immediately.
///
/// Before the fix, `clear()` called `file.flush()` which is a no-op on
/// `std::fs::File`. After the fix, `sync_data()` ensures the zeroed header
/// is durable without waiting for `Drop::sync_all()`.
#[test]
fn disk_cold_clear_header_durable_on_disk() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("cold.dtrm");

    let config = DiskColdConfig::new(&path);
    let mut cold = DiskColdTier::with_config(config).unwrap();

    // Push data so header is non-zero
    let (compressed, count) = create_test_page(10, "Page0");
    cold.push_compressed(&compressed, count).unwrap();

    let (pages_before, lines_before) = read_header_counts(&path);
    assert_eq!(pages_before, 1, "should have 1 page before clear");
    assert_eq!(lines_before, 10, "should have 10 lines before clear");

    // Clear resets header to zero and syncs to disk
    cold.clear().unwrap();

    // Verify on-disk header is zeroed immediately (not deferred to Drop)
    let (pages_after, lines_after) = read_header_counts(&path);
    assert_eq!(pages_after, 0, "clear() must zero page count on disk");
    assert_eq!(lines_after, 0, "clear() must zero line count on disk");
}

/// Verify push_compressed maintains transactional state consistency (#7575).
///
/// After each push, line_count, page_count, and cumulative_lines must all
/// agree. This test exercises the transactional commit pattern that was
/// introduced to prevent inconsistent state on I/O failure.
#[test]
fn disk_cold_push_compressed_state_consistency() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("consistency.dtrm");

    let config = DiskColdConfig::new(&path);
    let mut cold = DiskColdTier::with_config(config).unwrap();

    let page_sizes = [3, 7, 1, 10, 5];
    let mut expected_total = 0;

    for (i, &size) in page_sizes.iter().enumerate() {
        let (compressed, count) = create_test_page(size, &format!("P{i}"));
        cold.push_compressed(&compressed, count).unwrap();
        expected_total += size;

        // Invariant: line_count must equal the sum of all pushed line counts.
        assert_eq!(
            cold.line_count(),
            expected_total,
            "line_count inconsistency after push {i}"
        );
        // Invariant: page_count must match the number of pushes.
        assert_eq!(
            cold.page_count(),
            i + 1,
            "page_count inconsistency after push {i}"
        );
        // Invariant: data from all pages must be readable.
        let first_line = cold.get_line(0).unwrap().unwrap();
        assert_eq!(
            first_line.to_string(),
            "P0-Line0",
            "first line must always be accessible"
        );
        let last_line = cold.get_line(expected_total - 1).unwrap().unwrap();
        assert_eq!(
            last_line.to_string(),
            format!("P{i}-Line{}", size - 1),
            "last line of most recent page must be accessible"
        );
    }
}

/// push_compressed with empty data or zero lines is a no-op (#7575).
#[test]
fn disk_cold_push_compressed_empty_is_noop() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("noop.dtrm");

    let config = DiskColdConfig::new(&path);
    let mut cold = DiskColdTier::with_config(config).unwrap();

    // Push real data first.
    let (compressed, count) = create_test_page(5, "P0");
    cold.push_compressed(&compressed, count).unwrap();
    assert_eq!(cold.line_count(), 5);
    assert_eq!(cold.page_count(), 1);

    // Push empty compressed data — should be no-op.
    cold.push_compressed(&[], 10).unwrap();
    assert_eq!(
        cold.line_count(),
        5,
        "empty data push must not change state"
    );
    assert_eq!(cold.page_count(), 1);

    // Push with zero line count — should be no-op.
    cold.push_compressed(&compressed, 0).unwrap();
    assert_eq!(
        cold.line_count(),
        5,
        "zero line_count push must not change state"
    );
    assert_eq!(cold.page_count(), 1);
}

// Performance regression tests — drain + single-line extraction (P10 6001)

/// Exercises drain(..k) path that replaced O(k*n) remove(0) loops.
#[test]
fn disk_cold_truncate_multi_page_drain() {
    let dir = tempdir().unwrap();
    let config = DiskColdConfig::new(dir.path().join("drain.dtrm"));
    let mut cold = DiskColdTier::with_config(config).unwrap();

    for i in 0..8 {
        let (c, n) = create_test_page(5, &format!("P{i}"));
        cold.push_compressed(&c, n).unwrap();
    }
    assert_eq!((cold.line_count(), cold.page_count()), (40, 8));

    // Remove 27 lines — consumes 5 full pages, offset 2 into P5.
    cold.truncate_front_lines(27);
    assert_eq!((cold.line_count(), cold.page_count()), (13, 3));
    assert_eq!(cold.get_line(0).unwrap().unwrap().to_string(), "P5-Line2");
    assert_eq!(cold.get_line(12).unwrap().unwrap().to_string(), "P7-Line4");
    assert!(cold.get_line(13).unwrap().is_none());
}

/// Exercises load_line() single-line extraction path (cache miss then hits).
#[test]
fn disk_cold_get_line_cache_hit_single_extraction() {
    let dir = tempdir().unwrap();
    let config = DiskColdConfig::new(dir.path().join("cache-hit.dtrm"));
    let mut cold = DiskColdTier::with_config(config).unwrap();

    let (c, n) = create_test_page(10, "Pg");
    cold.push_compressed(&c, n).unwrap();

    // Cache miss then cache hits — each returns correct single line.
    assert_eq!(cold.get_line(0).unwrap().unwrap().to_string(), "Pg-Line0");
    assert_eq!(cold.get_line(5).unwrap().unwrap().to_string(), "Pg-Line5");
    assert_eq!(cold.get_line(9).unwrap().unwrap().to_string(), "Pg-Line9");
    assert!(cold.get_line(10).unwrap().is_none());
}

/// Regression: a malformed `PageIndexEntry` whose `offset + compressed_size`
/// exceeds the mapped/file length must return an `Err`, never read out of
/// bounds (which would be a SIGBUS / OOB read against the raw mmap pointer).
#[test]
fn disk_cold_decompress_oob_offset_returns_err() {
    let dir = tempdir().unwrap();
    let config = DiskColdConfig::new(dir.path().join("oob-offset.dtrm"));
    let mut cold = DiskColdTier::with_config(config).unwrap();

    let (compressed, line_count) = create_test_page(10, "Pg");
    cold.push_compressed(&compressed, line_count).unwrap();

    // Valid read works before corruption.
    assert!(cold.decompress_page_for_test(0).is_ok());

    // Push the page range far past the end of the mapped file.
    let map_len = cold.mmap.as_ref().expect("mmap present").len() as u64;
    cold.corrupt_last_entry_range(map_len + 1, 4096);

    let err = cold
        .decompress_page_for_test(0)
        .expect_err("out-of-bounds page range must error, not read OOB");
    assert!(
        matches!(err, ScrollbackError::Io(_)),
        "expected I/O error for OOB range, got: {err:?}"
    );
}

/// Regression: an entry whose offset stays in-bounds but whose
/// `compressed_size` runs past the mapped length must error rather than
/// slicing past the mapping.
#[test]
fn disk_cold_decompress_oob_length_returns_err() {
    let dir = tempdir().unwrap();
    let config = DiskColdConfig::new(dir.path().join("oob-length.dtrm"));
    let mut cold = DiskColdTier::with_config(config).unwrap();

    let (compressed, line_count) = create_test_page(10, "Pg");
    cold.push_compressed(&compressed, line_count).unwrap();

    // Offset 32 (just after the file header) is valid, but a huge length
    // overruns the mapping.
    cold.corrupt_last_entry_range(HEADER_SIZE as u64, u32::MAX);

    let err = cold
        .decompress_page_for_test(0)
        .expect_err("oversized compressed_size must error, not read OOB");
    assert!(
        matches!(err, ScrollbackError::Io(_)),
        "expected I/O error for oversized length, got: {err:?}"
    );
}

/// Regression: simulate another process truncating the backing file after the
/// mapping was created. The live-file-length re-check must catch the shrink
/// and return an `Err` instead of dereferencing past EOF (SIGBUS).
#[test]
fn disk_cold_decompress_external_truncation_returns_err() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("truncated.dtrm");
    let config = DiskColdConfig::new(&path);
    let mut cold = DiskColdTier::with_config(config).unwrap();

    let (compressed, line_count) = create_test_page(10, "Pg");
    cold.push_compressed(&compressed, line_count).unwrap();
    assert!(cold.decompress_page_for_test(0).is_ok());

    // Simulate an out-of-band truncation by another process: shrink the file
    // to just the header while the mapping still records the original length.
    let truncator = std::fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap();
    truncator.set_len(HEADER_SIZE as u64).unwrap();
    drop(truncator);

    let err = cold
        .decompress_page_for_test(0)
        .expect_err("read against a truncated file must error, not SIGBUS");
    assert!(
        matches!(err, ScrollbackError::Io(_)),
        "expected I/O error for truncated file, got: {err:?}"
    );
}
