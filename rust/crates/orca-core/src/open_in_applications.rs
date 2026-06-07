//! "Open in application" list normalisation, ported from
//! `src/shared/open-in-applications.ts`.
//!
//! Trims fields, drops invalid rows, keeps the first occurrence of a duplicate
//! id, generates ids for blank ones, and caps the list length.

use std::collections::HashSet;

pub const OPEN_IN_APPLICATIONS_MAX: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenInApplication {
    pub id: String,
    pub label: String,
    pub command: String,
}

/// A persisted/raw row before validation — every field optional, mirroring the
/// `unknown`-typed input the TS guards against.
#[derive(Clone, Debug, Default)]
pub struct RawOpenInApplication {
    pub id: Option<String>,
    pub label: Option<String>,
    pub command: Option<String>,
}

pub fn default_open_in_applications() -> Vec<OpenInApplication> {
    vec![OpenInApplication {
        id: "vscode".to_string(),
        label: "VS Code".to_string(),
        command: "code".to_string(),
    }]
}

fn trim_token(value: &Option<String>) -> String {
    value.as_deref().map(str::trim).unwrap_or("").to_string()
}

fn make_fallback_id(index: usize) -> String {
    format!("open-in-{}", index + 1)
}

/// `value` is `None` when the persisted field is absent (not an array);
/// `create_id` mirrors the optional id generator (return `None` to fall back to
/// a positional id); `seed_defaults` seeds the default list only when absent.
pub fn normalize_open_in_applications<F>(
    value: Option<&[RawOpenInApplication]>,
    mut create_id: F,
    seed_defaults: bool,
) -> Vec<OpenInApplication>
where
    F: FnMut() -> Option<String>,
{
    let Some(rows) = value else {
        return if seed_defaults {
            default_open_in_applications()
        } else {
            Vec::new()
        };
    };

    let mut normalized: Vec<OpenInApplication> = Vec::new();
    let mut seen_ids: HashSet<String> = HashSet::new();

    for (index, row) in rows.iter().enumerate() {
        if normalized.len() >= OPEN_IN_APPLICATIONS_MAX {
            break;
        }
        let label = trim_token(&row.label);
        let command = trim_token(&row.command);
        if label.is_empty() || command.is_empty() {
            continue;
        }

        let mut id = trim_token(&row.id);
        if id.is_empty() {
            id = create_id()
                .as_deref()
                .map(str::trim)
                .unwrap_or("")
                .to_string();
            if id.is_empty() {
                id = make_fallback_id(index);
            }
        }

        if seen_ids.contains(&id) {
            continue;
        }
        seen_ids.insert(id.clone());
        normalized.push(OpenInApplication { id, label, command });
    }

    normalized
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(id: Option<&str>, label: &str, command: &str) -> RawOpenInApplication {
        RawOpenInApplication {
            id: id.map(str::to_string),
            label: Some(label.to_string()),
            command: Some(command.to_string()),
        }
    }

    fn app(id: &str, label: &str, command: &str) -> OpenInApplication {
        OpenInApplication {
            id: id.to_string(),
            label: label.to_string(),
            command: command.to_string(),
        }
    }

    #[test]
    fn trims_drops_invalid_keeps_first_duplicate_and_caps() {
        let rows = [
            raw(Some("a"), " Cursor ", " cursor "),
            raw(Some("a"), "Dup", "dup"),
            raw(Some("b"), "   ", "zed"),
            raw(Some("c"), "Zed", "   "),
            raw(Some("d"), "D", "d"),
            raw(Some("e"), "E", "e"),
            raw(Some("f"), "F", "f"),
            raw(Some("g"), "G", "g"),
            raw(Some("h"), "H", "h"),
            raw(Some("i"), "I", "i"),
            raw(Some("j"), "J", "j"),
        ];
        let out = normalize_open_in_applications(Some(&rows), || None, false);
        assert_eq!(
            out,
            vec![
                app("a", "Cursor", "cursor"),
                app("d", "D", "d"),
                app("e", "E", "e"),
                app("f", "F", "f"),
                app("g", "G", "g"),
                app("h", "H", "h"),
                app("i", "I", "i"),
                app("j", "J", "j"),
            ]
        );
    }

    #[test]
    fn generates_ids_for_missing_or_blank_ids() {
        let rows = [raw(None, "Cursor", "cursor"), raw(Some("   "), "Zed", "zed")];
        let mut counter = 0;
        let out = normalize_open_in_applications(
            Some(&rows),
            || {
                counter += 1;
                Some(format!("gen-{counter}"))
            },
            false,
        );
        assert_eq!(
            out,
            vec![app("gen-1", "Cursor", "cursor"), app("gen-2", "Zed", "zed")]
        );
    }

    #[test]
    fn seeds_defaults_only_when_field_missing() {
        assert_eq!(
            normalize_open_in_applications(None, || None, true),
            default_open_in_applications()
        );
        assert_eq!(
            normalize_open_in_applications(Some(&[]), || None, true),
            Vec::<OpenInApplication>::new()
        );
    }
}
