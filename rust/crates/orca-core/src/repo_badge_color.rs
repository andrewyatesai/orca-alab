//! Repo badge colour normalisation, ported from `src/shared/repo-badge-color.ts`.
//!
//! Accepts `#rgb`/`#rrggbb` (with or without the leading `#`, any case,
//! surrounding whitespace), expands shorthand, and lowercases. Rejects anything
//! non-hex so persisted colours can't smuggle e.g. `url(javascript:...)`.

/// The curated repo palette (`src/shared/constants.ts`). Membership does not
/// change `normalize`'s output — it returns the normalised value either way —
/// but the palette is captured here so the data lives with the logic.
pub const REPO_COLORS: &[&str] = &[
    "#737373", "#ef4444", "#f97316", "#eab308", "#22c55e", "#14b8a6", "#8b5cf6", "#ec4899",
];

/// `REPO_COLORS[0]` in the TS source.
pub const DEFAULT_REPO_BADGE_COLOR: &str = "#737373";

fn is_hex_run(s: &str, len: usize) -> bool {
    s.len() == len && s.bytes().all(|b| b.is_ascii_hexdigit())
}

pub fn normalize_repo_badge_color(value: &str) -> Option<String> {
    let trimmed = value.trim();
    let raw = trimmed.strip_prefix('#').unwrap_or(trimmed);
    if !(is_hex_run(raw, 3) || is_hex_run(raw, 6)) {
        return None;
    }
    let lower = raw.to_lowercase();
    let hex = if lower.len() == 3 {
        lower.chars().flat_map(|c| [c, c]).collect::<String>()
    } else {
        lower
    };
    Some(format!("#{hex}"))
}

pub fn resolve_repo_badge_color(value: &str) -> String {
    normalize_repo_badge_color(value).unwrap_or_else(|| DEFAULT_REPO_BADGE_COLOR.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_six_digit_hex_colors() {
        assert_eq!(
            normalize_repo_badge_color(" ABCDEF "),
            Some("#abcdef".to_string())
        );
        assert_eq!(
            normalize_repo_badge_color("#ABCDEF"),
            Some("#abcdef".to_string())
        );
    }

    #[test]
    fn expands_shorthand_hex_colors() {
        assert_eq!(normalize_repo_badge_color("#abc"), Some("#aabbcc".to_string()));
    }

    #[test]
    fn rejects_non_hex_colors() {
        assert_eq!(normalize_repo_badge_color("blue"), None);
        assert_eq!(normalize_repo_badge_color("url(javascript:alert(1))"), None);
        assert_eq!(normalize_repo_badge_color("#12zz12"), None);
    }

    #[test]
    fn falls_back_to_default_when_resolving_invalid_input() {
        assert_eq!(resolve_repo_badge_color("blue"), DEFAULT_REPO_BADGE_COLOR);
    }
}
