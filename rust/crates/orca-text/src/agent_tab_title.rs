//! Generated agent tab-title derivation, ported from `src/shared/agent-tab-title.ts`.
//!
//! Turns a free-form prompt into a short, clean tab title: take the first
//! clause, strip leading filler ("can you please …"), markup, links, and
//! punctuation, capitalize, and truncate at a word boundary. Regex-backed
//! (needs `\p{L}`/`\p{N}` general-category classes).

use regex::Regex;
use std::sync::OnceLock;

pub const GENERATED_TAB_TITLE_MAX_LENGTH: usize = 40;
/// Titles are previews — cleanup must not scan a paste-sized prompt on the
/// renderer state path. Counted in UTF-16 code units (the TS `.slice(0, 512)`).
pub const GENERATED_TAB_TITLE_SOURCE_SCAN_LIMIT: usize = 512;

/// First `limit` UTF-16 code units of `value` (TS `String.prototype.slice`
/// semantics — a surrogate pair counts as two units; we never split a pair,
/// matching `slice`'s behaviour of keeping whole chars via lone-surrogate
/// replacement being irrelevant here because a trailing lone surrogate would
/// be stripped by the later non-text pass anyway).
fn utf16_prefix(value: &str, limit: usize) -> &str {
    let mut units = 0;
    for (byte_index, ch) in value.char_indices() {
        let next = units + ch.len_utf16();
        if next > limit {
            return &value[..byte_index];
        }
        units = next;
    }
    value
}

fn capitalize_first_letter(value: &str) -> String {
    letter_re().replace(value, |captures: &regex::Captures| captures[0].to_uppercase()).into_owned()
}

fn truncate_at_word_boundary(value: &str, max_length: usize) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= max_length {
        return value.to_string();
    }
    let raw_slice: String = chars[..max_length].iter().collect();
    let sliced = raw_slice.trim();
    if sliced.chars().count() < raw_slice.chars().count() {
        return sliced.to_string();
    }
    let sliced_chars: Vec<char> = sliced.chars().collect();
    let threshold = (max_length as f64 * 0.55).floor() as usize;
    match sliced_chars.iter().rposition(|&c| c == ' ') {
        Some(last_space) if last_space >= threshold => {
            sliced_chars[..last_space].iter().collect::<String>().trim().to_string()
        }
        _ => sliced.to_string(),
    }
}

pub fn derive_generated_tab_title(prompt: &str) -> Option<String> {
    let prompt_preview = utf16_prefix(prompt, GENERATED_TAB_TITLE_SOURCE_SCAN_LIMIT);
    // Strip URLs BEFORE markdown punctuation: a GitLab URL like `/merge_requests/42`
    // contains `_`, and folding that to a space first would split the URL and leak
    // fragments ("requests") into the title.
    let without_links = url_re().replace_all(prompt_preview.trim(), " ");
    let stripped_markup = markup_re().replace_all(&without_links, " ");
    let without_prefix = issue_prefix_re().replace(&stripped_markup, "");
    let first_clause = without_prefix
        .split(['.', '!', '?', ';', '\n', '\r', '\u{2028}', '\u{2029}'])
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    if first_clause.is_empty() {
        return None;
    }

    let mut candidate = first_clause;
    for _ in 0..3 {
        let before = candidate.clone();
        for pattern in filler_patterns() {
            candidate = pattern.replace(&candidate, "").into_owned();
        }
        candidate = candidate.trim().to_string();
        if candidate == before.trim() {
            break;
        }
    }

    let no_symbols = non_text_re().replace_all(&candidate, " ");
    let candidate = fold_generated_tab_title_whitespace(&no_symbols);
    if candidate.is_empty() {
        return None;
    }
    let candidate = candidate.as_str();

    Some(truncate_at_word_boundary(&capitalize_first_letter(candidate), GENERATED_TAB_TITLE_MAX_LENGTH))
}

fn markup_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"[`*_~#>\[\]{}()]").unwrap())
}

fn issue_prefix_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)^(?:issue|task|bug|feature|pr)\s*(?:#?\d+)?\s*[:-]\s*").unwrap())
}

fn url_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // No `\b` anchor: a URL wrapped in markdown emphasis (`_https://…_`) is
    // preceded by a word char, where `\bhttps` fails to match and leaks the URL.
    RE.get_or_init(|| Regex::new(r"(?i)https?://\S+").unwrap())
}

fn non_text_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"[^\p{L}\p{N}\s]").unwrap())
}

/// The TS `isGeneratedTabTitleWhitespace` code list — NOT Unicode White_Space:
/// it includes U+FEFF (BOM) and excludes U+0085 (NEL), so a `\s` regex is not a
/// faithful substitute.
fn is_generated_tab_title_whitespace(c: char) -> bool {
    let code = c as u32;
    code == 32
        || (9..=13).contains(&code)
        || code == 160
        || code == 5760
        || (8192..=8202).contains(&code)
        || code == 8232
        || code == 8233
        || code == 8239
        || code == 8287
        || code == 12288
        || code == 65279
}

/// Mirror of `foldGeneratedTabTitleWhitespace`: collapse runs of the fold set
/// to single spaces, dropping leading/trailing runs entirely.
fn fold_generated_tab_title_whitespace(value: &str) -> String {
    let mut normalized = String::with_capacity(value.len());
    let mut pending_whitespace = false;
    for c in value.chars() {
        if is_generated_tab_title_whitespace(c) {
            pending_whitespace = !normalized.is_empty();
            continue;
        }
        if pending_whitespace {
            normalized.push(' ');
            pending_whitespace = false;
        }
        normalized.push(c);
    }
    normalized
}

fn letter_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\p{L}").unwrap())
}

fn filler_patterns() -> &'static [Regex] {
    static RE: OnceLock<Vec<Regex>> = OnceLock::new();
    RE.get_or_init(|| {
        [
            r"(?i)^(?:can|could|would)\s+you(?:\s+please)?\s+",
            r"(?i)^please(?:\s+|$)",
            r"(?i)^i\s+(?:want|need)\s+(?:you\s+)?to\s+",
            r"(?i)^help\s+me(?:\s+to)?\s+",
            r"(?i)^help\s+",
            r"(?i)^let'?s\s+",
            r"(?i)^we\s+need\s+to\s+",
            r"(?i)^need\s+to\s+",
        ]
        .iter()
        .map(|pattern| Regex::new(pattern).unwrap())
        .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_a_short_title_from_the_first_useful_prompt_clause() {
        assert_eq!(
            derive_generated_tab_title("Can you please refactor the auth middleware to use JWT tokens?").as_deref(),
            Some("Refactor the auth middleware to use JWT")
        );
    }

    #[test]
    fn strips_markup_links_emoji_and_punctuation_from_generated_titles() {
        assert_eq!(
            derive_generated_tab_title("Please fix `src/auth.ts`!!! https://example.com 🔥 then add tests").as_deref(),
            Some("Fix src auth")
        );
    }

    #[test]
    fn keeps_useful_text_after_common_issue_prefixes() {
        assert_eq!(
            derive_generated_tab_title("Issue #2056: Opt-in generated tab titles for agents").as_deref(),
            Some("Opt in generated tab titles for agents")
        );
    }

    #[test]
    fn bounds_titles_to_the_maximum_length_without_adding_punctuation() {
        let title = derive_generated_tab_title(
            "I want to replace the terminal reconnection hydration flow with a safer retry path",
        )
        .unwrap();
        assert!(title.chars().count() <= GENERATED_TAB_TITLE_MAX_LENGTH);
        assert!(non_text_re().find(&title).is_none());
        assert!(!title.is_empty());
    }

    #[test]
    fn returns_none_when_the_prompt_has_no_useful_title_text() {
        assert_eq!(derive_generated_tab_title("please!!!"), None);
    }

    #[test]
    fn preserves_non_ascii_text_while_folding_the_ts_whitespace_set() {
        // NBSP + ideographic space fold; BOM (not Unicode WS) folds too.
        assert_eq!(
            derive_generated_tab_title("Please 修正\u{00a0}résumé\t検索\u{3000}１２３!!!").as_deref(),
            Some("修正 résumé 検索 １２３")
        );
    }

    #[test]
    fn scans_only_the_first_512_utf16_units_of_a_paste_sized_prompt() {
        // A paste whose first sentence terminator sits beyond the scan limit:
        // the preview slice caps the clause, so the title derives from the
        // truncated preview instead of scanning the full prompt.
        let prompt = format!("{} end. Second sentence", "word ".repeat(200));
        let title = derive_generated_tab_title(&prompt).unwrap();
        assert!(title.chars().count() <= GENERATED_TAB_TITLE_MAX_LENGTH);
        // And the slice counts UTF-16 units: 512 BMP chars → identical result
        // whether or not a multi-byte char follows the boundary.
        let mut multibyte = "é".repeat(512);
        multibyte.push_str(". tail");
        assert_eq!(
            derive_generated_tab_title(&multibyte),
            derive_generated_tab_title(&"é".repeat(512))
        );
    }
}
