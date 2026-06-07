//! Terminal-scrollback pruning for persisted workspace sessions, ported from
//! `src/shared/workspace-session-terminal-buffers.ts`.
//!
//! Local terminals cold-restore their scrollback from the daemon's
//! history/checkpoints, so renderer-captured buffers for local tabs are dead
//! weight that makes every session-state write scale with old terminal output.
//! SSH/relay-backed terminals (and worktrees we can't yet classify because the
//! repo catalog isn't hydrated) keep their buffers — relay teardown may leave no
//! local history to cold-restore from.
//!
//! Operates on tolerant `serde_json::Value` (the persisted-JSON tier): extra
//! fields, unknown layout keys, and key order all round-trip untouched, and only
//! the `terminalLayoutsByTabId` entries that change are rewritten.

use orca_core::worktree_id::get_repo_id_from_worktree_id;
use serde_json::{Map, Value};
use std::collections::HashMap;

/// Synthetic worktree id for the global floating terminal. It has no backing
/// repo, so its buffers are always treated as local (pruned).
pub const FLOATING_TERMINAL_WORKTREE_ID: &str = "global-floating-terminal";

/// Cap for a single persisted scrollback buffer, in UTF-16 code units (matches
/// JS `string.length`). Keeps session JSON from scaling with raw scrollback.
pub const TERMINAL_SCROLLBACK_SESSION_BUFFER_CHAR_LIMIT: usize = 512 * 1024;

/// The repo fields the classifier reads: `connection_id` is `None` for a local
/// repo (JS `null`/`undefined`) and `Some(id)` for an SSH/relay target.
#[derive(Clone, Debug)]
pub struct RepoConnection {
    pub id: String,
    pub connection_id: Option<String>,
}

/// JS truthiness for the `!layout.buffersByLeafId` guards: only `null`, `false`,
/// `0`, `""` (and absent) are falsy; any object/array — even empty — is truthy.
fn js_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
        Value::String(s) => !s.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

fn build_connection_map(repos: &[RepoConnection]) -> HashMap<String, Option<String>> {
    // Last-writer-wins on duplicate ids, matching `new Map(repos.map(...))`.
    repos.iter().map(|repo| (repo.id.clone(), repo.connection_id.clone())).collect()
}

fn should_preserve_for_repo_map(
    worktree_id: Option<&str>,
    connection_id_by_repo_id: &HashMap<String, Option<String>>,
) -> bool {
    let Some(worktree_id) = worktree_id else {
        return false;
    };
    if worktree_id == FLOATING_TERMINAL_WORKTREE_ID {
        return false;
    }
    let repo_id = get_repo_id_from_worktree_id(worktree_id);
    match connection_id_by_repo_id.get(&repo_id) {
        // Truthy connectionId → SSH/relay target → keep the only scrollback source.
        Some(connection_id) => connection_id.as_deref().is_some_and(|id| !id.is_empty()),
        // Why: when the repo catalog is not hydrated, treating the worktree as SSH
        // avoids losing the only scrollback source a relay-backed terminal may have.
        None => true,
    }
}

pub fn should_preserve_terminal_scrollback_buffers(
    worktree_id: Option<&str>,
    repos: &[RepoConnection],
) -> bool {
    should_preserve_for_repo_map(worktree_id, &build_connection_map(repos))
}

/// Keep the last `TERMINAL_SCROLLBACK_SESSION_BUFFER_CHAR_LIMIT` UTF-16 code
/// units (`buffer.slice(-LIMIT)`). UTF-16 length semantics via `encode_utf16`.
// Trust contract: inert under stock cargo, proved under `--cfg trust_verify`.
// Postcondition — the result never exceeds the cap in UTF-16 code units.
#[cfg_attr(trust_verify, trust::ensures(|out: &String|
    out.encode_utf16().count() <= TERMINAL_SCROLLBACK_SESSION_BUFFER_CHAR_LIMIT))]
pub fn cap_terminal_scrollback_session_buffer(buffer: &str) -> String {
    let units: Vec<u16> = buffer.encode_utf16().collect();
    if units.len() <= TERMINAL_SCROLLBACK_SESSION_BUFFER_CHAR_LIMIT {
        return buffer.to_string();
    }
    let start = units.len() - TERMINAL_SCROLLBACK_SESSION_BUFFER_CHAR_LIMIT;
    // from_utf16_lossy replaces a split-pair lone surrogate with U+FFFD (1 unit),
    // so the cap still holds; JS `slice` keeps the lone surrogate instead.
    String::from_utf16_lossy(&units[start..])
}

/// Cap each leaf buffer; returns the rewritten map (or `None` when it collapses
/// to empty, matching `Object.keys(...).length > 0 ? capped : undefined`) and
/// whether any buffer actually changed.
fn cap_leaf_buffers(buffers: Option<&Map<String, Value>>) -> (Option<Map<String, Value>>, bool) {
    let Some(buffers) = buffers else {
        return (None, false);
    };
    let mut changed = false;
    let mut capped = Map::new();
    for (leaf_id, buffer) in buffers {
        let next = match buffer.as_str() {
            Some(text) => Value::String(cap_terminal_scrollback_session_buffer(text)),
            None => buffer.clone(),
        };
        changed = changed || &next != buffer;
        capped.insert(leaf_id.clone(), next);
    }
    if capped.is_empty() {
        (None, changed)
    } else {
        (Some(capped), changed)
    }
}

pub fn prune_local_terminal_scrollback_buffers(session: &Value, repos: &[RepoConnection]) -> Value {
    let connection_id_by_repo_id = build_connection_map(repos);

    let mut worktree_id_by_tab_id: HashMap<String, String> = HashMap::new();
    if let Some(tabs_by_worktree) = session.get("tabsByWorktree").and_then(Value::as_object) {
        for (worktree_id, tabs) in tabs_by_worktree {
            if let Some(tabs) = tabs.as_array() {
                for tab in tabs {
                    if let Some(tab_id) = tab.get("id").and_then(Value::as_str) {
                        worktree_id_by_tab_id.insert(tab_id.to_string(), worktree_id.clone());
                    }
                }
            }
        }
    }

    // None until the first divergent layout; then a clone of the original map we
    // mutate in place (mirrors `terminalLayoutsByTabId ??= { ...original }`).
    let mut next_layouts: Option<Map<String, Value>> = None;
    if let Some(layouts) = session.get("terminalLayoutsByTabId").and_then(Value::as_object) {
        for (tab_id, layout) in layouts {
            let has_buffers = layout.get("buffersByLeafId").is_some_and(js_truthy);
            let has_refs = layout.get("scrollbackRefsByLeafId").is_some_and(js_truthy);
            if !has_buffers && !has_refs {
                continue;
            }
            let worktree_id = worktree_id_by_tab_id.get(tab_id).map(String::as_str);
            if should_preserve_for_repo_map(worktree_id, &connection_id_by_repo_id) {
                let (buffers, changed) =
                    cap_leaf_buffers(layout.get("buffersByLeafId").and_then(Value::as_object));
                if changed {
                    let target = next_layouts.get_or_insert_with(|| layouts.clone());
                    let mut updated = layout.as_object().cloned().unwrap_or_default();
                    match buffers {
                        // `{ ...layout, buffersByLeafId: undefined }` drops the key.
                        Some(map) => {
                            updated.insert("buffersByLeafId".to_string(), Value::Object(map));
                        }
                        None => {
                            updated.remove("buffersByLeafId");
                        }
                    }
                    target.insert(tab_id.clone(), Value::Object(updated));
                }
                continue;
            }

            let target = next_layouts.get_or_insert_with(|| layouts.clone());
            let mut without_buffers = layout.as_object().cloned().unwrap_or_default();
            without_buffers.remove("buffersByLeafId");
            without_buffers.remove("scrollbackRefsByLeafId");
            target.insert(tab_id.clone(), Value::Object(without_buffers));
        }
    }

    match next_layouts {
        None => session.clone(),
        Some(layouts) => {
            let mut updated = session.as_object().cloned().unwrap_or_default();
            updated.insert("terminalLayoutsByTabId".to_string(), Value::Object(layouts));
            Value::Object(updated)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn repo(id: &str, connection_id: Option<&str>) -> RepoConnection {
        RepoConnection { id: id.to_string(), connection_id: connection_id.map(str::to_string) }
    }

    fn make_session() -> Value {
        json!({
            "activeRepoId": null,
            "activeWorktreeId": null,
            "activeTabId": null,
            "tabsByWorktree": {
                "local-repo::/local/worktree": [{
                    "id": "local-tab",
                    "title": "local",
                    "customTitle": null,
                    "color": null,
                    "sortOrder": 0,
                    "createdAt": 1,
                    "ptyId": "local-pty",
                    "worktreeId": "local-repo::/local/worktree"
                }],
                "remote-repo::/remote/worktree": [{
                    "id": "remote-tab",
                    "title": "remote",
                    "customTitle": null,
                    "color": null,
                    "sortOrder": 0,
                    "createdAt": 1,
                    "ptyId": "remote-pty",
                    "worktreeId": "remote-repo::/remote/worktree"
                }]
            },
            "terminalLayoutsByTabId": {
                "local-tab": {
                    "root": null,
                    "activeLeafId": null,
                    "expandedLeafId": null,
                    "buffersByLeafId": { "pane:1": "local-scrollback" },
                    "scrollbackRefsByLeafId": { "pane:1": "v1-local" },
                    "ptyIdsByLeafId": { "pane:1": "local-pty" }
                },
                "remote-tab": {
                    "root": null,
                    "activeLeafId": null,
                    "expandedLeafId": null,
                    "buffersByLeafId": { "pane:1": "remote-scrollback" },
                    "scrollbackRefsByLeafId": { "pane:1": "v1-remote" },
                    "ptyIdsByLeafId": { "pane:1": "remote-pty" }
                }
            }
        })
    }

    fn make_session_with(overrides: Value) -> Value {
        let mut base = make_session();
        if let (Some(base_obj), Some(over)) = (base.as_object_mut(), overrides.as_object()) {
            for (key, value) in over {
                base_obj.insert(key.clone(), value.clone());
            }
        }
        base
    }

    #[test]
    fn classifies_which_worktrees_need_renderer_captured_scrollback() {
        let repos = [repo("local-repo", None), repo("remote-repo", Some("ssh-target-1"))];

        assert!(!should_preserve_terminal_scrollback_buffers(
            Some("local-repo::/local/worktree"),
            &repos
        ));
        assert!(should_preserve_terminal_scrollback_buffers(
            Some("remote-repo::/remote/worktree"),
            &repos
        ));
        assert!(!should_preserve_terminal_scrollback_buffers(
            Some(FLOATING_TERMINAL_WORKTREE_ID),
            &repos
        ));
        assert!(should_preserve_terminal_scrollback_buffers(
            Some("unknown-repo::/maybe-remote/worktree"),
            &repos
        ));
    }

    #[test]
    fn drops_local_scrollback_while_preserving_ssh_scrollback_and_pty_bindings() {
        let result = prune_local_terminal_scrollback_buffers(
            &make_session(),
            &[repo("local-repo", None), repo("remote-repo", Some("ssh-target-1"))],
        );

        assert_eq!(
            result["terminalLayoutsByTabId"]["local-tab"],
            json!({
                "root": null,
                "activeLeafId": null,
                "expandedLeafId": null,
                "ptyIdsByLeafId": { "pane:1": "local-pty" }
            })
        );
        assert_eq!(
            result["terminalLayoutsByTabId"]["remote-tab"]["buffersByLeafId"],
            json!({ "pane:1": "remote-scrollback" })
        );
        assert_eq!(
            result["terminalLayoutsByTabId"]["remote-tab"]["scrollbackRefsByLeafId"],
            json!({ "pane:1": "v1-remote" })
        );
    }

    #[test]
    fn caps_preserved_ssh_buffers_so_session_json_cannot_scale_with_raw_scrollback() {
        let huge_scrollback =
            format!("start-{}", "x".repeat(TERMINAL_SCROLLBACK_SESSION_BUFFER_CHAR_LIMIT + 10));
        let result = prune_local_terminal_scrollback_buffers(
            &make_session_with(json!({
                "terminalLayoutsByTabId": {
                    "remote-tab": {
                        "root": null,
                        "activeLeafId": null,
                        "expandedLeafId": null,
                        "buffersByLeafId": { "pane:1": huge_scrollback }
                    }
                }
            })),
            &[repo("remote-repo", Some("ssh-target-1"))],
        );

        let buffer = result["terminalLayoutsByTabId"]["remote-tab"]["buffersByLeafId"]["pane:1"]
            .as_str()
            .unwrap();
        assert_eq!(
            buffer.encode_utf16().count(),
            TERMINAL_SCROLLBACK_SESSION_BUFFER_CHAR_LIMIT
        );
        assert!(!buffer.starts_with("start-"));
    }

    #[test]
    fn drops_floating_terminal_buffers_even_though_synthetic_worktree_has_no_repo() {
        let result = prune_local_terminal_scrollback_buffers(
            &make_session_with(json!({
                "tabsByWorktree": {
                    FLOATING_TERMINAL_WORKTREE_ID: [{
                        "id": "floating-tab",
                        "title": "floating",
                        "customTitle": null,
                        "color": null,
                        "sortOrder": 0,
                        "createdAt": 1,
                        "ptyId": "floating-pty",
                        "worktreeId": FLOATING_TERMINAL_WORKTREE_ID
                    }]
                },
                "terminalLayoutsByTabId": {
                    "floating-tab": {
                        "root": null,
                        "activeLeafId": null,
                        "expandedLeafId": null,
                        "buffersByLeafId": { "pane:1": "floating-scrollback" },
                        "ptyIdsByLeafId": { "pane:1": "floating-pty" }
                    }
                }
            })),
            &[],
        );

        assert_eq!(
            result["terminalLayoutsByTabId"]["floating-tab"],
            json!({
                "root": null,
                "activeLeafId": null,
                "expandedLeafId": null,
                "ptyIdsByLeafId": { "pane:1": "floating-pty" }
            })
        );
    }

    #[test]
    fn treats_orphaned_layouts_as_local_and_prunes_their_buffers() {
        let result = prune_local_terminal_scrollback_buffers(
            &make_session_with(json!({
                "terminalLayoutsByTabId": {
                    "orphan-tab": {
                        "root": null,
                        "activeLeafId": null,
                        "expandedLeafId": null,
                        "buffersByLeafId": { "pane:1": "orphan-scrollback" }
                    }
                }
            })),
            &[repo("remote-repo", Some("ssh-target-1"))],
        );

        assert!(result["terminalLayoutsByTabId"]["orphan-tab"]
            .get("buffersByLeafId")
            .is_none());
    }

    #[test]
    fn preserves_buffers_for_unresolved_repo_catalogs_until_worktrees_can_be_classified() {
        let result = prune_local_terminal_scrollback_buffers(
            &make_session_with(json!({
                "tabsByWorktree": {
                    "remote-repo::/remote/worktree": [{
                        "id": "remote-tab",
                        "title": "remote",
                        "customTitle": null,
                        "color": null,
                        "sortOrder": 0,
                        "createdAt": 1,
                        "ptyId": "remote-pty",
                        "worktreeId": "remote-repo::/remote/worktree"
                    }]
                },
                "terminalLayoutsByTabId": {
                    "remote-tab": {
                        "root": null,
                        "activeLeafId": null,
                        "expandedLeafId": null,
                        "buffersByLeafId": { "pane:1": "maybe-remote-scrollback" }
                    }
                }
            })),
            &[],
        );

        assert_eq!(
            result["terminalLayoutsByTabId"]["remote-tab"]["buffersByLeafId"],
            json!({ "pane:1": "maybe-remote-scrollback" })
        );
    }

    #[test]
    fn keeps_persisted_session_size_from_scaling_with_local_scrollback_buffers() {
        let large_scrollback = "x".repeat(8 * 1024);
        let mut tabs = Vec::new();
        let mut layouts = Map::new();
        for index in 0..8 {
            let tab_id = format!("local-tab-{index}");
            let pty_id = format!("local-pty-{index}");
            tabs.push(json!({
                "id": tab_id,
                "title": format!("local {index}"),
                "customTitle": null,
                "color": null,
                "sortOrder": index,
                "createdAt": index,
                "ptyId": pty_id,
                "worktreeId": "local-repo::/local/worktree"
            }));
            layouts.insert(
                tab_id.clone(),
                json!({
                    "root": null,
                    "activeLeafId": null,
                    "expandedLeafId": null,
                    "buffersByLeafId": { "pane:1": format!("{large_scrollback}-{index}") },
                    "ptyIdsByLeafId": { "pane:1": pty_id }
                }),
            );
        }
        let session = make_session_with(json!({
            "tabsByWorktree": { "local-repo::/local/worktree": tabs },
            "terminalLayoutsByTabId": Value::Object(layouts)
        }));

        let original_bytes = serde_json::to_string(&session).unwrap().len();
        let result = prune_local_terminal_scrollback_buffers(&session, &[repo("local-repo", None)]);
        let pruned_bytes = serde_json::to_string(&result).unwrap().len();

        assert!(!serde_json::to_string(&result).unwrap().contains(&large_scrollback));
        assert!(pruned_bytes < original_bytes / 5);
    }
}
