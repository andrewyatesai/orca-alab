//! Tab title/label resolution, ported from `src/shared/tab-title-resolution.ts`.
//!
//! Priority is manual title → generated title (only when the feature is on) →
//! live title → fallback, each trimmed and treated as absent when blank.

/// Trimmed value, or `None` when missing or blank.
fn first_nonblank(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|t| !t.is_empty())
}

pub struct TerminalTabTitleParts<'a> {
    pub custom_title: Option<&'a str>,
    pub generated_title: Option<&'a str>,
    pub title: Option<&'a str>,
}

pub fn resolve_terminal_tab_title(
    parts: &TerminalTabTitleParts<'_>,
    generated_titles_enabled: bool,
    fallback: &str,
) -> String {
    first_nonblank(parts.custom_title)
        .or_else(|| {
            if generated_titles_enabled {
                first_nonblank(parts.generated_title)
            } else {
                None
            }
        })
        .or_else(|| first_nonblank(parts.title))
        .unwrap_or(fallback)
        .to_string()
}

pub struct UnifiedTabLabelParts<'a> {
    pub custom_label: Option<&'a str>,
    pub generated_label: Option<&'a str>,
    pub label: Option<&'a str>,
}

pub fn resolve_unified_tab_label(
    parts: Option<&UnifiedTabLabelParts<'_>>,
    generated_titles_enabled: bool,
    fallback: &str,
) -> String {
    first_nonblank(parts.and_then(|p| p.custom_label))
        .or_else(|| {
            if generated_titles_enabled {
                first_nonblank(parts.and_then(|p| p.generated_label))
            } else {
                None
            }
        })
        .or_else(|| first_nonblank(parts.and_then(|p| p.label)))
        .unwrap_or(fallback)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_live_titles_when_generated_titles_disabled() {
        let parts = TerminalTabTitleParts {
            custom_title: None,
            generated_title: Some("Refactor auth"),
            title: Some("Claude working"),
        };
        assert_eq!(resolve_terminal_tab_title(&parts, false, ""), "Claude working");
    }

    #[test]
    fn places_generated_titles_between_manual_and_live_when_enabled() {
        let parts = TerminalTabTitleParts {
            custom_title: None,
            generated_title: Some("Refactor auth"),
            title: Some("Claude working"),
        };
        assert_eq!(resolve_terminal_tab_title(&parts, true, ""), "Refactor auth");
        let parts = TerminalTabTitleParts {
            custom_title: Some("Payments"),
            generated_title: Some("Refactor auth"),
            title: Some("Claude working"),
        };
        assert_eq!(resolve_terminal_tab_title(&parts, true, ""), "Payments");
    }

    #[test]
    fn unified_tab_labels_use_the_same_priority() {
        let parts = UnifiedTabLabelParts {
            custom_label: None,
            generated_label: Some("Fix flaky tests"),
            label: Some("Codex working"),
        };
        assert_eq!(resolve_unified_tab_label(Some(&parts), true, ""), "Fix flaky tests");
    }
}
