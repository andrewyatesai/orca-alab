//! Browser search-query heuristics, ported from the self-contained pure parts
//! of `src/shared/browser-url.ts`. (The full `normalizeBrowserNavigationUrl`,
//! which does `URL` parsing + file:// + local-path handling, is deferred.)

use crate::uri_component::encode_uri_component;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchEngine {
    Google,
    DuckDuckGo,
    Bing,
    Kagi,
}

pub const DEFAULT_SEARCH_ENGINE: SearchEngine = SearchEngine::Google;

impl SearchEngine {
    pub fn label(self) -> &'static str {
        match self {
            SearchEngine::Google => "Google",
            SearchEngine::DuckDuckGo => "DuckDuckGo",
            SearchEngine::Bing => "Bing",
            SearchEngine::Kagi => "Kagi",
        }
    }

    fn query_base_url(self) -> &'static str {
        match self {
            SearchEngine::Google => "https://www.google.com/search?q=",
            SearchEngine::DuckDuckGo => "https://duckduckgo.com/?q=",
            SearchEngine::Bing => "https://www.bing.com/search?q=",
            SearchEngine::Kagi => "https://kagi.com/search?q=",
        }
    }
}

/// Build a search URL for `query` on `engine`. (The Kagi private-session-link
/// variant is part of the deferred navigation normaliser.)
pub fn build_search_url(query: &str, engine: SearchEngine) -> String {
    format!("{}{}", engine.query_base_url(), encode_uri_component(query))
}

/// `^[^\s]+\.[a-z]{2,}(/.*)?$` — a domain-like token (host with a letter TLD).
fn looks_like_url(input: &str) -> bool {
    if input.is_empty() || input.chars().any(char::is_whitespace) {
        return false;
    }
    let host = match input.find('/') {
        Some(i) => &input[..i],
        None => input,
    };
    let Some(dot) = host.rfind('.') else {
        return false;
    };
    if dot == 0 {
        return false; // need a non-empty label before the dot
    }
    let tld = &host[dot + 1..];
    tld.len() >= 2 && tld.chars().all(|c| c.is_ascii_alphabetic())
}

/// Whether `input` should be treated as a search query rather than a URL:
/// multi-word → yes; domain-like or containing `.`/`:` → no.
pub fn looks_like_search_query(input: &str) -> bool {
    if input.contains(' ') {
        return true;
    }
    if looks_like_url(input) {
        return false;
    }
    if input.contains('.') || input.contains(':') {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_search_urls_per_engine() {
        assert_eq!(build_search_url("hello world", SearchEngine::Google), "https://www.google.com/search?q=hello%20world");
        assert_eq!(build_search_url("hello world", SearchEngine::DuckDuckGo), "https://duckduckgo.com/?q=hello%20world");
        assert_eq!(build_search_url("hello world", SearchEngine::Bing), "https://www.bing.com/search?q=hello%20world");
        assert_eq!(build_search_url("hello world", SearchEngine::Kagi), "https://kagi.com/search?q=hello%20world");
    }

    #[test]
    fn multi_word_input_is_a_search() {
        assert!(looks_like_search_query("react hooks"));
        assert!(looks_like_search_query("what is typescript"));
    }

    #[test]
    fn bare_word_is_a_search_but_domains_are_not() {
        assert!(looks_like_search_query("singleword"));
        assert!(!looks_like_search_query("example.com"));
        assert!(!looks_like_search_query("github.com/org/repo"));
        assert!(!looks_like_search_query("example.co.uk"));
    }

    #[test]
    fn dotted_or_coloned_tokens_are_not_searches() {
        assert!(!looks_like_search_query("1.2.3.4")); // IP-ish → navigate
        assert!(!looks_like_search_query("host:8080"));
    }

    #[test]
    fn engine_labels() {
        assert_eq!(SearchEngine::Google.label(), "Google");
        assert_eq!(DEFAULT_SEARCH_ENGINE, SearchEngine::Google);
    }
}
