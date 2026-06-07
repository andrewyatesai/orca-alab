//! Skill markdown summarization, ported from `src/shared/skill-metadata.ts`.
//!
//! Extracts `{name, description}` from a skill's markdown: prefer the YAML
//! frontmatter (a minimal parser supporting scalars, quoted values, `-` lists,
//! and `|`/`>` block scalars), else fall back to the first `# heading` and first
//! paragraph of the body. Regex-backed (no YAML crate).

use regex::Regex;
use std::collections::HashMap;
use std::sync::OnceLock;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SkillFrontmatterSummary {
    pub name: Option<String>,
    pub description: Option<String>,
}

fn strip_quote_pair(value: &str) -> String {
    let trimmed = value.trim();
    let first = trimmed.chars().next();
    let last = trimmed.chars().next_back();
    if trimmed.chars().count() >= 2
        && ((first == Some('"') && last == Some('"')) || (first == Some('\'') && last == Some('\'')))
    {
        let mut chars = trimmed.chars();
        chars.next();
        chars.next_back();
        return chars.as_str().to_string();
    }
    trimmed.to_string()
}

/// Parse frontmatter to scalar key→value pairs. Block scalars (`|`/`>`) collapse
/// to a single string. List-valued keys (`key:` + `- item` lines) are consumed
/// (so their items aren't misread as keys) and recorded as an empty value —
/// `name`/`description` are always scalars, so a list there falls back exactly
/// as the TS does when it sees an array instead of a string.
fn parse_yaml_frontmatter(raw: &str) -> HashMap<String, String> {
    let normalized = raw.replace("\r\n", "\n");
    let lines: Vec<&str> = normalized.split('\n').collect();
    let mut data: HashMap<String, String> = HashMap::new();
    let mut index = 0;
    while index < lines.len() {
        let Some(captures) = key_re().captures(lines[index]) else {
            index += 1;
            continue;
        };
        let key = captures.get(1).map_or("", |m| m.as_str()).to_string();
        let value = captures.get(2).map_or("", |m| m.as_str()).trim().to_string();

        if matches!(value.as_str(), "|" | "|-" | ">" | ">-") {
            let mut block: Vec<String> = Vec::new();
            index += 1;
            while index < lines.len() && block_continuation_re().is_match(lines[index]) {
                block.push(block_indent_re().replace(lines[index], "").into_owned());
                index += 1;
            }
            let joined = block.join(if value.starts_with('>') { " " } else { "\n" });
            data.insert(key, whitespace_re().replace_all(&joined, " ").trim().to_string());
            continue;
        }

        if value.is_empty() {
            index += 1;
            while index < lines.len() && list_item_re().is_match(lines[index]) {
                index += 1;
            }
            data.insert(key, String::new());
            continue;
        }

        data.insert(key, strip_quote_pair(&value));
        index += 1;
    }
    data
}

fn first_heading(body: &str) -> Option<String> {
    heading_re()
        .captures(body)
        .and_then(|captures| captures.get(1))
        .map(|m| m.as_str().trim().to_string())
        .filter(|heading| !heading.is_empty())
}

fn first_paragraph(body: &str) -> Option<String> {
    let normalized = body.replace("\r\n", "\n");
    let mut paragraph: Vec<&str> = Vec::new();
    for line in normalized.split('\n') {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("```") {
            if !paragraph.is_empty() {
                break;
            }
            continue;
        }
        paragraph.push(trimmed);
        if paragraph.join(" ").len() > 240 {
            break;
        }
    }
    if paragraph.is_empty() {
        None
    } else {
        Some(paragraph.join(" "))
    }
}

fn nonempty_scalar(value: Option<&String>) -> Option<String> {
    value.map(|text| text.trim()).filter(|text| !text.is_empty()).map(str::to_string)
}

pub fn summarize_skill_markdown(markdown: &str) -> SkillFrontmatterSummary {
    let normalized = markdown.strip_prefix('\u{feff}').unwrap_or(markdown);
    let (body, frontmatter): (&str, HashMap<String, String>) = match frontmatter_re().captures(normalized) {
        Some(captures) => {
            let end = captures.get(0).map_or(0, |m| m.end());
            (&normalized[end..], parse_yaml_frontmatter(captures.get(1).map_or("", |m| m.as_str())))
        }
        None => (normalized, HashMap::new()),
    };

    let name = nonempty_scalar(frontmatter.get("name")).or_else(|| first_heading(body));
    let description = nonempty_scalar(frontmatter.get("description")).or_else(|| first_paragraph(body));

    SkillFrontmatterSummary {
        name: name.filter(|value| !value.is_empty()),
        description: description.filter(|value| !value.is_empty()),
    }
}

fn key_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^([A-Za-z0-9_-]+):\s*(.*)$").unwrap())
}

fn block_continuation_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(?:\s{2,}|\s*$)").unwrap())
}

fn block_indent_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\s{2}").unwrap())
}

fn list_item_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\s*-\s*(.+)$").unwrap())
}

fn whitespace_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\s+").unwrap())
}

fn heading_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?m)^#\s+(.+)$").unwrap())
}

fn frontmatter_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?s)^---\s*\n(.*?)\n---\s*(?:\n|$)").unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_name_and_folded_description_from_yaml_frontmatter() {
        let summary = summarize_skill_markdown(
            "---\nname: orca-cli\ndescription: >-\n  Use the orca CLI to drive a running editor;\n  keep worktree comments current.\n---\n\n# Orca CLI\n",
        );
        assert_eq!(
            summary,
            SkillFrontmatterSummary {
                name: Some("orca-cli".to_string()),
                description: Some(
                    "Use the orca CLI to drive a running editor; keep worktree comments current.".to_string()
                ),
            }
        );
    }

    #[test]
    fn falls_back_to_heading_and_first_paragraph_when_frontmatter_is_absent() {
        let summary =
            summarize_skill_markdown("# Design Review\n\nUse when reviewing UI implementation quality.\n");
        assert_eq!(
            summary,
            SkillFrontmatterSummary {
                name: Some("Design Review".to_string()),
                description: Some("Use when reviewing UI implementation quality.".to_string()),
            }
        );
    }
}
