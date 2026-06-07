//! Web-terminal surface-id mapping, ported from `src/shared/terminal-surface-id.ts`.
//!
//! Host session surface ids use `tab::leaf`, but renderer pane keys reserve `:`
//! as the tab/leaf delimiter. To carry a host surface identity through a local
//! tab id that can flow through `makePaneKey()`, the host id is percent-encoded
//! behind a `web-terminal-` prefix.

use crate::uri_component::{decode_uri_component, encode_uri_component};

pub const WEB_TERMINAL_SURFACE_TAB_PREFIX: &str = "web-terminal-";
pub const HOST_TERMINAL_SURFACE_SEPARATOR: &str = "::";

/// Wrap a host surface id (`tab::leaf`) into a `:`-safe local tab id.
pub fn to_web_terminal_surface_tab_id(host_surface_id: &str) -> String {
    format!("{WEB_TERMINAL_SURFACE_TAB_PREFIX}{}", encode_uri_component(host_surface_id))
}

/// Recover the host surface id from a web-terminal tab id; non-prefixed ids
/// (and ids whose encoding is malformed) pass through unchanged.
pub fn to_host_session_tab_id(tab_id: &str) -> String {
    match tab_id.strip_prefix(WEB_TERMINAL_SURFACE_TAB_PREFIX) {
        Some(encoded) => decode_uri_component(encoded),
        None => tab_id.to_string(),
    }
}

pub fn is_web_terminal_surface_tab_id(tab_id: &str) -> bool {
    tab_id.starts_with(WEB_TERMINAL_SURFACE_TAB_PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_host_surface_id_percent_encoding_the_separator() {
        // `::` must not survive raw — pane keys reserve `:`.
        let id = to_web_terminal_surface_tab_id("host-tab-1::leaf-9");
        assert_eq!(id, "web-terminal-host-tab-1%3A%3Aleaf-9");
        assert!(!id["web-terminal-".len()..].contains(':'));
    }

    #[test]
    fn round_trips_host_surface_ids() {
        let host = "host-tab-1::/repo/path leaf";
        assert_eq!(to_host_session_tab_id(&to_web_terminal_surface_tab_id(host)), host);
    }

    #[test]
    fn passes_through_non_prefixed_ids() {
        assert_eq!(to_host_session_tab_id("plain-tab"), "plain-tab");
        assert_eq!(to_host_session_tab_id("host-tab::leaf"), "host-tab::leaf");
    }

    #[test]
    fn passes_through_malformed_encoding() {
        // Prefixed but malformed percent-escape → original slice, like the TS catch.
        assert_eq!(to_host_session_tab_id("web-terminal-%zz"), "%zz");
    }

    #[test]
    fn detects_the_prefix() {
        assert!(is_web_terminal_surface_tab_id("web-terminal-abc"));
        assert!(!is_web_terminal_surface_tab_id("host-tab::leaf"));
    }
}
