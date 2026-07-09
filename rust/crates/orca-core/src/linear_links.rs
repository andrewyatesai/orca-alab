//! Linear deep-link builders, ported from `src/shared/linear-links.ts`.
//!
//! Builds `linear.app` team/settings URLs (percent-encoding path segments) and
//! extracts the workspace url-key from an issue URL. Pure: percent-encoding via
//! `crate::uri_component`, and a minimal host/first-path-segment parse in place
//! of `new URL`.

use crate::uri_component::{decode_uri_component, encode_uri_component};

/// `https://linear.app/<org>/team/<team>/all`, or `None` if either key is blank.
pub fn build_linear_team_url(organization_url_key: Option<&str>, team_key: Option<&str>) -> Option<String> {
    let organization_url_key = organization_url_key.map(str::trim).filter(|key| !key.is_empty())?;
    let team_key = team_key.map(str::trim).filter(|key| !key.is_empty())?;
    Some(format!(
        "https://linear.app/{}/team/{}/all",
        encode_uri_component(organization_url_key),
        encode_uri_component(team_key)
    ))
}

pub fn build_linear_personal_api_key_settings_url(organization_url_key: Option<&str>) -> String {
    match organization_url_key.map(str::trim).filter(|key| !key.is_empty()) {
        Some(key) => format!("https://linear.app/{}/settings/account/security", encode_uri_component(key)),
        None => "https://linear.app/settings/account/security".to_string(),
    }
}

pub fn build_linear_workspace_api_settings_url(organization_url_key: Option<&str>) -> String {
    match organization_url_key.map(str::trim).filter(|key| !key.is_empty()) {
        Some(key) => format!("https://linear.app/{}/settings/api", encode_uri_component(key)),
        None => "https://linear.app/settings/api".to_string(),
    }
}

/// The workspace url-key (first path segment) from a `linear.app` issue URL, or
/// `None` for a non-Linear host or an unparseable URL.
pub fn get_linear_organization_url_key_from_issue_url(issue_url: Option<&str>) -> Option<String> {
    let (scheme, rest) = issue_url?.split_once("://")?;
    if scheme.is_empty() {
        return None;
    }
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    let host = authority.rsplit('@').next().unwrap_or(authority);
    let hostname = host.split(':').next().unwrap_or(host);
    if !hostname.eq_ignore_ascii_case("linear.app") {
        return None;
    }
    let after_authority = &rest[authority_end..];
    let path_end = after_authority.find(['?', '#']).unwrap_or(after_authority.len());
    after_authority[..path_end].split('/').find(|segment| !segment.is_empty()).map(str::to_string)
}

/// Parsed Linear issue input: the canonical identifier plus the workspace
/// url-key when the input was a full issue URL. Mirrors `ParsedLinearIssueInput`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedLinearIssueInput {
    pub identifier: String,
    pub organization_url_key: Option<String>,
}

/// Hand-rolled `^[A-Za-z][A-Za-z0-9_]*-\d+$` (no regex crate). Since the prefix
/// class excludes `-` and the suffix is all digits, a valid match has exactly one
/// `-` — the first one splits prefix from the digit run.
fn matches_linear_identifier_pattern(value: &str) -> bool {
    let Some(dash) = value.find('-') else {
        return false;
    };
    let (prefix, suffix) = (&value[..dash], &value[dash + 1..]);
    if suffix.is_empty() || !suffix.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    let mut bytes = prefix.bytes();
    match bytes.next() {
        Some(first) if first.is_ascii_alphabetic() => {}
        _ => return false,
    }
    bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// Minimal `new URL` stand-in: the (case-insensitive) hostname and the non-empty
/// path segments, or `None` when the input isn't an absolute URL (the TS
/// `new URL` throw path). Query/hash are excluded, matching `URL.pathname`.
fn parse_absolute_url(input: &str) -> Option<(String, Vec<String>)> {
    let (scheme, rest) = input.split_once("://")?;
    if scheme.is_empty() {
        return None;
    }
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    let host = authority.rsplit('@').next().unwrap_or(authority);
    let hostname = host.split(':').next().unwrap_or(host);
    let after_authority = &rest[authority_end..];
    let path_end = after_authority.find(['?', '#']).unwrap_or(after_authority.len());
    let segments = after_authority[..path_end]
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(str::to_string)
        .collect();
    Some((hostname.to_string(), segments))
}

/// Parse a bare Linear identifier (`ENG-123`) or a `linear.app` issue URL into a
/// canonical (uppercased) identifier plus, for URLs, the workspace url-key.
/// Returns `None` for blank/invalid input or a non-Linear URL.
pub fn parse_linear_issue_input(input: &str) -> Option<ParsedLinearIssueInput> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    if matches_linear_identifier_pattern(trimmed) {
        return Some(ParsedLinearIssueInput {
            identifier: trimmed.to_uppercase(),
            organization_url_key: None,
        });
    }
    let (hostname, segments) = parse_absolute_url(trimmed)?;
    if !hostname.eq_ignore_ascii_case("linear.app") {
        return None;
    }
    let organization_url_key = segments.first()?;
    let raw_identifier = segments
        .iter()
        .position(|segment| segment.as_str() == "issue")
        .and_then(|issue_index| segments.get(issue_index + 1))?;
    // Decode then take up to the first `/ ? #`, matching `split(/[/?#]/)[0]`.
    let decoded = decode_uri_component(raw_identifier);
    let identifier = decoded.split(['/', '?', '#']).next().unwrap_or("");
    if !matches_linear_identifier_pattern(identifier) {
        return None;
    }
    Some(ParsedLinearIssueInput {
        identifier: identifier.to_uppercase(),
        organization_url_key: Some(decode_uri_component(organization_url_key)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_team_urls_from_workspace_and_team_keys() {
        assert_eq!(
            build_linear_team_url(Some("acme"), Some("ENG")).unwrap(),
            "https://linear.app/acme/team/ENG/all"
        );
    }

    #[test]
    fn encodes_url_path_segments() {
        assert_eq!(
            build_linear_team_url(Some("acme inc"), Some("A/B")).unwrap(),
            "https://linear.app/acme%20inc/team/A%2FB/all"
        );
    }

    #[test]
    fn extracts_the_workspace_url_key_from_linear_issue_urls() {
        assert_eq!(
            get_linear_organization_url_key_from_issue_url(Some("https://linear.app/acme/issue/ENG-1")),
            Some("acme".to_string())
        );
    }

    #[test]
    fn builds_organization_scoped_api_key_settings_urls() {
        assert_eq!(
            build_linear_personal_api_key_settings_url(Some("acme inc")),
            "https://linear.app/acme%20inc/settings/account/security"
        );
        assert_eq!(
            build_linear_workspace_api_settings_url(Some("acme/inc")),
            "https://linear.app/acme%2Finc/settings/api"
        );
    }

    #[test]
    fn falls_back_to_global_api_settings_urls_when_no_organization_slug() {
        assert_eq!(
            build_linear_personal_api_key_settings_url(None),
            "https://linear.app/settings/account/security"
        );
        assert_eq!(build_linear_workspace_api_settings_url(Some("   ")), "https://linear.app/settings/api");
    }

    #[test]
    fn parses_bare_linear_issue_identifiers() {
        assert_eq!(
            parse_linear_issue_input("eng-123"),
            Some(ParsedLinearIssueInput { identifier: "ENG-123".to_string(), organization_url_key: None })
        );
        // Underscores are allowed in the prefix.
        assert_eq!(
            parse_linear_issue_input("ENG_TEAM-45"),
            Some(ParsedLinearIssueInput { identifier: "ENG_TEAM-45".to_string(), organization_url_key: None })
        );
    }

    #[test]
    fn parses_linear_issue_urls_with_organization_keys() {
        assert_eq!(
            parse_linear_issue_input("https://linear.app/acme/issue/eng-123/fix-auth"),
            Some(ParsedLinearIssueInput {
                identifier: "ENG-123".to_string(),
                organization_url_key: Some("acme".to_string()),
            })
        );
        assert_eq!(
            parse_linear_issue_input("https://linear.app/stably/issue/STA-335/test-issue"),
            Some(ParsedLinearIssueInput {
                identifier: "STA-335".to_string(),
                organization_url_key: Some("stably".to_string()),
            })
        );
        // The org url-key is percent-decoded.
        assert_eq!(
            parse_linear_issue_input("https://linear.app/acme%20inc/issue/ENG-9"),
            Some(ParsedLinearIssueInput {
                identifier: "ENG-9".to_string(),
                organization_url_key: Some("acme inc".to_string()),
            })
        );
    }

    #[test]
    fn rejects_non_linear_and_invalid_issue_input() {
        assert_eq!(parse_linear_issue_input("https://example.com/acme/issue/ENG-123"), None);
        assert_eq!(parse_linear_issue_input("not an issue"), None);
        assert_eq!(parse_linear_issue_input(""), None);
        // Bare-looking but invalid identifiers aren't URLs either.
        assert_eq!(parse_linear_issue_input("ENG-"), None);
        assert_eq!(parse_linear_issue_input("ENG-1-2"), None);
    }
}
