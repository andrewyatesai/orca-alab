//! Browser context-grab shared contracts, ported from
//! `src/shared/browser-grab-types.ts`.
//!
//! The payload budgets, safe-attribute allowlist, secret-redaction patterns, and
//! the curated computed-style key list are the parts of the grab contract that
//! carry behaviour (the rest of the TS module is pure `type` declarations with no
//! runtime shape). `is_aria_attribute` is the one predicate: `aria-*` names are
//! always included regardless of the allowlist.

/// Payload budgets — enforced in both guest and main. Lengths are UTF-16 code
/// units on the TS side; callers that truncate must use `encode_utf16` semantics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GrabBudget {
    pub text_snippet_max_length: usize,
    pub nearby_text_entry_max_length: usize,
    pub nearby_text_max_entries: usize,
    pub html_snippet_max_length: usize,
    pub ancestor_path_max_entries: usize,
    pub nearby_elements_max_entries: usize,
    pub nearby_element_max_length: usize,
    pub selector_max_length: usize,
    pub path_max_length: usize,
    pub css_classes_max_length: usize,
    pub selected_text_max_length: usize,
    pub source_file_max_length: usize,
    pub react_components_max_length: usize,
    pub annotation_comment_max_length: usize,
    pub annotations_max_per_page: usize,
    /// Hard byte budget for the screenshot PNG data URL before we omit it.
    pub screenshot_max_bytes: usize,
}

pub const GRAB_BUDGET: GrabBudget = GrabBudget {
    text_snippet_max_length: 200,
    nearby_text_entry_max_length: 200,
    nearby_text_max_entries: 10,
    html_snippet_max_length: 4096,
    ancestor_path_max_entries: 10,
    nearby_elements_max_entries: 6,
    nearby_element_max_length: 160,
    selector_max_length: 700,
    path_max_length: 900,
    css_classes_max_length: 500,
    selected_text_max_length: 500,
    source_file_max_length: 500,
    react_components_max_length: 500,
    annotation_comment_max_length: 2000,
    annotations_max_per_page: 20,
    screenshot_max_bytes: 2 * 1024 * 1024,
};

/// Only these attribute names are included in the payload by default.
pub const GRAB_SAFE_ATTRIBUTE_NAMES: &[&str] = &[
    "id",
    "class",
    "name",
    "type",
    "role",
    "href",
    "src",
    "alt",
    "title",
    "placeholder",
    "for",
    "action",
    "method",
];

/// Attribute-value substrings that indicate secrets — matching values get
/// redacted. Why tighter patterns than broad words like `code`/`state`: those
/// match normal CSS class names (`source-code`, `stateful`) and would degrade
/// extraction on most real sites. Intent: catch OAuth callback params and
/// credential-like values.
pub const GRAB_SECRET_PATTERNS: &[&str] = &[
    "access_token",
    "auth_token",
    "api_key",
    "apikey",
    "client_secret",
    "oauth_state",
    "x-amz-",
    "session_id",
    "sessionid",
    "csrf",
    "secret",
    "password",
    "passwd",
];

/// Computed style properties to extract — matches the `BrowserGrabComputedStyles`
/// keys (camelCase preserved so the keys round-trip to the DOM API names).
pub const GRAB_STYLE_PROPERTIES: &[&str] = &[
    "display",
    "position",
    "width",
    "height",
    "margin",
    "padding",
    "color",
    "backgroundColor",
    "border",
    "borderRadius",
    "fontFamily",
    "fontSize",
    "fontWeight",
    "lineHeight",
    "textAlign",
    "zIndex",
];

/// Attribute names matching `aria-*` are always included.
pub fn is_aria_attribute(name: &str) -> bool {
    name.starts_with("aria-")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defines_all_required_budget_fields() {
        assert_eq!(GRAB_BUDGET.text_snippet_max_length, 200);
        assert_eq!(GRAB_BUDGET.nearby_text_entry_max_length, 200);
        assert_eq!(GRAB_BUDGET.nearby_text_max_entries, 10);
        assert_eq!(GRAB_BUDGET.html_snippet_max_length, 4096);
        assert_eq!(GRAB_BUDGET.ancestor_path_max_entries, 10);
        assert_eq!(GRAB_BUDGET.nearby_element_max_length, 160);
        assert_eq!(GRAB_BUDGET.css_classes_max_length, 500);
        assert_eq!(GRAB_BUDGET.selected_text_max_length, 500);
        assert_eq!(GRAB_BUDGET.annotations_max_per_page, 20);
        assert_eq!(GRAB_BUDGET.screenshot_max_bytes, 2 * 1024 * 1024);
    }

    #[test]
    fn returns_true_for_aria_prefixed_attributes() {
        assert!(is_aria_attribute("aria-label"));
        assert!(is_aria_attribute("aria-labelledby"));
        assert!(is_aria_attribute("aria-hidden"));
    }

    #[test]
    fn returns_false_for_non_aria_attributes() {
        assert!(!is_aria_attribute("class"));
        assert!(!is_aria_attribute("id"));
        assert!(!is_aria_attribute("notaria-label"));
    }

    #[test]
    fn includes_core_safe_attributes() {
        assert!(GRAB_SAFE_ATTRIBUTE_NAMES.contains(&"id"));
        assert!(GRAB_SAFE_ATTRIBUTE_NAMES.contains(&"class"));
        assert!(GRAB_SAFE_ATTRIBUTE_NAMES.contains(&"role"));
        assert!(GRAB_SAFE_ATTRIBUTE_NAMES.contains(&"type"));
    }

    #[test]
    fn does_not_include_unsafe_attributes() {
        assert!(!GRAB_SAFE_ATTRIBUTE_NAMES.contains(&"onclick"));
        assert!(!GRAB_SAFE_ATTRIBUTE_NAMES.contains(&"style"));
        assert!(!GRAB_SAFE_ATTRIBUTE_NAMES.contains(&"data-secret"));
    }

    #[test]
    fn includes_precise_secret_patterns() {
        assert!(GRAB_SECRET_PATTERNS.contains(&"access_token"));
        assert!(GRAB_SECRET_PATTERNS.contains(&"api_key"));
        assert!(GRAB_SECRET_PATTERNS.contains(&"password"));
        assert!(GRAB_SECRET_PATTERNS.contains(&"secret"));
        assert!(GRAB_SECRET_PATTERNS.contains(&"session_id"));
        assert!(GRAB_SECRET_PATTERNS.contains(&"csrf"));
    }

    #[test]
    fn does_not_include_overly_broad_patterns() {
        // Why: 'code' and 'state' match normal CSS classes like 'source-code'
        // and 'stateful', causing false positive redactions on most sites.
        assert!(!GRAB_SECRET_PATTERNS.contains(&"code"));
        assert!(!GRAB_SECRET_PATTERNS.contains(&"state"));
        assert!(!GRAB_SECRET_PATTERNS.contains(&"auth"));
        assert!(!GRAB_SECRET_PATTERNS.contains(&"token"));
    }

    #[test]
    fn includes_the_curated_subset_of_computed_styles() {
        assert!(GRAB_STYLE_PROPERTIES.contains(&"display"));
        assert!(GRAB_STYLE_PROPERTIES.contains(&"fontSize"));
        assert!(GRAB_STYLE_PROPERTIES.contains(&"backgroundColor"));
        assert!(GRAB_STYLE_PROPERTIES.contains(&"zIndex"));
    }

    #[test]
    fn has_exactly_16_properties_matching_the_type() {
        assert_eq!(GRAB_STYLE_PROPERTIES.len(), 16);
    }
}
