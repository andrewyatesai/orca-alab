//! Quick Open fuzzy ranking, ported from
//! `src/renderer/src/components/quick-open-search.ts`.
//!
//! Runs per-keystroke on the renderer UI thread over the active worktree's file
//! list (thousands to tens of thousands of paths). The hot path is a greedy
//! subsequence scan per candidate plus a bounded top-N insertion. Match ranges
//! are NOT produced — the Quick Open list renders plain text with no per-char
//! highlight — so the result is just `{ path, score }`, best (lowest) first.
//!
//! FAITHFULNESS: character indexing is UTF-16 CODE UNITS, not Rust `char`s,
//! because the TS gap arithmetic uses JS string indices (UTF-16). Lower-casing
//! uses `str::to_lowercase` (Unicode default case conversion), which matches JS
//! `String.prototype.toLowerCase` for ASCII/BMP paths — the honest scope of this
//! port (real file paths). The `-1` no-match sentinel deliberately collides with
//! a genuine score of `-1`: the TS `rankQuickOpenFiles` skips `score === -1`, so
//! both a non-subsequence AND a coincidental `-1` are dropped; this port
//! reproduces that exactly.

/// Default result cap (mirrors `QUICK_OPEN_RESULT_LIMIT`).
pub const QUICK_OPEN_RESULT_LIMIT: usize = 50;
/// Default query byte budget (mirrors `QUICK_OPEN_QUERY_MAX_BYTES`).
pub const QUICK_OPEN_QUERY_MAX_BYTES: usize = 2 * 1024;

/// One ranked result: the ORIGINAL path (not the normalized form) + its score.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuickOpenResult {
    pub path: String,
    pub score: i32,
}

/// Whether the raw query exceeds the byte budget. Mirrors
/// `isClipboardTextByteLengthOverLimit`'s effective bound: a UTF-16 length over
/// `max_bytes` implies a UTF-8 byte length over it too, so the UTF-8 byte length
/// (`str::len`) is the deciding measure.
#[must_use]
pub fn is_query_too_large(query: &str, max_bytes: usize) -> bool {
    query.len() > max_bytes
}

/// Faithful port of `rankQuickOpenFiles(query, prepareQuickOpenFiles(paths), limit)`:
/// the raw path list + raw query in, up to `limit` `{ path, score }` out, best
/// first, ties broken by original input order.
#[must_use]
pub fn rank_quick_open_files(query: &str, paths: &[&str], limit: usize) -> Vec<QuickOpenResult> {
    if limit == 0 {
        return Vec::new();
    }
    if is_query_too_large(query, QUICK_OPEN_QUERY_MAX_BYTES) {
        return Vec::new();
    }
    // Quick Open presents slash-normalized paths even on Windows; users still
    // naturally type backslashes in path queries. Trim with the JS
    // `String.prototype.trim` whitespace set (not Rust's) so a pasted BOM is
    // stripped and a bare NEL is not — matching the raw `deferredQuery` the
    // renderer feeds in.
    let normalized_query = query.trim_matches(is_js_trim_whitespace).replace('\\', "/").to_lowercase();
    if normalized_query.is_empty() {
        return paths
            .iter()
            .take(limit)
            .map(|p| QuickOpenResult { path: (*p).to_string(), score: 0 })
            .collect();
    }
    let query_units: Vec<u16> = normalized_query.encode_utf16().collect();

    let mut ranked: Vec<Ranked> = Vec::new();
    for (input_index, path) in paths.iter().enumerate() {
        let prepared = prepare(path);
        let score = fuzzy_match(&query_units, &prepared.lower_path, &prepared.lower_filename);
        if score == -1 {
            continue;
        }
        insert_top(&mut ranked, Ranked { path: (*path).to_string(), score, input_index }, limit);
    }
    ranked.into_iter().map(|r| QuickOpenResult { path: r.path, score: r.score }).collect()
}

/// The ECMAScript `String.prototype.trim` whitespace set = Unicode White_Space
/// MINUS U+0085 (NEL — not ES whitespace) PLUS U+FEFF (BOM/ZWNBSP — ES-only).
/// Rust's `str::trim` uses Unicode White_Space, which differs on exactly those
/// two code points, so a faithful port cannot reuse it.
fn is_js_trim_whitespace(c: char) -> bool {
    (c.is_whitespace() && c != '\u{0085}') || c == '\u{FEFF}'
}

/// Normalized, UTF-16-encoded forms of one candidate. Mirrors
/// `prepareQuickOpenFiles`: the filename is sliced from the slash-normalized
/// (pre-lowercase) path, then lower-cased — matching the TS slice/lowercase order.
struct Prepared {
    lower_path: Vec<u16>,
    lower_filename: Vec<u16>,
}

fn prepare(path: &str) -> Prepared {
    let search_path = path.replace('\\', "/");
    // '/' is ASCII, so the byte index of the last '/' slices the same text as JS
    // `lastIndexOf('/')` + `slice` (both take "everything after the last /").
    let filename = match search_path.rfind('/') {
        Some(i) => &search_path[i + 1..],
        None => search_path.as_str(),
    };
    Prepared {
        lower_path: search_path.to_lowercase().encode_utf16().collect(),
        lower_filename: filename.to_lowercase().encode_utf16().collect(),
    }
}

struct Ranked {
    path: String,
    score: i32,
    input_index: usize,
}

/// Greedy subsequence match with gap penalty + boundary/filename bonuses.
/// Returns `-1` when the query is not a subsequence (the TS sentinel).
fn fuzzy_match(query: &[u16], lower_path: &[u16], lower_filename: &[u16]) -> i32 {
    const SLASH: u16 = b'/' as u16;
    const DOT: u16 = b'.' as u16;
    const DASH: u16 = b'-' as u16;
    let mut qi = 0usize;
    let mut score: i32 = 0;
    let mut last_match: i32 = -1;
    let mut ti = 0usize;
    while ti < lower_path.len() && qi < query.len() {
        if lower_path[ti] == query[qi] {
            let gap = if last_match == -1 { 0 } else { ti as i32 - last_match - 1 };
            score += gap;
            if ti > 0 {
                let prev = lower_path[ti - 1];
                if prev == SLASH || prev == DOT || prev == DASH {
                    score -= 5;
                }
            }
            last_match = ti as i32;
            qi += 1;
        }
        ti += 1;
    }
    if qi < query.len() {
        return -1;
    }
    if contains_subslice(lower_filename, query) {
        score -= 100;
    }
    score
}

/// `JS String.includes` over UTF-16 units: is `needle` a contiguous subslice?
fn contains_subslice(haystack: &[u16], needle: &[u16]) -> bool {
    if needle.is_empty() {
        return true;
    }
    if needle.len() > haystack.len() {
        return false;
    }
    haystack.windows(needle.len()).any(|window| window == needle)
}

/// Score ascending, then original input order — mirrors `compareRankedResult`.
fn compare(a: &Ranked, b: &Ranked) -> std::cmp::Ordering {
    a.score.cmp(&b.score).then(a.input_index.cmp(&b.input_index))
}

/// Bounded top-N insertion into a sorted-ascending vec (mirrors `insertTopResult`):
/// reject early when full and no better than the worst, else binary-search insert
/// and drop the tail past `limit`.
fn insert_top(results: &mut Vec<Ranked>, candidate: Ranked, limit: usize) {
    if results.len() == limit {
        if let Some(worst) = results.last() {
            if compare(&candidate, worst) != std::cmp::Ordering::Less {
                return;
            }
        }
    }
    let at = find_insertion_index(results, &candidate);
    results.insert(at, candidate);
    if results.len() > limit {
        results.pop();
    }
}

fn find_insertion_index(results: &[Ranked], candidate: &Ranked) -> usize {
    let mut low = 0usize;
    let mut high = results.len();
    while low < high {
        let mid = (low + high) / 2;
        if compare(candidate, &results[mid]) == std::cmp::Ordering::Less {
            high = mid;
        } else {
            low = mid + 1;
        }
    }
    low
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths_of(results: &[QuickOpenResult]) -> Vec<&str> {
        results.iter().map(|r| r.path.as_str()).collect()
    }

    #[test]
    fn rejects_non_subsequence_candidates() {
        let out = rank_quick_open_files("xyz", &["src/main.rs", "README.md"], 50);
        assert!(out.is_empty());
    }

    #[test]
    fn empty_query_returns_first_limit_in_input_order_score_zero() {
        let files = ["a.ts", "b.ts", "c.ts"];
        let out = rank_quick_open_files("   ", &files, 2);
        assert_eq!(paths_of(&out), ["a.ts", "b.ts"]);
        assert!(out.iter().all(|r| r.score == 0));
    }

    #[test]
    fn prefers_a_filename_substring_match() {
        // "app" is a substring of the filename app.ts (−100) but only a scattered
        // subsequence of the other path → app.ts ranks first.
        let out =
            rank_quick_open_files("app", &["src/a/p/p/other.ts", "src/app.ts"], 50);
        assert_eq!(out[0].path, "src/app.ts");
        assert!(out[0].score < out[1].score);
    }

    #[test]
    fn word_boundary_after_separator_scores_better_than_mid_token() {
        // Query "m": in "src/m.ts" the 'm' follows '/', earning the −5 boundary
        // bonus; in "arm.ts" the 'm' is mid-token. Lower score wins.
        let boundary = rank_quick_open_files("m", &["src/m.ts"], 50);
        let midtoken = rank_quick_open_files("m", &["arm.ts"], 50);
        assert!(boundary[0].score < midtoken[0].score);
    }

    #[test]
    fn ties_break_by_original_input_order() {
        // Both filenames contain "x" identically; the earlier input wins the tie.
        let out = rank_quick_open_files("x", &["x1/x.ts", "x2/x.ts"], 50);
        assert_eq!(out[0].path, "x1/x.ts");
        assert_eq!(out[1].path, "x2/x.ts");
    }

    #[test]
    fn normalizes_backslashes_in_both_query_and_paths() {
        // A Windows-style path + a backslash query both normalize to '/'.
        let out = rank_quick_open_files("src\\app", &["src\\app.ts"], 50);
        assert_eq!(out[0].path, "src\\app.ts");
    }

    #[test]
    fn caps_results_and_sorts_ascending_by_score() {
        let files: Vec<String> = (0..1000).map(|i| format!("dir{i}/file.ts")).collect();
        let refs: Vec<&str> = files.iter().map(String::as_str).collect();
        let out = rank_quick_open_files("file", &refs, 50);
        assert_eq!(out.len(), 50);
        for pair in out.windows(2) {
            assert!(pair[0].score <= pair[1].score);
        }
    }

    #[test]
    fn oversize_query_returns_empty() {
        let big = "a".repeat(QUICK_OPEN_QUERY_MAX_BYTES + 1);
        let out = rank_quick_open_files(&big, &["a.ts"], 50);
        assert!(out.is_empty());
    }

    #[test]
    fn non_positive_limit_returns_empty() {
        let out = rank_quick_open_files("a", &["a.ts"], 0);
        assert!(out.is_empty());
    }

    #[test]
    fn multibyte_paths_match_by_utf16_units() {
        // é is one UTF-16 unit; the query still finds the subsequence.
        let out = rank_quick_open_files("caf", &["src/café.ts"], 50);
        assert_eq!(out[0].path, "src/café.ts");
    }

    #[test]
    fn strips_a_leading_bom_like_js_trim() {
        // U+FEFF is ES trim whitespace (a pasted BOM) but NOT Unicode White_Space;
        // it must be trimmed so the query still matches, matching JS.
        let out = rank_quick_open_files("\u{FEFF}src", &["src/app.ts"], 50);
        assert_eq!(out[0].path, "src/app.ts");
    }

    #[test]
    fn bom_only_query_takes_the_empty_query_fast_path() {
        let out = rank_quick_open_files("\u{FEFF}", &["a.ts", "b.ts"], 5);
        assert_eq!(paths_of(&out), ["a.ts", "b.ts"]);
        assert!(out.iter().all(|r| r.score == 0));
    }

    #[test]
    fn does_not_strip_nel_which_js_trim_keeps() {
        // U+0085 (NEL) IS Unicode White_Space but NOT ES trim whitespace, so JS
        // leaves it in the query — no path contains it, so nothing matches.
        let out = rank_quick_open_files("\u{0085}src", &["src/app.ts"], 50);
        assert!(out.is_empty());
    }
}
