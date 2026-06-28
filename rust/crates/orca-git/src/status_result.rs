//! `serde_json` builders producing the exact TS JSON shapes for the parser-level
//! status result, numstat map, and line stats. `None` fields are omitted (the TS
//! object-spread `...(x ? {x} : {})` pattern); array order and field omission are
//! the load-bearing parts (object key order is BTreeMap-sorted under this crate's
//! `default-features = false` serde_json — not load-bearing).

use crate::line_count::LineStats;
use crate::numstat::NumstatEntry;
use crate::status::{GitStagingArea, GitStatusEntry};
use crate::status_parse::{GitConflictKind, GitFileStatus};
use crate::status_stream::StatusParseResult;
use serde_json::{Map, Value};

fn file_status_str(status: GitFileStatus) -> &'static str {
    match status {
        GitFileStatus::Modified => "modified",
        GitFileStatus::Added => "added",
        GitFileStatus::Deleted => "deleted",
        GitFileStatus::Renamed => "renamed",
        GitFileStatus::Untracked => "untracked",
        GitFileStatus::Copied => "copied",
    }
}

fn staging_area_str(area: GitStagingArea) -> &'static str {
    match area {
        GitStagingArea::Staged => "staged",
        GitStagingArea::Unstaged => "unstaged",
        GitStagingArea::Untracked => "untracked",
    }
}

fn conflict_kind_str(kind: GitConflictKind) -> &'static str {
    match kind {
        GitConflictKind::BothModified => "both_modified",
        GitConflictKind::BothAdded => "both_added",
        GitConflictKind::BothDeleted => "both_deleted",
        GitConflictKind::AddedByUs => "added_by_us",
        GitConflictKind::AddedByThem => "added_by_them",
        GitConflictKind::DeletedByUs => "deleted_by_us",
        GitConflictKind::DeletedByThem => "deleted_by_them",
    }
}

fn entry_to_json(e: &GitStatusEntry) -> Value {
    let mut m = Map::new();
    m.insert("path".into(), Value::String(e.path.clone()));
    m.insert("status".into(), Value::String(file_status_str(e.status).into()));
    m.insert("area".into(), Value::String(staging_area_str(e.area).into()));
    if let Some(old) = &e.old_path {
        m.insert("oldPath".into(), Value::String(old.clone()));
    }
    if let Some(kind) = e.conflict_kind {
        m.insert("conflictKind".into(), Value::String(conflict_kind_str(kind).into()));
    }
    if let Some(status) = e.conflict_status {
        m.insert("conflictStatus".into(), Value::String(status.into()));
    }
    if let Some(sub) = e.submodule {
        let mut sm = Map::new();
        sm.insert("commitChanged".into(), Value::Bool(sub.commit_changed));
        sm.insert("trackedChanges".into(), Value::Bool(sub.tracked_changes));
        sm.insert("untrackedChanges".into(), Value::Bool(sub.untracked_changes));
        m.insert("submodule".into(), Value::Object(sm));
    }
    if let Some(added) = e.added {
        m.insert("added".into(), Value::from(added));
    }
    if let Some(removed) = e.removed {
        m.insert("removed".into(), Value::from(removed));
    }
    Value::Object(m)
}

/// Build the parser-level status JSON (the shape returned by the streaming parser
/// and the relay one-shot). `head`/`branch`/`upstreamName` are omitted when None;
/// `ahead`/`behind` only when `# branch.ab` was parsed; `didHitLimit` only when
/// capped; `statusLength` always.
pub fn status_parse_result_to_json(result: &StatusParseResult) -> Value {
    let mut m = Map::new();
    m.insert(
        "entries".into(),
        Value::Array(result.entries.iter().map(entry_to_json).collect()),
    );
    m.insert(
        "ignoredPaths".into(),
        Value::Array(result.ignored_paths.iter().cloned().map(Value::String).collect()),
    );
    m.insert(
        "unmergedLines".into(),
        Value::Array(result.unmerged_lines.iter().cloned().map(Value::String).collect()),
    );
    if let Some(head) = &result.branch.head {
        m.insert("head".into(), Value::String(head.clone()));
    }
    if let Some(branch) = &result.branch.branch {
        m.insert("branch".into(), Value::String(branch.clone()));
    }
    if let Some(name) = &result.branch.upstream_name {
        m.insert("upstreamName".into(), Value::String(name.clone()));
    }
    if let Some((ahead, behind)) = result.branch.ahead_behind {
        m.insert("ahead".into(), Value::from(ahead));
        m.insert("behind".into(), Value::from(behind));
    }
    if result.did_hit_limit {
        m.insert("didHitLimit".into(), Value::Bool(true));
    }
    m.insert("statusLength".into(), Value::from(result.status_length as u64));
    Value::Object(m)
}

/// Build the numstat JSON: `{"path": {"added": N, "removed": N}, "binpath": {}}`.
/// A binary file (both counts `None`) yields an empty object.
pub fn numstat_to_json(entries: &[NumstatEntry]) -> Value {
    let mut m = Map::new();
    for e in entries {
        let mut inner = Map::new();
        if let Some(added) = e.added {
            inner.insert("added".into(), Value::from(added));
        }
        if let Some(removed) = e.removed {
            inner.insert("removed".into(), Value::from(removed));
        }
        m.insert(e.path.clone(), Value::Object(inner));
    }
    Value::Object(m)
}

/// Build the line-stats JSON: `{"added": N, "removed": N}` or `null`.
pub fn line_stats_to_json(stats: Option<LineStats>) -> Value {
    match stats {
        None => Value::Null,
        Some(s) => {
            let mut m = Map::new();
            m.insert("added".into(), Value::from(s.added));
            m.insert("removed".into(), Value::from(s.removed));
            Value::Object(m)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::numstat::parse_numstat;
    use crate::status_stream::parse_status_porcelain;
    use serde_json::json;

    #[test]
    fn omits_none_branch_fields_and_includes_status_length() {
        let result = parse_status_porcelain(b"? a.txt\n", 0);
        let value = status_parse_result_to_json(&result);
        let obj = value.as_object().unwrap();
        assert!(!obj.contains_key("head"));
        assert!(!obj.contains_key("branch"));
        assert!(!obj.contains_key("upstreamName"));
        assert!(!obj.contains_key("ahead"));
        assert!(!obj.contains_key("didHitLimit"));
        assert_eq!(obj.get("statusLength"), Some(&Value::from(1u64)));
    }

    #[test]
    fn includes_branch_ahead_behind_and_entry_array_order() {
        let result = parse_status_porcelain(
            b"# branch.oid abc\n# branch.head main\n# branch.upstream origin/main\n\
              # branch.ab +2 -1\n1 M. N... 1 1 1 a a first.ts\n? second.txt\n",
            0,
        );
        let value = status_parse_result_to_json(&result);
        assert_eq!(value["head"], json!("abc"));
        assert_eq!(value["branch"], json!("refs/heads/main"));
        assert_eq!(value["upstreamName"], json!("origin/main"));
        assert_eq!(value["ahead"], json!(2));
        assert_eq!(value["behind"], json!(1));
        // Array order is load-bearing.
        assert_eq!(value["entries"][0]["path"], json!("first.ts"));
        assert_eq!(value["entries"][1]["path"], json!("second.txt"));
    }

    #[test]
    fn caps_entries_and_flags_did_hit_limit() {
        let mut s = String::new();
        for i in 0..10 {
            s.push_str(&format!("? f{i}.txt\n"));
        }
        let result = parse_status_porcelain(s.as_bytes(), 3);
        let value = status_parse_result_to_json(&result);
        assert_eq!(value["entries"].as_array().unwrap().len(), 3);
        assert_eq!(value["didHitLimit"], json!(true));
        assert_eq!(value["statusLength"], json!(4));
    }

    #[test]
    fn numstat_json_uses_empty_object_for_binary() {
        let entries = parse_numstat(b"3\t4\tsrc/app.ts\n-\t-\tlogo.png\n");
        let value = numstat_to_json(&entries);
        assert_eq!(value["src/app.ts"], json!({ "added": 3, "removed": 4 }));
        assert_eq!(value["logo.png"], json!({}));
    }

    #[test]
    fn line_stats_json_null_or_object() {
        assert_eq!(line_stats_to_json(None), Value::Null);
        assert_eq!(
            line_stats_to_json(Some(LineStats { added: 5, removed: 2 })),
            json!({ "added": 5, "removed": 2 })
        );
    }
}
