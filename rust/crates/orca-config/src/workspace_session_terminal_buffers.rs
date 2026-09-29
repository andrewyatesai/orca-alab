//! Terminal-scrollback pruning for persisted workspace sessions, ported from
//! `src/shared/workspace-session-terminal-buffers.ts`.
//!
//! Local terminals cold-restore their scrollback from the daemon's
//! history/checkpoints, so renderer-captured buffers for local tabs are dead
//! weight that makes every session-state write scale with old terminal output.
//! SSH/runtime-backed terminals (and worktrees we can't yet classify because the
//! repo catalog isn't hydrated) keep their buffers — teardown may leave no local
//! history to cold-restore from.
//!
//! Operates on tolerant `serde_json::Value` (the persisted-JSON tier): extra
//! fields, unknown layout keys, and key order all round-trip untouched, and only
//! the `terminalLayoutsByTabId` entries that change are rewritten.

use orca_core::execution_host::{normalize_execution_host_id, LOCAL_EXECUTION_HOST_ID};
use orca_core::worktree_id::get_repo_id_from_worktree_id;
use serde_json::{Map, Value};
use std::collections::HashMap;

/// Synthetic worktree id for the global floating terminal. It has no backing
/// repo, so its buffers are always treated as local (pruned).
pub const FLOATING_TERMINAL_WORKTREE_ID: &str = "global-floating-terminal";

/// Cap for a single persisted scrollback buffer, in UTF-8 BYTES — the unit the
/// twin's `TERMINAL_SCROLLBACK_SESSION_BUFFER_BYTE_LIMIT`
/// (`src/shared/terminal-scrollback-limits.ts`) uses. Capping in chars instead
/// persists 2× the intended payload for accented text and 3× for CJK.
pub const TERMINAL_SCROLLBACK_SESSION_BUFFER_BYTE_LIMIT: usize = 512 * 1024;

/// The repo fields the classifier reads. `connection_id` is `None` for a repo
/// with no SSH connection (JS `null`/`undefined`); `execution_host_id` is the
/// persisted `local` / `ssh:<id>` / `runtime:<id>` host reference.
#[derive(Clone, Debug, Default)]
pub struct RepoConnection {
    pub id: String,
    pub connection_id: Option<String>,
    pub execution_host_id: Option<String>,
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

/// `repoNeedsRendererCapturedScrollback`: a truthy `connectionId` short-circuits
/// to true; otherwise the repo needs renderer capture only when its execution
/// host parses AND is not the local host (i.e. `ssh:` or `runtime:`).
fn repo_needs_renderer_captured_scrollback(repo: &RepoConnection) -> bool {
    if repo.connection_id.as_deref().is_some_and(|id| !id.is_empty()) {
        return true;
    }
    match repo.execution_host_id.as_deref().and_then(normalize_execution_host_id) {
        Some(host_id) => host_id != LOCAL_EXECUTION_HOST_ID,
        None => false,
    }
}

/// `new Map(repos.map(...))` then `repoNeedsRendererCapturedScrollback` per
/// entry. The classification is stored, not the raw fields, so absence (`None`)
/// stays distinguishable from "present and local" — the twin branches on
/// `repoById.has(repoId)` separately from the repo's own answer.
fn build_needs_capture_map(repos: &[RepoConnection]) -> HashMap<String, bool> {
    // Last-writer-wins on duplicate ids, matching `new Map(repos.map(...))`.
    repos
        .iter()
        .map(|repo| (repo.id.clone(), repo_needs_renderer_captured_scrollback(repo)))
        .collect()
}

fn should_preserve_for_repo_map(
    worktree_id: Option<&str>,
    needs_capture_by_repo_id: &HashMap<String, bool>,
) -> bool {
    let Some(worktree_id) = worktree_id else {
        return false;
    };
    if worktree_id == FLOATING_TERMINAL_WORKTREE_ID {
        return false;
    }
    let repo_id = get_repo_id_from_worktree_id(worktree_id);
    match needs_capture_by_repo_id.get(&repo_id) {
        Some(needs_capture) => *needs_capture,
        // Why: when the repo catalog is not hydrated, treating the worktree as
        // remote avoids losing the only scrollback source a relay/runtime
        // terminal may have.
        None => true,
    }
}

pub fn should_preserve_terminal_scrollback_buffers(
    worktree_id: Option<&str>,
    repos: &[RepoConnection],
) -> bool {
    should_preserve_for_repo_map(worktree_id, &build_needs_capture_map(repos))
}

/// `clampUtf8TextTail` from `src/shared/utf8-byte-limits.ts`: the longest
/// code-point-aligned SUFFIX whose UTF-8 encoding fits in `max_bytes`. The TS
/// walks code points backwards accumulating byte costs; on a Rust `&str` the
/// byte length is already known, so the same suffix is the first char boundary
/// at or after `len - max_bytes`.
// Trust contract: inert unless verification runs; checked under `targo trust`.
#[cfg_attr(trust_verify, trust::ensures(|out: &&str| out.len() <= max_bytes))]
fn clamp_utf8_text_tail(text: &str, max_bytes: usize) -> &str {
    if text.is_empty() || max_bytes == 0 {
        return "";
    }
    if text.len() <= max_bytes {
        return text;
    }
    let mut start = text.len() - max_bytes;
    // Terminates: `is_char_boundary(text.len())` is true, so `start` never
    // passes the end, and slicing at a boundary <= len cannot panic.
    while start < text.len() && !text.is_char_boundary(start) {
        start += 1;
    }
    &text[start..]
}

// Trust contract: inert unless verification runs; checked under `targo trust`.
// Postcondition — the result never exceeds the cap in UTF-8 bytes. It lives on
// this `usize` overload rather than the `Option` one because the contract
// renderer cannot parse a method call in the predicate: it answers "not a
// well-typed source predicate … postcondition is withheld", i.e. it drops the
// obligation with only a warning.
#[cfg_attr(trust_verify, trust::ensures(|out: &String| out.len() <= byte_limit))]
fn cap_to_byte_limit(buffer: &str, byte_limit: usize) -> String {
    // The twin's fast path is `buffer.length <= byteLimit && !exceededLimit`;
    // UTF-16 length never exceeds the UTF-8 byte length, so the pair reduces to
    // "the UTF-8 encoding already fits".
    if buffer.len() <= byte_limit {
        return buffer.to_string();
    }
    clamp_utf8_text_tail(buffer, byte_limit).to_string()
}

/// Keep the last `byte_limit` UTF-8 bytes (default
/// `TERMINAL_SCROLLBACK_SESSION_BUFFER_BYTE_LIMIT`), never splitting a code
/// point. `None` selects the twin's default parameter value.
pub fn cap_terminal_scrollback_session_buffer(buffer: &str, byte_limit: Option<usize>) -> String {
    cap_to_byte_limit(buffer, byte_limit.unwrap_or(TERMINAL_SCROLLBACK_SESSION_BUFFER_BYTE_LIMIT))
}

/// Cap each leaf buffer; returns the rewritten map (or `None` when it collapses
/// to empty, matching `Object.keys(...).length > 0 ? capped : undefined`) and
/// whether any buffer actually changed.
fn cap_leaf_buffers(
    buffers: Option<&Map<String, Value>>,
    byte_limit: usize,
) -> (Option<Map<String, Value>>, bool) {
    let Some(buffers) = buffers else {
        return (None, false);
    };
    let mut changed = false;
    let mut capped = Map::new();
    for (leaf_id, buffer) in buffers {
        let next = match buffer.as_str() {
            Some(text) => Value::String(cap_to_byte_limit(text, byte_limit)),
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

/// `buffer_byte_limit` is the twin's `opts.bufferByteLimit` override: callers
/// that immediately migrate buffers into disk snapshot refs may keep more than
/// the session-JSON bound; `None` selects the default.
pub fn prune_local_terminal_scrollback_buffers(
    session: &Value,
    repos: &[RepoConnection],
    buffer_byte_limit: Option<usize>,
) -> Value {
    let buffer_byte_limit =
        buffer_byte_limit.unwrap_or(TERMINAL_SCROLLBACK_SESSION_BUFFER_BYTE_LIMIT);
    let needs_capture_by_repo_id = build_needs_capture_map(repos);

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
            if should_preserve_for_repo_map(worktree_id, &needs_capture_by_repo_id) {
                let (buffers, changed) = cap_leaf_buffers(
                    layout.get("buffersByLeafId").and_then(Value::as_object),
                    buffer_byte_limit,
                );
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
        RepoConnection {
            id: id.to_string(),
            connection_id: connection_id.map(str::to_string),
            execution_host_id: None,
        }
    }

    fn repo_on_host(id: &str, connection_id: Option<&str>, host: &str) -> RepoConnection {
        RepoConnection {
            id: id.to_string(),
            connection_id: connection_id.map(str::to_string),
            execution_host_id: Some(host.to_string()),
        }
    }

    fn utf8_byte_length(text: &str) -> usize {
        text.len()
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

    /// `makeRuntimeSession()` from the twin's test file.
    fn make_runtime_session() -> Value {
        make_session_with(json!({
            "tabsByWorktree": {
                "runtime-repo::/runtime/worktree": [{
                    "id": "runtime-tab",
                    "title": "runtime",
                    "customTitle": null,
                    "color": null,
                    "sortOrder": 0,
                    "createdAt": 1,
                    "ptyId": "runtime-pty",
                    "worktreeId": "runtime-repo::/runtime/worktree"
                }]
            },
            "terminalLayoutsByTabId": {
                "runtime-tab": {
                    "root": null,
                    "activeLeafId": null,
                    "expandedLeafId": null,
                    "buffersByLeafId": { "pane:1": "runtime-scrollback" },
                    "scrollbackRefsByLeafId": { "pane:1": "v1-runtime" },
                    "ptyIdsByLeafId": { "pane:1": "runtime-pty" }
                }
            }
        }))
    }

    #[test]
    fn tolerates_legacy_sessions_without_terminal_maps() {
        let legacy_session = json!({
            "activeRepoId": null,
            "activeWorktreeId": null,
            "activeTabId": null
        });

        assert_eq!(
            prune_local_terminal_scrollback_buffers(&legacy_session, &[], None),
            legacy_session
        );
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
    fn preserves_runtime_host_scrollback_without_requiring_an_ssh_connection_id() {
        assert!(should_preserve_terminal_scrollback_buffers(
            Some("runtime-repo::/runtime/worktree"),
            &[repo_on_host("runtime-repo", None, "runtime:env-1")]
        ));

        let result = prune_local_terminal_scrollback_buffers(
            &make_runtime_session(),
            &[repo_on_host("runtime-repo", None, "runtime:env-1")],
            None,
        );

        assert_eq!(
            result["terminalLayoutsByTabId"]["runtime-tab"]["buffersByLeafId"],
            json!({ "pane:1": "runtime-scrollback" })
        );
        assert_eq!(
            result["terminalLayoutsByTabId"]["runtime-tab"]["scrollbackRefsByLeafId"],
            json!({ "pane:1": "v1-runtime" })
        );
    }

    #[test]
    fn drops_scrollback_for_explicitly_local_execution_hosts() {
        let result = prune_local_terminal_scrollback_buffers(
            &make_runtime_session(),
            &[repo_on_host("runtime-repo", None, "local")],
            None,
        );

        assert_eq!(
            result["terminalLayoutsByTabId"]["runtime-tab"],
            json!({
                "root": null,
                "activeLeafId": null,
                "expandedLeafId": null,
                "ptyIdsByLeafId": { "pane:1": "runtime-pty" }
            })
        );
    }

    #[test]
    fn drops_local_scrollback_while_preserving_ssh_scrollback_and_pty_bindings() {
        let result = prune_local_terminal_scrollback_buffers(
            &make_session(),
            &[repo("local-repo", None), repo("remote-repo", Some("ssh-target-1"))],
            None,
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
            format!("start-{}", "x".repeat(TERMINAL_SCROLLBACK_SESSION_BUFFER_BYTE_LIMIT + 10));
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
            None,
        );

        let buffer = result["terminalLayoutsByTabId"]["remote-tab"]["buffersByLeafId"]["pane:1"]
            .as_str()
            .unwrap();
        // `toHaveLength` is UTF-16 units; this input is ASCII, so units == bytes.
        assert_eq!(
            buffer.encode_utf16().count(),
            TERMINAL_SCROLLBACK_SESSION_BUFFER_BYTE_LIMIT
        );
        assert!(!buffer.starts_with("start-"));
    }

    #[test]
    fn caps_preserved_ssh_buffers_by_utf8_bytes_for_multibyte_scrollback() {
        let multibyte_row = "é".repeat(1024);
        let huge_scrollback = multibyte_row.repeat(512);
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
            None,
        );

        let buffer = result["terminalLayoutsByTabId"]["remote-tab"]["buffersByLeafId"]["pane:1"]
            .as_str()
            .unwrap_or_default();
        assert!(!buffer.is_empty());
        assert!(utf8_byte_length(buffer) <= TERMINAL_SCROLLBACK_SESSION_BUFFER_BYTE_LIMIT);
        // The twin's `toHaveLength(LIMIT / 2)`: UTF-16 units, and 'é' costs two
        // UTF-8 bytes per unit — the char cap would answer LIMIT units here.
        assert_eq!(
            buffer.encode_utf16().count(),
            TERMINAL_SCROLLBACK_SESSION_BUFFER_BYTE_LIMIT / 2
        );
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
            None,
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
            None,
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
            None,
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
        let result =
            prune_local_terminal_scrollback_buffers(&session, &[repo("local-repo", None)], None);
        let pruned_bytes = serde_json::to_string(&result).unwrap().len();

        assert!(!serde_json::to_string(&result).unwrap().contains(&large_scrollback));
        assert!(pruned_bytes < original_bytes / 5);
    }

    // ---- Beyond the twin's own cases: the unit the cap counts ----

    #[test]
    fn cap_counts_utf8_bytes_not_chars() {
        // 10 × 'é' = 20 UTF-8 bytes. A char cap of 8 would keep 8 chars (16
        // bytes) — exactly the 2× overshoot this port had.
        let buffer = "é".repeat(10);
        let capped = cap_terminal_scrollback_session_buffer(&buffer, Some(8));
        assert_eq!(capped.len(), 8);
        assert_eq!(capped.chars().count(), 4);
        assert_eq!(capped, "éééé");
    }

    #[test]
    fn cap_never_splits_a_code_point_and_may_answer_under_the_limit() {
        // A 3-byte 'あ' cannot half-fit a 7-byte budget, so the tail is 6 bytes.
        let buffer = "あ".repeat(4);
        let capped = cap_terminal_scrollback_session_buffer(&buffer, Some(7));
        assert_eq!(capped, "ああ");
        assert_eq!(capped.len(), 6);
        // 4-byte astral code points behave the same way.
        let astral = "😀".repeat(3);
        assert_eq!(cap_terminal_scrollback_session_buffer(&astral, Some(7)), "😀");
    }

    #[test]
    fn cap_returns_the_buffer_unchanged_when_it_already_fits() {
        assert_eq!(cap_terminal_scrollback_session_buffer("héllo", Some(6)), "héllo");
        assert_eq!(cap_terminal_scrollback_session_buffer("", Some(0)), "");
        assert_eq!(cap_terminal_scrollback_session_buffer("abc", Some(0)), "");
        assert_eq!(cap_terminal_scrollback_session_buffer("abc", None), "abc");
    }

    #[test]
    fn prune_honours_the_buffer_byte_limit_override() {
        let result = prune_local_terminal_scrollback_buffers(
            &make_session_with(json!({
                "terminalLayoutsByTabId": {
                    "remote-tab": {
                        "root": null,
                        "activeLeafId": null,
                        "expandedLeafId": null,
                        "buffersByLeafId": { "pane:1": "é".repeat(10) }
                    }
                }
            })),
            &[repo("remote-repo", Some("ssh-target-1"))],
            Some(8),
        );

        assert_eq!(
            result["terminalLayoutsByTabId"]["remote-tab"]["buffersByLeafId"],
            json!({ "pane:1": "éééé" })
        );
    }

    #[test]
    fn an_unparseable_execution_host_does_not_preserve_scrollback() {
        // `parseExecutionHostId` returns null for a blank/unknown/undecodable
        // reference, and `null !== 'local'` must NOT read as remote.
        for host in ["", "   ", "bogus", "ssh:", "runtime:", "ssh:%ZZ"] {
            assert!(
                !should_preserve_terminal_scrollback_buffers(
                    Some("host-repo::/worktree"),
                    &[repo_on_host("host-repo", None, host)]
                ),
                "host {host:?} must not preserve"
            );
        }
        for host in ["ssh:target-1", "runtime:env-1", "  runtime:env-1  "] {
            assert!(
                should_preserve_terminal_scrollback_buffers(
                    Some("host-repo::/worktree"),
                    &[repo_on_host("host-repo", None, host)]
                ),
                "host {host:?} must preserve"
            );
        }
    }

    #[test]
    fn a_truthy_connection_id_wins_over_a_local_execution_host() {
        assert!(should_preserve_terminal_scrollback_buffers(
            Some("host-repo::/worktree"),
            &[repo_on_host("host-repo", Some("ssh-target-1"), "local")]
        ));
        // An empty connectionId is falsy in JS, so the host decides.
        assert!(!should_preserve_terminal_scrollback_buffers(
            Some("host-repo::/worktree"),
            &[repo_on_host("host-repo", Some(""), "local")]
        ));
    }
}
