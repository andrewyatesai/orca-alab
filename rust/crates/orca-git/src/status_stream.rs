//! The single `git status --porcelain=v2 --branch` record scanner, ported from
//! `src/main/git/status-porcelain-parser.ts`. It is fed RAW bytes (git runs with
//! `core.quotePath=false`, so path bytes can be invalid UTF-8) and splits records
//! on `0x0A`, carrying a partial trailing line across `update` calls. The caller
//! can cap the changed-entry count to keep memory bounded on a huge worktree.
//!
//! This is the one scanner: `status.rs::parse_porcelain_v2_status` (full status)
//! and `parse_status_porcelain` (the relay one-shot) both build on it, so the cap
//! is applied DURING the scan — fixing the relay's full-materialize-then-truncate.

use crate::status::{GitStagingArea, GitStatusEntry};
use crate::status_parse::{
    parse_branch_ahead_behind, parse_status_char, parse_submodule_status, GitFileStatus,
};
use orca_core::git_cquoted_path::decode_git_cquoted_path;

/// Branch headers parsed from the `# branch.*` records.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BranchMetadata {
    pub head: Option<String>,
    pub branch: Option<String>,
    pub upstream_name: Option<String>,
    pub ahead_behind: Option<(i64, i64)>,
}

/// The parser-level result: changed entries (sliced to the cap when stopped),
/// raw unmerged lines for the caller to resolve, ignored paths, branch metadata,
/// whether the cap was hit, and the total entries observed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusParseResult {
    pub entries: Vec<GitStatusEntry>,
    pub ignored_paths: Vec<String>,
    pub unmerged_lines: Vec<String>,
    pub branch: BranchMetadata,
    pub did_hit_limit: bool,
    pub status_length: usize,
}

/// Incremental porcelain-v2 scanner. Feed decoded-but-raw bytes via [`update`];
/// it parses complete `0x0A`-delimited records and carries the partial tail.
///
/// [`update`]: StatusPorcelainParser::update
#[derive(Debug, Default)]
pub struct StatusPorcelainParser {
    carry: Vec<u8>,
    count: usize,
    stopped: bool,
    entries: Vec<GitStatusEntry>,
    ignored_paths: Vec<String>,
    unmerged_lines: Vec<String>,
    branch: BranchMetadata,
}

impl StatusPorcelainParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Total changed-file entries observed, including any pushed past the cap.
    pub fn status_length(&self) -> usize {
        self.count
    }

    /// Feed one raw chunk. Returns `true` once the changed-entry count exceeds
    /// `limit` (`limit == 0` disables the cap), signaling the caller to stop git.
    /// Complete records are parsed; an incomplete trailing record is carried.
    pub fn update(&mut self, chunk: &[u8], limit: usize) -> bool {
        // Mirror the TS `text = carry + chunk`: own the buffer so `parse_line`'s
        // `&mut self` does not alias the bytes we are scanning.
        let mut text = std::mem::take(&mut self.carry);
        text.extend_from_slice(chunk);

        let mut start = 0;
        // Checked slicing only — no get_unchecked (the single-borrow lexical lemma
        // that would license it is a documented precondition, not a theorem).
        while let Some(rel) = memchr::memchr(0x0A, &text[start..]) {
            let nl = start + rel;
            // Strip one trailing 0x0D so Windows CRLF output parses cleanly.
            let end = if nl > start && text[nl - 1] == 0x0D { nl - 1 } else { nl };
            self.parse_line(&text[start..end]);
            start = nl + 1;
            if limit != 0 && self.count > limit {
                self.stopped = true;
                // carry was taken (now empty) — leave it cleared.
                return true;
            }
        }
        self.carry = text[start..].to_vec();
        false
    }

    /// Flush a final record with no trailing newline (e.g. when git exits). Does
    /// nothing once the cap stopped the scan.
    pub fn finish(&mut self) {
        if self.stopped {
            return;
        }
        if !self.carry.is_empty() {
            let line = std::mem::take(&mut self.carry);
            self.parse_line(&line);
        }
    }

    /// Consume the parser into a result, slicing `entries` to `min(count, limit)`
    /// when the cap stopped the scan.
    pub fn into_result(self, limit: usize) -> StatusParseResult {
        let entries = if self.stopped {
            let keep = self.count.min(limit);
            self.entries.into_iter().take(keep).collect()
        } else {
            self.entries
        };
        StatusParseResult {
            entries,
            ignored_paths: self.ignored_paths,
            unmerged_lines: self.unmerged_lines,
            branch: self.branch,
            did_hit_limit: self.stopped,
            status_length: self.count,
        }
    }

    fn parse_line(&mut self, line: &[u8]) {
        if line.is_empty() {
            return;
        }
        // Lossy per-line decode: invalid path bytes become U+FFFD (the TS path
        // uses a lossy StringDecoder), never a panic (crate is no-panic).
        if let Some(rest) = line.strip_prefix(b"# branch.oid ") {
            self.branch.head = Some(String::from_utf8_lossy(rest).trim().to_string());
            return;
        }
        if let Some(rest) = line.strip_prefix(b"# branch.head ") {
            let head = String::from_utf8_lossy(rest);
            let head = head.trim();
            // undefined (None), not "" — the renderer turns "head without branch"
            // into an explicit detached-HEAD clear.
            self.branch.branch = (!head.is_empty() && head != "(detached)")
                .then(|| format!("refs/heads/{head}"));
            return;
        }
        if let Some(rest) = line.strip_prefix(b"# branch.upstream ") {
            let name = String::from_utf8_lossy(rest);
            let name = name.trim();
            self.branch.upstream_name = (!name.is_empty()).then(|| name.to_string());
            return;
        }
        if line.starts_with(b"# branch.ab ") {
            self.branch.ahead_behind = parse_branch_ahead_behind(&String::from_utf8_lossy(line));
            return;
        }
        if line.starts_with(b"1 ") || line.starts_with(b"2 ") {
            self.parse_changed_entry(line);
            return;
        }
        if let Some(rest) = line.strip_prefix(b"? ") {
            let path = decode_git_cquoted_path(&String::from_utf8_lossy(rest));
            self.push(GitStatusEntry {
                path,
                status: GitFileStatus::Untracked,
                area: GitStagingArea::Untracked,
                old_path: None,
                conflict_kind: None,
                conflict_status: None,
                submodule: None,
                added: None,
                removed: None,
            });
            return;
        }
        if let Some(rest) = line.strip_prefix(b"! ") {
            self.ignored_paths
                .push(decode_git_cquoted_path(&String::from_utf8_lossy(rest)));
            return;
        }
        if line.starts_with(b"u ") {
            // Unmerged records need async per-file lookups; collect the raw line.
            self.unmerged_lines
                .push(String::from_utf8_lossy(line).into_owned());
        }
    }

    fn parse_changed_entry(&mut self, line: &[u8]) {
        let line = String::from_utf8_lossy(line);
        let parts: Vec<&str> = line.split(' ').collect();
        let xy = parts.get(1).copied().unwrap_or("");
        let submodule = parse_submodule_status(parts.get(2).copied());
        let mut xy_chars = xy.chars();
        let index_status = xy_chars.next().unwrap_or('.');
        let worktree_status = xy_chars.next().unwrap_or('.');

        let (path, old_path) = if line.starts_with("2 ") {
            // type-2 (rename/copy): new path after 9 space fields, old after the tab.
            let tab_parts: Vec<&str> = line.split('\t').collect();
            let before_tab: Vec<&str> = tab_parts[0].split(' ').collect();
            let new_path =
                decode_git_cquoted_path(&before_tab[9.min(before_tab.len())..].join(" "));
            let old = decode_git_cquoted_path(&tab_parts[1..].join("\t"));
            (new_path, Some(old))
        } else {
            (decode_git_cquoted_path(&parts[8.min(parts.len())..].join(" ")), None)
        };

        if index_status != '.' {
            self.push(GitStatusEntry {
                path: path.clone(),
                status: parse_status_char(index_status),
                area: GitStagingArea::Staged,
                old_path: old_path.clone(),
                conflict_kind: None,
                conflict_status: None,
                submodule,
                added: None,
                removed: None,
            });
        }
        if worktree_status != '.' {
            self.push(GitStatusEntry {
                path,
                status: parse_status_char(worktree_status),
                area: GitStagingArea::Unstaged,
                old_path,
                conflict_kind: None,
                conflict_status: None,
                submodule,
                added: None,
                removed: None,
            });
        }
    }

    fn push(&mut self, entry: GitStatusEntry) {
        self.count += 1;
        self.entries.push(entry);
    }
}

/// One-shot scan — the relay entry point. The cap is applied DURING the scan, so
/// `entries` is bounded by `limit` instead of the old materialize-then-truncate.
pub fn parse_status_porcelain(stdout: &[u8], limit: usize) -> StatusParseResult {
    let mut parser = StatusPorcelainParser::new();
    parser.update(stdout, limit);
    parser.finish();
    parser.into_result(limit)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn untracked(path: &str) -> GitStatusEntry {
        GitStatusEntry {
            path: path.to_string(),
            status: GitFileStatus::Untracked,
            area: GitStagingArea::Untracked,
            old_path: None,
            conflict_kind: None,
            conflict_status: None,
            submodule: None,
            added: None,
            removed: None,
        }
    }

    #[test]
    fn parses_branch_headers_and_changed_untracked_ignored() {
        let out = b"# branch.oid abc123\n\
            # branch.head feature/x\n\
            # branch.upstream origin/feature/x\n\
            # branch.ab +2 -1\n\
            1 M. N... 100644 100644 100644 aaaa aaaa src/staged.ts\n\
            1 .M N... 100644 100644 100644 bbbb bbbb src/unstaged.ts\n\
            ? new.txt\n\
            ! dist/\n";
        let mut p = StatusPorcelainParser::new();
        let stopped = p.update(out, 0);
        p.finish();
        assert!(!stopped);
        let r = p.into_result(0);
        assert_eq!(r.branch.head.as_deref(), Some("abc123"));
        assert_eq!(r.branch.branch.as_deref(), Some("refs/heads/feature/x"));
        assert_eq!(r.branch.upstream_name.as_deref(), Some("origin/feature/x"));
        assert_eq!(r.branch.ahead_behind, Some((2, 1)));
        assert_eq!(r.entries.len(), 3);
        assert_eq!(r.entries[0].path, "src/staged.ts");
        assert_eq!(r.entries[0].area, GitStagingArea::Staged);
        assert_eq!(r.entries[1].path, "src/unstaged.ts");
        assert_eq!(r.entries[1].area, GitStagingArea::Unstaged);
        assert_eq!(r.entries[2], untracked("new.txt"));
        assert_eq!(r.ignored_paths, vec!["dist/".to_string()]);
        assert_eq!(r.status_length, 3);
    }

    #[test]
    fn parses_type2_rename_with_old_path() {
        let r = parse_status_porcelain(
            b"2 R. N... 100644 100644 100644 aaaa bbbb R100 new.ts\told.ts\n",
            0,
        );
        assert_eq!(r.entries.len(), 1);
        assert_eq!(r.entries[0].path, "new.ts");
        assert_eq!(r.entries[0].old_path.as_deref(), Some("old.ts"));
        assert_eq!(r.entries[0].status, GitFileStatus::Renamed);
        assert_eq!(r.entries[0].area, GitStagingArea::Staged);
    }

    #[test]
    fn collects_unmerged_lines_instead_of_parsing_inline() {
        let r = parse_status_porcelain(
            b"u UU N... 100644 100644 100644 100644 aa bb cc both.ts\n",
            0,
        );
        assert!(r.entries.is_empty());
        assert_eq!(r.unmerged_lines.len(), 1);
    }

    #[test]
    fn carries_partial_trailing_line_across_chunks() {
        let mut p = StatusPorcelainParser::new();
        p.update(b"? partial", 0);
        p.update(b"-name.txt\n", 0);
        p.finish();
        let r = p.into_result(0);
        assert_eq!(r.entries, vec![untracked("partial-name.txt")]);
    }

    #[test]
    fn strips_trailing_cr_for_crlf_output() {
        let r = parse_status_porcelain(b"? win.txt\r\n", 0);
        assert_eq!(r.entries, vec![untracked("win.txt")]);
    }

    #[test]
    fn signals_stop_once_count_exceeds_limit() {
        let mut p = StatusPorcelainParser::new();
        let lines = b"? f0.txt\n? f1.txt\n? f2.txt\n? f3.txt\n? f4.txt\n";
        let stopped = p.update(lines, 3);
        assert!(stopped);
        // The 4th entry (count 4 > limit 3) tripped the stop; the buffer holds it.
        assert_eq!(p.status_length(), 4);
        let r = p.into_result(3);
        // into_result slices the over-pushed buffer back to the cap.
        assert_eq!(r.entries.len(), 3);
        assert!(r.did_hit_limit);
        assert_eq!(r.status_length, 4);
    }

    #[test]
    fn limit_zero_disables_the_cap() {
        let mut s = String::new();
        for i in 0..50 {
            s.push_str(&format!("? f{i}.txt\n"));
        }
        let r = parse_status_porcelain(s.as_bytes(), 0);
        assert_eq!(r.entries.len(), 50);
        assert!(!r.did_hit_limit);
        assert_eq!(r.status_length, 50);
    }

    #[test]
    fn parses_submodule_dirtiness_flags() {
        let r = parse_status_porcelain(
            b"1 AM S..U 000000 160000 160000 0000 7844 nested-repo\n",
            0,
        );
        assert_eq!(r.entries.len(), 2);
        let sub = r.entries[0].submodule.unwrap();
        assert!(!sub.commit_changed);
        assert!(!sub.tracked_changes);
        assert!(sub.untracked_changes);
        assert_eq!(r.entries[0].status, GitFileStatus::Added);
        assert_eq!(r.entries[1].status, GitFileStatus::Modified);
        assert_eq!(r.entries[1].submodule, r.entries[0].submodule);
    }

    #[test]
    fn invalid_utf8_path_byte_round_trips_lossily_without_panic() {
        // git with core.quotePath=false emits raw filename bytes; a lone 0xE9 is
        // invalid UTF-8 and must become U+FFFD (lossy), never panic.
        let r = parse_status_porcelain(b"? \xE9.txt\n", 0);
        assert_eq!(r.entries.len(), 1);
        assert_eq!(r.entries[0].path, "\u{FFFD}.txt");
    }

    #[test]
    fn detached_head_clears_branch() {
        let r = parse_status_porcelain(b"# branch.head (detached)\n", 0);
        assert_eq!(r.branch.branch, None);
    }

    #[test]
    fn cap_during_scan_matches_streaming_local_path() {
        // Dual-run regression for the relay materialize bug: the one-shot relay
        // entry point must produce the SAME bounded result as the chunked local
        // streaming path — both cap during the scan, neither materializes all rows.
        let mut s = String::new();
        for i in 0..1000 {
            s.push_str(&format!("? f{i}.txt\n"));
        }
        let limit = 25;

        let one_shot = parse_status_porcelain(s.as_bytes(), limit);

        let mut streamed = StatusPorcelainParser::new();
        let mut stopped = false;
        for chunk in s.as_bytes().chunks(7) {
            if streamed.update(chunk, limit) {
                stopped = true;
                break;
            }
        }
        if !stopped {
            streamed.finish();
        }
        let streamed = streamed.into_result(limit);

        assert_eq!(one_shot.entries, streamed.entries);
        assert_eq!(one_shot.did_hit_limit, streamed.did_hit_limit);
        assert_eq!(one_shot.status_length, streamed.status_length);
        assert!(one_shot.did_hit_limit);
        assert_eq!(one_shot.entries.len(), limit);
        // The buffer never exceeds limit + 2 (cap_invariant proof's bound).
        assert!(one_shot.status_length <= limit + 2);
    }
}
