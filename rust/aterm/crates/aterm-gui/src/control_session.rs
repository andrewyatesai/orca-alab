// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! Session-graph + capability-authority verbs: `sessions`, `family`, `ready`,
//! `cast`, `edges`/`grants`, `grant`, `revoke`, `whoami`. Moved verbatim from
//! `control.rs` (behavior-preserving). The `Scope` authority enum + the
//! proxy-forward / cross-session auth cluster stay in `control.rs`; this module
//! reaches `Scope` and the shared JSON helpers via `super::`.

use std::sync::{Arc, Mutex};

use aterm_core::terminal::Terminal;
use aterm_session::{EdgeToken, Op, SessionId};

use super::{Scope, json_ok, json_str_field, pct_encode};
use crate::session_store::Store;
use crate::{SessionCtx, term_lock};

/// `sessions` -> list the process-wide registry: `OK <n>\n` then one line per
/// session, sorted by local id: `<local> <sid> <parent|-> <state> <title>`. On a
/// single-session window this is exactly one line == the lone session (the
/// zero-regression base case). The store snapshot is cloned out before formatting,
/// so this never holds the registry lock across a `Terminal` lock.
pub(crate) fn cmd_sessions(_self_ctx: &SessionCtx, store: &Store) -> String {
    let snapshot = {
        let g = store.read().unwrap_or_else(|p| p.into_inner());
        g.snapshot()
    };
    let mut out = format!("OK {}\n", snapshot.len());
    for h in &snapshot {
        let parent = h
            .parent
            .as_ref()
            .map_or("-", aterm_session::SessionId::as_str);
        let title = pct_encode(&h.title);
        out.push_str(&format!(
            "{} {} {} {} {}\n",
            h.local_id,
            h.sid.as_str(),
            parent,
            h.state.as_str(),
            title,
        ));
    }
    out
}

/// `edges` / `grants` -> list this session's INBOUND capability edges (the rows
/// of its [`EdgeTable`]), the query face of the authority data `grant`/`revoke`
/// mint and remove (which had zero read surface before).
///
/// Header `OK <n>\n`, then one line per edge: `<src> <dst> <op>` where `<op>` is
/// the wire op token (`read-screen`/`write-input`/`signal`/`derive-loop`) and
/// `<dst>` is always THIS session's id (every row in the table targets it). The
/// bearer TOKEN is DELIBERATELY never emitted — it is the unforgeable secret; an
/// agent enumerates WHO may reach this session for WHAT, not the secrets. Sorted
/// by `(src, op)` for a stable listing. Cross-session (`@<sel>`) reads a sibling's
/// table through the same `@<selector>` resolution + `ReadScreen` gate.
pub(crate) fn cmd_edges(ctx: &SessionCtx) -> String {
    let mut edges = {
        let tbl = ctx.edges.lock().unwrap_or_else(|p| p.into_inner());
        tbl.edges()
    };
    edges.sort_by(|a, b| (a.src.as_str(), a.op.as_str()).cmp(&(b.src.as_str(), b.op.as_str())));
    let mut out = format!("OK {}\n", edges.len());
    for e in &edges {
        out.push_str(&format!(
            "{} {} {}\n",
            e.src.as_str(),
            e.dst.as_str(),
            e.op.as_str()
        ));
    }
    out
}

/// `edges --json` / `grants --json` -> `{"edges":[{"src":"..","dst":"..",
/// "op":".."}],"dst":"<self>"}`. The SAME edges `cmd_edges` lists (sorted, no
/// token), as a structured object an agent can consume without line-splitting.
pub(crate) fn cmd_edges_json(ctx: &SessionCtx) -> String {
    let (self_id, mut edges) = {
        let tbl = ctx.edges.lock().unwrap_or_else(|p| p.into_inner());
        (ctx.self_id.clone(), tbl.edges())
    };
    edges.sort_by(|a, b| (a.src.as_str(), a.op.as_str()).cmp(&(b.src.as_str(), b.op.as_str())));
    let items: Vec<String> = edges
        .iter()
        .map(|e| {
            format!(
                "{{{},{},{}}}",
                json_str_field("src", e.src.as_str()),
                json_str_field("dst", e.dst.as_str()),
                json_str_field("op", e.op.as_str()),
            )
        })
        .collect();
    json_ok(&format!(
        "{{\"edges\":[{}],{}}}",
        items.join(","),
        json_str_field("dst", self_id.as_str()),
    ))
}

/// `family [<sid>]` -> the session HIERARCHY for a target: its parent and its
/// direct children, from the registry's `parent` links (only a flat `sessions`
/// list was queryable before).
///
/// With NO argument the target is the RESOLVED session (`@<sel>` or self); with an
/// explicit `<sid>` argument the target is that session id (so an Owner can walk
/// the tree from any node without re-addressing). Header `OK\n`, then:
///   `self <sid> <state> <title>`
///   `parent <sid|-> ...`            (one line; `-` sid when the node is a root)
///   `child <sid> <state> <title>`  (zero or more, sorted by local id)
/// Titles are percent-encoded (single space-free tokens), matching `sessions`.
/// An unknown target id yields `ERR no such session\n` (fail-closed). An EXPLICIT
/// `<sid>` argument is Owner-only (a scoped Edge gets `ERR denied`); the no-arg
/// form is scoped to the already-gated resolved session.
pub(crate) fn cmd_family(ctx: &SessionCtx, store: &Store, scope: Scope, rest: &str) -> String {
    // Target sid: an explicit argument (Owner-only — arbitrary-node enumeration),
    // else the resolved session's own id (already gated by the dispatch).
    let target_sid = match rest.trim() {
        "" => ctx.self_id.clone(),
        s => {
            if scope != Scope::Owner {
                return "ERR denied\n".to_string();
            }
            SessionId::new(s)
        }
    };
    let snapshot = {
        let g = store.read().unwrap_or_else(|p| p.into_inner());
        g.snapshot()
    };
    let Some(node) = snapshot.iter().find(|h| h.sid == target_sid) else {
        return "ERR no such session\n".to_string();
    };
    let line = |kind: &str, h: &crate::session_store::SessionHandle| {
        format!(
            "{kind} {} {} {}\n",
            h.sid.as_str(),
            h.state.as_str(),
            pct_encode(&h.title),
        )
    };
    let mut out = String::from("OK\n");
    out.push_str(&line("self", node));
    // Parent row: the parent sid + its live state/title if still registered, else
    // a bare `-` (root, or a parent that has since deregistered).
    match node.parent.as_ref() {
        Some(psid) => match snapshot.iter().find(|h| h.sid == *psid) {
            Some(ph) => out.push_str(&line("parent", ph)),
            None => out.push_str(&format!("parent {} unknown -\n", psid.as_str())),
        },
        None => out.push_str("parent - - -\n"),
    }
    // Direct children: every registered session whose parent is this node, sorted
    // by local id (snapshot is already local-id sorted).
    for h in snapshot
        .iter()
        .filter(|h| h.parent.as_ref() == Some(&target_sid))
    {
        out.push_str(&line("child", h));
    }
    out
}

/// `ready [timeout_ms]` -> block until the target session is ALIVE and IDLE, then
/// `OK ready <reason>\n`; `OK timeout\n` if it does not become ready in time
/// (default 30 000 ms, capped at 600 000); `ERR exited\n` if the session has
/// exited (it will never become ready). Lets an agent CHAIN sessions — spawn one,
/// `ready` on it, then drive it — without busy-polling a screen read.
///
/// IDLE is defined by shell integration when present and lifecycle otherwise:
///   * `prompt`  — the newest OSC-133 block is at a fresh prompt (`PromptOnly`)
///     or a finished command (`Complete`): the shell is waiting for
///     input. This is the precise "prompt-end" signal.
///   * `no-command` — shell integration is present but NO block is in flight
///     (`Executing`/`EnteringCommand`): nothing is running.
///   * `idle`    — no shell integration at all, but `content_seq` has been STABLE
///     across a short settle window (the output stopped changing), so
///     the best-effort idle heuristic fires for plain shells too.
///
/// Polls server-side, releasing the Terminal lock between checks so the PTY reader
/// keeps advancing; checks the registry lifecycle each pass so an exit is reported
/// promptly rather than waited out.
pub(crate) fn cmd_ready(
    term: &Arc<Mutex<Terminal>>,
    store: &Store,
    session: u64,
    rest: &str,
) -> String {
    use crate::session_store::SessionState;
    use aterm_core::terminal::BlockState;
    let timeout_ms = rest.trim().parse::<u64>().unwrap_or(30_000).min(600_000);
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);

    // The lifecycle state of THIS resolved session, by its local id. `None` (not in
    // the registry — e.g. a headless unit term) is treated as Alive: the block /
    // settle heuristics still decide readiness.
    let lifecycle = |store: &Store| -> Option<SessionState> {
        let g = store.read().unwrap_or_else(|p| p.into_inner());
        g.by_local(session).map(|h| h.state)
    };

    // Settle tracking for the no-shell-integration case: ready only once content_seq
    // has held the SAME value across `SETTLE` consecutive idle polls.
    const SETTLE: u32 = 3;
    let mut last_seq: Option<u64> = None;
    let mut stable: u32 = 0;

    loop {
        if matches!(lifecycle(store), Some(SessionState::Exited)) {
            return "ERR exited\n".to_string();
        }
        {
            let t = term_lock(term);
            // Newest block decides the shell-integration verdict (`all_blocks` yields
            // oldest-first, so the LAST item is the newest — the in-flight/current one).
            let newest_state = t.all_blocks().last().map(|b| b.state);
            match newest_state {
                Some(BlockState::PromptOnly | BlockState::Complete) => {
                    return "OK ready prompt\n".to_string();
                }
                Some(BlockState::Executing | BlockState::EnteringCommand) => {
                    // A command is in flight: not ready. Reset the settle counter so a
                    // later quiet period is measured fresh.
                    stable = 0;
                    last_seq = None;
                }
                _ => {
                    // No shell integration: settle on a stable content_seq.
                    let seq = t.content_seq();
                    if last_seq == Some(seq) {
                        stable += 1;
                    } else {
                        stable = 0;
                        last_seq = Some(seq);
                    }
                    if stable >= SETTLE {
                        return "OK ready idle\n".to_string();
                    }
                }
            }
        }
        if std::time::Instant::now() >= deadline {
            return "OK timeout\n".to_string();
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

/// `cast` -> `OK <nbytes>\n` then the session's full asciicast v2 recording as
/// the body (design A.5.1 / B.7). The body is the JSON header line followed by
/// one `[t, "o", …]`/`[t, "r", …]` event per recorded burst — exactly what
/// `asciinema play -`/`agg` consume. `<nbytes>` is the byte length of the body
/// that follows (UTF-8), matching the read-verb framing so the existing client
/// can read the body without guessing where it ends. Output-only and bounded
/// (drop-oldest) by the recorder; this verb only serializes the snapshot, never
/// the renderer, so it is cheap and lock-disjoint from the PTY write path.
pub(crate) fn cmd_cast(ctx: &SessionCtx) -> String {
    let body = {
        let rec = ctx.cast.lock().unwrap_or_else(|p| p.into_inner());
        rec.to_asciicast()
    };
    format!("OK {}\n{}", body.len(), body)
}

/// `grant <src-id> <op>` -> mint an edge (src -> this session, op) and return its
/// bearer token hex. Owner-only (also enforced by the gate's catch-all Deny).
pub(crate) fn cmd_grant(ctx: &SessionCtx, scope: Scope, rest: &str) -> String {
    if scope != Scope::Owner {
        return "ERR denied\n".to_string();
    }
    let mut it = rest.split_whitespace();
    let (Some(src), Some(op_s)) = (it.next(), it.next()) else {
        return "ERR usage: grant <src-id> <op>\n".to_string();
    };
    let Some(op) = Op::parse(op_s) else {
        return "ERR unknown op\n".to_string();
    };
    let mut edges = ctx.edges.lock().unwrap_or_else(|p| p.into_inner());
    let tok = edges.grant(SessionId::new(src), ctx.self_id.clone(), op, ctx.nonce);
    format!("OK {}\n", tok.to_hex())
}

/// `revoke <edge-hex>` -> remove an edge. Owner-only.
pub(crate) fn cmd_revoke(ctx: &SessionCtx, scope: Scope, rest: &str) -> String {
    if scope != Scope::Owner {
        return "ERR denied\n".to_string();
    }
    let Some(tok) = EdgeToken::from_hex(rest.trim()) else {
        return "ERR bad token\n".to_string();
    };
    let mut edges = ctx.edges.lock().unwrap_or_else(|p| p.into_inner());
    if edges.revoke(&tok) {
        "OK\n".to_string()
    } else {
        "ERR no such edge\n".to_string()
    }
}

/// `whoami` -> report this session's fabric id + nonce + the connection's EFFECTIVE
/// scope against the session active RIGHT NOW. For an edge, the op is re-derived from
/// the presented token via `authorize` (the same per-request authority the gate uses)
/// rather than a cached connect-time op — so whoami can never over-report power the
/// token no longer holds after the ActiveHandle swung `@.` to a different session
/// (`edge unauthorized` when the token grants nothing against the now-active table).
pub(crate) fn cmd_whoami(ctx: &SessionCtx, scope: Scope) -> String {
    let s = match scope {
        Scope::Owner => "owner".to_string(),
        Scope::Edge(presented) => {
            let table = ctx.edges.lock().unwrap_or_else(|p| p.into_inner());
            match table.authorize(&presented, &ctx.self_id, &ctx.nonce) {
                Some(op) => format!("edge {}", op.as_str()),
                None => "edge unauthorized".to_string(),
            }
        }
    };
    format!("OK {} {} {}\n", ctx.self_id.as_str(), ctx.nonce.to_hex(), s)
}
