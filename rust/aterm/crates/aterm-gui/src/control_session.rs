// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Andrew Yates

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
/// Exactly two ready reasons are emitted:
///   * `prompt` — the newest OSC-133 block is at a fresh prompt (`PromptOnly`)
///     or a finished command (`Complete`): the shell is waiting for input. The
///     precise "prompt-end" signal, used when shell integration is present.
///   * `idle`   — the kernel's `IdleFor` watcher latched: `content_seq` held
///     stable across the settle window (output stopped changing). This is the
///     fallback for a session with no in-flight completed block — covering plain
///     shells (no shell integration) and the between-commands case alike.
///
/// Fully event-driven (NO poll): arms an `IdleFor` watcher, registers a
/// subscriber, and parks on its wake — driven by output / exit notifications and
/// the idle deadline. The registry lifecycle is re-checked on each wake, and a
/// session exit `notify`s us (`Wake::Exit`), so an exit is reported promptly.
pub(crate) fn cmd_ready(
    term: &Arc<Mutex<Terminal>>,
    store: &Store,
    session: u64,
    rest: &str,
    subscribers: &crate::subscribe::Subscribers,
) -> String {
    use std::time::{Duration, Instant};

    use aterm_core::terminal::{BlockState, WatcherSpec};

    use crate::session_store::SessionState;
    use crate::subscribe::SubscriberSet;

    let timeout_ms = rest.trim().parse::<u64>().unwrap_or(30_000).min(600_000);
    let now0 = Instant::now();
    let deadline = now0 + Duration::from_millis(timeout_ms);
    // The no-shell-integration settle window. This now drives the model-checked
    // kernel `IdleFor` (deterministic, no-silent-loss) INSTEAD of the old racy
    // "3 stable 20ms samples" — the engine resets the deadline on every content
    // advance (`observe_at`), so it latches only after SETTLE of TRUE quiet.
    const SETTLE: Duration = Duration::from_millis(60);

    // The lifecycle state of THIS resolved session, by its local id. `None` (not in
    // the registry — e.g. a headless unit term) is treated as Alive — UNLESS the
    // session was registered at arm and is now gone (deregistered on teardown),
    // which is a dead session the `Wake::Exit` notify woke us for (see `gone`).
    let was_registered = store
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .by_local(session)
        .is_some();
    let gone = |store: &Store| -> bool {
        let g = store.read().unwrap_or_else(|p| p.into_inner());
        match g.by_local(session).map(|h| h.state) {
            Some(SessionState::Exited) => true,
            None => was_registered,
            _ => false,
        }
    };

    // Arm one idle watcher up front; the engine auto-resets its deadline on output.
    let idle_id = term_lock(term).watch(WatcherSpec::IdleFor { dur: SETTLE }, now0);
    let disarm = |term: &Arc<Mutex<Terminal>>| {
        if let Some(id) = idle_id {
            term_lock(term).watch_disarm(id);
        }
    };
    // Subscribe so output wakes us (block-state changes ride content_seq, so the
    // notify covers the shell-integration path too) — event-driven, no poll.
    let sub = SubscriberSet::register(subscribers, &[session]);

    loop {
        if gone(store) {
            disarm(term);
            return "ERR exited\n".to_string();
        }
        let now = Instant::now();
        let (prompt, settled, next_dl) = {
            let mut t = term_lock(term);
            // Shell-integration fast path: newest block prompt/complete => ready
            // prompt (read directly so an ALREADY-ready session returns at once).
            let prompt = matches!(
                t.all_blocks().last().map(|b| b.state),
                Some(BlockState::PromptOnly | BlockState::Complete)
            );
            t.watch_expire(now); // host-injected idle fire
            let settled = idle_id.and_then(|id| t.watch_poll(id)).is_some();
            (prompt, settled, t.watch_next_deadline())
        };
        if prompt {
            disarm(term);
            return "OK ready prompt\n".to_string();
        }
        if settled {
            disarm(term);
            return "OK ready idle\n".to_string();
        }
        if now >= deadline {
            disarm(term);
            return "OK timeout\n".to_string();
        }
        // Park until a REAL event wakes us — fully event-driven, no re-poll: an
        // output burst or session exit (both `notify` us), the kernel idle
        // deadline (`next_dl`, always armed here so block-state transitions are
        // re-checked within the settle window), or the overall deadline.
        let mut wake = deadline;
        if let Some(dl) = next_dl {
            wake = wake.min(dl);
        }
        let dur = wake
            .saturating_duration_since(now)
            .max(Duration::from_millis(1));
        let _ = sub.wait(dur);
    }
}

/// `await <idle <ms> | seq [<n>] | match <re> [rows <a> <b>] | block> [timeout <ms>]`
/// — block until the Observation Kernel (L0) latches the predicate, then return
/// `OK <kind> <seq>`; `OK timeout` if the overall deadline elapses; `ERR exited`
/// if the session dies.
///
/// This is the L1 exposure of the core primitive. The CORRECTNESS — no-silent-
/// loss for content/match/block, and a deterministic idle deadline — lives in the
/// kernel (`observe_at` at the `post_process` seam, model-checked by
/// `watcher_latch_model` / `idle_deadline_model`). This verb only *waits*, and it
/// is **fully event-driven, with no polling**: it registers a subscriber and
/// parks on its wake, so it sleeps until a REAL event arrives —
///   * an output burst   (`Wake::Output` → `Subscribers::notify`) for content/
///     match/block predicates,
///   * the next idle deadline (the exact `IdleFor` fire instant, via
///     `watch_next_deadline`) for `await idle`,
///   * session exit       (`Wake::Exit` → notify) → `ERR exited`,
///   * the overall timeout (the ultimate liveness backstop) → `OK timeout`.
///
/// CPU is ~0% while parked; every wake corresponds to an event the caller cares
/// about.
pub(crate) fn cmd_await(
    term: &Arc<Mutex<Terminal>>,
    store: &Store,
    session: u64,
    rest: &str,
    subscribers: &crate::subscribe::Subscribers,
) -> String {
    use std::time::{Duration, Instant};

    use aterm_core::terminal::{RowRange, WatcherSpec};

    use crate::session_store::SessionState;
    use crate::subscribe::SubscriberSet;

    const USAGE: &str =
        "ERR usage: await <idle <ms>|seq [<n>]|match <re> [rows <a> <b>]|block> [timeout <ms>]\n";

    // Split off an optional `timeout <ms>` anywhere in the args; the rest is the
    // predicate + its arguments.
    let toks: Vec<&str> = rest.split_whitespace().collect();
    let mut timeout_ms = 30_000u64;
    let mut args: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        if toks[i] == "timeout" && i + 1 < toks.len() {
            timeout_ms = toks[i + 1].parse().unwrap_or(30_000);
            i += 2;
        } else {
            args.push(toks[i]);
            i += 1;
        }
    }
    let timeout_ms = timeout_ms.min(600_000);
    let Some(&kind) = args.first() else {
        return USAGE.to_string();
    };

    let now0 = Instant::now();
    // Arm the predicate (under the term lock, released immediately).
    let armed = {
        let mut t = term_lock(term);
        match kind {
            "idle" => {
                let Some(ms) = args.get(1).and_then(|s| s.parse::<u64>().ok()) else {
                    return USAGE.to_string();
                };
                t.watch(
                    WatcherSpec::IdleFor {
                        dur: Duration::from_millis(ms),
                    },
                    now0,
                )
            }
            "seq" => {
                // Default `after` = the current content_seq (wait for the NEXT change).
                let after = args
                    .get(1)
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or_else(|| t.content_seq());
                t.watch(WatcherSpec::SeqAdvanced { after }, now0)
            }
            "block" => t.watch(WatcherSpec::BlockComplete, now0),
            "match" => {
                let Some(pat) = args.get(1) else {
                    return USAGE.to_string();
                };
                let range = match (args.get(2), args.get(3), args.get(4)) {
                    (Some(&"rows"), Some(a), Some(b)) => {
                        match (a.parse::<usize>(), b.parse::<usize>()) {
                            (Ok(start), Ok(end)) => RowRange::Span { start, end },
                            _ => return USAGE.to_string(),
                        }
                    }
                    _ => RowRange::All,
                };
                // Compile the regex in `aterm-observe` (regex out of the engine core).
                match aterm_observe::row_matcher(pat) {
                    Ok(m) => t.watch_rows(m, range, now0),
                    Err(_) => return "ERR badregex\n".to_string(),
                }
            }
            _ => return USAGE.to_string(),
        }
    };
    let Some(id) = armed else {
        return "ERR watcher budget full\n".to_string();
    };

    let overall = now0 + Duration::from_millis(timeout_ms);

    // Register a subscriber on THIS session: the producer's `Wake::Output` and
    // `Wake::Exit` hooks (`Subscribers::notify`) wake us the instant output lands
    // or the session dies — so content predicates (`seq`/`match`/`block`) and
    // exit detection are event-driven with NO fixed-interval poll. `await idle`
    // parks straight to the kernel's `next_deadline`. The single-slot notify is
    // lossless (a wake that arrives between two `wait`s stays pending), and the
    // overall timeout is the ultimate backstop — so no re-query is needed.
    let sub = SubscriberSet::register(subscribers, &[session]);

    // Whether the session was in the registry at arm. A session that WAS
    // registered but is later GONE (deregistered during teardown) is dead — and
    // the `Wake::Exit` notify can race the deregistration, so the woken thread may
    // see `None`. Treating "was registered, now None" as exited closes that race;
    // a headless unit term (never registered) stays Alive on `None` as before.
    let was_registered = store
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .by_local(session)
        .is_some();
    let exited = |store: &Store| -> bool {
        let g = store.read().unwrap_or_else(|p| p.into_inner());
        match g.by_local(session).map(|h| h.state) {
            Some(SessionState::Exited) => true,
            None => was_registered,
            _ => false,
        }
    };

    loop {
        if exited(store) {
            term_lock(term).watch_disarm(id);
            return "ERR exited\n".to_string();
        }
        let now = Instant::now();
        let (sat, next_dl) = {
            let mut t = term_lock(term);
            t.watch_expire(now); // fire any elapsed idle deadline (host-injected `now`)
            (t.watch_poll(id), t.watch_next_deadline())
        };
        if let Some(s) = sat {
            term_lock(term).watch_disarm(id);
            return format!("OK {kind} {}\n", s.seq);
        }
        if now >= overall {
            term_lock(term).watch_disarm(id);
            return "OK timeout\n".to_string();
        }
        // Park until a REAL event wakes us — fully event-driven, no re-poll:
        // an output burst or session exit (both `notify` us), the next idle
        // deadline (`next_dl`), or the overall timeout (the backstop).
        let mut wake = overall;
        if let Some(dl) = next_dl {
            wake = wake.min(dl);
        }
        let dur = wake
            .saturating_duration_since(now)
            .max(Duration::from_millis(1));
        let _ = sub.wait(dur);
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
