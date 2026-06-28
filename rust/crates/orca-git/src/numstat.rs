//! `git diff --numstat` parsing, ported from `parseNumstat` in
//! `src/shared/git-uncommitted-line-stats.ts`. Two modes: a `0x00` byte anywhere
//! selects `-z` (NUL-delimited, raw postimage path, no decode); otherwise text
//! mode (tab columns, C-quote-decoded + rename-normalized path key).

use orca_core::git_cquoted_path::decode_git_cquoted_path;
use regex::Regex;
use std::sync::OnceLock;

/// One numstat row: added/removed are `None` for a binary file (git's `-` column).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NumstatEntry {
    pub path: String,
    pub added: Option<u32>,
    pub removed: Option<u32>,
}

/// git reports binary files as `-`; otherwise parse the leading integer (JS
/// `parseInt(value, 10)` semantics — a leading run of digits, else `None`).
fn parse_numstat_count(value: &str) -> Option<u32> {
    if value == "-" {
        return None;
    }
    let digits: String = value.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse::<u32>().ok()
}

fn brace_rename_re() -> &'static Regex {
    // `git diff -M` reports renames as `old => new` or `dir/{old => new}/file`.
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(.*)\{(.+) => (.+)\}(.*)$").expect("static numstat regex"))
}

/// Normalize a text-mode rename path to the post-rename (new) path so it keys to
/// the porcelain status entry, which always reports the new path.
fn normalize_numstat_path(raw: &str) -> String {
    let decoded = decode_git_cquoted_path(raw);
    if let Some(caps) = brace_rename_re().captures(&decoded) {
        return format!("{}{}{}", &caps[1], &caps[3], &caps[4]);
    }
    let marker = " => ";
    match decoded.rfind(marker) {
        Some(idx) => decoded[idx + marker.len()..].to_string(),
        None => decoded,
    }
}

/// JS `Map.set` semantics: update the value in place on a duplicate key (keeping
/// the original insertion position), otherwise append.
fn set_last_wins(out: &mut Vec<NumstatEntry>, path: String, added: Option<u32>, removed: Option<u32>) {
    if let Some(existing) = out.iter_mut().find(|e| e.path == path) {
        existing.added = added;
        existing.removed = removed;
    } else {
        out.push(NumstatEntry { path, added, removed });
    }
}

pub fn parse_numstat(stdout: &[u8]) -> Vec<NumstatEntry> {
    if memchr::memchr(0, stdout).is_some() {
        parse_nul_delimited_numstat(stdout)
    } else {
        parse_text_numstat(stdout)
    }
}

fn parse_text_numstat(stdout: &[u8]) -> Vec<NumstatEntry> {
    let text = String::from_utf8_lossy(stdout);
    let mut out = Vec::new();
    for raw_line in text.split('\n') {
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split('\t').collect();
        let raw_path = if parts.len() > 2 { parts[2..].join("\t") } else { String::new() };
        if raw_path.is_empty() {
            continue;
        }
        let added = parse_numstat_count(parts.first().copied().unwrap_or(""));
        let removed = parse_numstat_count(parts.get(1).copied().unwrap_or(""));
        set_last_wins(&mut out, normalize_numstat_path(&raw_path), added, removed);
    }
    out
}

fn parse_nul_delimited_numstat(stdout: &[u8]) -> Vec<NumstatEntry> {
    let text = String::from_utf8_lossy(stdout);
    let records: Vec<&str> = text.split('\0').collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < records.len() {
        let record = records[i];
        if record.is_empty() {
            i += 1;
            continue;
        }
        let parts: Vec<&str> = record.split('\t').collect();
        let raw_path = if parts.len() > 2 { parts[2..].join("\t") } else { String::new() };
        let mut path = raw_path;
        if path.is_empty() {
            // git -z emits renames as "added\tremoved\t\0old\0new\0": the header's
            // path is empty; the postimage is two records on (raw, no decode).
            i += 2;
            path = records.get(i).copied().unwrap_or("").to_string();
        }
        if path.is_empty() {
            i += 1;
            continue;
        }
        let added = parse_numstat_count(parts.first().copied().unwrap_or(""));
        let removed = parse_numstat_count(parts.get(1).copied().unwrap_or(""));
        set_last_wins(&mut out, path, added, removed);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find<'a>(out: &'a [NumstatEntry], path: &str) -> Option<&'a NumstatEntry> {
        out.iter().find(|e| e.path == path)
    }

    #[test]
    fn parses_added_removed_counts_keyed_by_path() {
        let out = parse_numstat(b"3\t4\tsrc/app.ts\n10\t0\tsrc/new.ts\n");
        let app = find(&out, "src/app.ts").unwrap();
        assert_eq!((app.added, app.removed), (Some(3), Some(4)));
        let new = find(&out, "src/new.ts").unwrap();
        assert_eq!((new.added, new.removed), (Some(10), Some(0)));
    }

    #[test]
    fn treats_binary_dash_columns_as_none() {
        let out = parse_numstat(b"-\t-\tassets/logo.png\n");
        let e = find(&out, "assets/logo.png").unwrap();
        assert_eq!((e.added, e.removed), (None, None));
    }

    #[test]
    fn keys_renames_to_post_rename_path() {
        let braced = parse_numstat(b"2\t1\tsrc/{old => new}/file.ts\n");
        let e = find(&braced, "src/new/file.ts").unwrap();
        assert_eq!((e.added, e.removed), (Some(2), Some(1)));

        let plain = parse_numstat(b"2\t1\told.ts => new.ts\n");
        let e = find(&plain, "new.ts").unwrap();
        assert_eq!((e.added, e.removed), (Some(2), Some(1)));
    }

    #[test]
    fn keeps_literal_rename_marker_filenames_in_nul_mode() {
        let out = parse_numstat(b"1\t0\tdocs/a => b.txt\0");
        let e = find(&out, "docs/a => b.txt").unwrap();
        assert_eq!((e.added, e.removed), (Some(1), Some(0)));
    }

    #[test]
    fn keys_nul_delimited_renames_to_post_rename_path() {
        let out = parse_numstat(b"2\t1\t\0old.ts\0new.ts\0");
        let e = find(&out, "new.ts").unwrap();
        assert_eq!((e.added, e.removed), (Some(2), Some(1)));
        assert!(find(&out, "old.ts").is_none());
    }

    #[test]
    fn decodes_cquoted_paths_before_keying() {
        let out = parse_numstat(b"1\t1\t\"tab\\tfile.txt\"\n");
        let e = find(&out, "tab\tfile.txt").unwrap();
        assert_eq!((e.added, e.removed), (Some(1), Some(1)));
    }

    #[test]
    fn ignores_blank_lines() {
        assert!(parse_numstat(b"").is_empty());
    }
}
