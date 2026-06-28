//! Line-counting hot paths ported from the renderer/shared layer:
//! - [`is_binary_buffer`] ← `src/shared/binary-buffer.ts`
//! - [`count_additions_in_buffer`] ← the untracked-file counter in
//!   `src/shared/git-uncommitted-line-stats.ts`
//! - [`compute_line_stats`] ← `src/renderer/src/components/editor/diff-line-stats.ts`

use std::collections::HashMap;

/// A NUL byte in the first chunk is git's own heuristic for "this is binary".
const BINARY_SNIFF_BYTES: usize = 8192;

/// `LineStats` for a diff (added/removed line counts).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineStats {
    pub added: u32,
    pub removed: u32,
}

/// True if a `0x00` appears within the first `min(len, 8192)` bytes.
pub fn is_binary_buffer(bytes: &[u8]) -> bool {
    let sniff = bytes.len().min(BINARY_SNIFF_BYTES);
    memchr::memchr(0, &bytes[..sniff]).is_some()
}

/// Count additions for an untracked file's contents: `None` for binary, `Some(0)`
/// for empty, else the newline count — trailing-newline-aware (a final partial
/// line with no newline still counts, matching git's numstat).
pub fn count_additions_in_buffer(bytes: &[u8]) -> Option<u32> {
    if is_binary_buffer(bytes) {
        return None;
    }
    if bytes.is_empty() {
        return Some(0);
    }
    let newlines = memchr::memchr_iter(0x0A, bytes).count() as u32;
    Some(if bytes.last() == Some(&0x0A) { newlines } else { newlines + 1 })
}

/// Number of lines in `content`: `1 + count('\n')`, matching the TS
/// `countLinesWithoutAllocation` (an empty string is one line).
fn line_count(content: &str) -> u32 {
    1 + memchr::memchr_iter(0x0A, content.as_bytes()).count() as u32
}

/// Approximate added/removed counts between `original` and `modified`. `None`
/// for very large inputs (the TS guard against blocking React render).
pub fn compute_line_stats(original: &str, modified: &str, status: &str) -> Option<LineStats> {
    // Byte length proxy for the TS UTF-16 `.length` guard (equal for ASCII).
    if original.len() + modified.len() > 500_000 {
        return None;
    }
    if status == "added" {
        let added = if modified.is_empty() { 0 } else { line_count(modified) };
        return Some(LineStats { added, removed: 0 });
    }
    if status == "deleted" {
        let removed = if original.is_empty() { 0 } else { line_count(original) };
        return Some(LineStats { added: 0, removed });
    }

    // Multiset match: count original lines, then decrement on each modified match.
    let mut orig_counts: HashMap<&str, i32> = HashMap::new();
    let mut original_line_count = 0i64;
    for line in original.split('\n') {
        original_line_count += 1;
        *orig_counts.entry(line).or_insert(0) += 1;
    }

    let mut modified_line_count = 0i64;
    let mut matched = 0i64;
    for line in modified.split('\n') {
        modified_line_count += 1;
        if let Some(count) = orig_counts.get_mut(line) {
            if *count > 0 {
                *count -= 1;
                matched += 1;
            }
        }
    }

    Some(LineStats {
        added: (modified_line_count - matched) as u32,
        removed: (original_line_count - matched) as u32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_nul_in_sniff_window() {
        assert!(!is_binary_buffer(b""));
        assert!(!is_binary_buffer(b"plain text\n"));
        assert!(is_binary_buffer(&[0x00, 0x01, 0x02]));
        // A NUL past the sniff window is not detected.
        let mut big = vec![b'a'; BINARY_SNIFF_BYTES];
        big.push(0x00);
        assert!(!is_binary_buffer(&big));
    }

    #[test]
    fn counts_additions_trailing_newline_aware() {
        assert_eq!(count_additions_in_buffer(b"a\nb\nc\n"), Some(3));
        assert_eq!(count_additions_in_buffer(b"a\nb\nc"), Some(3));
        assert_eq!(count_additions_in_buffer(b""), Some(0));
        assert_eq!(count_additions_in_buffer(&[0x00, 0x01, 0x02]), None);
    }

    #[test]
    fn keeps_added_deleted_modified_behavior() {
        assert_eq!(compute_line_stats("", "a\nb", "added"), Some(LineStats { added: 2, removed: 0 }));
        assert_eq!(compute_line_stats("a\nb\n", "", "deleted"), Some(LineStats { added: 0, removed: 3 }));
        assert_eq!(
            compute_line_stats("a\nb\nc", "a\nc\nd", "modified"),
            Some(LineStats { added: 1, removed: 1 })
        );
    }

    #[test]
    fn counts_newline_heavy_added_and_deleted() {
        let content: String = "\n".repeat(100_000);
        assert_eq!(
            compute_line_stats("", &content, "added"),
            Some(LineStats { added: 100_001, removed: 0 })
        );
        assert_eq!(
            compute_line_stats(&content, "", "deleted"),
            Some(LineStats { added: 0, removed: 100_001 })
        );
    }

    #[test]
    fn compares_modified_files_via_multiset() {
        assert_eq!(
            compute_line_stats("same\nold\nkept", "same\nnew\nkept", "modified"),
            Some(LineStats { added: 1, removed: 1 })
        );
    }

    #[test]
    fn large_modified_guard_returns_none() {
        let original = "x".repeat(250_001);
        let modified = "y".repeat(250_000);
        assert_eq!(compute_line_stats(&original, &modified, "modified"), None);
    }
}
