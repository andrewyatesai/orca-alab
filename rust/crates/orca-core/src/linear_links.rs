//! Linear deep-link builders, ported from `src/shared/linear-links.ts`.
//!
//! Builds `linear.app` team/settings URLs (percent-encoding path segments) and
//! extracts the workspace url-key from an issue URL. Pure: percent-encoding via
//! `crate::uri_component`, and a minimal host/first-path-segment parse in place
//! of `new URL`.

use crate::uri_component::encode_uri_component;

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
}
