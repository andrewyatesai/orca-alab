//! Repo-icon sanitization + builders, ported from `src/shared/repo-icon.ts`.
//!
//! Validates a persisted repo icon (lucide / emoji / image) — rejecting unsafe
//! image URLs and oversized data URLs — and builds the GitHub-avatar and
//! favicon icons. `sanitize` is tri-state: `Undefined` (absent/invalid →
//! leave as-is), `Reset` (explicit null), or a validated `Icon`. JSON input
//! over vendored `serde_json`; percent-encoding via `orca-core`.

use orca_core::uri_component::encode_uri_component;
use serde_json::Value;

pub const MAX_REPO_ICON_UPLOAD_BYTES: usize = 256 * 1024;
pub const MAX_REPO_ICON_DATA_URL_LENGTH: usize = 400 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepoIconImageSource {
    Upload,
    File,
    Favicon,
    Github,
}

impl RepoIconImageSource {
    fn from_id(value: &str) -> Option<RepoIconImageSource> {
        match value {
            "upload" => Some(RepoIconImageSource::Upload),
            "file" => Some(RepoIconImageSource::File),
            "favicon" => Some(RepoIconImageSource::Favicon),
            "github" => Some(RepoIconImageSource::Github),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RepoIcon {
    Lucide { name: String },
    Emoji { emoji: String },
    Image { src: String, source: RepoIconImageSource, label: Option<String> },
}

/// Result of [`sanitize_repo_icon`]: `Undefined` = leave the stored value as-is,
/// `Reset` = explicit clear (JSON `null`), `Icon` = a validated icon.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RepoIconSanitizeResult {
    Undefined,
    Reset,
    Icon(RepoIcon),
}

/// `None` models the JS `undefined`; `Some(Value::Null)` the explicit `null`.
pub fn sanitize_repo_icon(value: Option<&Value>) -> RepoIconSanitizeResult {
    let value = match value {
        None => return RepoIconSanitizeResult::Undefined,
        Some(Value::Null) => return RepoIconSanitizeResult::Reset,
        Some(value) => value,
    };
    let Some(object) = value.as_object() else {
        return RepoIconSanitizeResult::Undefined;
    };

    match object.get("type").and_then(Value::as_str) {
        Some("lucide") => {
            let name = object.get("name").and_then(Value::as_str).map(str::trim).unwrap_or("");
            if !is_lucide_icon_name(name) || utf16_len(name) > 40 {
                return RepoIconSanitizeResult::Undefined;
            }
            RepoIconSanitizeResult::Icon(RepoIcon::Lucide { name: name.to_string() })
        }
        Some("emoji") => {
            let emoji = object.get("emoji").and_then(Value::as_str).map(str::trim).unwrap_or("");
            if emoji.is_empty() || utf16_len(emoji) > 16 {
                return RepoIconSanitizeResult::Undefined;
            }
            RepoIconSanitizeResult::Icon(RepoIcon::Emoji { emoji: emoji.to_string() })
        }
        Some("image") => {
            let src = object.get("src").and_then(Value::as_str).map(str::trim).unwrap_or("");
            let Some(source) = RepoIconImageSource::from_id(object.get("source").and_then(Value::as_str).unwrap_or(""))
            else {
                return RepoIconSanitizeResult::Undefined;
            };
            if utf16_len(src) > MAX_REPO_ICON_DATA_URL_LENGTH || !is_supported_image_src(src, source) {
                return RepoIconSanitizeResult::Undefined;
            }
            let label: String =
                object.get("label").and_then(Value::as_str).map(|l| l.trim().chars().take(80).collect()).unwrap_or_default();
            RepoIconSanitizeResult::Icon(RepoIcon::Image {
                src: src.to_string(),
                source,
                label: if label.is_empty() { None } else { Some(label) },
            })
        }
        _ => RepoIconSanitizeResult::Undefined,
    }
}

/// `https://www.google.com/s2/favicons?domain=<host>&sz=64`, or `None` for a
/// blank/invalid/non-http(s) website URL.
pub fn favicon_url_from_website(raw_url: &str) -> Option<String> {
    let trimmed = raw_url.trim();
    if trimmed.is_empty() {
        return None;
    }
    let to_parse = if trimmed.contains("://") { trimmed.to_string() } else { format!("https://{trimmed}") };
    let url = parse_url(&to_parse)?;
    if !matches!(url.protocol.as_str(), "http:" | "https:") || url.hostname.is_empty() {
        return None;
    }
    Some(format!("https://www.google.com/s2/favicons?domain={}&sz=64", encode_uri_component(&url.hostname)))
}

/// The default repo icon: the GitHub owner avatar.
pub fn github_avatar_icon(owner: &str, repo: &str) -> RepoIcon {
    RepoIcon::Image {
        src: format!("https://github.com/{}.png?size=64", encode_uri_component(owner)),
        source: RepoIconImageSource::Github,
        label: Some(format!("{owner}/{repo}")),
    }
}

fn utf16_len(value: &str) -> usize {
    value.encode_utf16().count()
}

fn is_lucide_icon_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphabetic() => chars.all(|c| c.is_ascii_alphanumeric()),
        _ => false,
    }
}

fn is_supported_image_src(src: &str, source: RepoIconImageSource) -> bool {
    if matches!(source, RepoIconImageSource::Upload | RepoIconImageSource::File) {
        return is_data_png_base64(src);
    }
    let Some(url) = parse_url(src) else {
        return false;
    };
    if url.protocol != "https:" {
        return false;
    }
    match source {
        RepoIconImageSource::Github => url.hostname == "github.com" && is_github_png_path(&url.pathname),
        _ => url.hostname == "www.google.com" && url.pathname == "/s2/favicons",
    }
}

fn is_data_png_base64(src: &str) -> bool {
    const PREFIX: &str = "data:image/png;base64,";
    match src.get(..PREFIX.len()) {
        Some(prefix) if prefix.eq_ignore_ascii_case(PREFIX) => {
            let rest = &src[PREFIX.len()..];
            !rest.is_empty()
                && rest.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=') || c.is_whitespace())
        }
        _ => false,
    }
}

fn is_github_png_path(pathname: &str) -> bool {
    let Some(rest) = pathname.strip_prefix('/') else {
        return false;
    };
    if rest.contains(['/', '?', '#']) {
        return false;
    }
    matches!(rest.to_ascii_lowercase().strip_suffix(".png"), Some(name) if !name.is_empty())
}

struct ParsedUrl {
    protocol: String,
    hostname: String,
    pathname: String,
}

fn parse_url(input: &str) -> Option<ParsedUrl> {
    let colon = input.find(':')?;
    let scheme = &input[..colon];
    if scheme.is_empty() || !scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')) {
        return None;
    }
    let protocol = format!("{}:", scheme.to_ascii_lowercase());
    let after = &input[colon + 1..];
    let Some(rest) = after.strip_prefix("//") else {
        // Opaque URL (e.g. `javascript:alert(1)`): no host/path.
        return Some(ParsedUrl { protocol, hostname: String::new(), pathname: String::new() });
    };
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    let host = authority.rsplit('@').next().unwrap_or(authority);
    let hostname = host.split(':').next().unwrap_or(host).to_ascii_lowercase();
    let after_authority = &rest[authority_end..];
    let path_end = after_authority.find(['?', '#']).unwrap_or(after_authority.len());
    Some(ParsedUrl { protocol, hostname, pathname: after_authority[..path_end].to_string() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn icon(value: Value) -> RepoIconSanitizeResult {
        sanitize_repo_icon(Some(&value))
    }

    #[test]
    fn accepts_lucide_emoji_and_supported_image_icons() {
        assert_eq!(
            icon(json!({ "type": "lucide", "name": "Folder" })),
            RepoIconSanitizeResult::Icon(RepoIcon::Lucide { name: "Folder".to_string() })
        );
        assert_eq!(
            icon(json!({ "type": "emoji", "emoji": "🚀" })),
            RepoIconSanitizeResult::Icon(RepoIcon::Emoji { emoji: "🚀".to_string() })
        );
        assert_eq!(
            icon(json!({ "type": "image", "src": "https://github.com/stablyai.png?size=64", "source": "github", "label": "stablyai/orca" })),
            RepoIconSanitizeResult::Icon(RepoIcon::Image {
                src: "https://github.com/stablyai.png?size=64".to_string(),
                source: RepoIconImageSource::Github,
                label: Some("stablyai/orca".to_string()),
            })
        );
        assert_eq!(
            icon(json!({ "type": "image", "src": "https://www.google.com/s2/favicons?domain=example.com&sz=64", "source": "favicon" })),
            RepoIconSanitizeResult::Icon(RepoIcon::Image {
                src: "https://www.google.com/s2/favicons?domain=example.com&sz=64".to_string(),
                source: RepoIconImageSource::Favicon,
                label: None,
            })
        );
        for source in ["upload", "file"] {
            let expected_source = if source == "upload" { RepoIconImageSource::Upload } else { RepoIconImageSource::File };
            assert_eq!(
                icon(json!({ "type": "image", "src": "data:image/png;base64,aGVsbG8=", "source": source })),
                RepoIconSanitizeResult::Icon(RepoIcon::Image {
                    src: "data:image/png;base64,aGVsbG8=".to_string(),
                    source: expected_source,
                    label: None,
                })
            );
        }
    }

    #[test]
    fn keeps_null_as_an_explicit_reset() {
        assert_eq!(sanitize_repo_icon(Some(&Value::Null)), RepoIconSanitizeResult::Reset);
    }

    #[test]
    fn rejects_unsupported_image_urls_and_oversized_payloads() {
        assert_eq!(
            icon(json!({ "type": "image", "src": "javascript:alert(1)", "source": "favicon" })),
            RepoIconSanitizeResult::Undefined
        );
        let oversized = format!("data:image/png;base64,{}", "a".repeat(401 * 1024));
        assert_eq!(
            icon(json!({ "type": "image", "src": oversized, "source": "upload" })),
            RepoIconSanitizeResult::Undefined
        );
        assert_eq!(
            icon(json!({ "type": "image", "src": "data:image/svg+xml;base64,PHN2Zz48L3N2Zz4=", "source": "upload" })),
            RepoIconSanitizeResult::Undefined
        );
        assert_eq!(
            icon(json!({ "type": "image", "src": "https://example.com/icon.png", "source": "github" })),
            RepoIconSanitizeResult::Undefined
        );
    }

    #[test]
    fn builds_favicon_and_github_avatar_icons() {
        assert_eq!(
            favicon_url_from_website("example.com").as_deref(),
            Some("https://www.google.com/s2/favicons?domain=example.com&sz=64")
        );
        assert_eq!(favicon_url_from_website("   ").as_deref(), None);
        assert_eq!(favicon_url_from_website("ftp://example.com").as_deref(), None);
        assert_eq!(
            github_avatar_icon("stablyai", "orca"),
            RepoIcon::Image {
                src: "https://github.com/stablyai.png?size=64".to_string(),
                source: RepoIconImageSource::Github,
                label: Some("stablyai/orca".to_string()),
            }
        );
    }
}
