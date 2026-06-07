//! Terminal tab-id validation, ported from `src/shared/terminal-tab-id.ts`.
//!
//! A valid tab id is non-empty and free of `:` (which pane keys reserve as the
//! tab/leaf delimiter). A *host* terminal tab id additionally must not be a
//! web-terminal surface id (those are renderer-local wrappers, not host tabs).

use crate::terminal_surface_id::is_web_terminal_surface_tab_id;

pub fn is_valid_terminal_tab_id(value: &str) -> bool {
    !value.is_empty() && !value.contains(':')
}

pub fn is_valid_host_terminal_tab_id(value: &str) -> bool {
    is_valid_terminal_tab_id(value) && !is_web_terminal_surface_tab_id(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_or_colon_bearing_ids() {
        assert!(!is_valid_terminal_tab_id(""));
        assert!(!is_valid_terminal_tab_id("host-tab::leaf"));
        assert!(!is_valid_terminal_tab_id("a:b"));
        assert!(is_valid_terminal_tab_id("plain-tab"));
    }

    #[test]
    fn host_id_excludes_web_terminal_surface_ids() {
        // `web-terminal-abc` has no `:` so it passes the base check, but it is a
        // renderer-local surface id, not a host tab.
        assert!(is_valid_terminal_tab_id("web-terminal-abc"));
        assert!(!is_valid_host_terminal_tab_id("web-terminal-abc"));
        assert!(is_valid_host_terminal_tab_id("plain-tab"));
    }
}
