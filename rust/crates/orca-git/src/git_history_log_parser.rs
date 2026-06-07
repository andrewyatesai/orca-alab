//! Pure parser for `git log` records + ref decorations, ported from
//! `src/shared/git-history-log-parser.ts`. No IO: it turns the NUL-delimited
//! `git log` stdout (in the `GIT_HISTORY_COMMIT_FORMAT` layout) into
//! [`GitHistoryItem`]s, and decodes ref decorations into [`GitHistoryItemRef`]s.

use std::cmp::Ordering;

use crate::git_history_types::{GitHistoryItem, GitHistoryItemRef, GitHistoryRefCategory};

/// Unit separator (0x1F) — Git ref names can contain commas, so the log format
/// asks Git to join decorations with this control char instead.
const GIT_HISTORY_DECORATION_SEPARATOR: char = '\u{1f}';

/// The `git log --format` template. `%x1f` is Git's hex-byte escape for the
/// separator above; the literal text is what Git receives.
pub const GIT_HISTORY_COMMIT_FORMAT: &str =
    "%H%n%aN%n%aE%n%at%n%ct%n%P%n%(decorate:prefix=,suffix=,separator=%x1f)%n%B";

/// First 7 UTF-16 code units of a hash, matching the TS `hash.slice(0, 7)`.
#[cfg_attr(trust_verify, trust::ensures(|out: &String| out.encode_utf16().count() <= 7))]
pub fn short_git_hash(hash: &str) -> String {
    let units: Vec<u16> = hash.encode_utf16().take(7).collect();
    String::from_utf16_lossy(&units)
}

fn commit_subject(message: &str) -> String {
    // Why: TS `split(/\r?\n/, 1)[0].trim()` takes the first line; `.trim()` also
    // drops the trailing `\r` from a CRLF break.
    let first_line = message.split('\n').next().unwrap_or("").trim();
    if first_line.is_empty() {
        "(no commit message)".to_string()
    } else {
        first_line.to_string()
    }
}

/// Mirror of JS `Number.parseInt(value, 10)` + `Number.isFinite`: optional
/// leading whitespace, optional sign, then base-10 digits. `None` == `NaN`.
fn js_parse_int_base10(value: &str) -> Option<i64> {
    let bytes = value.trim_start().as_bytes();
    let mut index = 0;
    let mut sign: i64 = 1;
    if let Some(&first) = bytes.first() {
        if first == b'+' || first == b'-' {
            if first == b'-' {
                sign = -1;
            }
            index = 1;
        }
    }
    let digits_start = index;
    let mut magnitude: i64 = 0;
    while let Some(&byte) = bytes.get(index) {
        if !byte.is_ascii_digit() {
            break;
        }
        magnitude = magnitude.saturating_mul(10).saturating_add(i64::from(byte - b'0'));
        index += 1;
    }
    if index == digits_start {
        return None;
    }
    Some(sign.saturating_mul(magnitude))
}

fn is_commit_hash(hash: &str) -> bool {
    // TS regex `^[0-9a-fA-F]{40,64}$` — char count, all hex.
    let len = hash.chars().count();
    (40..=64).contains(&len) && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// True for `refs/remotes/<remote>/HEAD` followed by whitespace or end — the
/// symbolic remote-HEAD pointer, which is dropped (TS `/^refs\/remotes\/[^/]+\/HEAD(?:\s|$)/`).
fn is_remote_head(ref_str: &str) -> bool {
    let Some(rest) = ref_str.strip_prefix("refs/remotes/") else {
        return false;
    };
    let Some(slash) = rest.find('/') else {
        return false;
    };
    if slash == 0 {
        return false; // `[^/]+` needs at least one char
    }
    let after = &rest[slash + 1..];
    match after.strip_prefix("HEAD") {
        Some(tail) => tail.is_empty() || tail.chars().next().is_some_and(char::is_whitespace),
        None => false,
    }
}

fn parse_git_decoration_refs(raw: &str, revision: &str) -> Vec<GitHistoryItemRef> {
    if raw.trim().is_empty() {
        return Vec::new();
    }

    let parts: Vec<&str> = if raw.contains(GIT_HISTORY_DECORATION_SEPARATOR) {
        raw.split(GIT_HISTORY_DECORATION_SEPARATOR).collect()
    } else {
        raw.split(',').collect()
    };

    let mut refs: Vec<GitHistoryItemRef> = Vec::new();
    for part in parts {
        let ref_str = part.trim();
        if ref_str.is_empty() || ref_str == "HEAD" || is_remote_head(ref_str) {
            continue;
        }

        if let Some(without_head) = ref_str.strip_prefix("HEAD -> refs/heads/") {
            refs.push(GitHistoryItemRef {
                id: ref_str["HEAD -> ".len()..].to_string(),
                name: without_head.to_string(),
                revision: Some(revision.to_string()),
                category: Some(GitHistoryRefCategory::Branches),
                ..Default::default()
            });
            continue;
        }

        if let Some(name) = ref_str.strip_prefix("refs/heads/") {
            refs.push(GitHistoryItemRef {
                id: ref_str.to_string(),
                name: name.to_string(),
                revision: Some(revision.to_string()),
                category: Some(GitHistoryRefCategory::Branches),
                ..Default::default()
            });
            continue;
        }

        if let Some(name) = ref_str.strip_prefix("refs/remotes/") {
            refs.push(GitHistoryItemRef {
                id: ref_str.to_string(),
                name: name.to_string(),
                revision: Some(revision.to_string()),
                category: Some(GitHistoryRefCategory::RemoteBranches),
                ..Default::default()
            });
            continue;
        }

        if let Some(name) = ref_str.strip_prefix("tag: refs/tags/") {
            refs.push(GitHistoryItemRef {
                id: ref_str["tag: ".len()..].to_string(),
                name: name.to_string(),
                revision: Some(revision.to_string()),
                category: Some(GitHistoryRefCategory::Tags),
                ..Default::default()
            });
        }
    }

    refs.sort_by(compare_git_history_item_refs_by_category);
    refs
}

pub fn compare_git_history_item_refs_by_category(
    ref1: &GitHistoryItemRef,
    ref2: &GitHistoryItemRef,
) -> Ordering {
    fn order(ref_: &GitHistoryItemRef) -> i32 {
        if ref_.id.starts_with("refs/heads/") {
            return 1;
        }
        if ref_.id.starts_with("refs/remotes/") {
            return 2;
        }
        if ref_.id.starts_with("refs/tags/") {
            return 3;
        }
        99
    }

    match order(ref1).cmp(&order(ref2)) {
        Ordering::Equal => ref1.name.cmp(&ref2.name),
        other => other,
    }
}

pub fn parse_git_history_log(stdout: &str) -> Vec<GitHistoryItem> {
    let mut items: Vec<GitHistoryItem> = Vec::new();
    for raw_record in stdout.split('\0') {
        let record = raw_record.trim_start_matches('\n');
        if record.trim().is_empty() {
            continue;
        }

        let lines: Vec<&str> = record.split('\n').collect();
        let hash = lines.first().map_or("", |line| line.trim());
        if !is_commit_hash(hash) {
            continue;
        }

        let author_name = *lines.get(1).unwrap_or(&"");
        let author_email = *lines.get(2).unwrap_or(&"");
        let author_date_seconds = js_parse_int_base10(lines.get(3).unwrap_or(&""));
        let parents = lines.get(5).unwrap_or(&"").trim();
        let decorations = *lines.get(6).unwrap_or(&"");
        let message = lines
            .get(7..)
            .map(|rest| rest.join("\n"))
            .unwrap_or_default();
        let message = message.strip_suffix('\n').unwrap_or(&message).to_string();

        items.push(GitHistoryItem {
            id: hash.to_string(),
            parent_ids: if parents.is_empty() {
                Vec::new()
            } else {
                parents.split(' ').map(str::to_string).collect()
            },
            subject: commit_subject(&message),
            display_id: Some(short_git_hash(hash)),
            author: (!author_name.is_empty()).then(|| author_name.to_string()),
            author_email: (!author_email.is_empty()).then(|| author_email.to_string()),
            timestamp: author_date_seconds.map(|seconds| seconds.saturating_mul(1000)),
            references: Some(parse_git_decoration_refs(decorations, hash)),
            statistics: None,
            message,
        });
    }
    items
}

pub fn git_history_ref_from_full_name(
    full_name: Option<&str>,
    fallback_name: &str,
    revision: &str,
) -> GitHistoryItemRef {
    let id = full_name.filter(|name| !name.is_empty()).unwrap_or(fallback_name);

    if let Some(name) = id.strip_prefix("refs/heads/") {
        return GitHistoryItemRef {
            id: id.to_string(),
            name: name.to_string(),
            revision: Some(revision.to_string()),
            category: Some(GitHistoryRefCategory::Branches),
            ..Default::default()
        };
    }
    if let Some(name) = id.strip_prefix("refs/remotes/") {
        return GitHistoryItemRef {
            id: id.to_string(),
            name: name.to_string(),
            revision: Some(revision.to_string()),
            category: Some(GitHistoryRefCategory::RemoteBranches),
            ..Default::default()
        };
    }
    if let Some(name) = id.strip_prefix("refs/tags/") {
        return GitHistoryItemRef {
            id: id.to_string(),
            name: name.to_string(),
            revision: Some(revision.to_string()),
            category: Some(GitHistoryRefCategory::Tags),
            ..Default::default()
        };
    }
    GitHistoryItemRef {
        id: id.to_string(),
        name: if fallback_name.is_empty() {
            short_git_hash(revision)
        } else {
            fallback_name.to_string()
        },
        revision: Some(revision.to_string()),
        category: Some(GitHistoryRefCategory::Commits),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DECORATION_SEPARATOR: char = '\u{1f}';

    fn log_record(hash: &str, parents: &[&str], decorations: &str, message: &str) -> String {
        let timestamp = "1700000000";
        let fields = [
            hash,
            "Ada Lovelace",
            "ada@example.com",
            timestamp,
            timestamp,
            &parents.join(" "),
            decorations,
            message,
        ];
        format!("{}\0", fields.join("\n"))
    }

    fn ref_tuples(item: &GitHistoryItem) -> Vec<(String, String, GitHistoryRefCategory)> {
        item.references
            .as_ref()
            .unwrap()
            .iter()
            .map(|r| (r.id.clone(), r.name.clone(), r.category.unwrap()))
            .collect()
    }

    #[test]
    fn parses_vs_code_compatible_git_log_records_with_decorations_and_multiline_messages() {
        let head_oid = "a".repeat(40);
        let base_oid = "c".repeat(40);
        let stdout = log_record(
            &head_oid,
            &[&base_oid],
            "HEAD -> refs/heads/feature, refs/remotes/origin/HEAD -> refs/remotes/origin/feature, refs/remotes/origin/feature, tag: refs/tags/v1.0.0",
            "feat: add graph\n\nbody line",
        );

        let items = parse_git_history_log(&stdout);
        let item = &items[0];

        assert_eq!(item.id, head_oid);
        assert_eq!(item.parent_ids, vec![base_oid]);
        assert_eq!(item.subject, "feat: add graph");
        assert_eq!(item.message, "feat: add graph\n\nbody line");
        assert_eq!(item.author.as_deref(), Some("Ada Lovelace"));
        assert_eq!(item.author_email.as_deref(), Some("ada@example.com"));
        assert_eq!(item.display_id.as_deref(), Some(&head_oid[0..7]));
        assert_eq!(
            ref_tuples(item),
            vec![
                (
                    "refs/heads/feature".to_string(),
                    "feature".to_string(),
                    GitHistoryRefCategory::Branches
                ),
                (
                    "refs/remotes/origin/feature".to_string(),
                    "origin/feature".to_string(),
                    GitHistoryRefCategory::RemoteBranches
                ),
                (
                    "refs/tags/v1.0.0".to_string(),
                    "v1.0.0".to_string(),
                    GitHistoryRefCategory::Tags
                ),
            ]
        );
    }

    #[test]
    fn preserves_commas_inside_branch_and_tag_decoration_names() {
        let head_oid = "a".repeat(40);
        let decorations = [
            "HEAD -> refs/heads/feat,one",
            "tag: refs/tags/v1,0",
            "refs/heads/master",
        ]
        .join(&DECORATION_SEPARATOR.to_string());
        let stdout = log_record(&head_oid, &[], &decorations, "initial");

        let items = parse_git_history_log(&stdout);
        let item = &items[0];

        assert_eq!(
            ref_tuples(item),
            vec![
                (
                    "refs/heads/feat,one".to_string(),
                    "feat,one".to_string(),
                    GitHistoryRefCategory::Branches
                ),
                (
                    "refs/heads/master".to_string(),
                    "master".to_string(),
                    GitHistoryRefCategory::Branches
                ),
                (
                    "refs/tags/v1,0".to_string(),
                    "v1,0".to_string(),
                    GitHistoryRefCategory::Tags
                ),
            ]
        );
    }
}
