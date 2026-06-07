//! Generated agent tab-title derivation, ported from `src/shared/agent-tab-title.ts`.
//!
//! Turns a free-form prompt into a short, clean tab title: take the first
//! clause, strip leading filler ("can you please …"), markup, links, and
//! punctuation, capitalize, and truncate at a word boundary. Regex-backed
//! (needs `\p{L}`/`\p{N}` general-category classes).

use regex::Regex;
use std::sync::OnceLock;

pub const GENERATED_TAB_TITLE_MAX_LENGTH: usize = 40;

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
    let stripped_markup = markup_re().replace_all(prompt.trim(), " ");
    let without_prefix = issue_prefix_re().replace(&stripped_markup, "");
    let without_links = url_re().replace_all(&without_prefix, " ");
    let first_clause = without_links
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
    let collapsed = whitespace_re().replace_all(&no_symbols, " ");
    let candidate = collapsed.trim();
    if candidate.is_empty() {
        return None;
    }

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
    RE.get_or_init(|| Regex::new(r"(?i)\bhttps?://\S+").unwrap())
}

fn non_text_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"[^\p{L}\p{N}\s]").unwrap())
}

fn whitespace_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\s+").unwrap())
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
}
