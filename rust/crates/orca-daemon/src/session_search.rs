//! `searchSessions` / `searchContext` / `searchReplay*` — the daemon side of
//! federated search (fed design §2.2/§2.3, Wave-4 4E groundwork).
//!
//! Warm sessions are searched in-memory via the E-5 entry
//! (`HeadlessTerminal::search_scrollback`); only match SUMMARIES cross the
//! socket. Cold/parked content is replayed through a transient
//! `HeadlessTerminal` cached keyed by `(sessionId, checkpoint generation)` —
//! the client owns checkpoint.json/output.log in this fork (the daemon never
//! reads client disk), so it ships the stored ANSI once per generation and
//! repeat queries hit the cache with no content bytes at all.

use crate::protocol::{rpc_err, rpc_ok};
use crate::registry::Registry;
use crate::rpc::{field_str, field_u16, scrollback_rows};
use orca_terminal::{replay_for_search, HeadlessTerminal, MatchSummary, SearchOptions};
use serde_json::{json, Value};
use std::sync::Arc;

/// Per-session match cap (fed design K=50 default), clamped so a hostile
/// client can't request an unbounded summary payload.
const DEFAULT_MAX_PER_SESSION: usize = 50;
const MAX_MATCHES_CEILING: usize = 500;

/// Context windows stay small — inline expansion shows ±20 lines by design.
const MAX_CONTEXT_LINES: usize = 200;

/// Replay content ceiling: the 5MB persisted-log cap plus snapshot headroom.
/// Beyond this the request is rejected rather than truncated (honest failure).
const MAX_REPLAY_CONTENT_BYTES: usize = 8 * 1024 * 1024;

/// Bounded transient-replay cache: cold-session search is ≤500ms on a full
/// 5MB replay, so a handful of warm entries covers a federated re-query burst
/// without letting N dead sessions pin N engines.
const REPLAY_CACHE_CAPACITY: usize = 4;

struct ReplayEntry {
    session_id: String,
    generation: u64,
    terminal: HeadlessTerminal,
}

/// LRU by Vec order (front = most recent); capacity is tiny so O(n) moves are free.
#[derive(Default)]
pub struct SearchReplayCache {
    entries: Vec<ReplayEntry>,
}

impl SearchReplayCache {
    /// Borrow the cached replay terminal for `(session_id, generation)`,
    /// promoting it to most-recent. A stale generation for the same session is
    /// evicted — the checkpoint that bumped it reset the log, so the old replay
    /// can never be valid again.
    fn take(&mut self, session_id: &str, generation: u64) -> Option<ReplayEntry> {
        let idx = self.entries.iter().position(|e| e.session_id == session_id)?;
        let entry = self.entries.remove(idx);
        if entry.generation != generation {
            return None; // dropped: stale-generation replay
        }
        Some(entry)
    }

    fn put(&mut self, entry: ReplayEntry) {
        self.entries.retain(|e| e.session_id != entry.session_id);
        self.entries.insert(0, entry);
        self.entries.truncate(REPLAY_CACHE_CAPACITY);
    }
}

fn search_options(payload: &Value) -> SearchOptions {
    SearchOptions {
        case_sensitive: payload
            .get("caseSensitive")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        regex: payload.get("regex").and_then(Value::as_bool).unwrap_or(false),
    }
}

fn max_matches(payload: &Value, key: &str) -> usize {
    match payload.get(key).and_then(Value::as_u64) {
        Some(v) => (v as usize).clamp(1, MAX_MATCHES_CEILING),
        None => DEFAULT_MAX_PER_SESSION,
    }
}

fn context_span(payload: &Value, key: &str, default: usize) -> usize {
    match payload.get(key).and_then(Value::as_u64) {
        Some(v) => (v as usize).min(MAX_CONTEXT_LINES),
        None => default,
    }
}

fn match_json(m: &MatchSummary) -> Value {
    json!({ "absRow": m.abs_row, "col": m.col, "len": m.len, "line": m.line })
}

/// `searchSessions {query, caseSensitive?, regex?, sessionIds?, maxPerSession?,
/// cutoffRows?, gen?}` → `{sessions: [{sessionId, matches, total, incomplete}], gen}`.
///
/// `sessionIds` is the controller's allowlist (dedup rule: attached sessions
/// are excluded, or included with a `cutoffRows[sid]` depth-extension cutoff so
/// only rows older than the live window report). Read-only: any authenticated
/// client may search — same authority as `getSnapshot`.
pub fn search_sessions(id: &str, payload: &Value, registry: &Arc<Registry>) -> String {
    let query = field_str(payload, "query");
    let opts = search_options(payload);
    let per_session = max_matches(payload, "maxPerSession");
    let allowlist: Option<Vec<String>> = payload.get("sessionIds").and_then(Value::as_array).map(
        |arr| arr.iter().filter_map(Value::as_str).map(str::to_string).collect(),
    );
    let cutoffs = payload.get("cutoffRows").and_then(Value::as_object);
    let mut sessions = Vec::new();
    for (session_id, engine) in registry.engines_for_search(allowlist.as_deref()) {
        let cutoff = cutoffs
            .and_then(|m| m.get(&session_id))
            .and_then(Value::as_u64)
            .map(|v| v as usize);
        let outcome = match engine.lock() {
            Ok(mut engine) => engine.terminal.search_scrollback(query, opts, per_session, cutoff),
            Err(_) => continue, // poisoned engine: skip, never fail the whole query
        };
        // Sessions with zero matches are omitted — summaries only, no noise.
        if outcome.total == 0 {
            continue;
        }
        sessions.push(json!({
            "sessionId": session_id,
            "matches": outcome.matches.iter().map(match_json).collect::<Vec<_>>(),
            "total": outcome.total,
            "incomplete": outcome.incomplete,
        }));
    }
    rpc_ok(id, json!({ "sessions": sessions, "gen": payload.get("gen").cloned() }))
}

/// `searchContext {sessionId, absRow, before?, after?}` → `{lines, firstAbsRow}`
/// for a WARM session (the fed inline context expansion). Unknown session errors
/// like `getCwd` — the caller degrades to the replay path or a toast.
pub fn search_context(id: &str, payload: &Value, registry: &Arc<Registry>) -> String {
    let session_id = field_str(payload, "sessionId");
    let Some(abs_row) = payload.get("absRow").and_then(Value::as_u64) else {
        return rpc_err(id, "missing absRow");
    };
    let before = context_span(payload, "before", 20);
    let after = context_span(payload, "after", 20);
    match registry.engine_of(session_id) {
        Some(engine) => {
            let (lines, first) = match engine.lock() {
                Ok(mut engine) => engine.terminal.search_context(abs_row as usize, before, after),
                Err(_) => return rpc_err(id, "engine unavailable"),
            };
            rpc_ok(id, json!({ "lines": lines, "firstAbsRow": first }))
        }
        None => rpc_err(id, "unknown session"),
    }
}

/// Replay `content` (or reuse the generation-keyed cache) into a transient
/// terminal and run `f` against it. The two-step contract: a request WITHOUT
/// `content` on a cache miss answers `{needsContent:true}` so the client ships
/// the stored ANSI exactly once per checkpoint generation.
fn with_replay_terminal(
    id: &str,
    payload: &Value,
    registry: &Arc<Registry>,
    f: impl FnOnce(&mut HeadlessTerminal) -> Value,
) -> String {
    let session_id = field_str(payload, "sessionId").to_string();
    if session_id.is_empty() {
        return rpc_err(id, "missing sessionId");
    }
    let Some(generation) = payload.get("generation").and_then(Value::as_u64) else {
        return rpc_err(id, "missing generation");
    };
    let cache = registry.search_replay_cache();
    let mut guard = cache.lock().unwrap();
    let mut entry = match guard.take(&session_id, generation) {
        Some(entry) => entry,
        None => match payload.get("content") {
            Some(content) => match build_replay(&session_id, generation, content) {
                Ok(entry) => entry,
                Err(e) => return rpc_err(id, e),
            },
            None => return rpc_ok(id, json!({ "needsContent": true })),
        },
    };
    let result = f(&mut entry.terminal);
    guard.put(entry);
    result_with_ok(id, result)
}

fn result_with_ok(id: &str, mut payload: Value) -> String {
    // Cache-backed replies always mark content as consumed/unneeded.
    payload["needsContent"] = json!(false);
    rpc_ok(id, payload)
}

/// Feed `content.chunks` (checkpoint scrollbackAnsi + snapshotAnsi + log-record
/// tail, in order) through a fresh headless parse — the policy-mandated Rust
/// ANSI strip for cold/parked search.
fn build_replay(
    session_id: &str,
    generation: u64,
    content: &Value,
) -> Result<ReplayEntry, &'static str> {
    let Some(chunks) = content.get("chunks").and_then(Value::as_array) else {
        return Err("missing content.chunks");
    };
    let texts: Vec<&str> = chunks.iter().filter_map(Value::as_str).collect();
    let total: usize = texts.iter().map(|t| t.len()).sum();
    if total > MAX_REPLAY_CONTENT_BYTES {
        return Err("replay content too large");
    }
    let rows = field_u16(content, "rows", 24) as usize;
    let cols = field_u16(content, "cols", 80) as usize;
    let terminal = replay_for_search(
        rows,
        cols,
        scrollback_rows(content),
        texts.iter().map(|t| t.as_bytes()),
    );
    Ok(ReplayEntry { session_id: session_id.to_string(), generation, terminal })
}

/// `searchReplay {sessionId, generation, query, opts…, maxMatches?, cutoffRow?,
/// content?}` → `{matches, total, incomplete, needsContent:false}` — the cold /
/// parked-session search (fed §2.2 parked adapter, §2.3 cold sessions).
pub fn search_replay(id: &str, payload: &Value, registry: &Arc<Registry>) -> String {
    let query = field_str(payload, "query").to_string();
    let opts = search_options(payload);
    let max = max_matches(payload, "maxMatches");
    let cutoff = payload.get("cutoffRow").and_then(Value::as_u64).map(|v| v as usize);
    with_replay_terminal(id, payload, registry, |terminal| {
        let outcome = terminal.search_scrollback(&query, opts, max, cutoff);
        json!({
            "matches": outcome.matches.iter().map(match_json).collect::<Vec<_>>(),
            "total": outcome.total,
            "incomplete": outcome.incomplete,
        })
    })
}

/// `searchReplayContext {sessionId, generation, absRow, before?, after?,
/// content?}` → `{lines, firstAbsRow, needsContent:false}` — inline context
/// expansion for DEAD sessions (no pane, identity = sessionId).
pub fn search_replay_context(id: &str, payload: &Value, registry: &Arc<Registry>) -> String {
    let Some(abs_row) = payload.get("absRow").and_then(Value::as_u64) else {
        return rpc_err(id, "missing absRow");
    };
    let before = context_span(payload, "before", 20);
    let after = context_span(payload, "after", 20);
    with_replay_terminal(id, payload, registry, |terminal| {
        let (lines, first) = terminal.search_context(abs_row as usize, before, after);
        json!({ "lines": lines, "firstAbsRow": first })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pending_output::PendingOutput;
    use crate::registry::{SessionEngine, SessionEntry};
    use orca_pty::{PtyCommand, PtySession, PtySize};
    use std::sync::Mutex;

    fn parse(response: &str) -> Value {
        serde_json::from_str(response).expect("valid JSON response")
    }

    #[cfg(unix)]
    fn insert_session_with_output(registry: &Arc<Registry>, sid: &str, output: &str) {
        let pty = PtySession::spawn(
            &PtyCommand {
                program: "/bin/sh".to_string(),
                args: vec!["-c".to_string(), "sleep 30".to_string()],
                ..Default::default()
            },
            PtySize { rows: 24, cols: 80 },
        )
        .expect("spawn");
        let mut terminal = HeadlessTerminal::with_scrollback(4, 40, 1000);
        terminal.process_str(output);
        let pid = pty.process_id();
        registry.insert_session(
            sid.to_string(),
            SessionEntry {
                pty,
                client_id: "test-client".to_string(),
                cols: 40,
                rows: 4,
                pid,
                created_at_ms: 0,
                engine: Arc::new(Mutex::new(SessionEngine {
                    terminal,
                    pending: PendingOutput::default(),
                })),
                barrier: None,
                terminating: false,
            },
        );
    }

    #[cfg(unix)]
    #[test]
    fn search_sessions_returns_summaries_for_matching_sessions_only() {
        let registry = Arc::new(Registry::new());
        insert_session_with_output(&registry, "s-hit", "alpha needle beta\r\nplain\r\n");
        insert_session_with_output(&registry, "s-miss", "nothing here\r\n");
        let response = search_sessions(
            "r1",
            &json!({ "query": "needle", "gen": 7 }),
            &registry,
        );
        let v = parse(&response);
        assert_eq!(v["ok"], true);
        assert_eq!(v["payload"]["gen"], 7);
        let sessions = v["payload"]["sessions"].as_array().unwrap();
        assert_eq!(sessions.len(), 1, "zero-match sessions are omitted: {sessions:?}");
        assert_eq!(sessions[0]["sessionId"], "s-hit");
        assert_eq!(sessions[0]["total"], 1);
        assert_eq!(sessions[0]["incomplete"], false);
        let m = &sessions[0]["matches"][0];
        assert_eq!(m["col"], 6);
        assert_eq!(m["len"], 6);
        assert_eq!(m["line"], "alpha needle beta");
        registry.kill_all_sessions();
    }

    #[cfg(unix)]
    #[test]
    fn search_sessions_allowlist_and_cutoff_are_honored() {
        let registry = Arc::new(Registry::new());
        insert_session_with_output(&registry, "s-a", "needle one\r\nneedle two\r\n");
        insert_session_with_output(&registry, "s-b", "needle b\r\n");
        // Allowlist excludes s-b entirely.
        let only_a = parse(&search_sessions(
            "r1",
            &json!({ "query": "needle", "sessionIds": ["s-a"] }),
            &registry,
        ));
        let sessions = only_a["payload"]["sessions"].as_array().unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0]["sessionId"], "s-a");
        // Depth-extension cutoff: only rows STRICTLY older than row 1 report.
        let cut = parse(&search_sessions(
            "r2",
            &json!({ "query": "needle", "sessionIds": ["s-a"], "cutoffRows": { "s-a": 1 } }),
            &registry,
        ));
        let cut_sessions = cut["payload"]["sessions"].as_array().unwrap();
        assert_eq!(cut_sessions[0]["total"], 1);
        assert_eq!(cut_sessions[0]["matches"][0]["absRow"], 0);
        registry.kill_all_sessions();
    }

    #[cfg(unix)]
    #[test]
    fn search_context_returns_window_for_warm_session() {
        let registry = Arc::new(Registry::new());
        insert_session_with_output(&registry, "s", "l0\r\nl1\r\nl2\r\nl3\r\nl4\r\nl5\r\n");
        let v = parse(&search_context(
            "r1",
            &json!({ "sessionId": "s", "absRow": 2, "before": 1, "after": 1 }),
            &registry,
        ));
        assert_eq!(v["ok"], true);
        assert_eq!(v["payload"]["firstAbsRow"], 1);
        assert_eq!(v["payload"]["lines"], json!(["l1", "l2", "l3"]));
        registry.kill_all_sessions();
    }

    #[test]
    fn search_context_unknown_session_errors() {
        let registry = Arc::new(Registry::new());
        let v = parse(&search_context(
            "r1",
            &json!({ "sessionId": "nope", "absRow": 0 }),
            &registry,
        ));
        assert_eq!(v["ok"], false);
        assert_eq!(v["error"], "unknown session");
    }

    #[test]
    fn search_replay_needs_content_once_then_hits_the_generation_cache() {
        let registry = Arc::new(Registry::new());
        let base = json!({ "sessionId": "dead-1", "generation": 3, "query": "needle" });
        // Cache miss without content → needsContent handshake, not an error.
        let miss = parse(&search_replay("r1", &base, &registry));
        assert_eq!(miss["ok"], true);
        assert_eq!(miss["payload"]["needsContent"], true);
        // Content ships once; ANSI is stripped by the headless parse.
        let mut with_content = base.clone();
        with_content["content"] = json!({
            "rows": 4, "cols": 40, "scrollbackRows": 1000,
            "chunks": ["\x1b[31mold needle\x1b[0m\r\n", "tail line\r\n"]
        });
        let hit = parse(&search_replay("r2", &with_content, &registry));
        assert_eq!(hit["payload"]["needsContent"], false);
        assert_eq!(hit["payload"]["total"], 1);
        assert_eq!(hit["payload"]["matches"][0]["line"], "old needle");
        // Repeat query WITHOUT content now hits the cache.
        let cached = parse(&search_replay("r3", &json!({
            "sessionId": "dead-1", "generation": 3, "query": "tail"
        }), &registry));
        assert_eq!(cached["payload"]["needsContent"], false);
        assert_eq!(cached["payload"]["total"], 1);
        // A NEW generation invalidates the cached replay (checkpoint reset the log).
        let stale = parse(&search_replay("r4", &json!({
            "sessionId": "dead-1", "generation": 4, "query": "tail"
        }), &registry));
        assert_eq!(stale["payload"]["needsContent"], true);
    }

    #[test]
    fn search_replay_context_serves_dead_session_expansion_from_cache() {
        let registry = Arc::new(Registry::new());
        let seed = json!({
            "sessionId": "dead-2", "generation": 1, "query": "b",
            "content": { "rows": 2, "cols": 20, "chunks": ["a\r\nb\r\nc\r\nd\r\ne"] }
        });
        assert_eq!(parse(&search_replay("r1", &seed, &registry))["ok"], true);
        let v = parse(&search_replay_context("r2", &json!({
            "sessionId": "dead-2", "generation": 1, "absRow": 1, "before": 1, "after": 1
        }), &registry));
        assert_eq!(v["payload"]["needsContent"], false);
        assert_eq!(v["payload"]["firstAbsRow"], 0);
        assert_eq!(v["payload"]["lines"], json!(["a", "b", "c"]));
    }

    #[test]
    fn search_replay_rejects_oversized_content() {
        let registry = Arc::new(Registry::new());
        let big = "x".repeat(MAX_REPLAY_CONTENT_BYTES + 1);
        let v = parse(&search_replay("r1", &json!({
            "sessionId": "s", "generation": 1, "query": "x",
            "content": { "chunks": [big] }
        }), &registry));
        assert_eq!(v["ok"], false);
        assert_eq!(v["error"], "replay content too large");
    }

    #[test]
    fn search_replay_requires_generation() {
        let registry = Arc::new(Registry::new());
        let v = parse(&search_replay("r1", &json!({ "sessionId": "s", "query": "x" }), &registry));
        assert_eq!(v["ok"], false);
        assert_eq!(v["error"], "missing generation");
    }

    #[test]
    fn replay_cache_is_bounded_and_lru() {
        let mut cache = SearchReplayCache::default();
        for i in 0..6 {
            cache.put(ReplayEntry {
                session_id: format!("s{i}"),
                generation: 1,
                terminal: HeadlessTerminal::new(2, 20),
            });
        }
        assert_eq!(cache.entries.len(), REPLAY_CACHE_CAPACITY);
        // Oldest (s0, s1) evicted; newest retained.
        assert!(cache.take("s0", 1).is_none());
        assert!(cache.take("s5", 1).is_some());
    }
}
