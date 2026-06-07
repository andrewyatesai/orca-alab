//! Pairing-offer encode/decode, ported from `src/shared/pairing.ts`.
//!
//! The desktop emits an `orca://pair?code=<base64url>` deep link carrying a
//! versioned offer (endpoint + device token + the desktop's Curve25519 public
//! key, base64). The mobile client scans/pastes it to bootstrap the encrypted
//! session. JSON rides the vendored `serde_json`; base64url and the minimal
//! `orca://` URL parse are hand-rolled (no `zod`/`url` crate). Schema validation
//! mirrors the original zod schema: `v` literal 2, the three fields non-empty.

use serde_json::Value;

pub const PAIRING_OFFER_VERSION: u32 = 2;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PairingOffer {
    /// Always [`PAIRING_OFFER_VERSION`]; decode rejects any other value.
    pub v: u32,
    pub endpoint: String,
    pub device_token: String,
    pub public_key_b64: String,
}

pub fn encode_pairing_offer(offer: &PairingOffer) -> String {
    let value = serde_json::json!({
        "v": offer.v,
        "endpoint": offer.endpoint,
        "deviceToken": offer.device_token,
        "publicKeyB64": offer.public_key_b64,
    });
    let json = serde_json::to_string(&value).unwrap_or_default();
    // Query param, not fragment: Android camera intents / Expo Router preserve
    // query params more reliably than URL fragments on custom-scheme launches.
    format!("orca://pair?code={}", crate::base64::encode_url_safe_no_pad(json.as_bytes()))
}

/// Decode an `orca://pair` deep link. `Err` carries an "Invalid pairing URL"
/// message for a bad/foreign URL, or a payload error for a malformed offer.
pub fn decode_pairing_offer(url: &str) -> Result<PairingOffer, String> {
    let code = extract_pairing_code_from_url(url).ok_or_else(|| {
        "Invalid pairing URL: must start with orca://pair and include a pairing code".to_string()
    })?;
    decode_pairing_base64(&code).ok_or_else(|| "Invalid pairing offer payload".to_string())
}

/// Accept either an `orca://pair?...` URL or a bare base64url payload (the
/// mobile paste-pair flow takes whichever the user copied). `None` on any
/// failure — no error surfaced.
pub fn parse_pairing_code(input: &str) -> Option<PairingOffer> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.to_ascii_lowercase().starts_with("orca://") {
        decode_pairing_offer(trimmed).ok()
    } else {
        decode_pairing_base64(trimmed)
    }
}

fn extract_pairing_code_from_url(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    // Only the `orca://pair` deep-link host may carry runtime auth material;
    // prefix-style routes like `orca://pairing` must be rejected.
    if !scheme.eq_ignore_ascii_case("orca") {
        return None;
    }
    let (before_fragment, fragment) = match rest.split_once('#') {
        Some((before, frag)) => (before, Some(frag)),
        None => (rest, None),
    };
    let (authority_path, query) = match before_fragment.split_once('?') {
        Some((before, q)) => (before, Some(q)),
        None => (before_fragment, None),
    };
    let (authority, path) = match authority_path.split_once('/') {
        Some((auth, rest)) => (auth, format!("/{rest}")),
        None => (authority_path, String::new()),
    };
    let host = authority.rsplit('@').next().unwrap_or(authority);
    let hostname = host.split(':').next().unwrap_or(host);
    if !hostname.eq_ignore_ascii_case("pair") {
        return None;
    }
    if !path.is_empty() && path != "/" {
        return None;
    }
    if let Some(code) = query.and_then(|q| query_param(q, "code")) {
        if !code.is_empty() {
            return Some(code);
        }
    }
    // Legacy fallback: the code in the URL fragment.
    fragment.filter(|frag| !frag.is_empty()).map(str::to_string)
}

fn query_param(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        (name == key).then(|| value.to_string())
    })
}

fn decode_pairing_base64(base64url: &str) -> Option<PairingOffer> {
    let bytes = crate::base64::decode(base64url)?;
    let json = String::from_utf8(bytes).ok()?;
    let value: Value = serde_json::from_str(&json).ok()?;
    parse_offer_schema(&value)
}

fn parse_offer_schema(value: &Value) -> Option<PairingOffer> {
    let object = value.as_object()?;
    if object.get("v")?.as_u64()? != u64::from(PAIRING_OFFER_VERSION) {
        return None;
    }
    Some(PairingOffer {
        v: PAIRING_OFFER_VERSION,
        endpoint: non_empty_string(object.get("endpoint"))?,
        device_token: non_empty_string(object.get("deviceToken"))?,
        public_key_b64: non_empty_string(object.get("publicKeyB64"))?,
    })
}

fn non_empty_string(value: Option<&Value>) -> Option<String> {
    let text = value?.as_str()?;
    (!text.is_empty()).then(|| text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offer() -> PairingOffer {
        PairingOffer {
            v: 2,
            endpoint: "ws://192.168.1.10:6768".to_string(),
            device_token: "abcdef1234567890abcdef1234567890abcdef1234567890".to_string(),
            public_key_b64: "dGVzdC1wdWJsaWMta2V5LWJhc2U2NC1lbmNvZGVk".to_string(),
        }
    }

    fn code_of(url: &str) -> String {
        url.strip_prefix("orca://pair?code=").unwrap().to_string()
    }

    fn fragment_url(json: &str) -> String {
        format!("orca://pair#{}", crate::base64::encode_url_safe_no_pad(json.as_bytes()))
    }

    #[test]
    fn encode_then_decode_round_trips_correctly() {
        let url = encode_pairing_offer(&offer());
        assert!(url.starts_with("orca://pair?code="));
        assert_eq!(decode_pairing_offer(&url).unwrap(), offer());
    }

    #[test]
    fn encoded_url_uses_base64url_no_plus_slash_or_equals() {
        let code = code_of(&encode_pairing_offer(&offer()));
        assert!(!code.contains('+') && !code.contains('/') && !code.contains('='));
    }

    #[test]
    fn rejects_urls_with_wrong_scheme() {
        let error = decode_pairing_offer("https://example.com#abc").unwrap_err();
        assert!(error.contains("Invalid pairing URL"));
    }

    #[test]
    fn rejects_orca_urls_outside_the_exact_pairing_route() {
        let code = code_of(&encode_pairing_offer(&offer()));
        assert_eq!(parse_pairing_code(&format!("orca://pairing?code={code}")), None);
        assert_eq!(parse_pairing_code(&format!("orca://pair-extra?code={code}")), None);
        assert!(decode_pairing_offer(&format!("orca://pairing?code={code}"))
            .unwrap_err()
            .contains("Invalid pairing URL"));
    }

    #[test]
    fn rejects_urls_without_a_pairing_code() {
        assert!(decode_pairing_offer("orca://pair").unwrap_err().contains("Invalid pairing URL"));
    }

    #[test]
    fn decodes_legacy_hash_urls() {
        let code = code_of(&encode_pairing_offer(&offer()));
        assert_eq!(decode_pairing_offer(&format!("orca://pair#{code}")).unwrap(), offer());
    }

    #[test]
    fn rejects_payloads_with_missing_fields() {
        let url = fragment_url(r#"{"v":2,"endpoint":"ws://host:1234"}"#);
        assert!(decode_pairing_offer(&url).is_err());
    }

    #[test]
    fn rejects_payloads_with_wrong_version() {
        let url = fragment_url(
            r#"{"v":1,"endpoint":"ws://host:1234","deviceToken":"tok","publicKeyB64":"k"}"#,
        );
        assert!(decode_pairing_offer(&url).is_err());
    }

    #[test]
    fn rejects_payloads_with_missing_public_key_b64() {
        let url = fragment_url(r#"{"v":2,"endpoint":"ws://host:1234","deviceToken":"tok"}"#);
        assert!(decode_pairing_offer(&url).is_err());
    }

    // --- parse_pairing_code ---

    fn paste_offer() -> PairingOffer {
        PairingOffer {
            v: 2,
            endpoint: "ws://192.168.1.10:6768".to_string(),
            device_token: "token-abc".to_string(),
            public_key_b64: "pubkey-xyz".to_string(),
        }
    }

    #[test]
    fn parses_a_full_orca_pair_url() {
        let url = encode_pairing_offer(&paste_offer());
        assert_eq!(parse_pairing_code(&url), Some(paste_offer()));
    }

    #[test]
    fn parses_a_bare_base64url_payload_without_scheme_prefix() {
        let code = code_of(&encode_pairing_offer(&paste_offer()));
        assert_eq!(parse_pairing_code(&code), Some(paste_offer()));
    }

    #[test]
    fn tolerates_surrounding_whitespace_from_clipboard() {
        let url = encode_pairing_offer(&paste_offer());
        assert_eq!(parse_pairing_code(&format!("  {url}\n")), Some(paste_offer()));
    }

    #[test]
    fn returns_none_for_empty_input() {
        assert_eq!(parse_pairing_code(""), None);
        assert_eq!(parse_pairing_code("   "), None);
    }

    #[test]
    fn returns_none_for_garbage_input() {
        assert_eq!(parse_pairing_code("not a pairing code"), None);
        assert_eq!(parse_pairing_code("https://example.com"), None);
    }

    #[test]
    fn returns_none_for_valid_base64_of_unrelated_json() {
        let bogus = crate::base64::encode_url_safe_no_pad(br#"{"hello":"world"}"#);
        assert_eq!(parse_pairing_code(&bogus), None);
    }
}
