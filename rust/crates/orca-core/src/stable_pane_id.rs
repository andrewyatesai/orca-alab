//! Stable pane id + pane-key handling, ported from `src/shared/stable-pane-id.ts`.
//!
//! A pane key (`<tabId>:<leafUuid>`) crosses renderer reloads, PTY env, hook
//! IPC, and retained UI rows, so it keys on the durable terminal-layout leaf
//! UUID, never the renderer-local numeric pane id. Validation is strict so a
//! legacy numeric key can't masquerade as a stable one.

/// `^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$`
/// (lowercase only — uppercase UUIDs are rejected, matching the un-flagged regex).
pub fn is_stable_pane_id(value: &str) -> bool {
    let b = value.as_bytes();
    if b.len() != 36 {
        return false;
    }
    if b[8] != b'-' || b[13] != b'-' || b[18] != b'-' || b[23] != b'-' {
        return false;
    }
    let hex = |c: u8| c.is_ascii_digit() || (b'a'..=b'f').contains(&c);
    let hex_run = |range: std::ops::Range<usize>| range.clone().all(|i| hex(b[i]));
    hex_run(0..8)
        && hex_run(9..13)
        && (b'1'..=b'5').contains(&b[14])
        && hex_run(15..18)
        && matches!(b[19], b'8' | b'9' | b'a' | b'b')
        && hex_run(20..23)
        && hex_run(24..36)
}

pub fn is_terminal_leaf_id(value: &str) -> bool {
    is_stable_pane_id(value)
}

pub fn make_pane_key(tab_id: &str, stable_leaf_id: &str) -> Result<String, String> {
    if tab_id.is_empty() || tab_id.contains(':') {
        return Err("tabId must be non-empty and must not contain \":\"".to_string());
    }
    if !is_terminal_leaf_id(stable_leaf_id) {
        return Err("stableLeafId must be a UUID".to_string());
    }
    Ok(format!("{tab_id}:{stable_leaf_id}"))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedPaneKey {
    pub tab_id: String,
    pub leaf_id: String,
}

/// Splits a single-colon `<tabId>:<uuid>` key, validating the UUID.
pub fn parse_pane_key(pane_key: &str) -> Option<ParsedPaneKey> {
    let first = pane_key.find(':')?;
    if first == 0 || pane_key.rfind(':') != Some(first) || first == pane_key.len() - 1 {
        return None;
    }
    let tab_id = &pane_key[..first];
    let leaf_id = &pane_key[first + 1..];
    if !is_terminal_leaf_id(leaf_id) {
        return None;
    }
    Some(ParsedPaneKey {
        tab_id: tab_id.to_string(),
        leaf_id: leaf_id.to_string(),
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LegacyNumericPaneKey {
    pub tab_id: String,
    pub numeric_pane_id: String,
    pub pane_key: String,
}

/// Parses a legacy `<tabId>:<numeric>` key (migration aliases only).
pub fn parse_legacy_numeric_pane_key(pane_key: &str) -> Option<LegacyNumericPaneKey> {
    if pane_key.len() > 256 {
        return None;
    }
    let trimmed = pane_key.trim();
    let delimiter = trimmed.find(':')?;
    if delimiter == 0 || trimmed.rfind(':') != Some(delimiter) || delimiter == trimmed.len() - 1 {
        return None;
    }
    let numeric = &trimmed[delimiter + 1..];
    if numeric.is_empty() || !numeric.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(LegacyNumericPaneKey {
        tab_id: trimmed[..delimiter].to_string(),
        numeric_pane_id: numeric.to_string(),
        pane_key: trimmed.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEAF_ID: &str = "11111111-1111-4111-8111-111111111111";

    #[test]
    fn recognizes_uuid_leaf_ids_as_stable_pane_ids() {
        assert!(is_stable_pane_id(LEAF_ID));
        assert!(is_terminal_leaf_id(LEAF_ID));
    }

    #[test]
    fn rejects_legacy_numeric_ids_and_malformed_uuids() {
        for value in ["1", "pane:1", "11111111-1111-6111-8111-111111111111", ""] {
            assert!(!is_stable_pane_id(value), "{value}");
            assert!(!is_terminal_leaf_id(value), "{value}");
        }
    }

    #[test]
    fn builds_and_parses_pane_keys() {
        let pane_key = make_pane_key("tab-1", LEAF_ID).unwrap();
        assert_eq!(pane_key, format!("tab-1:{LEAF_ID}"));
        assert_eq!(
            parse_pane_key(&pane_key),
            Some(ParsedPaneKey {
                tab_id: "tab-1".to_string(),
                leaf_id: LEAF_ID.to_string(),
            })
        );
    }

    #[test]
    fn rejects_ambiguous_tab_ids_and_non_uuid_leaf_ids_when_building() {
        assert!(make_pane_key("", LEAF_ID).unwrap_err().contains("tabId"));
        assert!(make_pane_key("tab:1", LEAF_ID).unwrap_err().contains("tabId"));
        assert!(make_pane_key("tab-1", "1").unwrap_err().contains("UUID"));
    }

    #[test]
    fn rejects_ambiguous_or_legacy_inputs_when_parsing() {
        assert_eq!(parse_pane_key("tab-1:1"), None);
        assert_eq!(parse_pane_key(&format!("tab:1:{LEAF_ID}")), None);
        assert_eq!(parse_pane_key(&format!(":{LEAF_ID}")), None);
        assert_eq!(parse_pane_key("tab-1:"), None);
    }

    #[test]
    fn parses_legacy_numeric_pane_keys_only_for_migration_aliases() {
        assert_eq!(
            parse_legacy_numeric_pane_key(" tab-1:12 "),
            Some(LegacyNumericPaneKey {
                tab_id: "tab-1".to_string(),
                numeric_pane_id: "12".to_string(),
                pane_key: "tab-1:12".to_string(),
            })
        );
        assert_eq!(parse_legacy_numeric_pane_key(&format!("tab-1:{LEAF_ID}")), None);
        assert_eq!(parse_legacy_numeric_pane_key("tab:1:12"), None);
    }
}
