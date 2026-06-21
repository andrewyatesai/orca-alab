// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! Live introspection CONTROL SOCKET (aterm introspection control protocol v1).
//!
//! A background thread binds a Unix domain socket and serves newline-delimited
//! text requests so an out-of-process intelligence can read the live screen
//! (text/cursor/cell/search), drive the shell (send/key), snapshot the pixels
//! (image), drive text selection (select — plain ranges plus the gesture
//! forms `word`/`line`/`block`/`extend` — and selection/copy), and resize the
//! engine + PTY — all against the SAME running terminal the window presents.
//! This is aterm introspecting itself: no OS screen recording, just the
//! engine's own grid and renderer.
//!
//! Threading: text/cursor/cell/search read the shared [`Terminal`] directly;
//! send/key/resize poke the PTY master fd. The `image` verb needs the renderer,
//! which lives on the MAIN thread, so this thread cannot render. Instead it
//! pushes an [`ImageReq`] onto a shared queue, wakes the event loop with
//! [`Wake::Control`], and blocks on the reply channel; the main thread drains
//! the queue, renders, writes the PNG, and replies with the frame dimensions.
//!
//! `image` (and the SIGUSR1 snapshot) is WYSIWYG: it renders the exact pixels
//! the window shows, INCLUDING the current cursor blink phase and the hollow
//! unfocused-cursor override — so a focused blinking session may legitimately
//! capture a frame with no cursor pixels. Headless sessions pin the blink
//! phase on (always deterministic). For deterministic cursor state regardless
//! of phase, use the `cursor` verb (row, col, visible, style).

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, OnceLock};

use aterm_containment::log_denial;
use aterm_core::grid::extra::{ImageData, ImageFormat};
use aterm_core::grid::{CellFlags, Grid, MAX_GRID_COLS, MAX_GRID_ROWS};
use aterm_core::selection::{SelectionSide, SelectionType, SmartSelection};
use aterm_core::terminal::{CursorStyle, RenderCell, Terminal, UnderlineStyle};
use aterm_session::sink::SinkWriter;
use aterm_session::{decide_edge, EdgeToken, Op, SessionId};
use winit::event_loop::EventLoopProxy;

use crate::control_auth::{self, AuthOutcome};
use crate::input::{seam_egress, Egress, InputEvent, InputOutcome, ScrollIntent, Source};
use crate::session_store::Store;
use crate::subscribe::{self, Streams, Subscribers};
use crate::{SessionCtx, TabAction, Wake, term_lock};

/// The containment subsystem name used in audit denials from this socket.
const AUDIT_SUBSYSTEM: &str = "control_socket";

/// The currently-ACTIVE tab's engine + PTY master, shared with the GUI so the
/// control socket's verbs follow tab switches instead of being pinned to the
/// session that happened to exist at startup. The GUI updates this on every tab
/// switch / open / close (`App::sync_active_session`); each request resolves the
/// current target from it ([`resolve_active`]). This changes ONLY which session a
/// verb targets — the auth gates (peer-uid + per-launch token) are untouched.
pub struct ActiveSession {
    pub term: Arc<Mutex<Terminal>>,
    pub master: i32,
    /// The active session's stable id, so a control verb that DIRECTLY mutates the
    /// engine (scroll/select) can request a repaint of the right tab.
    pub id: u64,
    /// The active session's fabric context (sink + edge table + identity), so the
    /// op-scope gate and the writer verbs resolve the live tab's sink/table.
    pub ctx: Arc<SessionCtx>,
}

/// Shared handle to the [`ActiveSession`]; cloned into the control thread.
pub type ActiveHandle = Arc<Mutex<ActiveSession>>;

/// Snapshot the current active engine + PTY master + id for one request.
/// Poison-recovery (matches `term_lock`): a panicked GUI thread must not wedge
/// introspection.
fn resolve_active(active: &ActiveHandle) -> (Arc<Mutex<Terminal>>, i32, u64, Arc<SessionCtx>) {
    let g = active.lock().unwrap_or_else(|p| p.into_inner());
    (g.term.clone(), g.master, g.id, g.ctx.clone())
}

/// What a connection is authorized to do, resolved at handshake.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Scope {
    /// The per-instance god token: full control of the active session (all verbs,
    /// incl. grant/revoke). Same-uid clients with the instance token keep zero-friction
    /// full power (no regression to aterm-ctl). Because the instance token is the
    /// launcher's per-process authority, an Owner connection may ALSO reach SIBLING
    /// sessions in the same process (the same-uid / same-trust-domain god token);
    /// a scoped `Edge` connection needs an explicit edge per target (see
    /// `resolve_target`).
    Owner,
    /// An edge token scoped to exactly one op against the connection's HANDSHAKE
    /// target (`decide_edge` semantics). ONLY the presented [`EdgeToken`] is carried —
    /// authority is ALWAYS re-derived from it per request (every verb runs
    /// `decide_edge`/`authorize` against the RESOLVED target's table+nonce), so a
    /// token authorizing session B says nothing about session C, and the global
    /// ActiveHandle swinging `@.` to another session cannot grant stale power. The
    /// connect-time op is deliberately NOT stored: caching it invited a confused
    /// deputy (whoami over-reporting / audit mis-attribution) where the cached op
    /// drifted from what the token actually authorizes against the now-active session.
    Edge(EdgeToken),
}

/// Map each verb to the `Op` it requires. The map is TOTAL over the dispatch
/// match's verbs (every arm appears) and DEFAULT-DENY for any unknown string
/// (`None`).
///
/// Classification rule (design 7.2):
/// * `ReadScreen` — anything that only OBSERVES, plus the CONTROLLER's own
///   view-state controls (`scroll`, `select`). `scroll` (display_offset) and
///   `select` (text_selection) touch only the controller's view of the surface;
///   NEITHER writes the PTY master and NEITHER is observable by the driven
///   program (cmd_scroll / cmd_select fire only a `Wake::Output` repaint). They
///   are read-SIDE view control, so a `ReadScreen` edge that may `selection`/`copy`
///   can also `select` the region it copies.
/// * `WriteInput` — injects the human input vocabulary INTO the driven process
///   (`send`/`key`/`ctrl`/`feed`/`mouse`/`paste`) plus `resize` (a SIGWINCH +
///   geometry change the program observes, design 7.2's input class).
/// * `Signal` — `signal` only (tcgetpgrp+killpg): a distinct out-of-band class so
///   a `WriteInput` edge canNOT signal and a `ReadScreen` edge canNOT write — the
///   precise read != write != signal split.
/// * `None` — grant/revoke/whoami (Owner-only, enforced by the gate's catch-all
///   Deny) and any UNKNOWN verb (the dispatch then returns `ERR unknown verb`).
fn required_op(verb: &str) -> Option<Op> {
    match verb {
        // read-side: pure observers + the controller's own view-state controls.
        // `subscribe` is the PUSH face of the read class — it only OBSERVES (the
        // server pushes screen/cursor/event deltas), so it authorizes EXACTLY like
        // a read verb (`ReadScreen`), per-target, via the same cross-session gate.
        "text" | "screen" | "cursor" | "cell" | "search" | "dims" | "lines" | "line" | "modes"
        | "title" | "cwd" | "blocks" | "blocktext" | "wait" | "colors" | "selection" | "copy"
        | "scroll" | "select" | "image" | "window" | "chrome" | "cast" | "subscribe" | "edges"
        | "grants" | "family" | "ready" | "metrics" => Some(Op::ReadScreen),
        // write-side: bytes/geometry the driven PROGRAM observes. `feed-bin` is the
        // length-prefixed binary twin of `feed`; the local path intercepts it via
        // `is_feed_bin_line` before this table is consulted, but the cross-process
        // forward (Item 5b) classifies it here. `tab` DRIVES the GUI (opens/switches
        // the front window's tabs, mutating `App` on the event loop), so it is classed
        // with the other write verbs rather than the read-side observers.
        "send" | "key" | "ctrl" | "feed" | "feed-bin" | "mouse" | "paste" | "resize" | "focus" | "tab" => {
            Some(Op::WriteInput)
        }
        // out-of-band signal, its own class
        "signal" => Some(Op::Signal),
        // grant/revoke/whoami => Owner-only (gate catch-all); unknown => default-deny
        _ => None,
    }
}

/// Interpret the handshake's hex as an edge token against the active session's
/// table. Returns the authorized op, the PRESENTED token (so a cross-session call
/// can re-check it against the TARGET's table via `decide_edge`), and any
/// folded-in inline verb, else None.
fn edge_scope_from_first_line(
    first: &str,
    ctx: &SessionCtx,
) -> Option<(Op, EdgeToken, Option<String>)> {
    let first = first.strip_suffix('\r').unwrap_or(first);
    let (head, rest) = first.split_once(' ')?;
    let (hex, inline) = match head {
        "AUTH" => (rest.trim_end(), None),
        "TOKEN" => match rest.split_once(' ') {
            Some((h, v)) => (h.trim_end(), Some(v.to_string())),
            None => (rest.trim_end(), None),
        },
        _ => return None,
    };
    let tok = EdgeToken::from_hex(hex)?;
    let edges = ctx.edges.lock().unwrap_or_else(|p| p.into_inner());
    // authorize() is the connect-time op-resolving lookup: it checks dst == self_id
    // AND nonce, fail-closed.
    let op = edges.authorize(&tok, &ctx.self_id, &ctx.nonce)?;
    Some((op, tok, inline))
}

/// A request for the MAIN thread to render the live screen to a PNG.
///
/// The control thread fills the CONFINED target (a canonical dir + single
/// filename, already validated by `confine_image_path`), sends [`Wake::Control`],
/// then blocks on `reply`; the main thread renders and sends back
/// `(width, height)`. TOCTOU-1: passing the dir + filename (not a re-resolvable
/// path string) lets the writer `openat` the final component under a dir fd, so
/// no intermediate path component can be symlink-swapped after the check.
pub struct ImageReq {
    /// The confined image target (canonical `images/` dir + validated filename).
    pub target: control_auth::ConfinedImage,
    /// Channel the main thread replies on with the rendered `(width, height)`.
    pub reply: Sender<(u32, u32)>,
}

/// Shared queue of pending [`ImageReq`]s, drained by the main thread.
pub type ImageQueue = Arc<Mutex<VecDeque<ImageReq>>>;

/// Spawn the control-listener thread: provision the capability token, bind
/// the plan's socket (sweeping crashed instances' stale files first), lock it
/// to `0600`, publish the `latest` symlink, then accept connections and serve
/// the protocol on each AUTHENTICATED connection.
///
/// Access control is DEFAULT-ON (see [`crate::control_auth`]):
/// the socket lives in a per-user `0700` directory, every accepted peer must be
/// the same uid, and every connection must present the per-launch token before
/// any verb runs. If the token cannot be provisioned we FAIL CLOSED — the
/// socket is never bound, so it cannot be driven without auth.
pub fn spawn(
    active: ActiveHandle,
    store: Store,
    subscribers: Subscribers,
    proxy: EventLoopProxy<Wake>,
    queue: ImageQueue,
    plan: control_auth::SocketPlan,
    cell_size: (u32, u32),
    // The root session's (id, nonce) to publish as the recursion discovery graph
    // entry, written AFTER bind so it never races the stale sweep. `None` = skip.
    root_identity: Option<(SessionId, aterm_session::LaunchNonce)>,
) {
    std::thread::spawn(move || {
        let sock_path = plan.sock_path.clone();
        // The token + image-confinement subdir live alongside the socket file.
        let sock_dir = control_auth::dir_of_socket(&sock_path);
        if control_auth::ensure_private_dir(&sock_dir).is_err() {
            eprintln!(
                "aterm-gui: control socket dir {} not creatable; socket disabled",
                sock_dir.display()
            );
            return;
        }
        // Crashed instances cannot clean up after themselves: sweep dead pids'
        // per-instance socket/token leftovers. Only in the shared default dir —
        // an explicit override path owns its directory.
        if plan.latest_link.is_some() {
            control_auth::sweep_stale_instances(&sock_dir);
            // Also sweep dead recursion discovery entries (Item 5b) left by crashed
            // sessions that never ran their graceful `remove_graph_entry`.
            crate::proxy::sweep_stale_graph(&sock_dir);
        }
        // Provision the per-launch capability token. FAIL CLOSED: no token =>
        // no socket (better to lose introspection than serve it unauthed).
        let token = match control_auth::provision_token(&plan.token_path) {
            Some(t) => Arc::new(t),
            None => {
                eprintln!(
                    "aterm-gui: could not provision control-socket token; socket disabled"
                );
                return;
            }
        };

        // Never unlink a LIVE socket: a nested aterm that still saw an explicit
        // socket path must not unlink+rebind (and thus HIJACK) its parent's live
        // listener (Item 5 GAP-5, the belt to the env deny-list's suspenders). A
        // stale file from a crashed prior run (no live listener) is removed so
        // bind() does not fail with EADDRINUSE.
        if control_auth::decide_bind(control_auth::socket_is_live(&sock_path))
            == control_auth::BindAction::RefuseLiveSocket
        {
            eprintln!(
                "aterm-gui: control socket {sock_path} already has a live listener; \
                 running without a control socket rather than hijacking it"
            );
            return;
        }
        let _ = std::fs::remove_file(&sock_path);
        let listener = match UnixListener::bind(&sock_path) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("aterm-gui: control socket bind failed at {sock_path}: {e}");
                return;
            }
        };
        // Lock the socket file itself to 0600 (owner-only connect).
        control_auth::lock_socket_file(&sock_path);
        // Newest instance wins the `latest` symlink, atomically — flagless
        // single-instance `aterm-ctl` keeps working unchanged.
        if let Some(link) = &plan.latest_link {
            control_auth::publish_latest_link(link, &sock_path);
        }
        eprintln!("aterm-gui: control socket listening at {sock_path} (token-gated, same-uid only)");
        // Recursion discovery (Item 5b): publish the root session's graph entry
        // ONLY NOW — AFTER bind succeeded — so a concurrent `sweep_stale_graph`
        // can never observe our entry pointing at a not-yet-bound socket and
        // delete it as stale (the sibling-respawn race). `None` skips it.
        if let Some((sid, nonce)) = &root_identity {
            crate::proxy::write_graph_entry(&sock_dir, sid, &sock_path, nonce);
        }
        let our_uid = control_auth::our_uid();
        for stream in listener.incoming() {
            let stream = match stream {
                Ok(s) => s,
                Err(_) => continue,
            };
            // Peer credential gate: refuse any connection NOT from our own uid
            // before spending a thread on it. `None` (cannot verify) also
            // refuses — fail closed.
            match control_auth::peer_uid(&stream) {
                Some(uid) if uid == our_uid => {}
                other => {
                    log_denial(
                        AUDIT_SUBSYSTEM,
                        &format!("connect (peer uid {other:?} != {our_uid})"),
                        aterm_containment::mode_or_containment(),
                        "peer uid mismatch",
                    );
                    // Dropping `stream` closes the connection.
                    continue;
                }
            }
            // One thread per connection so a client that holds the socket open
            // (serving many commands) never blocks other clients.
            let active = active.clone();
            let store = store.clone();
            let subscribers = subscribers.clone();
            let proxy = proxy.clone();
            let queue = queue.clone();
            let token = token.clone();
            let sock_dir = sock_dir.clone();
            std::thread::spawn(move || {
                serve(
                    stream, &active, &store, &subscribers, &proxy, &queue, cell_size, &token,
                    &sock_dir,
                );
            });
        }
    });
}

/// Resolve a stable-id selector to a child this aterm holds authority over plus
/// its live socket path, or `None`. Shared by the `@`-first verbs and the
/// selector-SECOND `subscribe` path. Fail-closed: a locally-hosted sid is NOT a
/// proxy hop, an unspawned child has no entry, and a graph entry whose nonce
/// mismatches the retained one (a relaunched child) is rejected.
fn resolve_proxy_child(
    sid: &SessionId,
    store: &Store,
    sock_dir: &std::path::Path,
) -> Option<(crate::proxy::ProxyEntry, String)> {
    if store.read().unwrap_or_else(|p| p.into_inner()).by_sid(sid).is_some() {
        return None; // hosted locally → a normal in-process target
    }
    let entry = crate::proxy::lookup_child(sid)?;
    let (sock_path, nonce) = crate::proxy::read_graph_entry(sock_dir, sid)?;
    if !entry.nonce.ct_eq(&nonce) {
        return None; // graph entry is for a DIFFERENT launch — fail closed
    }
    // CONFINE the discovered socket path to our own runtime dir before the forward
    // dials it and presents the edge token. The graph entry is same-uid writable and
    // its nonce is readable, so the nonce check above does NOT stop a hostile same-uid
    // process from redirecting `sock <path>` to an attacker socket to capture the
    // token (the same threat `confine_image_path` closes for image writes). Fail closed.
    let sock_path = crate::control_auth::confine_proxy_sock(sock_dir, &sock_path)?;
    Some((entry, sock_path))
}

/// PURE decision: given a request `line` + the connection `scope`, decide whether
/// it must be forwarded to a child's socket and return `(child_sock_path,
/// first_line)` to present, else `None` (handle locally). Split out so the
/// security-critical decision — Owner-only, local-hosted bypass, nonce-guarded
/// discovery, op→token — is unit-testable without a live relay.
fn proxy_forward_plan(
    line: &str,
    scope: Scope,
    store: &Store,
    sock_dir: &std::path::Path,
) -> Option<(String, String)> {
    // Only the OWNER of this aterm may use its retained child tokens to forward —
    // a scoped Edge connection cannot escalate to a child it was never granted
    // (it falls through to local resolution, which denies the cross-process sid).
    if !matches!(scope, Scope::Owner) {
        return None;
    }
    let line = line.strip_suffix('\r').unwrap_or(line);

    // `subscribe` is selector-SECOND (`subscribe @<sel> <streams> [since=] [every-frame]`),
    // so the generic `@`-first parse below cannot see its target. Handle it here so a
    // live `subscribe @<child> cells,bytes` reaches the inner session (a single remote
    // child only; a mixed local/remote comma list stays local).
    if let Some(rest) = line.strip_prefix("subscribe ") {
        let sel_tok = rest.split_whitespace().next()?;
        let sel_body = sel_tok.strip_prefix('@')?;
        if sel_body.contains(',') {
            return None;
        }
        let Selector::Sid(sid) = Selector::parse(sel_body) else {
            return None;
        };
        let (entry, sock_path) = resolve_proxy_child(&sid, store, sock_dir)?;
        let tok = entry.token_for(required_op("subscribe")?)?; // ReadScreen
        // Rewrite the child's selector to `@.` while preserving the streams + flags.
        let rewritten = format!("subscribe @.{}", &rest[sel_tok.len()..]);
        return Some((sock_path, crate::proxy::forward_first_line(&tok.to_hex(), &rewritten)));
    }

    let (first, verb_line) = line.split_once(' ')?;
    let sel_body = first.strip_prefix('@')?; // no @selector → local self path
    let Selector::Sid(sid) = Selector::parse(sel_body) else {
        return None; // only stable-id selectors cross processes
    };
    let (entry, sock_path) = resolve_proxy_child(&sid, store, sock_dir)?;
    let verb = verb_line.split_whitespace().next().unwrap_or("");
    let tok = entry.token_for(required_op(verb)?)?;
    // The child is the DIRECT target: rewrite its selector to `@.` (run on self).
    let rewritten = format!("@. {verb_line}");
    Some((sock_path, crate::proxy::forward_first_line(&tok.to_hex(), &rewritten)))
}

/// If `line` targets a session this process does NOT host but IS a child this
/// aterm spawned (Item 5b), forward the whole connection to the child's control
/// socket and RELAY bytes transparently, returning `true` (the caller then ends
/// the connection). Returns `false` for a normal local request. The forward
/// presents the per-op edge token this aterm minted for the child (so the child
/// authorizes the EXACT op the verb needs) and rewrites the child's own selector
/// to `@.` so it runs the verb on itself.
fn try_proxy_forward(
    line: &str,
    scope: Scope,
    store: &Store,
    sock_dir: &std::path::Path,
    client: &UnixStream,
    reader: &mut BufReader<UnixStream>,
) -> bool {
    let Some((sock_path, first_line)) = proxy_forward_plan(line, scope, store, sock_dir) else {
        return false;
    };
    // Forward anything the BufReader already read past the request line, then relay.
    let pre = crate::proxy::drain_buffered(reader);
    // A dial/handshake failure happens BEFORE any relay byte (the client stream is
    // untouched), so honor the contract and answer ERR rather than a silent EOF.
    if crate::proxy::connect_and_relay(&sock_path, &first_line, client, &pre).is_err() {
        let _ = (&*client).write_all(b"ERR forward\n");
        let _ = (&*client).flush();
    }
    true
}

/// Serve one connection: AUTHENTICATE the first line against the capability
/// token, then read newline-delimited requests and write one response each,
/// until the client disconnects or a write fails (dead client).
///
/// The peer's uid was already verified in [`spawn`]; here we require the token.
/// The first line MUST be `AUTH <hex>` or `TOKEN <hex> <verb...>`; anything else
/// gets `ERR auth\n` and the connection is closed BEFORE any verb executes.
///
/// PUSH-ONLY (P1.3): a `subscribe` verb FLIPS this connection to server-push. Once
/// `subscribe` authorizes, [`run_subscribe`] takes over the socket and never reads
/// another request line — the client thereafter only reads `DELTA`/`EVENT`/`GAP`
/// frames. The poll loop below is therefore left unchanged for every OTHER verb
/// (zero regression); `subscribe` is the sole exit into push mode.
#[allow(clippy::too_many_arguments)]
fn serve(
    stream: UnixStream,
    active: &ActiveHandle,
    store: &Store,
    subscribers: &Subscribers,
    proxy: &EventLoopProxy<Wake>,
    queue: &ImageQueue,
    cell_size: (u32, u32),
    token: &str,
    sock_dir: &std::path::Path,
) {
    let mut reader = match stream.try_clone() {
        Ok(s) => BufReader::new(s),
        Err(_) => return,
    };
    let mut writer = stream;

    // First line is the auth handshake. A `TOKEN <hex> <verb...>` form folds the
    // first verb in, so we may have a verb to dispatch immediately.
    //
    // We drive the BufReader with an explicit `read_line` loop (rather than the
    // `lines()` iterator) so the `feed-bin <n>` verb can `read_exact` the N raw
    // bytes that FOLLOW its request line from the SAME buffered stream — the
    // length-prefixed binary frame. Every other path is byte-identical: we strip
    // the trailing newline ourselves, exactly as `lines()` did.
    let first = match read_request_line(&mut reader) {
        Some(l) => l,
        None => return, // client hung up before authenticating
    };
    let (scope, inline_verb) = match control_auth::check_auth_line(&first, token) {
        // Tier 1: the per-instance god token => Owner.
        AuthOutcome::Ok(verb) => (Scope::Owner, verb),
        // Tier 2: not the instance token — try the same hex as an EDGE token.
        AuthOutcome::Denied => {
            let (_, _, _, ctx) = resolve_active(active);
            match edge_scope_from_first_line(&first, &ctx) {
                // The connect-time op proves the token is a LIVE edge (else None ->
                // `ERR auth`); it is not stored — per-request `decide_edge` re-derives it.
                Some((_op, tok, verb)) => (Scope::Edge(tok), verb),
                None => {
                    log_denial(
                        AUDIT_SUBSYSTEM,
                        "auth",
                        aterm_containment::mode_or_containment(),
                        "missing or invalid capability/edge token",
                    );
                    let _ = writer.write_all(b"ERR auth\n");
                    let _ = writer.flush();
                    return;
                }
            }
        }
    };

    // A folded-in verb runs first (empty tail = bare TOKEN line, just an ack).
    if let Some(verb) = inline_verb {
        if !verb.is_empty() {
            // Cross-process forward (Item 5b): a `@<child-sid>` we don't host but
            // spawned is relayed to the child's socket; this connection is then
            // owned by the relay and never returns here.
            if try_proxy_forward(&verb, scope, store, sock_dir, &writer, &mut reader) {
                return;
            }
            // A folded-in `subscribe` flips straight to push mode (and never
            // returns to this poll loop) the same as a request-line one.
            if is_subscribe_line(&verb) {
                run_subscribe(&verb, active, store, subscribers, scope, &mut writer);
                return;
            }
            // A folded-in `feed-bin <n>` reads its N-byte payload from the buffered
            // stream (same as a request-line one), then dispatches as a write verb.
            if is_feed_bin_line(&verb) {
                if !run_feed_bin(&verb, &mut reader, active, store, scope, &mut writer) {
                    return;
                }
            } else {
                // Resolve the ACTIVE tab per request so a long-lived connection
                // follows tab switches.
                let (term, master, sid, ctx) = resolve_active(active);
                let resp = handle(
                    &verb, &term, master, sid, &ctx, store, scope, proxy, queue, cell_size,
                    sock_dir,
                );
                if writer.write_all(resp.as_bytes()).is_err() {
                    return;
                }
                let _ = writer.flush();
            }
        }
    }

    while let Some(line) = read_request_line(&mut reader) {
        // Cross-process forward (Item 5b): relay a `@<child-sid>` we spawned but
        // don't host to the child's socket; the relay then owns the connection.
        if try_proxy_forward(&line, scope, store, sock_dir, &writer, &mut reader) {
            return;
        }
        // PUSH FLIP: `subscribe` authorizes its targets EXACTLY like a read verb,
        // then this connection becomes push-only (never reads another line). On an
        // auth/parse failure `run_subscribe` writes a single `ERR ...` and returns,
        // and we close the connection (a half-subscribed connection is meaningless).
        if is_subscribe_line(&line) {
            run_subscribe(&line, active, store, subscribers, scope, &mut writer);
            return;
        }
        // BINARY FRAME: `feed-bin <n>` consumes the following N raw bytes from the
        // SAME buffered stream and feeds them to the resolved target's PTY — the
        // length-prefixed (vs hex) wire form. It authorizes EXACTLY like `feed`
        // (WriteInput) via the normal `@<selector>` + op gate inside `run_feed_bin`.
        if is_feed_bin_line(&line) {
            if !run_feed_bin(&line, &mut reader, active, store, scope, &mut writer) {
                break;
            }
            continue;
        }
        let (term, master, sid, ctx) = resolve_active(active);
        let resp = handle(
            &line, &term, master, sid, &ctx, store, scope, proxy, queue, cell_size, sock_dir,
        );
        // A dead client (broken pipe) must not crash the app — just drop it.
        if writer.write_all(resp.as_bytes()).is_err() {
            break;
        }
        let _ = writer.flush();
    }
}

/// Read one newline-delimited request line from the buffered control stream,
/// stripping the trailing `\n` (and a `\r` so CRLF clients still work) — exactly
/// the line shape the `lines()` iterator used to yield. `None` on EOF or a read
/// error (the client hung up) OR on a line longer than [`MAX_REQUEST_LINE`] (a
/// runaway/abusive client is dropped rather than buffered unboundedly).
fn read_request_line(reader: &mut impl BufRead) -> Option<String> {
    let mut buf = Vec::with_capacity(64);
    loop {
        let mut byte = [0u8; 1];
        match reader.read(&mut byte) {
            Ok(0) => {
                // EOF: yield a final unterminated line if any, else stop.
                return if buf.is_empty() { None } else { Some(decode_request_line(buf)) };
            }
            Ok(_) => {
                if byte[0] == b'\n' {
                    return Some(decode_request_line(buf));
                }
                if buf.len() >= MAX_REQUEST_LINE {
                    return None; // runaway line: drop the connection
                }
                buf.push(byte[0]);
            }
            Err(_) => return None,
        }
    }
}

/// The upper bound on a single control REQUEST line (not a `feed-bin` payload,
/// which is length-prefixed and bounded separately). Generous for any real verb;
/// a line past it is treated as an abusive client and the connection is dropped.
const MAX_REQUEST_LINE: usize = 64 * 1024;

/// Turn a raw line's bytes (newline already consumed) into the `String` the
/// dispatch expects: strip a trailing `\r`, then UTF-8 lossily (a control line is
/// ASCII; lossy keeps a malformed byte from killing the whole connection).
fn decode_request_line(mut buf: Vec<u8>) -> String {
    if buf.last() == Some(&b'\r') {
        buf.pop();
    }
    String::from_utf8_lossy(&buf).into_owned()
}

/// Whether a request line is the `subscribe` verb (its first whitespace-delimited
/// token). Used by [`serve`] to FLIP the connection to push mode before the normal
/// per-verb dispatch — so the ~29 polling verbs are reached byte-identically and
/// only `subscribe` diverts into the push path.
fn is_subscribe_line(line: &str) -> bool {
    let line = line.strip_suffix('\r').unwrap_or(line);
    matches!(line.split_whitespace().next(), Some("subscribe"))
}

/// The maximum `feed-bin` payload accepted in one frame (256 KiB) — large enough
/// for a bracketed-paste or a control burst, bounded so a hostile/garbled length
/// cannot make the server `read_exact` an unbounded payload.
const MAX_FEED_BIN: usize = 256 * 1024;

/// Whether a request line is the `feed-bin` verb (optionally `@<sel>`-prefixed),
/// so [`serve`] reads its length-prefixed payload from the SAME stream BEFORE the
/// normal per-line dispatch (which only sees one line and cannot reach the bytes).
fn is_feed_bin_line(line: &str) -> bool {
    let line = line.strip_suffix('\r').unwrap_or(line);
    let mut it = line.split_whitespace();
    let first = it.next();
    // An optional leading `@<selector>` (cross-session) precedes the verb.
    match first {
        Some(tok) if tok.starts_with('@') => it.next() == Some("feed-bin"),
        Some("feed-bin") => true,
        _ => false,
    }
}

/// Parse a `[@<sel>] feed-bin <n>` request line into its optional selector and the
/// declared payload length `n`. `None` on a malformed line or a length past
/// [`MAX_FEED_BIN`] (fail-closed). Pure, so the framing parse is unit-testable.
fn parse_feed_bin(line: &str) -> Option<(Option<Selector>, usize)> {
    let line = line.strip_suffix('\r').unwrap_or(line);
    let mut it = line.split_whitespace();
    let first = it.next()?;
    let (selector, len_tok) = if let Some(body) = first.strip_prefix('@') {
        // `@<sel> feed-bin <n>`
        if it.next()? != "feed-bin" {
            return None;
        }
        (Some(Selector::parse(body)), it.next()?)
    } else {
        // `feed-bin <n>`
        if first != "feed-bin" {
            return None;
        }
        (None, it.next()?)
    };
    let n: usize = len_tok.parse().ok()?;
    if n > MAX_FEED_BIN {
        return None;
    }
    Some((selector, n))
}

/// Handle a `feed-bin <n>\n<bytes>` (or `@<sel> feed-bin <n>\n<bytes>`) frame: read
/// the declared `n` RAW bytes that FOLLOW the request line from the SAME buffered
/// stream, then route them to the resolved target's PTY EXACTLY as `feed` does — a
/// length-prefixed binary feed that halves an agent's wire cost vs hex. Returns
/// `true` to keep the connection (the reply was written), `false` to close it (a
/// dead writer or an unrecoverable framing error after the payload was consumed).
///
/// AUTH: identical to `feed` — `WriteInput`, per-target. A bad length / parse error
/// is reported BEFORE any payload read (`ERR usage`), so a malformed line does NOT
/// desync the stream. Once a VALID length is parsed the N bytes are ALWAYS consumed
/// (even on an auth denial), so the next request line is correctly framed — a denial
/// reads-and-discards the payload, then replies `ERR denied`.
fn run_feed_bin<W: Write>(
    line: &str,
    reader: &mut impl BufRead,
    active: &ActiveHandle,
    store: &Store,
    scope: Scope,
    writer: &mut W,
) -> bool {
    let Some((selector, n)) = parse_feed_bin(line) else {
        let _ = writer.write_all(b"ERR usage: feed-bin <n> then <n> raw bytes\n");
        let _ = writer.flush();
        return true; // no payload consumed (parse failed before any read)
    };
    // Read EXACTLY n raw payload bytes from the buffered stream. A short read (the
    // client hung up mid-frame) closes the connection — the stream is desynced.
    let mut payload = vec![0u8; n];
    if reader.read_exact(&mut payload).is_err() {
        return false;
    }

    // Resolve the target (self or `@<selector>`) and gate it like `feed` (WriteInput),
    // mirroring `handle()`'s self/cross split. The payload was already consumed, so
    // every path below replies AND keeps the stream framed.
    let (self_term, self_master, self_session, self_ctx) = resolve_active(active);
    let self_tuple: Target = (self_term, self_master, self_session, self_ctx.clone());
    let ctx = match &selector {
        None | Some(Selector::SelfTok) => self_ctx,
        Some(sel) => match resolve_target(&self_tuple, store, sel) {
            Some((_, _, _, ctx)) => ctx,
            None => {
                let _ = writer.write_all(b"ERR no such session\n");
                let _ = writer.flush();
                return true;
            }
        },
    };

    // Op-scope gate: `feed-bin` is `WriteInput`, exactly like `feed`. SELF and CROSS
    // collapse onto the SAME per-session predicate — Owner keeps full self-power; an
    // Edge (self OR cross) must hold a `decide_edge`-permitted token against the
    // RESOLVED target's table+nonce. Matching the op ALONE on the self path let an
    // edge scoped to session B inject raw bytes into whatever tab became frontmost
    // after a tab/window switch (the global ActiveHandle retargets `@.`) — the same
    // confused-deputy authority escape `handle()`'s self gate closes.
    let authorized =
        matches!(scope, Scope::Owner) || cross_session_authorized(scope, "feed", &ctx);
    if !authorized {
        log_denial(
            AUDIT_SUBSYSTEM,
            &format!("feed-bin -> {}", ctx.self_id.as_str()),
            aterm_containment::mode_or_containment(),
            "no authorizing edge for feed-bin",
        );
        let _ = writer.write_all(b"ERR denied\n");
        let _ = writer.flush();
        return true;
    }

    write_pty(&ctx.sink, &payload);
    let reply = format!("OK {n} bytes\n");
    if writer.write_all(reply.as_bytes()).is_err() {
        return false;
    }
    writer.flush().is_ok()
}

/// AUTHORIZE a `subscribe` request and, on success, FLIP this connection to push
/// mode by running the subscriber push loop (which never returns to the poll loop).
///
/// Grammar: `subscribe @<sel>[,<sel>...] <streams> [since=<seq>]` where `<streams>`
/// is a comma/space list ⊆ {screen,cursor,events}. Each `@<sel>` is resolved + gated
/// EXACTLY like a read verb: `@.`/self is always allowed; a cross-session target
/// needs `ReadScreen` authorization through [`resolve_target`] +
/// [`cross_session_authorized`] (Owner reaches same-uid siblings; a scoped Edge needs
/// a `decide_edge` grant against the TARGET's table+nonce). FAIL-CLOSED: a malformed
/// line, an unknown session, or ANY target that fails the gate writes a single
/// `ERR ...` and the connection is closed without entering push mode (no partial
/// subscription). On full success it writes `OK subscribe <n>\n` and hands the socket
/// to [`subscribe::push_loop`].
fn run_subscribe<W: Write>(
    line: &str,
    active: &ActiveHandle,
    store: &Store,
    subscribers: &Subscribers,
    scope: Scope,
    writer: &mut W,
) {
    let line = line.strip_suffix('\r').unwrap_or(line);
    // Strip the verb; the remainder is `@<sel>[,<sel>...] <streams> [since=<seq>]`.
    let rest = match line.split_once(' ') {
        Some(("subscribe", r)) => r.trim(),
        _ => {
            let _ = writer.write_all(b"ERR usage: subscribe @<sel>[,<sel>] <streams> [since=<seq>]\n");
            let _ = writer.flush();
            return;
        }
    };
    let mut it = rest.split_whitespace();
    let (Some(sel_tok), Some(stream_tok)) = (it.next(), it.next()) else {
        let _ = writer.write_all(b"ERR usage: subscribe @<sel>[,<sel>] <streams> [since=<seq>]\n");
        let _ = writer.flush();
        return;
    };
    // Optional trailing args: `since=<seq>` (last-seen content_seq) and the
    // `every-frame` flag (re-emit `cells` on every wake for animation fidelity).
    let mut since: Option<u64> = None;
    let mut non_coalesced = false;
    for tok in it {
        if let Some(v) = tok.strip_prefix("since=") {
            match v.parse::<u64>() {
                Ok(n) => since = Some(n),
                Err(_) => {
                    let _ = writer.write_all(b"ERR bad since\n");
                    let _ = writer.flush();
                    return;
                }
            }
        } else if tok == "every-frame" {
            non_coalesced = true;
        } else {
            let _ = writer.write_all(b"ERR unknown subscribe arg\n");
            let _ = writer.flush();
            return;
        }
    }

    let Some(streams) = Streams::parse(stream_tok) else {
        let _ = writer
            .write_all(b"ERR usage: streams are a subset of screen,cursor,events,cells,bytes\n");
        let _ = writer.flush();
        return;
    };

    // The connection's own session tuple, resolved like every other request so a
    // self `subscribe` (`@.`) follows the active tab the same way a self read does.
    let (self_term, self_master, self_session, self_ctx) = resolve_active(active);
    let self_tuple: Target = (self_term, self_master, self_session, self_ctx);

    // Resolve + GATE every `@<sel>` in the comma list. Fail-closed: the FIRST bad
    // selector aborts the whole subscribe (no partial push), and the gate denial is
    // audited exactly like a cross-session read denial.
    let mut targets: Vec<subscribe::ResolvedTarget> = Vec::new();
    for raw in sel_tok.split(',').filter(|s| !s.is_empty()) {
        let Some(body) = raw.strip_prefix('@') else {
            let _ = writer.write_all(b"ERR usage: targets are @<sel> (e.g. @., @1, @s-...)\n");
            let _ = writer.flush();
            return;
        };
        let sel = Selector::parse(body);
        let Some((term, _master, local_id, ctx)) = resolve_target(&self_tuple, store, &sel) else {
            let _ = writer.write_all(b"ERR no such session\n");
            let _ = writer.flush();
            return;
        };
        // Subscribe authorizes EXACTLY like a read (`ReadScreen`) via the same
        // per-session `cross_session_authorized` gate. Owner keeps full self-power; an
        // Edge — self (`@.`) OR cross — must hold a `decide_edge`-permitted token
        // against the RESOLVED target's table+nonce. Treating `@.` as "always
        // allowed" let an edge scoped to session B read whatever tab became frontmost
        // after a tab/window switch (the global ActiveHandle retargets `@.`) — the
        // same confused-deputy read escape `handle()`'s self gate closes.
        if !matches!(scope, Scope::Owner) && !cross_session_authorized(scope, "subscribe", &ctx) {
            log_denial(
                AUDIT_SUBSYSTEM,
                &format!("subscribe -> {}", ctx.self_id.as_str()),
                aterm_containment::mode_or_containment(),
                "no authorizing edge for cross-session subscribe",
            );
            let _ = writer.write_all(b"ERR denied\n");
            let _ = writer.flush();
            return;
        }
        targets.push((local_id, term, ctx.byte_fanout.clone()));
    }
    if targets.is_empty() {
        let _ = writer.write_all(b"ERR usage: at least one @<sel> target\n");
        let _ = writer.flush();
        return;
    }

    // Authorized. Ack, then FLIP to push-only: the loop owns the socket from here
    // and never reads another request line.
    if writer.write_all(format!("OK subscribe {}\n", targets.len()).as_bytes()).is_err() {
        return;
    }
    if writer.flush().is_err() {
        return;
    }
    subscribe::push_loop(subscribers, store, &targets, streams, since, non_coalesced, writer);
}

/// A resolved cross-session TARGET tuple: the SAME `(term, master, id, ctx)`
/// shape `resolve_active` produces, but for an `@<selector>`-addressed session.
/// Cloned OUT of the [`Store`] before the guard drops, so the dispatch never
/// holds the registry lock across a `Terminal` lock.
type Target = (Arc<Mutex<Terminal>>, i32, u64, Arc<SessionCtx>);

/// A parsed `@<selector>`. `SelfTok` (`@.` or bare `@`) names the connection's own
/// session; `Local`/`Sid` name a specific session. Total + fail-closed: an unknown
/// id resolves to `None` at lookup (`ERR no such session`), never to a wrong one.
enum Selector {
    /// `@.` — the connection's own session (explicit self; degenerates to the
    /// verbatim self path).
    SelfTok,
    /// `@<u64>` — by the process-local `Session.id`.
    Local(u64),
    /// `@s-<hex>` / `@<sid>` — by stable `SessionId`.
    Sid(SessionId),
}

impl Selector {
    /// Parse the body AFTER the leading `@`. `.` => self; an all-digits body =>
    /// a local id; anything else is taken verbatim as a `SessionId` string (the
    /// `s-<hex>` form, matching the wire id `whoami`/`sessions` report). An empty
    /// body is treated as self (`@` alone == `@.`).
    fn parse(body: &str) -> Selector {
        if body.is_empty() || body == "." {
            Selector::SelfTok
        } else if let Ok(n) = body.parse::<u64>() {
            Selector::Local(n)
        } else {
            Selector::Sid(SessionId::new(body))
        }
    }
}

/// Resolve an `@<selector>` to a TARGET tuple, CLONING it out of the registry and
/// dropping the store guard BEFORE the caller locks the target `Terminal` (the
/// clone-then-release discipline — the store lock is never held across a Terminal
/// lock, so mutually-driving agents cannot deadlock). `@.`/`@` resolves to the
/// connection's own `(self_*)` tuple verbatim. Returns `None` (fail closed) for an
/// unknown id — the caller maps that to `ERR no such session`.
fn resolve_target(
    self_tuple: &Target,
    store: &Store,
    sel: &Selector,
) -> Option<Target> {
    match sel {
        Selector::SelfTok => Some(self_tuple.clone()),
        Selector::Local(n) => {
            let g = store.read().unwrap_or_else(|p| p.into_inner());
            let h = g.by_local(*n)?;
            Some((h.term.clone(), h.master, h.local_id, h.ctx.clone()))
            // guard drops here, before any Terminal lock
        }
        Selector::Sid(sid) => {
            let g = store.read().unwrap_or_else(|p| p.into_inner());
            let h = g.by_sid(sid)?;
            Some((h.term.clone(), h.master, h.local_id, h.ctx.clone()))
        }
    }
}

/// Whether a CROSS-session call (target != connection's own session) is authorized
/// for `verb`. Default-DENY.
///
/// * `Owner` — the per-instance launcher god token (same-uid + per-launch token).
///   It is the process's root authority, so it may reach any SIBLING session in
///   the same process (the same trust domain). Still subject to `required_op`
///   being defined for the verb; privilege/identity verbs (`grant`/`revoke`/
///   `whoami`/`sessions`) are self-scoped and handled BEFORE this gate.
/// * `Edge(presented)` — a scoped connection must present a token that
///   `decide_edge` PERMITS against the TARGET's edge table, for the verb's exact
///   required op, bound to the TARGET's CURRENT launch nonce. A token authorizing
///   session B grants nothing toward session C (each hop is an independent
///   point-lookup); a target restarted under a reused id (nonce mismatch) fails
///   closed and is audited.
fn cross_session_authorized(scope: Scope, verb: &str, target_ctx: &SessionCtx) -> bool {
    let Some(need) = required_op(verb) else {
        // No op class (privilege/identity/unknown verb): never cross-session.
        return false;
    };
    match scope {
        Scope::Owner => true,
        Scope::Edge(presented) => {
            let table = target_ctx.edges.lock().unwrap_or_else(|p| p.into_inner());
            decide_edge(&table, &presented, &target_ctx.self_id, need, &target_ctx.nonce)
                .is_permitted()
        }
    }
}

// ── CROSS-SESSION input arms (P1.2 follow-up) ────────────────────────────────
//
// These run ON THE CONTROL THREAD against a RESOLVED `@<selector>` target — NOT
// the active tab — so there is no App UI/gesture/window state to touch and NO
// `Wake::Input` is posted. They reuse the source-blind seam (`seam_egress`) and
// the engine's own viewport/geometry APIs directly, with `(target_term,
// target_sink) = (term, &ctx.sink)` resolved exactly as `send`/`feed` do. The
// op-scope gate (`cross_session_authorized`) has already passed before any of
// these is reached.

/// Cross-session `key`/`ctrl`/`paste`/`focus`: feed a pre-built [`InputEvent`] to
/// the source-blind seam on the TARGET `(term, sink)`. The seam reads the target's
/// modes ONCE and writes the encoded PTY bytes to the target's sink (a no-op under
/// a mode that suppresses the event, e.g. focus reporting OFF) — byte-identical to
/// what the active-tab seam would emit for the SAME event, preserving the Tier-1
/// indistinguishability invariant (no `Source` is involved). `None` (a malformed
/// verb line) maps to `err`. Always `Egress::Reported` for these arms.
fn cross_input(
    term: &Arc<Mutex<Terminal>>,
    ctx: &SessionCtx,
    ev: Option<InputEvent>,
    err: &str,
) -> String {
    match ev {
        Some(ev) => {
            seam_egress(term, &ctx.sink, &ev);
            "OK\n".to_string()
        }
        None => err.to_string(),
    }
}

/// Cross-session `mouse`: build the engine-neutral event via [`parse_mouse`] and
/// feed it to the seam on the TARGET. When the target app IS mouse-tracking, the
/// seam writes the report to the target sink (`Egress::Reported`). When it is NOT
/// (`Egress::TrackingOff`):
///   * a WHEEL (`wheel_lines > 0`) moves the TARGET term's viewport via
///     `scroll_display` (positive offset = toward history; `wheel_up` => +lines),
///     mirroring the active-tab wheel fallback (`App::input`), and nudges the
///     target tab to repaint with `Wake::redraw(session)` (the same repaint
///     `cmd_select` fires for a background tab);
///   * a plain PRESS/RELEASE/MOVE (`wheel_lines == 0`) is a DELIBERATE no-op:
///     the active-tab fallback would drive the App SELECTION GESTURE, but a
///     background session has no controller-side selection UI to mutate. We still
///     reply `OK` (the event was accepted; there is simply nothing to render).
fn cross_mouse(
    term: &Arc<Mutex<Terminal>>,
    ctx: &SessionCtx,
    session: u64,
    proxy: &EventLoopProxy<Wake>,
    rest: &str,
) -> String {
    match cross_mouse_apply(term, ctx, rest) {
        // The viewport moved (a wheel under a non-tracking target): nudge the
        // (possibly-not-active) target tab to repaint, the same way `cmd_select`
        // repaints by the resolved local id.
        Ok(true) => {
            let _ = proxy.send_event(Wake::redraw(session));
            "OK\n".to_string()
        }
        // Tracking-on report, or a no-op press/move: nothing to repaint.
        Ok(false) => "OK\n".to_string(),
        Err(e) => e,
    }
}

/// The proxy-INDEPENDENT core of [`cross_mouse`] (so it is unit-testable headlessly,
/// where an `EventLoopProxy` cannot be built off the main thread). Parses + feeds
/// the event to the seam on the TARGET and applies the `TrackingOff` fallback,
/// returning `Ok(true)` when the TARGET viewport moved (the caller should repaint),
/// `Ok(false)` for a seam-reported event or a deliberate no-op, and `Err(usage)` on
/// a malformed verb line. The repaint nudge itself lives in the wrapper.
fn cross_mouse_apply(term: &Arc<Mutex<Terminal>>, ctx: &SessionCtx, rest: &str) -> Result<bool, String> {
    let ev = parse_mouse(rest)?;
    match seam_egress(term, &ctx.sink, &ev) {
        // Tracking ON but the PTY write failed: honest error, not a false OK.
        Egress::Reported(crate::input::Delivery::Failed) => Err("ERR write failed\n".to_string()),
        // Tracking ON: the seam already wrote the report to the target sink.
        Egress::Reported(_) => Ok(false),
        // Tracking OFF: only a wheel has a meaningful background fallback — move the
        // target viewport. A plain press/release/move is a deliberate no-op (no
        // controller-side selection UI for a background tab).
        Egress::TrackingOff { wheel_lines, wheel_up } if wheel_lines > 0 => {
            let delta = if wheel_up { wheel_lines } else { -wheel_lines };
            term_lock(term).scroll_display(delta);
            Ok(true)
        }
        Egress::TrackingOff { .. } => Ok(false),
    }
}

/// Cross-session `resize`: `Resize { echo_to_window: false }` confined to the
/// TARGET. This is a SINGLE-SESSION slice of the active-tab path — NOT a full
/// `apply_term_resize`, which is a WINDOW-level op that loops over EVERY session,
/// reconfigures the GPU swapchain, and (via the self `resize` verb's
/// `echo_to_window: true` -> `apply_grid_resize`) calls `request_inner_size`.
/// Here we touch ONLY the target's three artifacts, exactly as `apply_term_resize`
/// does PER SESSION (main.rs:2453-2463): `Terminal::resize` + `aterm_pty::resize` +
/// the asciicast geometry record. We never touch the active window/framebuffer or
/// any other session.
///
/// ASCIICAST FIDELITY (must match the self path): a self resize of session S
/// records `[t, "r", "<cols>x<rows>"]` into S's own `CastRecorder` so a `cast` verb
/// later sees the geometry change on S's timeline. We push the SAME record into the
/// TARGET's `ctx.cast` so a cross resize of S is INDISTINGUISHABLE from a self
/// resize of S in S's recorded history — `cast` is already cross-session-correct
/// (it reads the resolved `ctx.cast`), so omitting this would be an observable
/// cross/self divergence. This is a SIDE-EFFECT-equivalence claim, not a wire-byte
/// one: `seam_egress` emits zero bytes for `Resize`, so the cross arm applies the
/// geometry effect directly rather than routing through the seam.
///
/// Reuses [`parse_resize`] for the identical `ERR out of range` / usage strings.
fn cross_resize(term: &Arc<Mutex<Terminal>>, master: i32, ctx: &SessionCtx, rest: &str) -> String {
    let (rows, cols) = match parse_resize(rest) {
        Ok(rc) => rc,
        Err(e) => return e,
    };
    term_lock(term).resize(rows, cols);
    aterm_pty::resize(master, rows, cols);
    // Mirror `apply_term_resize`'s per-session asciicast record (main.rs:2459-2463)
    // so the target's own `screen.cast` timeline shows the geometry change.
    {
        let mut rec = ctx.cast.lock().unwrap_or_else(|p| p.into_inner());
        let t = rec.now();
        rec.record_resize(t, cols, rows);
    }
    "OK\n".to_string()
}

/// Cross-session `scroll`: apply the [`ScrollIntent`] DIRECTLY to the TARGET term's
/// viewport (the seam produces no bytes for `ScrollView`; the viewport move lives
/// in `App::input`). Reports `OK <offset> <max>` — the SAME wire shape as the self
/// path's [`cmd_scroll`]. No repaint is posted: a background tab is not visible, so
/// the next time it is shown it reads the new offset; `select` posts `Wake::Output`
/// only because it must repaint a possibly-active selection.
fn cross_scroll(term: &Arc<Mutex<Terminal>>, rest: &str) -> String {
    let intent = match rest.trim() {
        "" => ScrollIntent::By(0),
        "top" => ScrollIntent::Top,
        "bottom" => ScrollIntent::Bottom,
        "up" => ScrollIntent::Up,
        "down" => ScrollIntent::Down,
        n => match n.parse::<i32>() {
            Ok(d) => ScrollIntent::By(d),
            Err(_) => return "ERR usage: scroll <up|down|top|bottom|N>\n".to_string(),
        },
    };
    let mut t = term_lock(term);
    apply_scroll_intent(&mut t, intent);
    let offset = t.grid().display_offset();
    let max = t.grid().scrollback_lines();
    format!("OK {offset} {max}\n")
}

/// Apply a [`ScrollIntent`] to a locked [`Terminal`]'s viewport, the same mapping
/// the seam's `App::input` `ScrollView` arm uses (`Up`/`Down` = one screen;
/// `By(n)` = n lines toward history; `Top`/`Bottom` jump). Shared by the
/// cross-session `scroll` arm so its viewport semantics match the self path.
fn apply_scroll_intent(t: &mut Terminal, intent: ScrollIntent) {
    let page = i32::from(t.rows()).max(1);
    match intent {
        ScrollIntent::Up => t.scroll_display(page),
        ScrollIntent::Down => t.scroll_display(-page),
        ScrollIntent::By(n) => t.scroll_display(n),
        ScrollIntent::Top => t.scroll_to_top(),
        ScrollIntent::Bottom => t.scroll_to_bottom(),
    }
}

/// Dispatch a single request line to its handler, returning the full response
/// (including any trailing data rows) as a string.
///
/// CROSS-SESSION (P1.2): an OPTIONAL leading `@<selector>` token is parsed BEFORE
/// the verb split. A line whose first token does NOT start with `@` takes the
/// verbatim self path (`self_*` args) — byte-for-byte wire-identical, zero
/// regression. A `@<selector>` resolves a DIFFERENT target tuple via
/// `resolve_target` and gates the cross-session access via
/// `cross_session_authorized`; the ~29 per-verb handlers are UNTOUCHED.
#[allow(clippy::too_many_arguments)]
fn handle(
    line: &str,
    self_term: &Arc<Mutex<Terminal>>,
    self_master: i32,
    self_session: u64,
    self_ctx: &Arc<SessionCtx>,
    store: &Store,
    scope: Scope,
    proxy: &EventLoopProxy<Wake>,
    queue: &ImageQueue,
    cell_size: (u32, u32),
    sock_dir: &std::path::Path,
) -> String {
    // Tolerate CRLF clients; the protocol itself is bare-LF terminated.
    let line = line.strip_suffix('\r').unwrap_or(line);

    // P1.2: parse an OPTIONAL leading `@<selector>` BEFORE the verb split. Absence
    // (the first token does not start with '@') is the verbatim self path below —
    // byte-identical to the pre-P1.2 wire form.
    let (selector, line) = match line.split_once(' ') {
        Some((first, tail)) if first.starts_with('@') => {
            (Some(Selector::parse(&first[1..])), tail)
        }
        // A bare `@selector` with no verb (e.g. just `@s-ab12`) is meaningless;
        // strip it and let the empty verb fall through to `ERR unknown verb`.
        None if line.starts_with('@') => (Some(Selector::parse(&line[1..])), ""),
        _ => (None, line),
    };

    let (verb, rest) = match line.split_once(' ') {
        Some((v, r)) => (v, r),
        None => (line, ""),
    };

    // `version`: global build provenance (version/commit/build-time/binary signature).
    // No session, non-sensitive — answer it for ANY authenticated scope, before target
    // resolution. See crate::build_info (also shown in the macOS About panel).
    if verb == "version" {
        return crate::build_info::control_line();
    }

    // `sessions` and the privilege/identity verbs (`grant`/`revoke`/`whoami`) are
    // SELF-SCOPED: they read/mutate the connection's OWN ctx/registry and ignore a
    // target selector (a selector on them is rejected, fail-closed, so they can
    // never be redirected to mint authority on another session's table).
    match verb {
        "sessions" | "grant" | "revoke" | "whoami" => {
            if !matches!(selector, None | Some(Selector::SelfTok)) {
                return "ERR denied\n".to_string();
            }
            // Owner-only (gate catch-all below) — required_op is None for these.
            match (scope, required_op(verb)) {
                (Scope::Owner, _) => {}
                _ => return "ERR denied\n".to_string(),
            }
            return match verb {
                "sessions" => cmd_sessions(self_ctx, store),
                "grant" => cmd_grant(self_ctx, scope, rest),
                "revoke" => cmd_revoke(self_ctx, scope, rest),
                "whoami" => cmd_whoami(self_ctx, scope),
                _ => unreachable!(),
            };
        }
        _ => {}
    }

    // Resolve the dispatch target. No selector (or `@.`) => the verbatim self
    // tuple (zero regression). Otherwise resolve the sibling from the registry.
    let self_tuple: Target =
        (self_term.clone(), self_master, self_session, self_ctx.clone());
    let is_cross = !matches!(selector, None | Some(Selector::SelfTok));
    let (term, master, session, ctx) = match &selector {
        None | Some(Selector::SelfTok) => self_tuple,
        Some(sel) => match resolve_target(&self_tuple, store, sel) {
            Some(t) => t,
            None => return "ERR no such session\n".to_string(),
        },
    };
    let ctx: &SessionCtx = &ctx;

    // Op-scope gate (design 7.2). EXHAUSTIVE, fail-closed.
    //
    // SELF path (no `@`/`@.`): unchanged — Owner passes everything BEFORE any
    // required_op lookup (so the existing aterm-ctl client is byte-for-byte
    // unchanged); an Edge connection may run a verb ONLY if its edge's op == the
    // verb's required op; the catch-all denies an Edge for any None-op verb.
    //
    // CROSS path (`@other`): in ADDITION the cross-session authority must hold —
    // an Owner reaches siblings (same trust domain); a scoped Edge needs a
    // `decide_edge`-permitted token against the TARGET's table (default-DENY).
    if is_cross {
        if !cross_session_authorized(scope, verb, ctx) {
            log_denial(
                AUDIT_SUBSYSTEM,
                &format!("cross-session {verb} -> {}", ctx.self_id.as_str()),
                aterm_containment::mode_or_containment(),
                "no authorizing edge for cross-session access",
            );
            return "ERR denied\n".to_string();
        }
    } else if !matches!(scope, Scope::Owner)
        && !cross_session_authorized(scope, verb, ctx)
    {
        // SELF path, Edge scope: re-verify the token against the session that is
        // active RIGHT NOW — NOT op-match alone. The control socket has ONE global
        // ActiveHandle that `sync_active_session` retargets to the new frontmost
        // active tab on every tab switch / cross-window focus change. An edge token
        // is a single (src, dst, op) grant against ONE session's table; matching only
        // the op let an edge scoped to session B drive/read whatever session A became
        // frontmost after the user switched tabs or windows — a confused-deputy
        // authority escape (e.g. a WriteInput edge injecting keystrokes into, or
        // resizing, an arbitrary foreground session). Owner keeps full self-power (no
        // lookup, byte-for-byte the legacy client); this collapses SELF and CROSS onto
        // the SAME per-session `decide_edge` predicate, so the only difference is
        // whether UI side-effects run, never whether authority holds.
        log_denial(
            AUDIT_SUBSYSTEM,
            &format!("self {verb} -> {}", ctx.self_id.as_str()),
            aterm_containment::mode_or_containment(),
            "edge not authorized against the now-active session",
        );
        return "ERR denied\n".to_string();
    }
    let term = &term;

    // `--json` READ MODE: a structured-JSON foundation for the read verbs. The flag
    // is parsed off `rest` HERE (additive: a line without it is byte-identical text)
    // and routed to the matching `*_json` emitter; the flag is then STRIPPED so the
    // text fall-through below never sees it. Only the json-capable read verbs branch
    // — every other verb (and any json-capable verb WITHOUT the flag) is untouched,
    // preserving the existing text wire byte-for-byte. The op-scope gate above
    // already authorized `verb` (json is a serialization choice, not a new op).
    if rest.contains("json") {
        if let (true, body) = take_json_flag(rest) {
            let json = match verb {
                "text" => Some(cmd_text_json(term)),
                // `screen` is ALWAYS styled JSON; accept `screen --json` for symmetry.
                "screen" => Some(cmd_screen_styled_json(term)),
                "cursor" => Some(cmd_cursor_json(term)),
                "dims" => Some(cmd_dims_json(term, cell_size)),
                "blocks" => Some(cmd_blocks_json(term, &body)),
                "edges" | "grants" => Some(cmd_edges_json(ctx)),
                _ => None,
            };
            if let Some(out) = json {
                return out;
            }
        }
    }

    match verb {
        "text" => cmd_text(term),
        // The LOSSLESS styled-screen read (keystone): full per-cell colour +
        // resolved decorations + cursor + dims + seq as one JSON frame. Always
        // styled-JSON (no plaintext variant) — `--json` is implied.
        "screen" => cmd_screen_styled_json(term),
        "cursor" => cmd_cursor(term),
        "cell" => cmd_cell(term, rest),
        "search" => cmd_search(term, rest),
        // `edges`/`grants`: list this session's inbound capability edges (the
        // EdgeTable rows). A pure observer of the AUTHORITY surface, so it is gated
        // as `ReadScreen` like every other read verb; cross-session reads a sibling's
        // table through the same `@<selector>` resolution + gate.
        "edges" | "grants" => cmd_edges(ctx),
        // `family [<sid>]`: the session HIERARCHY (parent + children) for a target,
        // from the registry's parent links. The no-arg form walks from the RESOLVED
        // (gated) session; an EXPLICIT `<sid>` argument walks an ARBITRARY node, so it
        // is Owner-only (a scoped Edge may not enumerate trees it has no edge into) —
        // the `scope` guard mirrors the `sessions` verb's Owner gate.
        "family" => cmd_family(ctx, store, scope, rest),
        // `ready [timeout_ms]`: block until the target is Alive AND idle (at an
        // OSC-133 prompt, or no in-flight command), so an agent can chain sessions
        // without polling. Read-side (observes lifecycle/blocks), like `wait`.
        "ready" => cmd_ready(term, store, session, rest),
        "send" => cmd_send(&ctx.sink, rest),
        // Phase 0.5: the SELF (active-tab) path funnels `key`/`ctrl`/`mouse`/`paste`/
        // `focus`/`resize`/`scroll` through the source-blind `App::input` seam on the
        // EVENT LOOP (posts `Wake::Input` / a reply-bearing resize), so the bytes are
        // byte-identical to human input AND the renderer/window/gesture side-effects
        // (snap-to-bottom, selection gesture, `request_inner_size`) run for the tab
        // the user is looking at. Leave these arms UNCHANGED — they are the only ones
        // that may touch the active UI.
        //
        // P1.2 follow-up: the CROSS-session (`@other`) path no longer fails closed. A
        // background target is NOT the active tab, so there is no App UI/gesture/window
        // state to touch — and `seam_egress` is already source-blind and session-
        // agnostic (it reads the modes from the GIVEN term and writes the GIVEN sink).
        // So a cross arm resolves `(target_term, target_sink) = (term, &ctx.sink)` the
        // SAME way `send`/`feed` already do, builds the IDENTICAL `InputEvent` via the
        // shared `parse_*` helpers, and calls `seam_egress` DIRECTLY on the control
        // thread (no `Wake::Input`). Op-scope is already `WriteInput`-gated above
        // (`cross_session_authorized`), so these run only after the edge/owner check.
        "key" if is_cross => cross_input(term, ctx, parse_key(rest), "ERR\n"),
        "key" => cmd_key(proxy, rest),
        "ctrl" if is_cross => {
            cross_input(term, ctx, parse_ctrl(rest), "ERR usage: ctrl <single-letter>\n")
        }
        "ctrl" => cmd_ctrl(proxy, rest),
        "feed" => cmd_feed(&ctx.sink, rest),
        "signal" => cmd_signal(master, rest),
        "mouse" if is_cross => cross_mouse(term, ctx, session, proxy, rest),
        "mouse" => cmd_mouse(proxy, rest),
        "paste" if is_cross => {
            cross_input(term, ctx, Some(InputEvent::Paste(paste_text(rest))), "ERR\n")
        }
        "paste" => cmd_paste(proxy, rest),
        "focus" if is_cross => match parse_focus(rest) {
            Some(focused) => cross_input(term, ctx, Some(InputEvent::Focus(focused)), "ERR\n"),
            None => "ERR usage: focus <in|out>\n".to_string(),
        },
        "focus" => cmd_focus(proxy, rest),
        // `image` rides the shared renderer + event loop, which act on the ACTIVE tab;
        // cross-session pixel capture (offscreen render of a background session) is a
        // later P1.2 deliverable, so it stays fail-closed here rather than silently
        // capturing the WRONG (active) session.
        // `image read [...]` reads the STRUCTURED inline-image payloads from the
        // (target) terminal model — headless-safe and cross-session-correct, so it
        // is matched BEFORE the framebuffer-rasterize arms (which stay fail-closed
        // cross-session). `term` is already the resolved target for cross reads.
        "image" if rest.split_whitespace().next() == Some("read") => {
            cmd_image_read(term, rest.strip_prefix("read").unwrap_or(rest).trim_start())
        }
        "image" if is_cross => "ERR cross-session image unsupported\n".to_string(),
        "image" => cmd_image(proxy, queue, rest, sock_dir),
        // `window` captures the FRONT window's ENTIRE on-screen pixels (OS chrome +
        // content) to a PNG — the introspection an AI needs to SEE the whole window,
        // which `image` (terminal-content framebuffer only) cannot. Like `image` it
        // rides the event loop to read AppKit + the window number on the MAIN thread,
        // acting on the ACTIVE/front window; the on-screen window is a window-level
        // (not per-session) surface, so a cross-session `@<sel>` would capture the
        // SAME front window — meaningless. Keep it fail-closed for `@<sel>` like
        // `image`/`chrome`. The confined PNG path is validated EXACTLY like `image`.
        "window" if is_cross => "ERR cross-session window unsupported\n".to_string(),
        "window" => cmd_window(proxy, rest, sock_dir),
        // `chrome` reports the frontmost window's NATIVE macOS UI (the NSToolbar
        // items + app menu bar). It rides the event loop to read AppKit on the MAIN
        // thread, which acts on the ACTIVE/front window; the chrome (a per-process
        // menu bar + the front window's toolbar) is a window/app-level surface, not a
        // per-session one, so a cross-session `@<sel>` would report the SAME front
        // window's chrome — meaningless. Keep it fail-closed for `@<sel>` like `image`.
        "chrome" if is_cross => "ERR cross-session chrome unsupported\n".to_string(),
        "chrome" => cmd_chrome(proxy),
        // `tab` DRIVES the FRONT window's native tabs (open/switch/cycle). Like
        // `chrome`/`image` it rides the event loop to mutate `App` on the MAIN thread
        // and is a window-level (not per-session) op, so a cross-session `@<sel>` is
        // meaningless — keep it fail-closed for `@<sel>`.
        "tab" if is_cross => "ERR cross-session tab unsupported\n".to_string(),
        "tab" => cmd_tab(proxy, rest),
        // Cross-session `resize` does NOT go through the seam: `seam_egress` emits no
        // bytes for `Resize`, and `App::input`'s Resize arm resizes the WINDOW (every
        // tab + the GPU swapchain). A background target has no window to echo to, so we
        // replicate ONLY the term+PTY pair (`echo_to_window: false` semantics) on the
        // TARGET, never the active window/framebuffer.
        "resize" if is_cross => cross_resize(term, master, ctx, rest),
        "resize" => cmd_resize(proxy, rest),
        // Cross-session `scroll` also bypasses the seam (`ScrollView` emits no bytes;
        // the viewport move lives in `App::input`). It applies the `ScrollIntent`
        // DIRECTLY to the TARGET term's viewport and reports `OK <offset> <max>` — the
        // SAME wire shape as the self path's `cmd_scroll`. `select` is already
        // cross-correct (mutates the target term + fires a repaint keyed by target id).
        "scroll" if is_cross => cross_scroll(term, rest),
        "scroll" => cmd_scroll(term, proxy, rest),
        "dims" => cmd_dims(term, cell_size),
        // `metrics` -> live render/latency counters (process-global; the active tab's
        // grid supplies rows/cols). Lets a driving AI MEASURE responsiveness directly
        // rather than scraping the $ATERM_TRACE_LATENCY stderr log. Read-side.
        "metrics" => cmd_metrics(term, rest),
        "lines" => cmd_lines(term),
        "line" => cmd_line(term, rest),
        "modes" => cmd_modes(term),
        "title" => cmd_title(term),
        "cwd" => cmd_cwd(term),
        "blocks" => cmd_blocks(term, rest),
        "blocktext" => cmd_blocktext(term, rest),
        "wait" => cmd_wait(term, rest),
        "colors" => cmd_colors(term),
        "select" => cmd_select(term, proxy, session, rest),
        "selection" => cmd_selection(term),
        "copy" => cmd_copy(term),
        // `cast` reads the TARGET session's own asciicast recorder (its recorded
        // program-output history), not the shared renderer, so it is correct
        // cross-session — no `is_cross` guard.
        "cast" => cmd_cast(ctx),
        // `sessions`/`grant`/`revoke`/`whoami` are handled SELF-SCOPED above.
        _ => "ERR unknown verb\n".to_string(),
    }
}

/// `sessions` -> list the process-wide registry: `OK <n>\n` then one line per
/// session, sorted by local id: `<local> <sid> <parent|-> <state> <title>`. On a
/// single-session window this is exactly one line == the lone session (the
/// zero-regression base case). The store snapshot is cloned out before formatting,
/// so this never holds the registry lock across a `Terminal` lock.
fn cmd_sessions(_self_ctx: &SessionCtx, store: &Store) -> String {
    let snapshot = {
        let g = store.read().unwrap_or_else(|p| p.into_inner());
        g.snapshot()
    };
    let mut out = format!("OK {}\n", snapshot.len());
    for h in &snapshot {
        let parent = h.parent.as_ref().map_or("-", aterm_session::SessionId::as_str);
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
fn cmd_edges(ctx: &SessionCtx) -> String {
    let mut edges = {
        let tbl = ctx.edges.lock().unwrap_or_else(|p| p.into_inner());
        tbl.edges()
    };
    edges.sort_by(|a, b| {
        (a.src.as_str(), a.op.as_str()).cmp(&(b.src.as_str(), b.op.as_str()))
    });
    let mut out = format!("OK {}\n", edges.len());
    for e in &edges {
        out.push_str(&format!("{} {} {}\n", e.src.as_str(), e.dst.as_str(), e.op.as_str()));
    }
    out
}

/// `edges --json` / `grants --json` -> `{"edges":[{"src":"..","dst":"..",
/// "op":".."}],"dst":"<self>"}`. The SAME edges `cmd_edges` lists (sorted, no
/// token), as a structured object an agent can consume without line-splitting.
fn cmd_edges_json(ctx: &SessionCtx) -> String {
    let (self_id, mut edges) = {
        let tbl = ctx.edges.lock().unwrap_or_else(|p| p.into_inner());
        (ctx.self_id.clone(), tbl.edges())
    };
    edges.sort_by(|a, b| {
        (a.src.as_str(), a.op.as_str()).cmp(&(b.src.as_str(), b.op.as_str()))
    });
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
fn cmd_family(ctx: &SessionCtx, store: &Store, scope: Scope, rest: &str) -> String {
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
    for h in snapshot.iter().filter(|h| h.parent.as_ref() == Some(&target_sid)) {
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
///                 or a finished command (`Complete`): the shell is waiting for
///                 input. This is the precise "prompt-end" signal.
///   * `no-command` — shell integration is present but NO block is in flight
///                 (`Executing`/`EnteringCommand`): nothing is running.
///   * `idle`    — no shell integration at all, but `content_seq` has been STABLE
///                 across a short settle window (the output stopped changing), so
///                 the best-effort idle heuristic fires for plain shells too.
/// Polls server-side, releasing the Terminal lock between checks so the PTY reader
/// keeps advancing; checks the registry lifecycle each pass so an exit is reported
/// promptly rather than waited out.
fn cmd_ready(term: &Arc<Mutex<Terminal>>, store: &Store, session: u64, rest: &str) -> String {
    use aterm_core::terminal::BlockState;
    use crate::session_store::SessionState;
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

/// Map a [`RenderCell`](aterm_core::terminal::RenderCell) char to its on-screen
/// glyph, collapsing NUL/control chars to a space. `pub(crate)` because the push
/// face ([`crate::subscribe`]) must produce byte-identical rows to this poll face.
pub(crate) fn visible_char(ch: char) -> char {
    if ch == '\0' || ch.is_control() {
        ' '
    } else {
        ch
    }
}

/// The visible, trailing-trimmed text of screen row `r`: the engine's
/// combining-aware `get_line_text` with interior control chars collapsed to
/// spaces and the tail trimmed. THE single source for a screen row's text —
/// `text`, `text --json`, and the pushed `subscribe screen` DELTA all route here
/// so the polled and pushed faces stay byte-identical. Caller holds the term lock.
pub(crate) fn visible_row(t: &Terminal, r: usize) -> String {
    let line = t.get_line_text(r as i32, None).unwrap_or_default();
    line.chars().map(visible_char).collect::<String>().trim_end().to_string()
}

/// `text` -> `OK <nrows>\n` then each visible row (trailing spaces trimmed).
///
/// FIDELITY (I-1): each row is extracted through the engine's combining-aware
/// `get_line_text` — the SAME path `selection_to_string`/`copy` and the
/// renderer's `combining_row`/`cluster_row` use — so an NFD accent
/// (`e`+U+0301) or a ZWJ emoji cluster (👨‍👩‍👧) reads back intact instead of
/// being flattened to its base codepoint. (The old per-`RenderCell` scan only
/// saw the resolved base char and silently dropped combining marks / clusters,
/// corrupting the AI's primary screen-read.) Control chars still collapse to
/// spaces via the extraction's NUL→space rule plus an explicit visible map.
fn cmd_text(term: &Arc<Mutex<Terminal>>) -> String {
    let t = term_lock(term);
    let rows = t.rows() as usize;
    let mut out = format!("OK {rows}\n");
    for r in 0..rows {
        out.push_str(&visible_row(&t, r));
        out.push('\n');
    }
    out
}

/// `cast` -> `OK <nbytes>\n` then the session's full asciicast v2 recording as
/// the body (design A.5.1 / B.7). The body is the JSON header line followed by
/// one `[t, "o", …]`/`[t, "r", …]` event per recorded burst — exactly what
/// `asciinema play -`/`agg` consume. `<nbytes>` is the byte length of the body
/// that follows (UTF-8), matching the read-verb framing so the existing client
/// can read the body without guessing where it ends. Output-only and bounded
/// (drop-oldest) by the recorder; this verb only serializes the snapshot, never
/// the renderer, so it is cheap and lock-disjoint from the PTY write path.
fn cmd_cast(ctx: &SessionCtx) -> String {
    let body = {
        let rec = ctx.cast.lock().unwrap_or_else(|p| p.into_inner());
        rec.to_asciicast()
    };
    format!("OK {}\n{}", body.len(), body)
}

/// `cursor` -> `OK <row> <col> <visible:0|1> <style>\n` (0-based). `<style>`
/// is the terminal's DECSCUSR cursor style as a lowercase name:
/// `blinking_block` (default), `steady_block`, `blinking_underline`,
/// `steady_underline`, `blinking_bar`, `steady_bar`, `hidden`, `hollow_block`.
fn cmd_cursor(term: &Arc<Mutex<Terminal>>) -> String {
    let t = term_lock(term);
    let c = t.cursor();
    let vis = u8::from(t.cursor_visible());
    let style = cursor_style_name(t.cursor_style());
    format!("OK {} {} {} {}\n", c.row, c.col, vis, style)
}

/// The wire name of a [`CursorStyle`]: its variant in lowercase snake_case.
pub(crate) fn cursor_style_name(style: CursorStyle) -> &'static str {
    match style {
        CursorStyle::BlinkingBlock => "blinking_block",
        CursorStyle::SteadyBlock => "steady_block",
        CursorStyle::BlinkingUnderline => "blinking_underline",
        CursorStyle::SteadyUnderline => "steady_underline",
        CursorStyle::BlinkingBar => "blinking_bar",
        CursorStyle::SteadyBar => "steady_bar",
        CursorStyle::Hidden => "hidden",
        CursorStyle::HollowBlock => "hollow_block",
        // the enum is non-exhaustive; name future variants when they exist
        _ => "unknown",
    }
}

/// `cell <r> <c>` -> `OK <grapheme> <fg> <bg> <attrs>\n` or `ERR <msg>\n`.
///
/// `<grapheme>` is the cell's FULL on-screen grapheme — the resolved base char
/// plus any complex-cluster string and combining marks — percent-encoded into a
/// single space-free token (decode it the same way as `cwd`/`cmdline`). It is
/// the SAME text the `text`/`search`/selection paths and the renderer's
/// `combining_row`/`cluster_row` produce, so a single-cell read of `é`
/// (`e`+U+0301) or a ZWJ family (👨‍👩‍👧) is FAITHFUL — not the base codepoint
/// alone (FIDELITY I-1; this REPLACES the previous `char as u32` codepoint
/// field, which silently dropped combining marks / emoji clusters). A blank or
/// wide-continuation cell yields an empty token (`%20`-free → ``). `<fg>`/`<bg>`
/// are the fully-resolved `RRGGBB` colors the renderer would paint; `<attrs>` is
/// a comma-separated list (or `none`) of the cell's active text attributes —
/// `bold,dim,italic,underline,blink,inverse,strike,hidden`.
fn cmd_cell(term: &Arc<Mutex<Terminal>>, rest: &str) -> String {
    let mut it = rest.split_whitespace();
    let (Some(rs), Some(cs)) = (it.next(), it.next()) else {
        return "ERR usage: cell <r> <c>\n".to_string();
    };
    let (Ok(r), Ok(c)) = (rs.parse::<usize>(), cs.parse::<usize>()) else {
        return "ERR bad args\n".to_string();
    };
    let t = term_lock(term);
    // Bound by the GRID (per `dims`), not by row content: `render_row` trims
    // trailing blanks, but every 0<=r<rows, 0<=c<cols is a real, readable cell.
    if r >= t.rows() as usize || c >= t.cols() as usize {
        return "ERR out of range\n".to_string();
    }
    let row = t.render_row(r);
    let (fg, bg) = match row.get(c) {
        Some(cell) => (cell.fg, cell.bg),
        // a blank in-grid cell: the terminal's default colors
        None => {
            let (dfg, dbg) = (t.default_foreground(), t.default_background());
            ([dfg.r, dfg.g, dfg.b], [dbg.r, dbg.g, dbg.b])
        }
    };
    // Combining-aware grapheme for THIS cell, via the same core extraction the
    // selection/text paths use. A wide-continuation cell yields "" (its glyph
    // belongs to the lead cell); a blank cell yields "" (the consumer infers a
    // space from the in-grid position, matching `text`'s trailing trim).
    let grapheme = t.cell_grapheme(r, c).unwrap_or_default();
    let grapheme_tok = pct_encode(&grapheme);
    // Width markers, so a consumer can distinguish a full-width (CJK) glyph from
    // an ASCII space without inferring from columns:
    //   `wide`      — the LEAD cell, which holds the double-width glyph
    //   `wide_cont` — its blank right-half spacer
    // PROTECTED (DECSCA) shares a flag bit with WIDE_CONTINUATION;
    // `is_wide_continuation_at` disambiguates via the left neighbor, so a
    // protected character gets NEITHER token (it is ordinary text).
    let flags = cell_attrs(t.grid(), r, c);
    let mut attrs = attrs_string(flags);
    let wide_tok = if flags.contains(CellFlags::WIDE) {
        Some("wide")
    } else if t.grid().is_wide_continuation_at(r as u16, c as u16) {
        Some("wide_cont")
    } else {
        None
    };
    if let Some(tok) = wide_tok {
        if attrs == "none" {
            attrs = tok.to_string();
        } else {
            attrs.push(',');
            attrs.push_str(tok);
        }
    }
    // OSC 8 hyperlink target for this cell, surfaced so an introspecting
    // intelligence sees the link a human would click. Appended as a trailing
    // ` link=<url>` token only when present — positional fields 1-4 (grapheme,
    // fg, bg, attrs) are unchanged, so existing parsers keep working.
    let link = t
        .hyperlink_at(r as u16, c as u16)
        .map(|u| format!(" link={u}"))
        .unwrap_or_default();
    format!(
        "OK {grapheme_tok} {:02x}{:02x}{:02x} {:02x}{:02x}{:02x} {attrs}{link}\n",
        fg[0], fg[1], fg[2], bg[0], bg[1], bg[2],
    )
}

/// Resolve the effective [`CellFlags`] at grid `(r, c)`.
///
/// Inline-styled cells carry their attribute bits directly; cells that intern
/// their style in the grid's `StyleTable` keep only `USES_STYLE_ID` (plus any
/// extra flags) inline, so the real attributes are rehydrated from the table —
/// the same path [`Terminal::render_row`] uses for colors. Out-of-range
/// coordinates yield empty flags.
fn cell_attrs(grid: &Grid, r: usize, c: usize) -> CellFlags {
    let (Ok(row), Ok(col)) = (u16::try_from(r), u16::try_from(c)) else {
        return CellFlags::default();
    };
    let Some(cell) = grid.row(row).and_then(|gr| gr.get(col)) else {
        return CellFlags::default();
    };
    if cell.uses_style_id() {
        let extra = cell.flags().difference(CellFlags::USES_STYLE_ID);
        grid.resolve_style_to_colors(cell.style_id(), extra).2
    } else {
        cell.flags()
    }
}

/// Render active text attributes as a stable comma list, or `none` when bare.
///
/// `underline` is reported for any underline style (single/double/curly and the
/// dotted/dashed combinations, which all set one of those bits).
fn attrs_string(flags: CellFlags) -> String {
    let any_underline = CellFlags::UNDERLINE
        .union(CellFlags::DOUBLE_UNDERLINE)
        .union(CellFlags::CURLY_UNDERLINE);
    let mut parts: Vec<&str> = Vec::new();
    if flags.contains(CellFlags::BOLD) {
        parts.push("bold");
    }
    if flags.contains(CellFlags::DIM) {
        parts.push("dim");
    }
    if flags.contains(CellFlags::ITALIC) {
        parts.push("italic");
    }
    if flags.intersects(any_underline) {
        parts.push("underline");
    }
    if flags.contains(CellFlags::BLINK) {
        parts.push("blink");
    }
    if flags.contains(CellFlags::INVERSE) {
        parts.push("inverse");
    }
    if flags.contains(CellFlags::STRIKETHROUGH) {
        parts.push("strike");
    }
    if flags.contains(CellFlags::HIDDEN) {
        parts.push("hidden");
    }
    if parts.is_empty() {
        "none".to_string()
    } else {
        parts.join(",")
    }
}

/// `scroll <up|down|top|bottom|N>` -> move the scrollback viewport and report
/// the new position as `OK <display_offset> <scrollback_lines>\n`. `up`/`down`
/// move one screen into/out of history; `top`/`bottom` jump; a signed integer
/// `N` moves N lines into history (negative = toward the live bottom). With no
/// argument it just reports the current position. After moving it nudges a
/// windowed session to repaint (no-op when headless).
fn cmd_scroll(
    term: &Arc<Mutex<Terminal>>,
    proxy: &EventLoopProxy<Wake>,
    rest: &str,
) -> String {
    // Parse to a tracking-agnostic ScrollIntent; the SEAM is the sole
    // `scroll_display`/`scroll_to_*` caller. `""` (just report position) maps to a
    // zero-line `By(0)` so the round-trip still reports the current offset.
    let intent = match rest.trim() {
        "" => ScrollIntent::By(0),
        "top" => ScrollIntent::Top,
        "bottom" => ScrollIntent::Bottom,
        "up" => ScrollIntent::Up,
        "down" => ScrollIntent::Down,
        n => match n.parse::<i32>() {
            Ok(d) => ScrollIntent::By(d),
            Err(_) => return "ERR usage: scroll <up|down|top|bottom|N>\n".to_string(),
        },
    };
    // Reply-bearing: the reply is sent AFTER the seam applied the scroll on the
    // main thread, so the position read below is NOT racy with the apply.
    // `scroll` is read-side view control (display_offset only) — audit class ReadScreen.
    match post_input_reply(proxy, Op::ReadScreen, vec![InputEvent::ScrollView(intent)]) {
        Ok(_) => {}
        Err(e) => return e,
    }
    let t = term_lock(term);
    let offset = t.grid().display_offset();
    let max = t.grid().scrollback_lines();
    format!("OK {offset} {max}\n")
}

/// `dims` -> `OK <rows> <cols> <pixel_w> <pixel_h>\n`. Pixels are the renderer's
/// fixed per-glyph cell size multiplied by the live grid (the framebuffer size
/// the `image` verb would produce).
fn cmd_dims(term: &Arc<Mutex<Terminal>>, cell_size: (u32, u32)) -> String {
    let t = term_lock(term);
    let rows = u32::from(t.rows());
    let cols = u32::from(t.cols());
    let (cw, ch) = cell_size;
    format!("OK {rows} {cols} {} {}\n", cols * cw, rows * ch)
}

/// `metrics [reset]` -> one `OK k=v ...\n` line of live render/latency counters so a
/// driving AI can MEASURE responsiveness AND DETECT lag in the same loop it drives
/// with — `send`/`key`, then `metrics`. `metrics reset` first zeroes the
/// measurement-window stats (frames / maxima / slow count) so a SPECIFIC workload can
/// be timed: `metrics reset`, drive it, then `metrics`.
///
/// Fields: `backend=<cpu|gpu>`, grid `rows`/`cols`, `frames` (real presents since
/// reset — a steady screen does NOT advance it), `last_/max_present_latency_ms` (the
/// `output→present` slice `$ATERM_TRACE_LATENCY` logs, most-recent + worst), and the
/// LAG SIGNATURE: `last_/max_frame_render_ms` + `slow_frames` (frames over the ~30 fps
/// budget, `slow_threshold_ms`). A non-zero `slow_frames`, a large
/// `max_frame_render_ms`, or `backend=cpu` under heavy output all mean the terminal is
/// lagging. Values are the process-global [`crate::metrics`] counters + the grid size.
fn cmd_metrics(term: &Arc<Mutex<Terminal>>, rest: &str) -> String {
    if rest.trim() == "reset" {
        crate::metrics::reset();
    }
    let (rows, cols) = {
        let t = term_lock(term);
        (u32::from(t.rows()), u32::from(t.cols()))
    };
    let m = crate::metrics::snapshot();
    let backend = if m.backend_gpu { "gpu" } else { "cpu" };
    let ms = |ns: u64| ns as f64 / 1e6;
    format!(
        "OK backend={backend} rows={rows} cols={cols} frames={} \
         last_present_latency_ms={:.2} max_present_latency_ms={:.2} \
         last_frame_render_ms={:.2} max_frame_render_ms={:.2} \
         slow_frames={} slow_threshold_ms={:.1}\n",
        m.frames_presented,
        ms(m.last_present_latency_ns),
        ms(m.max_present_latency_ns),
        ms(m.last_frame_render_ns),
        ms(m.max_frame_render_ns),
        m.slow_frames,
        ms(crate::metrics::SLOW_FRAME_THRESHOLD_NS),
    )
}

/// `lines` -> `OK <total_scrollback_lines>\n` — how many lines of history
/// (tiered + ring-buffer scrollback) currently exist above the visible screen.
fn cmd_lines(term: &Arc<Mutex<Terminal>>) -> String {
    let t = term_lock(term);
    format!("OK {}\n", t.grid().scrollback_lines())
}

/// `line <n>` -> `OK <text>\n` for the line at MONOTONIC ABSOLUTE row `n`, or
/// `ERR out of range\n` / `ERR evicted\n`.
///
/// COORDINATE SPACE (B-2): `n` is an ABSOLUTE row — the same space `blocks` and
/// `search` report — NOT a 0-based history index. This is the ONE documented
/// read coordinate: `blocks` gives output/command/prompt rows as absolute
/// numbers and `search` returns absolute match rows, and BOTH are fed straight
/// to `line`/`text` with the conversion done HERE at the read site. The mapping
/// (identical to the engine's `text_range`):
///   `hist = n - grid.oldest_absolute_row()`
///   `hist <  scrollback_lines`        → scrollback history line `hist`
///   `hist >= scrollback_lines`        → visible row `hist - scrollback_lines`
/// A row OLDER than `oldest_absolute_row()` has scrolled past the scrollback cap
/// and is reported as an EXPLICIT `ERR evicted\n` (never silently-shifted text —
/// the same eviction contract `blocktext` honors). Control chars collapse to
/// spaces; trailing spaces are trimmed.
fn cmd_line(term: &Arc<Mutex<Terminal>>, rest: &str) -> String {
    let Ok(n) = rest.trim().parse::<u64>() else {
        return "ERR usage: line <abs_row>\n".to_string();
    };
    let t = term_lock(term);
    let text = match abs_row_text(&t, n) {
        AbsRow::Text(s) => s,
        AbsRow::Evicted => return "ERR evicted\n".to_string(),
        AbsRow::OutOfRange => return "ERR out of range\n".to_string(),
    };
    let mut s: String = text.chars().map(visible_char).collect();
    while s.ends_with(' ') {
        s.pop();
    }
    format!("OK {s}\n")
}

/// Outcome of resolving an absolute row to its text (B-2 coordinate space).
enum AbsRow {
    /// The combining-aware, NOT-yet-control-collapsed line text.
    Text(String),
    /// Older than `oldest_absolute_row()` — scrolled past the scrollback cap.
    Evicted,
    /// Newer than the live bottom visible row (no such row).
    OutOfRange,
}

/// Resolve a MONOTONIC ABSOLUTE row to its grapheme-faithful text, in the ONE
/// documented read coordinate space shared by `blocks`/`search`/`line`/`text`.
///
/// Conversion is identical to the engine's `text_range`: an absolute row maps to
/// a history index relative to the oldest retained line; indices at/above the
/// scrollback count land on the visible screen. Scrollback lines come from
/// `get_history_line` (Line text); visible rows from the combining-aware
/// `get_line_text` so accents / ZWJ clusters survive (FIDELITY I-1).
fn abs_row_text(t: &Terminal, abs_row: u64) -> AbsRow {
    let grid = t.grid();
    let oldest = grid.oldest_absolute_row();
    if abs_row < oldest {
        return AbsRow::Evicted;
    }
    let scrollback = grid.scrollback_lines() as u64;
    let visible_rows = u64::from(t.rows());
    let rel = abs_row - oldest;
    if rel < scrollback {
        // Scrollback history line `rel` (0 = oldest retained).
        match grid.get_history_line(rel as usize) {
            Some(line) => AbsRow::Text(line.to_string()),
            None => AbsRow::OutOfRange,
        }
    } else {
        let visible = rel - scrollback;
        if visible >= visible_rows {
            return AbsRow::OutOfRange;
        }
        AbsRow::Text(t.get_line_text(visible as i32, None).unwrap_or_default())
    }
}

/// `search <pat> [case] [regex]` -> `OK <count>[ incomplete]\n` then
/// `<abs_row> <col> <len>` per match.
///
/// SEARCH-1: backed by the engine's real `TerminalSearch`, indexing BOTH the
/// SCROLLBACK (`get_history_line(0..scrollback_lines)`) AND the visible rows
/// with grapheme-aware text — so a term that has scrolled OFF the screen is
/// still found, not just the visible page. Each match's row is an ABSOLUTE row
/// (B-2's one coordinate space): feed it straight to `line`/`text`, which
/// convert at the read site. `col`/`len` are CHARACTER columns within that row.
///
/// FLAGS (order-independent, after the pattern): `case` = case-SENSITIVE match
/// (default is case-insensitive); `regex` = treat `<pat>` as a regular
/// expression (requires the `aterm-search` `regex` feature, enabled for the
/// engine). The pattern is the first token; flags are any trailing `case`/`regex`
/// tokens, so a literal pattern containing spaces is not supported here (use a
/// single-token needle, as the naive scan also required).
///
/// INCOMPLETE (DL-2): if the search index evicted lines (the searchable window
/// is capped), the header carries a trailing ` incomplete` token so the AI knows
/// results are NOT exhaustive rather than trusting a short list silently.
fn cmd_search(term: &Arc<Mutex<Terminal>>, rest: &str) -> String {
    let mut it = rest.split_whitespace();
    let Some(pat) = it.next() else {
        return "OK 0\n".to_string();
    };
    // Parse order-independent trailing flags.
    let (mut case_sensitive, mut is_regex) = (false, false);
    for tok in it {
        match tok {
            "case" => case_sensitive = true,
            "regex" => is_regex = true,
            other => return format!("ERR unknown flag: {other}\n"),
        }
    }

    // P1.0b: reuse the cached full-content search index when the active grid's
    // content is unchanged. `indexed_search` builds the SAME index this used to
    // build inline (every still-retained addressable line keyed by ABSOLUTE row:
    // scrollback history -> oldest + i, visible rows -> oldest + scrollback + r),
    // so each returned SearchMatch.line is already an absolute row and results
    // (matches, order, absolute rows, INCOMPLETE) are identical. It rebuilds only
    // on a content change (content_seq bump) or alt-screen swap; an unchanged
    // repeat query reuses the index for the O(1) win. `&mut` for the cache.
    let mut t = term_lock(term);
    let results = match t.indexed_search().search_results_opts(pat, case_sensitive, is_regex) {
        Ok(r) => r,
        Err(e) => return format!("ERR search: {e}\n"),
    };
    drop(t);

    let incomplete = if results.incomplete { " incomplete" } else { "" };
    let mut out = format!("OK {}{incomplete}\n", results.matches.len());
    for m in &results.matches {
        // m.line is the ABSOLUTE row (we keyed the index by absolute row above).
        out.push_str(&format!("{} {} {}\n", m.line, m.start_col, m.len()));
    }
    out
}

/// `send <text>` -> write `<text>` to the PTY. A trailing literal `\n` (a
/// backslash followed by `n`) becomes carriage-return 0x0d so commands run.
fn cmd_send(sink: &SinkWriter, rest: &str) -> String {
    let bytes: Vec<u8> = if let Some(head) = rest.strip_suffix("\\n") {
        let mut b = head.as_bytes().to_vec();
        b.push(0x0d);
        b
    } else {
        rest.as_bytes().to_vec()
    };
    write_pty(sink, &bytes);
    "OK\n".to_string()
}

/// Parse the optional trailing `mods=<list>` token (e.g. `mods=ctrl+shift`),
/// returning the modifier mask and the rest of the line with the token removed.
/// Additive: a verb line WITHOUT `mods=` parses to `Modifiers::empty()` and the
/// untouched line, so every existing caller stays byte-compatible.
fn take_mods(rest: &str) -> (aterm_types::keyboard::Modifiers, String) {
    use aterm_types::keyboard::Modifiers;
    let mut m = Modifiers::empty();
    let mut kept: Vec<&str> = Vec::new();
    for tok in rest.split_whitespace() {
        if let Some(list) = tok.strip_prefix("mods=") {
            for name in list.split(['+', ',']) {
                match name {
                    "shift" => m |= Modifiers::SHIFT,
                    "ctrl" | "control" => m |= Modifiers::CTRL,
                    "alt" | "option" => m |= Modifiers::ALT,
                    // `meta` is its OWN modifier (Kitty CSI-u bit 8), distinct from
                    // ALT — a controller can now drive a real Meta chord. Legacy /
                    // xterm encoders ignore META/HYPER so their bytes are unchanged;
                    // only the Kitty keyboard protocol gains the extra bit.
                    "meta" => m |= Modifiers::META,
                    "hyper" => m |= Modifiers::HYPER,
                    "super" | "cmd" | "command" => m |= Modifiers::SUPER,
                    _ => {}
                }
            }
        } else {
            kept.push(tok);
        }
    }
    (m, kept.join(" "))
}

/// Parse the optional trailing `type=<press|repeat|release>` token, returning the
/// event type (default `Press`) and the body with the token removed. ADDITIVE: a
/// line without `type=` yields `Press` and the untouched body. An unrecognized
/// value yields `None` so [`parse_key`] rejects the whole line rather than
/// silently defaulting. `down`/`up` are accepted aliases for `press`/`release`.
fn take_event_type(rest: &str) -> Option<(aterm_types::keyboard::KeyEventType, String)> {
    use aterm_types::keyboard::KeyEventType;
    let mut et = KeyEventType::Press;
    let mut kept: Vec<&str> = Vec::new();
    for tok in rest.split_whitespace() {
        if let Some(v) = tok.strip_prefix("type=") {
            et = match v {
                "press" | "down" => KeyEventType::Press,
                "repeat" => KeyEventType::Repeat,
                "release" | "up" => KeyEventType::Release,
                _ => return None,
            };
        } else {
            kept.push(tok);
        }
    }
    Some((et, kept.join(" ")))
}

/// Parse the optional trailing `base=<char>` token — the US-QWERTY base-layout
/// key fed to Kitty `REPORT_ALTERNATE_KEYS` (the 3rd CSI-u sub-field), so a
/// controller on a non-US layout can reproduce the byte a human on that layout
/// emits. ADDITIVE: no `base=` yields `None` (the existing behaviour). A `base=`
/// whose value is not exactly one char yields the parser `None`.
fn take_base_layout(rest: &str) -> Option<(Option<char>, String)> {
    let mut base: Option<char> = None;
    let mut kept: Vec<&str> = Vec::new();
    for tok in rest.split_whitespace() {
        if let Some(v) = tok.strip_prefix("base=") {
            let mut chars = v.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => base = Some(c),
                _ => return None,
            }
        } else {
            kept.push(tok);
        }
    }
    Some((base, kept.join(" ")))
}

/// Map a `key` verb wire token to a [`NamedKey`](aterm_types::keyboard::NamedKey),
/// or `None` if it is not a named key (the caller then tries a single character).
/// Covers the FULL `NamedKey` vocabulary the engine models — navigation, editing,
/// locks/system, F1–F35, numpad, modifier-side keys, and media/audio — so every
/// physical key a human can press is reachable by a controller. The original 25
/// tokens keep their exact spelling for byte-compatibility.
fn named_key_from_token(body: &str) -> Option<aterm_types::keyboard::NamedKey> {
    use aterm_types::keyboard::NamedKey as Nk;
    Some(match body {
        // --- original 25 (byte-identical spellings) ---
        "enter" => Nk::Enter,
        "tab" => Nk::Tab,
        "esc" | "escape" => Nk::Escape,
        "backspace" => Nk::Backspace,
        "delete" | "del" => Nk::Delete,
        "insert" | "ins" => Nk::Insert,
        "up" => Nk::ArrowUp,
        "down" => Nk::ArrowDown,
        "right" => Nk::ArrowRight,
        "left" => Nk::ArrowLeft,
        "home" => Nk::Home,
        "end" => Nk::End,
        "pageup" | "pgup" => Nk::PageUp,
        "pagedown" | "pgdn" => Nk::PageDown,
        "f1" => Nk::F1,
        "f2" => Nk::F2,
        "f3" => Nk::F3,
        "f4" => Nk::F4,
        "f5" => Nk::F5,
        "f6" => Nk::F6,
        "f7" => Nk::F7,
        "f8" => Nk::F8,
        "f9" => Nk::F9,
        "f10" => Nk::F10,
        "f11" => Nk::F11,
        "f12" => Nk::F12,
        // --- editing / system ---
        "space" => Nk::Space,
        "capslock" => Nk::CapsLock,
        "numlock" => Nk::NumLock,
        "scrolllock" => Nk::ScrollLock,
        "printscreen" | "prtsc" => Nk::PrintScreen,
        "pause" | "break" => Nk::Pause,
        "menu" | "contextmenu" => Nk::ContextMenu,
        // --- F13..F35 ---
        "f13" => Nk::F13,
        "f14" => Nk::F14,
        "f15" => Nk::F15,
        "f16" => Nk::F16,
        "f17" => Nk::F17,
        "f18" => Nk::F18,
        "f19" => Nk::F19,
        "f20" => Nk::F20,
        "f21" => Nk::F21,
        "f22" => Nk::F22,
        "f23" => Nk::F23,
        "f24" => Nk::F24,
        "f25" => Nk::F25,
        "f26" => Nk::F26,
        "f27" => Nk::F27,
        "f28" => Nk::F28,
        "f29" => Nk::F29,
        "f30" => Nk::F30,
        "f31" => Nk::F31,
        "f32" => Nk::F32,
        "f33" => Nk::F33,
        "f34" => Nk::F34,
        "f35" => Nk::F35,
        // --- numpad (kp* spellings) ---
        "kp0" => Nk::Numpad0,
        "kp1" => Nk::Numpad1,
        "kp2" => Nk::Numpad2,
        "kp3" => Nk::Numpad3,
        "kp4" => Nk::Numpad4,
        "kp5" => Nk::Numpad5,
        "kp6" => Nk::Numpad6,
        "kp7" => Nk::Numpad7,
        "kp8" => Nk::Numpad8,
        "kp9" => Nk::Numpad9,
        "kpdot" | "kpdecimal" => Nk::NumpadDecimal,
        "kpdiv" | "kpdivide" => Nk::NumpadDivide,
        "kpmul" | "kpmultiply" => Nk::NumpadMultiply,
        "kpsub" | "kpminus" => Nk::NumpadSubtract,
        "kpadd" | "kpplus" => Nk::NumpadAdd,
        "kpenter" => Nk::NumpadEnter,
        "kpequal" => Nk::NumpadEqual,
        "kpsep" | "kpseparator" => Nk::NumpadSeparator,
        "kpbegin" => Nk::NumpadBegin,
        "kpleft" => Nk::NumpadArrowLeft,
        "kpright" => Nk::NumpadArrowRight,
        "kpup" => Nk::NumpadArrowUp,
        "kpdown" => Nk::NumpadArrowDown,
        "kppageup" | "kppgup" => Nk::NumpadPageUp,
        "kppagedown" | "kppgdn" => Nk::NumpadPageDown,
        "kphome" => Nk::NumpadHome,
        "kpend" => Nk::NumpadEnd,
        "kpinsert" | "kpins" => Nk::NumpadInsert,
        "kpdelete" | "kpdel" => Nk::NumpadDelete,
        // --- modifier-side keys (reported as key events under Kitty) ---
        "shiftleft" => Nk::ShiftLeft,
        "shiftright" => Nk::ShiftRight,
        "ctrlleft" | "controlleft" => Nk::ControlLeft,
        "ctrlright" | "controlright" => Nk::ControlRight,
        "altleft" => Nk::AltLeft,
        "altright" => Nk::AltRight,
        "superleft" => Nk::SuperLeft,
        "superright" => Nk::SuperRight,
        "hyperleft" => Nk::HyperLeft,
        "hyperright" => Nk::HyperRight,
        "metaleft" => Nk::MetaLeft,
        "metaright" => Nk::MetaRight,
        // --- media / audio ---
        "mediaplay" => Nk::MediaPlay,
        "mediapause" => Nk::MediaPause,
        "mediaplaypause" => Nk::MediaPlayPause,
        "mediastop" => Nk::MediaStop,
        "mediareverse" => Nk::MediaReverse,
        "mediafastforward" | "mediaff" => Nk::MediaFastForward,
        "mediarewind" => Nk::MediaRewind,
        "medianext" | "mediatracknext" => Nk::MediaTrackNext,
        "mediaprev" | "mediatrackprevious" => Nk::MediaTrackPrevious,
        "mediarecord" => Nk::MediaRecord,
        "volumeup" => Nk::AudioVolumeUp,
        "volumedown" => Nk::AudioVolumeDown,
        "mute" => Nk::AudioVolumeMute,
        _ => return None,
    })
}

/// PURE parser for `key <name> [mods=<list>] [type=<t>] [base=<c>]` -> an
/// [`InputEvent::Key`]. Factored out of [`cmd_key`] so the additive grammar is
/// unit-testable WITHOUT an `EventLoopProxy` (the verb can't run headless — it
/// posts a `Wake::Input`). The SAME (Key, mods, base_layout, event_type) tuple a
/// human's named-key press builds, so the seam (the sole encoder caller) yields
/// byte-identical output incl. Kitty REPORT_ALTERNATE_KEYS. All trailing tokens
/// are ADDITIVE — a bare `key up` still parses to empty mods / Press / no base.
/// Returns `None` for an unknown key name or a malformed `type=`/`base=` value.
pub(crate) fn parse_key(rest: &str) -> Option<InputEvent> {
    use aterm_types::keyboard::Key;
    let (mut mods, body) = take_mods(rest);
    let (event_type, body) = take_event_type(&body)?;
    let (base_explicit, body) = take_base_layout(&body)?;
    // Inline modifier prefixes: `ctrl+u`, `alt+x`, `ctrl+shift+a`, ... The
    // prefixes are ADDITIVE with any trailing `mods=` token, so `ctrl+u` and
    // `u mods=ctrl` agree. After stripping them, a single residual character
    // (e.g. `u`) becomes a `Key::Character` event — the SAME (Key, mods) seam
    // `parse_ctrl` builds, so the encoder derives the control byte itself
    // (`ctrl+u` -> 0x15) rather than us hand-rolling it.
    let (prefix_mods, body) = take_inline_mods(body.trim());
    mods |= prefix_mods;
    let body = body.trim();
    let Some(named) = named_key_from_token(body) else {
        // Not a named key: a single residual character (after stripping inline
        // modifier prefixes) becomes a `Key::Character`. `ctrl+u` lands here as
        // `u` + CTRL, byte-identical to `parse_ctrl("u")` -> the encoder emits
        // 0x15. Lower-cased so `ctrl+U` == `ctrl+u`, matching `parse_ctrl`.
        let mut chars = body.chars();
        return match (chars.next(), chars.next()) {
            (Some(c), None) => Some(InputEvent::Key {
                key: Key::Character(c.to_ascii_lowercase()),
                mods,
                base_layout: base_explicit,
                event_type,
            }),
            _ => None,
        };
    };
    Some(InputEvent::Key { key: Key::Named(named), mods, base_layout: base_explicit, event_type })
}

/// Strip leading inline modifier prefixes (`ctrl+`, `alt+`, `shift+`, `super+`
/// and their aliases) from a `key` body, returning the accumulated modifier
/// mask and the remaining body. Mirrors the `mods=` alias table in
/// [`take_mods`] so `ctrl+u` and `u mods=ctrl` are equivalent. Only consumes a
/// prefix when a `+` follows a recognized modifier name, so a bare named key
/// like `up` (no `+`) is returned untouched.
fn take_inline_mods(body: &str) -> (aterm_types::keyboard::Modifiers, &str) {
    use aterm_types::keyboard::Modifiers;
    let mut m = Modifiers::empty();
    let mut rest = body;
    while let Some(plus) = rest.find('+') {
        let bit = match &rest[..plus] {
            "shift" => Modifiers::SHIFT,
            "ctrl" | "control" => Modifiers::CTRL,
            "alt" | "option" => Modifiers::ALT,
            // `meta`/`hyper` are their own bits (see `take_mods`).
            "meta" => Modifiers::META,
            "hyper" => Modifiers::HYPER,
            "super" | "cmd" | "command" => Modifiers::SUPER,
            _ => break,
        };
        m |= bit;
        rest = &rest[plus + 1..];
    }
    (m, rest)
}

/// `key <name> [mods=<list>]` -> build an [`InputEvent::Key`] and post it to the
/// seam (the SOLE encoder caller, under the CURRENT keyboard mode). See
/// [`parse_key`] for the grammar.
fn cmd_key(proxy: &EventLoopProxy<Wake>, rest: &str) -> String {
    match parse_key(rest) {
        // Reply-bearing: OK means the seam APPLIED the event (bytes written),
        // not merely that it was enqueued. With no frontmost window the seam
        // drops the reply sender, so the caller gets ERR rather than a false OK.
        Some(ev) => input_reply_to_str(post_input_reply(proxy, Op::WriteInput, vec![ev])),
        None => "ERR\n".to_string(),
    }
}

/// Map a reply-bearing input outcome to a verb reply line. `Ok` (applied) and
/// `RangeRejected` (out-of-range geometry — not relevant to key/mouse/paste, but
/// handled for completeness) become OK / ERR; an `Err` (event loop closed / no
/// window) is already a full `ERR …\n` string.
fn input_reply_to_str(r: Result<InputOutcome, String>) -> String {
    match r {
        Ok(InputOutcome::Ok) => "OK\n".to_string(),
        Ok(InputOutcome::RangeRejected) => "ERR out of range\n".to_string(),
        Ok(InputOutcome::WriteFailed) => "ERR write failed\n".to_string(),
        Err(e) => e,
    }
}

/// PURE parser for `ctrl <letter>` -> a Control-modified character key. Factored
/// out of [`cmd_ctrl`] for headless unit-testing. The seam encodes it (under the
/// CURRENT keyboard mode) as a proper CSI-u sequence (Kitty/xterm) or the legacy
/// control byte, byte-identical to a human Ctrl chord. Returns `None` unless
/// `rest` is exactly one (non-whitespace) char.
pub(crate) fn parse_ctrl(rest: &str) -> Option<InputEvent> {
    use aterm_types::keyboard::{Key, Modifiers};
    let mut chars = rest.trim().chars();
    let (Some(c), None) = (chars.next(), chars.next()) else {
        return None;
    };
    Some(InputEvent::Key {
        key: Key::Character(c.to_ascii_lowercase()),
        mods: Modifiers::CTRL,
        base_layout: None,
        event_type: aterm_types::keyboard::KeyEventType::Press,
    })
}

/// `ctrl <letter>` -> a Control-modified character key posted to the seam. See
/// [`parse_ctrl`].
fn cmd_ctrl(proxy: &EventLoopProxy<Wake>, rest: &str) -> String {
    match parse_ctrl(rest) {
        Some(ev) => post_input(proxy, Op::WriteInput, vec![ev]),
        None => "ERR usage: ctrl <single-letter>\n".to_string(),
    }
}

/// `feed <hex>` -> write raw bytes (decoded from a hex string, whitespace
/// allowed) straight to the PTY. The escape hatch for control/binary bytes the
/// line-delimited `send` verb can't carry: `feed 03` = Ctrl-C, `feed 1b5b41` =
/// ESC[A, `feed 0a` = a real newline. Replies `OK <n> bytes\n` or `ERR bad hex`.
fn cmd_feed(sink: &SinkWriter, rest: &str) -> String {
    let hex: String = rest.chars().filter(|c| !c.is_whitespace()).collect();
    if hex.len() % 2 != 0 {
        return "ERR bad hex (odd length)\n".to_string();
    }
    let mut bytes = Vec::with_capacity(hex.len() / 2);
    let h = hex.as_bytes();
    let mut i = 0;
    while i < h.len() {
        let hi = (h[i] as char).to_digit(16);
        let lo = (h[i + 1] as char).to_digit(16);
        let (Some(hi), Some(lo)) = (hi, lo) else {
            return "ERR bad hex\n".to_string();
        };
        bytes.push((hi * 16 + lo) as u8);
        i += 2;
    }
    let n = bytes.len();
    write_pty(sink, &bytes);
    format!("OK {n} bytes\n")
}

/// `signal <name>` -> deliver a job-control signal to the PTY's CURRENT
/// foreground process group (via `tcgetpgrp` on the master + `killpg`).
/// `name` is one of `int`/`c`, `quit`, `tstp`/`z`, `hup`, `term`, `kill`.
/// This makes Ctrl-C/Ctrl-\\/Ctrl-Z effects deliverable and testable regardless
/// of the line discipline / launch context (which may not generate them).
fn cmd_signal(master: i32, rest: &str) -> String {
    let sig = match rest.trim() {
        "int" | "c" | "sigint" => libc::SIGINT,
        "quit" | "sigquit" => libc::SIGQUIT,
        "tstp" | "z" | "sigtstp" => libc::SIGTSTP,
        "hup" | "sighup" => libc::SIGHUP,
        "term" | "sigterm" => libc::SIGTERM,
        "kill" | "sigkill" => libc::SIGKILL,
        other => return format!("ERR unknown signal: {other}\n"),
    };
    let pgrp = unsafe { libc::tcgetpgrp(master) };
    if pgrp <= 0 {
        return "ERR no foreground process group\n".to_string();
    }
    let rc = unsafe { libc::killpg(pgrp, sig) };
    if rc == 0 {
        format!("OK signalled pgrp {pgrp}\n")
    } else {
        "ERR killpg failed\n".to_string()
    }
}

const MOUSE_USAGE: &str = "ERR usage: mouse <press|release|move|wheelup|wheeldown> <left|middle|right> <row> <col> [mods=..] [count=N] [side=left|right] [block=0|1]\n";

/// PURE parser for the `mouse` verb -> an engine-neutral mouse [`InputEvent`].
/// Factored out of [`cmd_mouse`] so the additive `mods=`/`count=`/`side=`/`block=`
/// grammar is unit-testable without an `EventLoopProxy`. Returns `Err(usage/err
/// string)` for a malformed line, `Ok(event)` otherwise.
///
/// Grammar: `mouse <action> <button> <row> <col> [mods=..] [count=N]
/// [side=left|right] [block=0|1]`. `action` is `press|release|move|wheelup|
/// wheeldown`; `button` is `left|middle|right` (ignored for the wheel actions);
/// `row`/`col` are 0-based. The additive tokens carry the data that closes the
/// human/controller divergences: `mods=` the real modifier mask (kills a),
/// `count=` the click depth 1..=3 (kills b), `side=` the cell-half (kills i),
/// `block=1` the rectangular-selection intent for a single-click press (the same
/// intent a human encodes from a held Alt, carried as DATA so the seam never
/// reads ambient modifier state).
pub(crate) fn parse_mouse(rest: &str) -> Result<InputEvent, String> {
    use aterm_core::selection::SelectionSide;
    use aterm_types::mouse::{MouseButton, ALT_MASK, CTRL_MASK, SHIFT_MASK};
    let mut action = "";
    let mut mods: u8 = 0;
    let mut click_count: u8 = 1;
    let mut side = SelectionSide::Left;
    let mut block = false;
    // POSITIONAL tokens (in order), interpreted per-action below: this keeps
    // press/release/wheel as `<button> <row> <col>` (byte-compatible with the
    // pre-Phase-0.5 grammar) AND lets `move` be EITHER `<row> <col>` (no-button
    // hover, code 3) OR `<button> <row> <col>` (held-button drag).
    let mut positional: Vec<&str> = Vec::new();
    for tok in rest.split_whitespace() {
        if let Some(list) = tok.strip_prefix("mods=") {
            for name in list.split(['+', ',']) {
                match name {
                    "shift" => mods |= SHIFT_MASK,
                    "alt" | "option" | "meta" => mods |= ALT_MASK,
                    "ctrl" | "control" => mods |= CTRL_MASK,
                    _ => {}
                }
            }
        } else if let Some(c) = tok.strip_prefix("count=") {
            click_count = c.parse::<u8>().unwrap_or(1).clamp(1, 3);
        } else if let Some(s) = tok.strip_prefix("side=") {
            side = if s == "right" { SelectionSide::Right } else { SelectionSide::Left };
        } else if let Some(b) = tok.strip_prefix("block=") {
            block = matches!(b, "1" | "true" | "yes" | "block");
        } else if action.is_empty() {
            action = tok;
        } else {
            positional.push(tok);
        }
    }
    let parse_btn = |s: &str| -> Result<MouseButton, String> {
        match s {
            "left" => Ok(MouseButton::Left),
            "middle" => Ok(MouseButton::Middle),
            "right" => Ok(MouseButton::Right),
            _ => Err("ERR bad button\n".to_string()),
        }
    };
    let parse_rc = |r: &str, c: &str| -> Result<(u16, u16), String> {
        match (r.parse::<u16>(), c.parse::<u16>()) {
            (Ok(r), Ok(c)) => Ok((r, c)),
            _ => Err("ERR bad args\n".to_string()),
        }
    };
    let ev = match action {
        // `move` with two positionals = no-button hover (code 3); with three =
        // `<button> <row> <col>` held-button drag (kills divergence c at the verb).
        "move" => match positional.as_slice() {
            [r, c] => {
                let (row, col) = parse_rc(r, c)?;
                InputEvent::MouseMove { buttons: 3, row, col, mods, side }
            }
            [b, r, c] => {
                let button = parse_btn(b)?;
                let (row, col) = parse_rc(r, c)?;
                InputEvent::MouseMove { buttons: button.code(), row, col, mods, side }
            }
            _ => return Err(MOUSE_USAGE.to_string()),
        },
        "press" | "release" | "wheelup" | "wheeldown" => {
            let [b, r, c] = positional.as_slice() else {
                return Err(MOUSE_USAGE.to_string());
            };
            // `button` is ignored for the wheel actions but still required as a
            // positional (byte-compatible with the old `<button> <row> <col>` form).
            let button = parse_btn(b)?;
            let (row, col) = parse_rc(r, c)?;
            match action {
                "press" => InputEvent::MouseButton {
                    button, pressed: true, row, col, mods, click_count, side, block,
                },
                "release" => InputEvent::MouseButton {
                    button, pressed: false, row, col, mods, click_count, side, block,
                },
                "wheelup" => InputEvent::Wheel { dir_up: true, lines: 1, row, col, mods },
                _ => InputEvent::Wheel { dir_up: false, lines: 1, row, col, mods },
            }
        }
        _ => return Err("ERR bad action\n".to_string()),
    };
    Ok(ev)
}

/// `mouse <action> <button> <row> <col> [mods=..] [count=N] [side=left|right]
/// [block=0|1]` -> BUILD an engine-neutral mouse [`InputEvent`] (via [`parse_mouse`])
/// and post it to the seam, which reads the CURRENT mouse mode ONCE and either
/// emits the `encode_mouse_*` report (tracking ON) or runs the local selection
/// gesture (tracking OFF).
///
/// Phase 0.5 CONTRACT CHANGE (divergences a/b/d/i): the old `OK (mouse off)`
/// short-circuit is GONE — a tracking-OFF press/release now runs the SAME
/// selection machinery as the human (not a no-op), and `mods`/`count`/`side`/
/// `block` are carried as data instead of hard-coded. The verb returns `OK\n`
/// (fire-and-forget) once the batch is posted.
///
/// DRAG CONVERGENCE (divergence c) — SCOPE: one `mouse move` verb line posts ONE
/// `MouseMove`, so a controller that wants intermediate motion reports under a
/// tracking app issues a `press` then N `move`s then a `release` as separate verb
/// lines (the seam reports each, identical to the human's per-pixel `MouseMove`s).
/// A single-line `press→N×move→release` BATCH grammar is deliberately deferred —
/// the seam already supports a batched `Wake::Input` (A.2.3), so it is additive
/// and out of scope for this convergence commit.
fn cmd_mouse(proxy: &EventLoopProxy<Wake>, rest: &str) -> String {
    match parse_mouse(rest) {
        // Reply-bearing: OK means the seam ran (report emitted or local fallback
        // applied), not merely enqueued. No window ⇒ ERR, not a false OK.
        Ok(ev) => input_reply_to_str(post_input_reply(proxy, Op::WriteInput, vec![ev])),
        Err(e) => e,
    }
}

/// `paste <text>` -> write `<text>` to the PTY exactly as if the user pasted
/// it: [`Terminal::format_paste`] strips control bytes that could escape the
/// guards (ESC, C1 controls), converts line breaks to CR, and wraps the body
/// in the bracketed-paste guards `ESC[200~` ... `ESC[201~` when the app has
/// enabled bracketed paste (DECSET 2004). The text is the rest of the line
/// taken literally; a literal trailing `\n` (backslash + n) becomes a line
/// break (sent as CR, like a real paste) so a paste can end in one. For raw
/// unsanitized bytes use `feed`/`send` instead.
fn cmd_paste(proxy: &EventLoopProxy<Wake>, rest: &str) -> String {
    // The seam runs `format_paste` (bracketing + sanitize) under the lock and the
    // snap-to-bottom, converging with the human Cmd-V path. Reply-bearing so OK
    // means the paste reached the PTY (no window ⇒ ERR, not a false OK).
    input_reply_to_str(post_input_reply(
        proxy,
        Op::WriteInput,
        vec![InputEvent::Paste(paste_text(rest))],
    ))
}

/// The `paste` verb's text transform: a literal trailing `\n` (backslash + n)
/// becomes a real line break (sent as CR by `format_paste`). Pure, so the
/// bracketing/sanitize bytes stay unit-testable without an event loop.
fn paste_text(rest: &str) -> String {
    match rest.strip_suffix("\\n") {
        Some(head) => format!("{head}\n"),
        None => rest.to_string(),
    }
}

/// `focus <in|out>` -> drive DEC 1004 focus reporting (kills divergence j: a
/// controller-only session can now satisfy a focus-tracking app's oracle). The
/// seam emits ESC[I / ESC[O when the app enabled focus reporting, byte-identical
/// to the human `on_focus` egress. `in`/`1`/`true` = focus-in.
fn cmd_focus(proxy: &EventLoopProxy<Wake>, rest: &str) -> String {
    match parse_focus(rest) {
        Some(focused) => post_input(proxy, Op::WriteInput, vec![InputEvent::Focus(focused)]),
        None => "ERR usage: focus <in|out>\n".to_string(),
    }
}

/// PURE parser for the `focus` verb's `in/out` argument, factored out of
/// [`cmd_focus`] so the self (active-tab) and cross-session paths build the SAME
/// [`InputEvent::Focus`] from the SAME grammar. `in`/`1`/`true`/`focus` => focus-in.
pub(crate) fn parse_focus(rest: &str) -> Option<bool> {
    match rest.trim() {
        "in" | "1" | "true" | "focus" => Some(true),
        "out" | "0" | "false" | "blur" => Some(false),
        _ => None,
    }
}

/// `modes` -> `OK\n` then one `key=value` line per introspected mode:
/// `alt_screen`, `cursor_visible`, `app_cursor_keys` (DECCKM),
/// `app_keypad` (DECPAM), `bracketed_paste` (2004), `mouse_mode`
/// (`none|normal|button|any|x10`), and `mouse_encoding`
/// (`x10|utf8|sgr|urxvt|sgr_pixel`).
fn cmd_modes(term: &Arc<Mutex<Terminal>>) -> String {
    use aterm_types::mouse::{MouseEncoding, MouseMode};
    let t = term_lock(term);
    let m = t.modes();
    let mouse_mode = match m.mouse_mode {
        MouseMode::None => "none",
        MouseMode::Normal => "normal",
        MouseMode::ButtonEvent => "button",
        MouseMode::AnyEvent => "any",
        MouseMode::X10 => "x10",
        _ => "unknown",
    };
    let mouse_encoding = match m.mouse_encoding {
        MouseEncoding::X10 => "x10",
        MouseEncoding::Utf8 => "utf8",
        MouseEncoding::Sgr => "sgr",
        MouseEncoding::Urxvt => "urxvt",
        MouseEncoding::SgrPixel => "sgr_pixel",
        _ => "unknown",
    };
    // Framed as `OK <n>` + n lines so the client streams the body (same shape
    // as `text`/`search`), rather than truncating to the status line.
    let lines = [
        format!("alt_screen={}", t.is_alternate_screen()),
        format!("cursor_visible={}", t.cursor_visible()),
        format!("app_cursor_keys={}", m.application_cursor_keys),
        format!("app_keypad={}", m.application_keypad),
        format!("bracketed_paste={}", m.bracketed_paste),
        format!("mouse_mode={mouse_mode}"),
        format!("mouse_encoding={mouse_encoding}"),
        // Affect how typed input / printed output lands, so a client driving the
        // terminal can predict behavior: IRM (insert vs overwrite), DECAWM
        // (auto-wrap at the right margin), DECOM (cursor origin = scroll region).
        format!("insert_mode={}", m.insert_mode),
        format!("auto_wrap={}", m.auto_wrap),
        format!("origin_mode={}", m.origin_mode),
    ];
    let mut out = format!("OK {}\n", lines.len());
    for l in &lines {
        out.push_str(l);
        out.push('\n');
    }
    out
}

/// `title` -> `OK <window title>\n` (the OSC 0/2 window title; empty if unset).
fn cmd_title(term: &Arc<Mutex<Terminal>>) -> String {
    let t = term_lock(term);
    format!("OK {}\n", t.title())
}

/// `cwd` -> `OK <working directory>\n` (the shell's directory as reported via
/// OSC 7; empty if never reported). Lets an introspecting client know where
/// commands will run without scraping the prompt.
fn cmd_cwd(term: &Arc<Mutex<Terminal>>) -> String {
    let t = term_lock(term);
    format!("OK {}\n", t.current_working_directory().unwrap_or(""))
}

/// Percent-encode a string so it occupies ONE space-free token in a response
/// line: every byte that is not ASCII-graphic (and `%` itself) becomes `%XX`.
/// Spaces, newlines and non-ASCII are escaped; the client decodes. Empty -> "".
fn pct_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_graphic() && b != b'%' {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// One `image read` result line:
/// `<row> <col> <img_cols> <img_rows> <cell_row> <cell_col> <format> <nbytes> <base64>`.
/// `row`/`col` are the image's TOP-LEFT anchor on the grid; `cell_row`/`cell_col`
/// are the tile of interest (0/0 for a whole-image report, the queried tile in
/// cell mode); `nbytes` is the raw (pre-base64) length; the trailing base64 is the
/// image's full raw payload (PNG bytes etc.), independent of the GUI framebuffer.
/// Per-image payload cap for the line + JSON image channels (audit finding F4). An
/// inline image is USER-supplied (the inner TUI emits OSC 1337), so a hostile or
/// careless inner could embed a multi-megabyte image and force a large base64
/// allocation on every `image read` AND every styled `cells`/`screen` frame. Above
/// this raw-byte cap the payload is OMITTED and the image marked `truncated` — the
/// metadata + real `nbytes` still report it, so a consumer learns an image is there
/// and how big it is, then fetches it deliberately, without the per-frame blowup.
const MAX_IMAGE_PAYLOAD_BYTES: usize = 4 * 1024 * 1024; // 4 MiB raw (~5.3 MiB base64)

/// `(format, base64)` for an image, applying the F4 cap: oversized images report
/// `("truncated", "")` instead of encoding their bytes.
fn image_payload(img: &ImageData) -> (&'static str, String) {
    let fmt = match img.format {
        ImageFormat::Png => "png",
        _ => "unknown",
    };
    if img.bytes.len() > MAX_IMAGE_PAYLOAD_BYTES {
        ("truncated", String::new())
    } else {
        (fmt, aterm_codec::base64::encode(&img.bytes))
    }
}

fn image_read_line(anchor_r: usize, anchor_c: usize, tile_row: u16, tile_col: u16, img: &ImageData) -> String {
    let (fmt, b64) = image_payload(img);
    format!(
        "{anchor_r} {anchor_c} {} {} {tile_row} {tile_col} {fmt} {} {b64}",
        img.cols,
        img.rows,
        img.bytes.len(),
    )
}

/// `image read [<r> [<c>]]` -> the structured inline-image payloads (iTerm2 OSC
/// 1337) as base64, readable HEADLESS and CROSS-SESSION (unlike the framebuffer
/// `image` rasterize verb). With no args it reports every DISTINCT image on the
/// grid (deduplicated by payload identity), one line per image at its top-left
/// anchor; `image read <r>` restricts to images intersecting row `r`; `image read
/// <r> <c>` returns the single image tile covering that exact cell (`ERR none` if
/// the cell has no image). Framed `OK <nlines>\n` + one line per image.
fn cmd_image_read(term: &Arc<Mutex<Terminal>>, rest: &str) -> String {
    let t = term_lock(term);
    let rows = t.rows() as usize;
    let cols = t.cols() as usize;
    let mut it = rest.split_whitespace();
    let r_tok = it.next();
    let c_tok = it.next();

    // Cell mode: the image covering exactly (r, c).
    if let (Some(rs), Some(cs)) = (r_tok, c_tok) {
        let (Ok(r), Ok(c)) = (rs.parse::<usize>(), cs.parse::<usize>()) else {
            return "ERR bad args\n".to_string();
        };
        if r >= rows || c >= cols {
            return "ERR out of range\n".to_string();
        }
        for (col, iref) in t.images_row(r) {
            if col == c {
                let anchor_r = r.saturating_sub(iref.cell_row as usize);
                let anchor_c = col.saturating_sub(iref.cell_col as usize);
                return format!(
                    "OK 1\n{}\n",
                    image_read_line(anchor_r, anchor_c, iref.cell_row, iref.cell_col, &iref.image)
                );
            }
        }
        return "ERR none\n".to_string();
    }

    // Row mode (one row) or screen mode (all rows): distinct images, anchored.
    let row_range: Vec<usize> = match r_tok {
        Some(rs) => match rs.parse::<usize>() {
            Ok(r) if r < rows => vec![r],
            Ok(_) => return "ERR out of range\n".to_string(),
            Err(_) => return "ERR bad args\n".to_string(),
        },
        None => (0..rows).collect(),
    };
    let mut seen: Vec<*const ImageData> = Vec::new();
    let mut lines: Vec<String> = Vec::new();
    for r in row_range {
        for (col, iref) in t.images_row(r) {
            let ptr = std::sync::Arc::as_ptr(&iref.image);
            if seen.contains(&ptr) {
                continue;
            }
            seen.push(ptr);
            let anchor_r = r.saturating_sub(iref.cell_row as usize);
            let anchor_c = col.saturating_sub(iref.cell_col as usize);
            // Whole-image report: anchor + tile 0/0 (the full payload is carried).
            lines.push(image_read_line(anchor_r, anchor_c, 0, 0, &iref.image));
        }
    }
    let mut out = format!("OK {}\n", lines.len());
    for l in lines {
        out.push_str(&l);
        out.push('\n');
    }
    out
}

/// Escape a string as a JSON string BODY (no surrounding quotes): the two-char
/// escapes for `"`, `\`, and the C0 whitespace controls, and `\u00XX` for the
/// remaining control bytes. Non-ASCII UTF-8 is emitted verbatim (a JSON string is
/// UTF-8), so this is allocation-light for ordinary text. Shared by every `*_json`
/// emitter so the `--json` read mode produces RFC 8259-valid strings. `pub(crate)`
/// so [`crate::cast`]'s asciicast emitter reuses the one JSON-escape (no divergence).
pub(crate) fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0C}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// A `"key":"<escaped>"` JSON member.
fn json_str_field(key: &str, val: &str) -> String {
    format!("\"{key}\":\"{}\"", json_escape(val))
}

/// Wrap a one-line JSON object body in the read-verb framing: `OK 1\n<json>\n`.
/// The framing matches the other read verbs (`OK <n>` header + body) so the
/// EXISTING client streams the body identically whether or not `--json` is set —
/// only the body bytes change. A JSON reply is always a single body line.
fn json_ok(body: &str) -> String {
    format!("OK 1\n{body}\n")
}

/// Whether `rest` carries the `--json` / `json` read-mode flag, and the `rest`
/// with that flag token removed (so each verb's existing positional parse runs
/// UNCHANGED on the remainder). Additive: a verb line WITHOUT the flag returns
/// `(false, rest.to_string())` verbatim, so the text path is byte-identical.
fn take_json_flag(rest: &str) -> (bool, String) {
    let mut json = false;
    let mut kept: Vec<&str> = Vec::new();
    for tok in rest.split_whitespace() {
        if tok == "--json" || tok == "json" {
            json = true;
        } else {
            kept.push(tok);
        }
    }
    (json, kept.join(" "))
}

/// `text --json` -> `{"rows":["<row0>",...],"cursor":{...},"seq":N,"dims":{...}}`.
/// The rows are the SAME grapheme-faithful, control-collapsed, tail-trimmed lines
/// `cmd_text` emits, the cursor/dims mirror the `cursor`/`dims` verbs, and `seq`
/// is the engine `content_seq` (so an agent can diff frames without re-reading).
fn cmd_text_json(term: &Arc<Mutex<Terminal>>) -> String {
    let t = term_lock(term);
    let rows = t.rows() as usize;
    let cols = t.cols();
    let mut row_items: Vec<String> = Vec::with_capacity(rows);
    for r in 0..rows {
        row_items.push(format!("\"{}\"", json_escape(&visible_row(&t, r))));
    }
    let c = t.cursor();
    let vis = t.cursor_visible();
    let style = cursor_style_name(t.cursor_style());
    json_ok(&format!(
        "{{\"rows\":[{}],\"cursor\":{{\"row\":{},\"col\":{},\"visible\":{vis},{}}},\
         \"dims\":{{\"rows\":{rows},\"cols\":{cols}}},\"seq\":{}}}",
        row_items.join(","),
        c.row,
        c.col,
        json_str_field("style", style),
        t.content_seq(),
    ))
}

/// `cursor --json` -> `{"row":R,"col":C,"visible":bool,"style":"<name>"}`.
fn cmd_cursor_json(term: &Arc<Mutex<Terminal>>) -> String {
    let t = term_lock(term);
    let c = t.cursor();
    json_ok(&format!(
        "{{\"row\":{},\"col\":{},\"visible\":{},{}}}",
        c.row,
        c.col,
        t.cursor_visible(),
        json_str_field("style", cursor_style_name(t.cursor_style())),
    ))
}

/// The wire name of an [`UnderlineStyle`]: lowercase, matching the SGR 4:x family.
fn underline_style_name(u: UnderlineStyle) -> &'static str {
    match u {
        UnderlineStyle::None => "none",
        UnderlineStyle::Single => "single",
        UnderlineStyle::Double => "double",
        UnderlineStyle::Curly => "curly",
        UnderlineStyle::Dotted => "dotted",
        UnderlineStyle::Dashed => "dashed",
    }
}

/// Serialize ONE cell as the canonical `StyledCell` JSON object — the LOSSLESS,
/// fully-resolved view a styled-screen consumer (an outer agent driving an inner
/// TUI) needs. Every rendition field is read from the RESOLVED [`RenderCell`]
/// (the renderer's own decisions: palette/RGB/bold-bright/dim/inverse/hidden/
/// DECSCNM already folded into `fg`/`bg`), NOT the raw flag bits — so this carries
/// the four decorations the legacy `cell` verb dropped (underline SUBSTYLE,
/// overline, underline colour, emoji presentation). `glyph` is the combining-aware
/// grapheme (same source as `cell`/`text`); `wide_lead` is the only geometry field
/// (the raw `WIDE` flag), kept distinct from the `wide` right-half continuation.
///
/// NOTE on semantic boundary: `dim`/`blink`/`inverse`/`hidden` are baked into the
/// resolved `fg`/`bg` by `render_row` and are deliberately NOT reported as attrs
/// (recovering them is the raw-flags path; byte-exact SGR replay is the `cast`
/// raw-bytes channel's job, not this resolved-screen view).
fn styled_cell_json(t: &Terminal, r: usize, c: usize, cell: &RenderCell) -> String {
    let mut attrs: Vec<&str> = Vec::new();
    if cell.bold {
        attrs.push("bold");
    }
    if cell.italic {
        attrs.push("italic");
    }
    if cell.underline != UnderlineStyle::None {
        attrs.push("underline");
    }
    if cell.strikethrough {
        attrs.push("strike");
    }
    let attrs_json = attrs
        .iter()
        .map(|a| format!("\"{a}\""))
        .collect::<Vec<_>>()
        .join(",");
    let glyph = t.cell_grapheme(r, c).unwrap_or_default();
    let underline_color = cell.underline_color.map_or_else(
        || "null".to_string(),
        |[r, g, b]| format!("\"{r:02x}{g:02x}{b:02x}\""),
    );
    let hyperlink = t.hyperlink_at(r as u16, c as u16).map_or_else(
        || "null".to_string(),
        |u| format!("\"{}\"", json_escape(u)),
    );
    let wide_lead = cell_attrs(t.grid(), r, c).contains(CellFlags::WIDE);
    format!(
        "{{\"glyph\":\"{}\",\"fg\":\"{:02x}{:02x}{:02x}\",\"bg\":\"{:02x}{:02x}{:02x}\",\
         \"attrs\":[{attrs_json}],\"underline_style\":\"{}\",\"overline\":{},\
         \"underline_color\":{underline_color},\"emoji_presentation\":{},\
         \"wide\":{},\"wide_lead\":{wide_lead},\"hyperlink\":{hyperlink}}}",
        json_escape(&glyph),
        cell.fg[0], cell.fg[1], cell.fg[2],
        cell.bg[0], cell.bg[1], cell.bg[2],
        underline_style_name(cell.underline),
        cell.overline,
        cell.emoji_presentation,
        cell.wide,
    )
}

/// Build the whole styled-screen frame as a single-line JSON object:
/// `{"seq":N,"dims":{...},"cursor":{...},"rows":[[StyledCell,...],...]}`.
///
/// Called with the `Terminal` lock ALREADY HELD (the subscribe `cells` stream
/// reuses it under one lock so the frame is internally consistent). Every row is
/// padded out to the FULL grid width with default-coloured blanks (NO `trim_end`,
/// unlike `text`) so the consumer receives exactly `dims.rows × dims.cols` cells —
/// the lossless contract. The blank-tail fallback mirrors [`cmd_cell`] exactly.
/// The wire name of a DEC [`LineSize`](aterm_core::grid::LineSize): the renderer
/// scales these rows, so a lossless frame must carry them (audit finding F2).
fn line_size_name(s: aterm_core::grid::LineSize) -> &'static str {
    use aterm_core::grid::LineSize;
    match s {
        LineSize::SingleWidth => "single",
        LineSize::DoubleWidth => "double_width",
        LineSize::DoubleHeightTop => "double_height_top",
        LineSize::DoubleHeightBottom => "double_height_bottom",
        _ => "single",
    }
}

/// The DEC line size of visible row `r` (default single-width).
fn row_line_size(t: &Terminal, r: usize) -> aterm_core::grid::LineSize {
    u16::try_from(r)
        .ok()
        .and_then(|rr| t.grid().row(rr))
        .map_or(aterm_core::grid::LineSize::SingleWidth, |row| row.line_size())
}

/// Every DISTINCT inline image on the visible grid, each at its top-left grid
/// anchor `(row, col)` (deduplicated by payload identity). Shared shape with
/// `cmd_image_read`'s screen mode; consumed by the styled frame (audit finding F1)
/// so a `subscribe cells` / `screen` watcher sees images, not blank cells.
fn distinct_images(t: &Terminal) -> Vec<(usize, usize, std::sync::Arc<ImageData>)> {
    let mut seen: Vec<*const ImageData> = Vec::new();
    let mut out: Vec<(usize, usize, std::sync::Arc<ImageData>)> = Vec::new();
    for r in 0..t.rows() as usize {
        for (col, iref) in t.images_row(r) {
            let ptr = std::sync::Arc::as_ptr(&iref.image);
            if seen.contains(&ptr) {
                continue;
            }
            seen.push(ptr);
            let anchor_r = r.saturating_sub(iref.cell_row as usize);
            let anchor_c = col.saturating_sub(iref.cell_col as usize);
            out.push((anchor_r, anchor_c, iref.image.clone()));
        }
    }
    out
}

/// One inline image as a JSON object for the styled frame: anchor grid position,
/// cell footprint, format, raw byte length, and the base64 payload — so a watcher
/// reconstructs the picture the human sees, independent of the GUI framebuffer.
fn styled_image_json(anchor_r: usize, anchor_c: usize, img: &ImageData) -> String {
    let (fmt, b64) = image_payload(img); // F4: oversized -> ("truncated", "")
    format!(
        "{{\"row\":{anchor_r},\"col\":{anchor_c},\"cols\":{},\"rows\":{},\"format\":\"{fmt}\",\
         \"nbytes\":{},\"b64\":\"{b64}\"}}",
        img.cols,
        img.rows,
        img.bytes.len(),
    )
}

pub(crate) fn styled_frame_payload(t: &Terminal) -> String {
    let rows = t.rows() as usize;
    let cols = t.cols() as usize;
    let (dfg, dbg) = (t.default_foreground(), t.default_background());
    let blank = RenderCell {
        ch: ' ',
        fg: [dfg.r, dfg.g, dfg.b],
        bg: [dbg.r, dbg.g, dbg.b],
        wide: false,
        emoji_presentation: false,
        bold: false,
        italic: false,
        underline: UnderlineStyle::None,
        strikethrough: false,
        overline: false,
        underline_color: None,
    };
    let mut row_items: Vec<String> = Vec::with_capacity(rows);
    let mut line_sizes: Vec<&'static str> = Vec::with_capacity(rows);
    for r in 0..rows {
        let rendered = t.render_row(r);
        let mut cells: Vec<String> = Vec::with_capacity(cols);
        for c in 0..cols {
            let cell = rendered.get(c).unwrap_or(&blank);
            cells.push(styled_cell_json(t, r, c, cell));
        }
        row_items.push(format!("[{}]", cells.join(",")));
        line_sizes.push(line_size_name(row_line_size(t, r))); // F2: DEC double-width/height
    }
    let line_sizes_json = line_sizes.iter().map(|s| format!("\"{s}\"")).collect::<Vec<_>>().join(",");
    // F1: inline images (OSC 1337), base64 once per distinct image at its anchor —
    // without this a `cells`/`screen` watcher sees blank cells where the human sees
    // a picture. Empty array (cheap) on the common no-image screen.
    let images_json = distinct_images(t)
        .iter()
        .map(|(ar, ac, img)| styled_image_json(*ar, *ac, img))
        .collect::<Vec<_>>()
        .join(",");
    // F3: text selection highlight (a human/peer-initiated selection a watcher
    // would otherwise miss); `null` when nothing is selected.
    let sel = t.text_selection();
    let selection_json = if sel.is_empty() {
        "null".to_string()
    } else {
        let (sr, sc, er, ec) = sel.normalized_bounds();
        format!("{{\"start_row\":{sr},\"start_col\":{sc},\"end_row\":{er},\"end_col\":{ec}}}")
    };
    let cur = t.cursor();
    format!(
        "{{\"seq\":{},\"dims\":{{\"rows\":{rows},\"cols\":{cols}}},\
         \"cursor\":{{\"row\":{},\"col\":{},\"visible\":{},{}}},\
         \"rows\":[{}],\"line_sizes\":[{line_sizes_json}],\"selection\":{selection_json},\
         \"images\":[{images_json}]}}",
        t.content_seq(),
        cur.row,
        cur.col,
        t.cursor_visible(),
        json_str_field("style", cursor_style_name(t.cursor_style())),
        row_items.join(","),
    )
}

/// COMPILE-GATE (the GENERAL fix for the F1/F2/F3 dropped-field class): every field
/// of the renderer's input MUST be a CONSCIOUS decision — reflected in the lossless
/// styled frame, or explicitly omitted with a reason. This destructures
/// [`RenderInput`](aterm_core::render::RenderInput) WITHOUT `..`, so adding a new
/// renderer-consumed field fails to compile until someone decides whether
/// `styled_frame_payload` carries it. That turns "we silently dropped a field"
/// (F1 images, F2 line_sizes, F3 selection — all present in `RenderInput`, all once
/// missing from the frame) into a build error. Never called; it exists to type-check.
#[allow(dead_code)]
fn _styled_frame_covers_every_render_input_field(ri: &aterm_core::render::RenderInput) {
    let aterm_core::render::RenderInput {
        rows: _,           // frame "dims.rows"
        cols: _,           // frame "dims.cols"
        cells: _,          // frame "rows" (per-cell styled_cell_json)
        cursor_row: _,     // frame "cursor.row"
        cursor_col: _,     // frame "cursor.col"
        cursor_visible: _, // frame "cursor.visible"
        cursor_style: _,   // frame "cursor.style"
        display_offset: _, // OMITTED: viewport scroll position, not visible-cell content
        selection: _,      // frame "selection" (F3)
        clusters: _,       // folded into per-cell "glyph" (cell_grapheme)
        combining: _,      // folded into per-cell "glyph" (cell_grapheme)
        line_sizes: _,     // frame "line_sizes" (F2)
        images: _,         // frame "images" (F1)
        snapshot_seq: _,   // frame "seq" (the engine content version stamp)
    } = ri;
}

/// `screen` -> the full LOSSLESS styled grid as a single-line JSON frame, wrapped
/// in the standard `OK 1\n<json>\n` read framing (so the existing line-count
/// client streams it unchanged). This is the keystone "see everything" verb: it
/// carries per-cell colour + every resolved decoration + the cursor + dims + seq,
/// so an outer agent reconstructs exactly what a human sees in the inner TUI.
/// `--json` is implied (the verb is always styled JSON).
fn cmd_screen_styled_json(term: &Arc<Mutex<Terminal>>) -> String {
    let t = term_lock(term);
    json_ok(&styled_frame_payload(&t))
}

/// `dims --json` -> `{"rows":R,"cols":C,"pixel_w":W,"pixel_h":H}`.
fn cmd_dims_json(term: &Arc<Mutex<Terminal>>, cell_size: (u32, u32)) -> String {
    let t = term_lock(term);
    let rows = u32::from(t.rows());
    let cols = u32::from(t.cols());
    let (cw, ch) = cell_size;
    json_ok(&format!(
        "{{\"rows\":{rows},\"cols\":{cols},\"pixel_w\":{},\"pixel_h\":{}}}",
        cols * cw,
        rows * ch,
    ))
}

/// `blocks [N] --json` -> `{"blocks":[{...}]}`: the SAME OSC 133/633 command
/// blocks `cmd_blocks` reports (oldest-first, optional last-N), one JSON object
/// per block with the absolute rows, exit code, state, cwd and commandline. An
/// absent optional row is JSON `null`; the cwd/commandline are JSON strings (not
/// percent-encoded — JSON carries spaces natively).
fn cmd_blocks_json(term: &Arc<Mutex<Terminal>>, rest: &str) -> String {
    use aterm_core::terminal::BlockState;
    let t = term_lock(term);
    let all: Vec<_> = t.all_blocks().collect();
    let slice: &[_] = match rest.trim().parse::<usize>() {
        Ok(n) if n < all.len() => &all[all.len() - n..],
        _ => &all,
    };
    let opt_row = |r: Option<u64>| r.map_or_else(|| "null".to_string(), |v| v.to_string());
    let mut items: Vec<String> = Vec::with_capacity(slice.len());
    for b in slice {
        let state = match b.state {
            BlockState::PromptOnly => "prompt",
            BlockState::EnteringCommand => "entering",
            BlockState::Executing => "executing",
            BlockState::Complete => "complete",
            _ => "unknown",
        };
        let exit = b.exit_code.map_or_else(|| "null".to_string(), |c| c.to_string());
        items.push(format!(
            "{{\"id\":{},{},\"exit\":{exit},\"prompt\":{},\"cmd\":{},\"out\":{},\"end\":{},{},{}}}",
            b.id,
            json_str_field("state", state),
            b.prompt_start_row,
            opt_row(b.command_start_row),
            opt_row(b.output_start_row),
            opt_row(b.end_row),
            json_str_field("cwd", b.working_directory.as_deref().unwrap_or("")),
            json_str_field("cmdline", b.commandline.as_deref().unwrap_or("")),
        ));
    }
    json_ok(&format!("{{\"blocks\":[{}]}}", items.join(",")))
}

/// `blocks [N]` -> the shell-integration command blocks (OSC 133/633), oldest
/// first (or the last `N`). This is the project's point made concrete: an AI
/// driving the terminal navigates by COMMAND — exit codes, the output's absolute
/// row range, the command text and cwd — instead of scraping the screen.
///
/// COORDINATE SPACE (B-2): every `prompt`/`cmd`/`out`/`end` row is a MONOTONIC
/// ABSOLUTE row, the SINGLE read coordinate this socket uses. Feed any of them
/// DIRECTLY to `line <abs_row>` (one row) or `text` (the visible screen) — those
/// verbs accept absolute rows and convert at the read site. (Previously `line`
/// took a 0-based history index, so feeding it a block's absolute row read the
/// WRONG line; `line` now shares the absolute-row space.) For a block's full
/// output prefer `blocktext <id>`, which reads the absolute range itself and
/// reports an EXPLICIT `ERR` when those rows have been EVICTED from scrollback
/// (never silently-shifted text).
///
/// Header `OK <shown>\n`, then one line per block: `block <id> <state>
/// exit=<code|-> prompt=<row> cmd=<row|-> out=<row|-> end=<row|-> cwd=<pct>
/// cmdline=<pct>`. `state` is prompt|entering|executing|complete; cwd/cmdline
/// are percent-encoded (single tokens even with spaces). Needs a shell emitting
/// OSC 133 (see the `shell_integration` injection); empty otherwise.
fn cmd_blocks(term: &Arc<Mutex<Terminal>>, rest: &str) -> String {
    use aterm_core::terminal::BlockState;
    let t = term_lock(term);
    let all: Vec<_> = t.all_blocks().collect();
    let slice: &[_] = match rest.trim().parse::<usize>() {
        Ok(n) if n < all.len() => &all[all.len() - n..],
        _ => &all,
    };
    let mut out = format!("OK {}\n", slice.len());
    let opt_row = |r: Option<u64>| r.map_or_else(|| "-".to_string(), |v| v.to_string());
    for b in slice {
        let state = match b.state {
            BlockState::PromptOnly => "prompt",
            BlockState::EnteringCommand => "entering",
            BlockState::Executing => "executing",
            BlockState::Complete => "complete",
            _ => "unknown",
        };
        let exit = b.exit_code.map_or_else(|| "-".to_string(), |c| c.to_string());
        out.push_str(&format!(
            "block {} {} exit={} prompt={} cmd={} out={} end={} cwd={} cmdline={}\n",
            b.id,
            state,
            exit,
            b.prompt_start_row,
            opt_row(b.command_start_row),
            opt_row(b.output_start_row),
            opt_row(b.end_row),
            pct_encode(b.working_directory.as_deref().unwrap_or("")),
            pct_encode(b.commandline.as_deref().unwrap_or("")),
        ));
    }
    out
}

/// `blocktext <id>` -> the OUTPUT text of command block `<id>` (from `blocks`),
/// one row per line after `OK <n>`. The engine reads the block's absolute row
/// range itself (across scrollback AND the visible screen), so the caller does
/// NOT juggle coordinate spaces — an AI reads a specific command's output (e.g.
/// the failed one's error) directly. `ERR` if the id is unknown or the block has
/// not produced output yet.
fn cmd_blocktext(term: &Arc<Mutex<Terminal>>, rest: &str) -> String {
    let Ok(id) = rest.trim().parse::<u64>() else {
        return "ERR usage: blocktext <id>\n".to_string();
    };
    let t = term_lock(term);
    let Some(block) = t.block_by_id(id).cloned() else {
        return "ERR no such block\n".to_string();
    };
    // Use the enum form so an EVICTED block returns an explicit signal instead
    // of silently-shifted or empty text (B-1 / DL-1).
    let text = match t.block_output_text(&block) {
        aterm_core::terminal::BlockText::Text(s) => s,
        aterm_core::terminal::BlockText::Evicted => {
            return "ERR block output evicted from scrollback\n".to_string();
        }
        aterm_core::terminal::BlockText::NotAvailable => {
            return "ERR block has no output yet\n".to_string();
        }
    };
    let lines: Vec<&str> = text.lines().collect();
    let mut out = format!("OK {}\n", lines.len());
    for line in lines {
        let s: String = line.chars().map(visible_char).collect();
        out.push_str(s.trim_end());
        out.push('\n');
    }
    out
}

/// `wait [timeout_ms]` -> block until a command block COMPLETES (a NEW one since
/// this call), then `OK complete <id> exit=<code|->`; `OK timeout` if none
/// completes in time (default 30 000 ms, capped at 600 000). The AI runs a
/// command then `wait`s for it to finish before reading with `blocktext`, with
/// no busy-polling. Needs shell integration (OSC 133); with none it times out.
/// Polls server-side, releasing the Terminal lock between checks so the PTY
/// reader keeps advancing the command.
fn cmd_wait(term: &Arc<Mutex<Terminal>>, rest: &str) -> String {
    use aterm_core::terminal::BlockState;
    let timeout_ms = rest.trim().parse::<u64>().unwrap_or(30_000).min(600_000);
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
    let complete_count =
        |t: &Terminal| t.all_blocks().filter(|b| matches!(b.state, BlockState::Complete)).count();
    let baseline = complete_count(&term_lock(term));
    loop {
        {
            let t = term_lock(term);
            let completed: Vec<_> =
                t.all_blocks().filter(|b| matches!(b.state, BlockState::Complete)).collect();
            if completed.len() > baseline {
                let b = completed.last().expect("len > baseline >= 0");
                let exit = b.exit_code.map_or_else(|| "-".to_string(), |c| c.to_string());
                return format!("OK complete {} exit={}\n", b.id, exit);
            }
        }
        if std::time::Instant::now() >= deadline {
            return "OK timeout\n".to_string();
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

/// `colors` -> the terminal's theme colors:
/// `OK fg=<rrggbb> bg=<rrggbb> cursor=<rrggbb|default>`.
/// Programs change these via OSC 10/11/12; the per-cell `cell` verb only reports
/// already-RESOLVED colors, so this surfaces the theme itself (the default
/// fg/bg and the cursor color) for a client deciding how to render or reason.
fn cmd_colors(term: &Arc<Mutex<Terminal>>) -> String {
    let t = term_lock(term);
    let h = |r: u8, g: u8, b: u8| format!("{r:02x}{g:02x}{b:02x}");
    let fg = t.default_foreground();
    let bg = t.default_background();
    let cursor = t
        .cursor_color()
        .map_or_else(|| "default".to_string(), |c| h(c.r, c.g, c.b));
    format!(
        "OK fg={} bg={} cursor={}\n",
        h(fg.r, fg.g, fg.b),
        h(bg.r, bg.g, bg.b),
        cursor,
    )
}

/// Process-wide smart-selection rules, built lazily ONCE (the builtin rules
/// compile a set of regexes). Shared by the GUI's double-click gesture and the
/// `select word` verb so both use identical word/URL/path boundaries.
static SMART_RULES: OnceLock<SmartSelection> = OnceLock::new();

/// The engine's builtin smart-selection rules (lazy singleton).
pub(crate) fn smart_rules() -> &'static SmartSelection {
    SMART_RULES.get_or_init(SmartSelection::with_builtin_rules)
}

/// Inclusive word-column bounds at live-screen `(row, col)`, from the engine's
/// builtin smart-selection rules (URL/path/email/... patterns, falling back to
/// plain alphanumeric+underscore words). `None` when the cell is whitespace or
/// to the right of the row's text — the caller selects just the clicked cell.
pub(crate) fn word_cols(t: &Terminal, row: i32, col: u16) -> Option<(u16, u16)> {
    let text = t.get_line_text(row, None)?;
    // `word_boundaries_at_column` clamps a past-the-text column INTO the text
    // (it would snap to the LAST word); a click right of the text is whitespace.
    if usize::from(col) >= aterm_core::grapheme::byte_to_column(&text, text.len()) {
        return None;
    }
    let (start, end) = smart_rules().word_boundaries_at_column(&text, usize::from(col))?;
    // The returned end column is EXCLUSIVE; selection anchors are inclusive cells.
    let last = end.saturating_sub(1).max(start);
    let clamp = |v: usize| u16::try_from(v).unwrap_or(u16::MAX);
    Some((clamp(start), clamp(last)))
}

/// Word-select at live-screen `(row, col)` — the double-click / `select word`
/// gesture: a `Semantic` selection spanning the word's cells (both boundary
/// cells inclusive, Left/Right anchor sides), or just the clicked cell when on
/// whitespace. Completes the selection and returns the inclusive
/// `(start_col, end_col)` actually selected.
pub(crate) fn select_word(t: &mut Terminal, row: i32, col: u16) -> (u16, u16) {
    let (start, end) = word_cols(t, row, col).unwrap_or((col, col));
    let sel = t.text_selection_mut();
    sel.start_selection(row, col, SelectionSide::Left, SelectionType::Semantic);
    sel.expand_semantic(start, end);
    sel.complete_selection();
    (start, end)
}

/// Line-select live-screen row `row` — the triple-click / `select line`
/// gesture: a `Lines` selection expanded to the full row width (the extracted
/// text is the whole row, trailing blanks trimmed). Completes the selection.
pub(crate) fn select_line(t: &mut Terminal, row: i32) {
    let max_col = t.cols().saturating_sub(1);
    let sel = t.text_selection_mut();
    sel.start_selection(row, 0, SelectionSide::Left, SelectionType::Lines);
    sel.expand_lines(max_col);
    sel.complete_selection();
}

/// `select ...` -> drive the engine's text selection. Forms:
///
/// * `select <r1> <c1> <r2> <c2>` — simple range from cell `(r1,c1)` to
///   `(r2,c2)`, BOTH endpoint cells INCLUSIVE (the two points are normalized
///   to reading order first, so either order works).
/// * `select word <r> <c>` — semantic (word/URL/path) selection at the cell
///   via the engine's builtin smart-selection rules; a whitespace cell selects
///   just itself. Same code path as the GUI's double-click.
/// * `select line <r>` — full-line selection of row `r` (triple-click).
/// * `select block <r1> <c1> <r2> <c2>` — rectangular (block) selection with
///   the two cells as INCLUSIVE corners (any corner order).
/// * `select extend <r> <c>` — extend the EXISTING selection so cell `(r,c)`
///   becomes its new (inclusive) endpoint (shift-click); `ERR no selection`
///   when nothing is selected.
/// * `select clear` — clear the selection.
///
/// Rows are LIVE-screen coords as signed integers: `0..rows` is the visible
/// live screen and NEGATIVE rows address scrollback (`-1` = the most recently
/// scrolled-off line). All forms nudge a windowed session to repaint the
/// highlight and reply `OK\n`.
///
/// SEAM CARVE-OUT (Phase 0.5): `select` mutates `text_selection_mut()` directly
/// rather than through [`App::input`](crate::App::input). This is DELIBERATE and
/// NOT a convergence gap: `select` produces NO PTY bytes (it sets ABSOLUTE
/// coordinates, not a press/drag GESTURE), so it has no byte-indistinguishability
/// stake. It is the controller analogue of an external "set the selection here"
/// command — there is no human winit event that produces an absolute-coordinate
/// selection (the human path is press → drag → release, which DOES go through the
/// seam's `MouseButton`/`MouseMove` gesture arms). Keeping it out of the seam
/// avoids inventing a synthetic gesture; the seam's "sole selection-mutation"
/// claim is about the GESTURE path, which both sources share.
fn cmd_select(
    term: &Arc<Mutex<Terminal>>,
    proxy: &EventLoopProxy<Wake>,
    session: u64,
    rest: &str,
) -> String {
    const USAGE: &str = "ERR usage: select <r1> <c1> <r2> <c2> | select word <r> <c> | \
                         select line <r> | select block <r1> <c1> <r2> <c2> | \
                         select extend <r> <c> | select clear\n";
    let rest = rest.trim();
    if rest == "clear" {
        term_lock(term).text_selection_mut().clear();
        let _ = proxy.send_event(Wake::redraw(session));
        return "OK\n".to_string();
    }
    let mut it = rest.split_whitespace();
    let Some(head) = it.next() else {
        return USAGE.to_string();
    };
    match head {
        "word" => {
            let (Some(Ok(r)), Some(Ok(c))) = (
                it.next().map(str::parse::<i32>),
                it.next().map(str::parse::<u16>),
            ) else {
                return "ERR usage: select word <r> <c>\n".to_string();
            };
            select_word(&mut term_lock(term), r, c);
        }
        "line" => {
            let Some(Ok(r)) = it.next().map(str::parse::<i32>) else {
                return "ERR usage: select line <r>\n".to_string();
            };
            select_line(&mut term_lock(term), r);
        }
        "block" => {
            let (Some(Ok(r1)), Some(Ok(c1)), Some(Ok(r2)), Some(Ok(c2))) = (
                it.next().map(str::parse::<i32>),
                it.next().map(str::parse::<u16>),
                it.next().map(str::parse::<i32>),
                it.next().map(str::parse::<u16>),
            ) else {
                return "ERR usage: select block <r1> <c1> <r2> <c2>\n".to_string();
            };
            // Block normalization is corner-order agnostic (min/max per axis)
            // and forces Left/Right sides on the normalized corners, so both
            // given cells are inclusive whichever corners they are.
            let mut t = term_lock(term);
            let sel = t.text_selection_mut();
            sel.start_selection(r1, c1, SelectionSide::Left, SelectionType::Block);
            sel.update_selection(r2, c2, SelectionSide::Right);
            sel.complete_selection();
        }
        "extend" => {
            let (Some(Ok(r)), Some(Ok(c))) = (
                it.next().map(str::parse::<i32>),
                it.next().map(str::parse::<u16>),
            ) else {
                return "ERR usage: select extend <r> <c>\n".to_string();
            };
            let mut t = term_lock(term);
            let sel = t.text_selection_mut();
            if !sel.has_selection() || sel.is_empty() {
                return "ERR no selection\n".to_string();
            }
            // Side by direction so the clicked cell is INCLUDED whichever way
            // the selection grows: extending backward the moving anchor is the
            // normalized START (Left side includes its cell), extending
            // forward it is the normalized END (Right side includes its cell).
            let st = sel.start();
            let side = if (r, c) < (st.row, st.col) {
                SelectionSide::Left
            } else {
                SelectionSide::Right
            };
            sel.extend_selection(r, c, side);
            sel.complete_selection();
        }
        r1s => {
            let (Some(c1s), Some(r2s), Some(c2s)) = (it.next(), it.next(), it.next()) else {
                return USAGE.to_string();
            };
            let (Ok(r1), Ok(c1), Ok(r2), Ok(c2)) = (
                r1s.parse::<i32>(),
                c1s.parse::<u16>(),
                r2s.parse::<i32>(),
                c2s.parse::<u16>(),
            ) else {
                return "ERR bad args\n".to_string();
            };
            // Normalize to reading order so the Left/Right anchor sides below
            // always make BOTH endpoint cells inclusive (a Right-sided end
            // includes its cell; after normalization the end is never
            // side-flipped into an exclusion).
            let ((sr, sc), (er, ec)) = if (r2, c2) < (r1, c1) {
                ((r2, c2), (r1, c1))
            } else {
                ((r1, c1), (r2, c2))
            };
            let mut t = term_lock(term);
            let sel = t.text_selection_mut();
            sel.start_selection(sr, sc, SelectionSide::Left, SelectionType::Simple);
            sel.update_selection(er, ec, SelectionSide::Right);
            sel.complete_selection();
        }
    }
    let _ = proxy.send_event(Wake::redraw(session));
    "OK\n".to_string()
}

/// `selection` -> the currently selected text as `OK <n>\n` + `n` data lines
/// (the text split on newlines, same framing as `text`). No or empty
/// selection -> `OK 0\n`.
fn cmd_selection(term: &Arc<Mutex<Terminal>>) -> String {
    match term_lock(term).selection_to_string() {
        Some(text) if !text.is_empty() => {
            let lines: Vec<&str> = text.split('\n').collect();
            let mut out = format!("OK {}\n", lines.len());
            for l in lines {
                out.push_str(l);
                out.push('\n');
            }
            out
        }
        _ => "OK 0\n".to_string(),
    }
}

/// `copy` -> copy the currently selected text to the macOS system clipboard
/// (`pbcopy`) and reply `OK <byte-count>\n`; no or empty selection -> `OK 0\n`
/// (the clipboard is left untouched). The selection is NOT cleared.
fn cmd_copy(term: &Arc<Mutex<Terminal>>) -> String {
    let text = term_lock(term).selection_to_string();
    match text {
        Some(t) if !t.is_empty() => {
            if pbcopy(&t) {
                format!("OK {}\n", t.len())
            } else {
                "ERR pbcopy failed\n".to_string()
            }
        }
        _ => "OK 0\n".to_string(),
    }
}

/// Pipe `text` to `/usr/bin/pbcopy`, placing it on the macOS system clipboard.
/// Shared by the `copy` verb and the GUI's Cmd-C. Returns success.
pub(crate) fn pbcopy(text: &str) -> bool {
    use std::process::{Command, Stdio};
    let Ok(mut child) = Command::new("/usr/bin/pbcopy").stdin(Stdio::piped()).spawn() else {
        return false;
    };
    let wrote = child
        .stdin
        .take()
        .is_some_and(|mut stdin| stdin.write_all(text.as_bytes()).is_ok());
    // Reap the child regardless of write success (no zombies on failure).
    let status = child.wait();
    wrote && status.is_ok_and(|s| s.success())
}

/// `image [path]` -> hand the render to the MAIN thread (it owns the renderer),
/// block on the reply, and report `OK <w> <h> <path>\n`.
///
/// PATH SAFETY: the PNG is confined to the `images/` subdir of the per-user
/// socket directory. A bare name (`image shot.png`) lands there; an empty
/// request defaults to `images/aterm-control.png`. A path that would escape the
/// subdir (`../`, an absolute path elsewhere, a symlink out) is refused with
/// `ERR path\n` and audited — the socket can no longer be used to overwrite an
/// arbitrary file via a caller-supplied path.
fn cmd_image(
    proxy: &EventLoopProxy<Wake>,
    queue: &ImageQueue,
    rest: &str,
    sock_dir: &std::path::Path,
) -> String {
    let requested = {
        let p = rest.trim();
        if p.is_empty() { "aterm-control.png" } else { p }
    };
    let Some(target) = control_auth::confine_image_path(sock_dir, requested) else {
        log_denial(
            AUDIT_SUBSYSTEM,
            &format!("image write '{requested}'"),
            aterm_containment::mode_or_containment(),
            "path escapes images/ subdir or names a nested target",
        );
        return "ERR path\n".to_string();
    };
    // For the reply only — the writer re-opens via the dir fd, not this string.
    let path = target.display_path().to_string_lossy().into_owned();
    let (tx, rx) = std::sync::mpsc::channel();
    queue
        .lock()
        .unwrap()
        .push_back(ImageReq { target, reply: tx });
    if proxy.send_event(Wake::Control).is_err() {
        return "ERR event loop gone\n".to_string();
    }
    match rx.recv() {
        Ok((w, h)) => format!("OK {w} {h} {path}\n"),
        Err(_) => "ERR render failed\n".to_string(),
    }
}

/// `window [path]` -> capture the FRONT window's ENTIRE on-screen pixels — the
/// native macOS chrome (titlebar, traffic lights, the unified toolbar, the
/// full-width tab strip) AND the terminal content — to a PNG, replying
/// `OK <w> <h> <path>` (the SAME wire shape as `image`). This closes the
/// introspection gap `image` leaves: `image` rasterizes only the terminal content
/// framebuffer with NO OS chrome, so an AI driving aterm could never SEE the whole
/// window; `window` photographs the real composited pixels, so its height is
/// GREATER than `image`'s (it includes the titlebar + tab strip).
///
/// PATH CONFINEMENT (mirrors [`cmd_image`]): the caller-supplied `path` is validated
/// by `confine_image_path` to a single filename inside the socket dir's `images/`
/// subdir, so the socket can never overwrite an arbitrary file. The default name
/// (`aterm-window.png`) parallels `image`'s `aterm-control.png`.
///
/// MAIN-THREAD HOP (mirrors [`cmd_chrome`]): reaching the front window's `NSWindow`
/// + reading its window number + calling `CGWindowListCreateImage` may ONLY happen
/// on the main thread (AppKit / window-server state), but this runs on a background
/// control thread. So we post [`Wake::CaptureWindow`] with the confined target + a
/// one-shot reply channel and BLOCK; the main thread captures (`App::capture_window`)
/// and replies `Ok((w, h))` or an `Err(msg)` we surface verbatim as `ERR <msg>`.
///
/// PERMISSION: `CGWindowListCreateImage` needs macOS Screen Recording permission; if
/// it is not granted the main thread replies with the clear, actionable grant
/// instructions (it does NOT crash). Off macOS the main thread replies that capture
/// is macOS-only; headless (no OS window) replies that there is no window to capture.
fn cmd_window(
    proxy: &EventLoopProxy<Wake>,
    rest: &str,
    sock_dir: &std::path::Path,
) -> String {
    let requested = {
        let p = rest.trim();
        if p.is_empty() { "aterm-window.png" } else { p }
    };
    let Some(target) = control_auth::confine_image_path(sock_dir, requested) else {
        log_denial(
            AUDIT_SUBSYSTEM,
            &format!("window write '{requested}'"),
            aterm_containment::mode_or_containment(),
            "path escapes images/ subdir or names a nested target",
        );
        return "ERR path\n".to_string();
    };
    // For the reply only — the writer re-opens via the dir fd, not this string.
    let path = target.display_path().to_string_lossy().into_owned();
    let (tx, rx) = std::sync::mpsc::channel();
    if proxy.send_event(Wake::CaptureWindow { path: target, reply: tx }).is_err() {
        return "ERR event loop gone\n".to_string();
    }
    match rx.recv() {
        Ok(Ok((w, h))) => format!("OK {w} {h} {path}\n"),
        // The main thread's clear, actionable message (missing permission / headless /
        // off-macOS / capture failure) is surfaced verbatim as a single `ERR` line.
        Ok(Err(msg)) => format!("ERR {msg}\n"),
        Err(_) => "ERR window capture failed\n".to_string(),
    }
}

/// `chrome` -> dump the frontmost window's NATIVE macOS UI: its `NSToolbar` items
/// (each `id=<identifier> label="<label>"`, e.g. the "+" New Tab button) and the
/// app menu bar (`menu "File": New Window, New Tab, ...`). A read-only
/// introspection verb so an AI driving aterm can SEE and verify the native chrome
/// — which `image`/`text` CANNOT capture, as they render only the terminal content
/// view, never the OS toolbar/menu bar.
///
/// MAIN-THREAD HOP (mirrors [`cmd_image`]): AppKit objects (`NSToolbar`/`NSMenu`/
/// `NSWindow`) may ONLY be touched on the main thread, but this runs on a
/// background control thread. So we build a one-shot reply channel, post
/// [`Wake::ReadChrome`] to wake the event loop, and BLOCK on the reply; the main
/// thread reads the chrome (`App::read_native_chrome`) and sends back the text
/// lines. The lines are returned in the SAME multi-line shape as `text`:
/// `OK <n>\n` followed by `<n>` data rows.
///
/// Off macOS the main thread replies with one explanatory line (no native chrome),
/// so the wire shape (`OK 1` + one row) is identical on every platform.
fn cmd_chrome(proxy: &EventLoopProxy<Wake>) -> String {
    let (tx, rx) = std::sync::mpsc::channel();
    if proxy.send_event(Wake::ReadChrome { reply: tx }).is_err() {
        return "ERR event loop gone\n".to_string();
    }
    let lines = match rx.recv() {
        Ok(lines) => lines,
        Err(_) => return "ERR chrome read failed\n".to_string(),
    };
    // Same `OK <n>\n` + n-rows framing the `text` verb uses, so the aterm-ctl client
    // prints the rows verbatim (it lists `chrome` among the multi-line verbs).
    let mut out = format!("OK {}\n", lines.len());
    for line in lines {
        out.push_str(&line);
        out.push('\n');
    }
    out
}

/// Parse a `tab <arg>` request into the [`TabAction`] it drives (the PURE part, so
/// it is unit-testable without an event loop). Grammar: `new` opens a tab, a
/// 0-based integer `<N>` selects tab N, `next`/`prev` cycle. `None` (an unknown /
/// missing arg) maps the caller to the usage error.
fn parse_tab(rest: &str) -> Option<TabAction> {
    let rest = rest.trim();
    // Multi-word forms first: `close [N]` and `move <from> <to>`.
    let mut it = rest.split_whitespace();
    match it.next() {
        Some("new") if it.next().is_none() => return Some(TabAction::New),
        Some("next") if it.next().is_none() => return Some(TabAction::Next),
        Some("prev") if it.next().is_none() => return Some(TabAction::Prev),
        // `close` (active tab) or `close <N>` (a specific tab).
        Some("close") => {
            return match it.next() {
                None => Some(TabAction::Close(None)),
                Some(n) => {
                    let i = n.parse::<usize>().ok()?;
                    // Reject trailing junk after the index.
                    it.next().is_none().then_some(TabAction::Close(Some(i)))
                }
            };
        }
        // `move <from> <to>` — reorder.
        Some("move") => {
            let (from, to) = (it.next()?, it.next()?);
            if it.next().is_some() {
                return None; // trailing junk
            }
            let from = from.parse::<usize>().ok()?;
            let to = to.parse::<usize>().ok()?;
            return Some(TabAction::Move { from, to });
        }
        _ => {}
    }
    // Otherwise a bare 0-based index selects a tab.
    rest.parse::<usize>().ok().map(TabAction::Select)
}

/// `tab new | <N> | next | prev` -> DRIVE the FRONT window's native tabs and reply
/// `OK <active_index> <tab_count>`.
///
/// MAIN-THREAD HOP (mirrors [`cmd_chrome`]): mutating `App` (its tabs) may ONLY
/// happen on the event loop, but this runs on a background control thread. So we
/// parse the action, post [`Wake::TabCmd`] with a one-shot reply channel, and BLOCK
/// on the reply; the main thread resolves `self.frontmost_window`, applies the
/// action via the SAME command paths the keyboard/menu use (`open_tab` / `switch_tab`
/// / `cycle_tab`), and sends back the resulting `(active, count)`. `new` reuses the
/// new-tab path; the native toolbar segments then re-track via `App::sync_window`.
fn cmd_tab(proxy: &EventLoopProxy<Wake>, rest: &str) -> String {
    let Some(action) = parse_tab(rest) else {
        return "ERR usage: tab <new|N|next|prev|close [N]|move <from> <to>>\n".to_string();
    };
    let (tx, rx) = std::sync::mpsc::channel();
    if proxy.send_event(Wake::TabCmd { action, reply: tx }).is_err() {
        return "ERR event loop gone\n".to_string();
    }
    match rx.recv() {
        Ok((active, count)) => format!("OK {active} {count}\n"),
        Err(_) => "ERR tab command failed\n".to_string(),
    }
}

/// Parse + range-check a `resize <r> <c>` request (the PURE part, so it is unit
/// testable without an event loop). Returns the validated `(rows, cols)` or the
/// exact error string the verb replies with.
///
/// Requests outside `1..=MAX_GRID_ROWS`/`MAX_GRID_COLS` are rejected with
/// `ERR out of range` rather than silently clamped, so a caller learns its
/// requested size was not applied.
fn parse_resize(rest: &str) -> Result<(u16, u16), String> {
    let mut it = rest.split_whitespace();
    let (Some(rs), Some(cs)) = (it.next(), it.next()) else {
        return Err("ERR usage: resize <r> <c>\n".to_string());
    };
    let (Ok(r), Ok(c)) = (rs.parse::<u16>(), cs.parse::<u16>()) else {
        return Err("ERR bad args\n".to_string());
    };
    if !(1..=MAX_GRID_ROWS).contains(&r) || !(1..=MAX_GRID_COLS).contains(&c) {
        return Err("ERR out of range\n".to_string());
    }
    Ok((r, c))
}

/// `resize <r> <c>` -> resize the engine grid, the PTY, AND the GUI (RES-1).
///
/// The main thread is the SOLE geometry owner (`App.rows/cols`, the framebuffer,
/// the window). Resizing the term + PTY here directly — as the verb used to —
/// left `App` stale and sent no repaint, so a follow-up `image`/`dims` (which
/// read `App`/the framebuffer) disagreed with the engine. So the verb now ONLY
/// validates and forwards an `InputEvent::Resize` (in a `Wake::Input`) to the main
/// thread, which applies the term + PTY + window resize and requests a redraw in
/// one owner. A dropped proxy (event loop gone) means the GUI is shutting down:
/// report it.
///
/// RES-1: the verb sets `echo_to_window: true` so the seam ALSO asks the window to
/// match the new grid pixel size (the verb has no window event of its own). The
/// interactive winit `Resized` path uses `echo_to_window: false` (the window is
/// already that size). `echo_to_window` is a transport flag, NOT a `Source` branch.
fn cmd_resize(proxy: &EventLoopProxy<Wake>, rest: &str) -> String {
    // Range-check up front (keeps the precise `ERR out of range` / usage strings),
    // then post a reply-bearing Resize through the seam. The seam re-clamps and
    // reports `RangeRejected` if somehow out of range — but a valid request here
    // returns `Ok`, so the contract is unchanged for existing callers.
    let (r, c) = match parse_resize(rest) {
        Ok(rc) => rc,
        Err(e) => return e,
    };
    match post_input_reply(
        proxy,
        Op::WriteInput,
        vec![InputEvent::Resize { rows: r, cols: c, echo_to_window: true }],
    ) {
        Ok(InputOutcome::RangeRejected) => "ERR out of range\n".to_string(),
        Ok(_) => "OK\n".to_string(),
        Err(e) => e,
    }
}

/// `grant <src-id> <op>` -> mint an edge (src -> this session, op) and return its
/// bearer token hex. Owner-only (also enforced by the gate's catch-all Deny).
fn cmd_grant(ctx: &SessionCtx, scope: Scope, rest: &str) -> String {
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
fn cmd_revoke(ctx: &SessionCtx, scope: Scope, rest: &str) -> String {
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
fn cmd_whoami(ctx: &SessionCtx, scope: Scope) -> String {
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

/// Funnel all control-verb bytes through the active session's single SinkWriter
/// (whole-frame atomicity vs the GUI keyboard path + reader-thread replies). Drops
/// a closed-peer error like the legacy writer did. Used ONLY by the audited raw
/// hatch (`send`/`feed`); the human-vocabulary verbs go through the seam instead.
fn write_pty(sink: &SinkWriter, data: &[u8]) {
    let _ = sink.write_frame(data);
}

/// Phase 0.5: post a fire-and-forget [`InputEvent`] batch to the main thread (the
/// sole owner of the seam), mirroring the `cmd_resize` -> `Wake::Resize` pattern.
/// Returns the verb's reply string. A whole controller gesture is sent as ONE
/// batch so it applies atomically in one main-loop turn (A.2.3).
///
/// `op` is the AUDIT class of the OPERATION being performed (`ReadScreen` for the
/// view-control verbs `scroll`, `WriteInput` for the input verbs) — captured from
/// the verb itself, NOT from the connection's scope. A control connection is always
/// a `Controller` (owner and edge alike; `Human` is built only by the in-thread
/// winit handlers), so the scope adds nothing to the audit `Source` and is not
/// threaded here: deriving the op from the operation keeps the (future §7.5) audit
/// log correct-by-construction and independent of any cached connect-time op.
fn post_input(proxy: &EventLoopProxy<Wake>, op: Op, batch: Vec<InputEvent>) -> String {
    let src = Source::Controller { op };
    if proxy.send_event(Wake::Input { batch, src, reply: None }).is_err() {
        return "ERR event loop closed\n".to_string();
    }
    "OK\n".to_string()
}

/// Phase 0.5: post a reply-bearing [`InputEvent`] and BLOCK on the seam's
/// [`InputOutcome`] (mirrors `cmd_image`'s `mpsc` round-trip). Used by `resize`
/// (range-reject) — the caller maps the outcome to its reply string. `op` is the
/// operation's audit class (see [`post_input`]).
fn post_input_reply(
    proxy: &EventLoopProxy<Wake>,
    op: Op,
    batch: Vec<InputEvent>,
) -> Result<InputOutcome, String> {
    let src = Source::Controller { op };
    let (tx, rx) = std::sync::mpsc::channel();
    if proxy
        .send_event(Wake::Input { batch, src, reply: Some(tx) })
        .is_err()
    {
        return Err("ERR event loop closed\n".to_string());
    }
    rx.recv().map_err(|_| "ERR event loop closed\n".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::InputEvent;
    use std::io::Read;
    use std::os::unix::io::FromRawFd;

    /// `take_mods` is ADDITIVE: a line WITHOUT `mods=` parses to empty mods and
    /// the untouched body, so every pre-Phase-0.5 caller stays byte-compatible.
    #[test]
    fn take_mods_is_additive() {
        use aterm_types::keyboard::Modifiers;
        let (m, body) = take_mods("up");
        assert_eq!(m, Modifiers::empty());
        assert_eq!(body, "up");
        let (m, body) = take_mods("up mods=ctrl+shift");
        assert_eq!(m, Modifiers::CTRL | Modifiers::SHIFT);
        assert_eq!(body, "up");
        // Aliases + comma separator + token position-independence.
        let (m, body) = take_mods("mods=cmd,alt end");
        assert_eq!(m, Modifiers::SUPER | Modifiers::ALT);
        assert_eq!(body, "end");
    }

    /// `parse_key` builds the named-key event the seam encodes; unknown -> None.
    #[test]
    fn parse_key_grammar() {
        use aterm_types::keyboard::{Key, KeyEventType, Modifiers, NamedKey as Nk};
        let press = KeyEventType::Press;
        assert_eq!(
            parse_key("up"),
            Some(InputEvent::Key { key: Key::Named(Nk::ArrowUp), mods: Modifiers::empty(), base_layout: None, event_type: press }),
        );
        assert_eq!(
            parse_key("f5 mods=ctrl"),
            Some(InputEvent::Key { key: Key::Named(Nk::F5), mods: Modifiers::CTRL, base_layout: None, event_type: press }),
        );
        assert_eq!(parse_key("nope"), None);

        // Inline modifier+character combos build the SAME (Key::Character, mods)
        // event `parse_ctrl` does, so the encoder derives the control byte
        // (`ctrl+u` -> 0x15) — see `parse_ctrl_eq_inline_key` for the byte proof.
        for c in ['u', 'c', 'a', 'd', 'l', 'w'] {
            assert_eq!(
                parse_key(&format!("ctrl+{c}")),
                Some(InputEvent::Key {
                    key: Key::Character(c),
                    mods: Modifiers::CTRL,
                    base_layout: None,
                    event_type: press,
                }),
                "ctrl+{c} should be Character('{c}') + CTRL",
            );
        }
        // Case-insensitive on the character, matching `parse_ctrl`.
        assert_eq!(
            parse_key("ctrl+U"),
            Some(InputEvent::Key { key: Key::Character('u'), mods: Modifiers::CTRL, base_layout: None, event_type: press }),
        );
        // alt+/shift+/super+ and stacked prefixes.
        assert_eq!(
            parse_key("alt+x"),
            Some(InputEvent::Key { key: Key::Character('x'), mods: Modifiers::ALT, base_layout: None, event_type: press }),
        );
        assert_eq!(
            parse_key("ctrl+shift+a"),
            Some(InputEvent::Key {
                key: Key::Character('a'),
                mods: Modifiers::CTRL | Modifiers::SHIFT,
                base_layout: None,
                event_type: press,
            }),
        );
        // Inline prefixes are additive with a trailing `mods=` token.
        assert_eq!(parse_key("ctrl+u"), parse_key("u mods=ctrl"));
        // Inline prefixes also apply to NAMED keys.
        assert_eq!(
            parse_key("ctrl+up"),
            Some(InputEvent::Key { key: Key::Named(Nk::ArrowUp), mods: Modifiers::CTRL, base_layout: None, event_type: press }),
        );
        // The literal `+` key (no recognized modifier before it) survives.
        assert_eq!(
            parse_key("+"),
            Some(InputEvent::Key { key: Key::Character('+'), mods: Modifiers::empty(), base_layout: None, event_type: press }),
        );
        // A multi-char residual that is not a named key is still rejected.
        assert_eq!(parse_key("ctrl+nope"), None);
    }

    /// The FULL `NamedKey` vocabulary is reachable — numpad, F13–F35, media/audio,
    /// modifier-side keys, system keys — so a controller can press any physical key
    /// a human can (closes the `key`-grammar fidelity gap). Table-driven.
    #[test]
    fn parse_key_full_named_vocabulary() {
        use aterm_types::keyboard::{Key, NamedKey as Nk};
        let cases: &[(&str, Nk)] = &[
            ("space", Nk::Space),
            ("capslock", Nk::CapsLock),
            ("menu", Nk::ContextMenu),
            ("contextmenu", Nk::ContextMenu),
            ("printscreen", Nk::PrintScreen),
            ("f13", Nk::F13),
            ("f35", Nk::F35),
            ("kp0", Nk::Numpad0),
            ("kp9", Nk::Numpad9),
            ("kpdot", Nk::NumpadDecimal),
            ("kpenter", Nk::NumpadEnter),
            ("kpadd", Nk::NumpadAdd),
            ("kpbegin", Nk::NumpadBegin),
            ("shiftleft", Nk::ShiftLeft),
            ("metaright", Nk::MetaRight),
            ("hyperleft", Nk::HyperLeft),
            ("mediaplaypause", Nk::MediaPlayPause),
            ("volumeup", Nk::AudioVolumeUp),
            ("mute", Nk::AudioVolumeMute),
        ];
        for (tok, want) in cases {
            assert_eq!(
                parse_key(tok),
                Some(InputEvent::Key {
                    key: Key::Named(*want),
                    mods: aterm_types::keyboard::Modifiers::empty(),
                    base_layout: None,
                    event_type: aterm_types::keyboard::KeyEventType::Press,
                }),
                "token `{tok}` should map to {want:?}",
            );
        }
    }

    /// `type=press|repeat|release` reaches the event (and drives the Kitty CSI-u
    /// event-type sub-field); an unknown value rejects the whole line.
    #[test]
    fn parse_key_event_type() {
        use aterm_types::keyboard::{Key, KeyEventType, Modifiers, NamedKey as Nk};
        let ev = |t| Some(InputEvent::Key {
            key: Key::Named(Nk::ArrowUp), mods: Modifiers::empty(), base_layout: None, event_type: t,
        });
        assert_eq!(parse_key("up"), ev(KeyEventType::Press));
        assert_eq!(parse_key("up type=press"), ev(KeyEventType::Press));
        assert_eq!(parse_key("up type=repeat"), ev(KeyEventType::Repeat));
        assert_eq!(parse_key("up type=release"), ev(KeyEventType::Release));
        assert_eq!(parse_key("up type=up"), ev(KeyEventType::Release));
        // Additive with mods=, position-independent.
        assert_eq!(
            parse_key("type=release mods=ctrl up"),
            Some(InputEvent::Key {
                key: Key::Named(Nk::ArrowUp), mods: Modifiers::CTRL, base_layout: None,
                event_type: KeyEventType::Release,
            }),
        );
        // Unknown event type rejects the line.
        assert_eq!(parse_key("up type=bogus"), None);
    }

    /// `base=<char>` carries the US-QWERTY base-layout key (Kitty
    /// REPORT_ALTERNATE_KEYS 3rd field); a non-single-char value rejects.
    #[test]
    fn parse_key_base_layout() {
        use aterm_types::keyboard::{Key, KeyEventType, Modifiers};
        assert_eq!(
            parse_key("q base=a"),
            Some(InputEvent::Key {
                key: Key::Character('q'), mods: Modifiers::empty(),
                base_layout: Some('a'), event_type: KeyEventType::Press,
            }),
        );
        assert_eq!(parse_key("q base=ab"), None);
    }

    /// `meta` and `hyper` are their OWN modifier bits (Kitty), distinct from ALT.
    #[test]
    fn take_mods_parses_meta_and_hyper_distinctly() {
        use aterm_types::keyboard::Modifiers;
        let (m, _) = take_mods("a mods=meta");
        assert_eq!(m, Modifiers::META);
        let (m, _) = take_mods("a mods=hyper");
        assert_eq!(m, Modifiers::HYPER);
        // alt is still ALT, and meta no longer aliases it.
        let (m, _) = take_mods("a mods=alt");
        assert_eq!(m, Modifiers::ALT);
        let (m, _) = take_mods("a mods=ctrl+meta+hyper");
        assert_eq!(m, Modifiers::CTRL | Modifiers::META | Modifiers::HYPER);
        // Inline-prefix form agrees.
        assert_eq!(parse_key("meta+x"), parse_key("x mods=meta"));
    }

    /// `send` is byte-faithful: interior whitespace is NOT collapsed (the line
    /// decoder / dispatcher preserve the tail verbatim).
    #[test]
    fn send_preserves_whitespace() {
        let mut fds = [0i32; 2];
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        let sink = SinkWriter::new(fds[1]);
        let reply = cmd_send(&sink, "a   b\tc");
        assert_eq!(reply, "OK\n");
        // `SinkWriter::new` does NOT own the fd, so close the write end explicitly
        // (mirroring `input::tests::egress_bytes`) or `read_to_end` blocks forever.
        unsafe { libc::close(fds[1]) };
        let mut got = Vec::new();
        let mut r = unsafe { std::fs::File::from_raw_fd(fds[0]) };
        r.read_to_end(&mut got).unwrap();
        assert_eq!(got, b"a   b\tc");
    }

    /// ITEM 1 keystone: the styled `screen` frame carries EVERY resolved
    /// decoration — including the four the legacy `cell` verb dropped (underline
    /// SUBSTYLE, overline, underline colour, and — via the bold path — the
    /// renderer's resolved rendition). This is the regression that proves
    /// losslessness vs the old plaintext/flag-bits projection.
    #[test]
    fn screen_styled_reports_all_resolved_decorations() {
        use aterm_core::terminal::Terminal;
        let mut t = Terminal::new(3, 10);
        // bold + curly underline (SGR 4:3) + overline (SGR 53) + RGB underline
        // colour (SGR 58:2::255:0:0) applied to 'Z'.
        t.process(b"\x1b[1m\x1b[4:3m\x1b[53m\x1b[58:2::255:0:0mZ");
        let frame = styled_frame_payload(&t);
        assert!(frame.contains("\"underline_style\":\"curly\""), "curly underline lost: {frame}");
        assert!(frame.contains("\"overline\":true"), "overline lost: {frame}");
        assert!(frame.contains("\"underline_color\":\"ff0000\""), "underline colour lost: {frame}");
        assert!(frame.contains("\"bold\""), "bold attr lost: {frame}");
    }

    /// The styled frame is the FULL grid with NO trim: every one of rows×cols
    /// cells is present (the lossless contract), dims/seq are reported.
    #[test]
    fn screen_styled_frame_shape_no_trim() {
        use aterm_core::terminal::Terminal;
        let mut t = Terminal::new(3, 10);
        t.process(b"hi");
        let frame = styled_frame_payload(&t);
        assert!(frame.contains("\"dims\":{\"rows\":3,\"cols\":10}"), "{frame}");
        // 3 rows × 10 cols = 30 cells, each carries exactly one "glyph" key.
        let glyphs = frame.matches("\"glyph\"").count();
        assert_eq!(glyphs, 30, "expected 30 cells with no trim, got {glyphs}");
        assert!(frame.contains(&format!("\"seq\":{}", t.content_seq())), "{frame}");
    }

    /// The glyph is the combining-aware grapheme (é = e+U+0301, not bare 'e'), and
    /// the cell's OSC-8 hyperlink target is surfaced — both lossless vs a human.
    #[test]
    fn screen_styled_glyph_and_hyperlink_faithful() {
        use aterm_core::terminal::Terminal;
        let mut t = Terminal::new(2, 20);
        t.process("e\u{0301}".as_bytes());
        t.process(b"\x1b]8;;https://example.com\x1b\\L\x1b]8;;\x1b\\");
        let frame = styled_frame_payload(&t);
        assert!(frame.contains("\"glyph\":\"e\u{0301}\""), "combining grapheme lost: {frame}");
        assert!(
            frame.contains("\"hyperlink\":\"https://example.com\""),
            "hyperlink lost: {frame}"
        );
    }

    /// The `screen` verb wraps the frame in the standard single-line `OK 1\n…\n`
    /// read framing — the JSON body is ONE physical line so the existing
    /// line-count client streams it unchanged regardless of grid size.
    #[test]
    fn screen_verb_framing_is_single_line_json() {
        use aterm_core::terminal::Terminal;
        let term = Arc::new(Mutex::new(Terminal::new(2, 4)));
        let out = cmd_screen_styled_json(&term);
        assert!(out.starts_with("OK 1\n"), "{out}");
        let body = out.strip_prefix("OK 1\n").unwrap().strip_suffix('\n').unwrap();
        assert!(!body.contains('\n'), "styled frame must be single-line JSON: {body}");
        assert!(body.starts_with("{\"seq\":") && body.ends_with('}'), "{body}");
    }

    /// `screen` is gated as a READ verb (ReadScreen), like every other observer.
    #[test]
    fn screen_verb_is_read_gated() {
        assert_eq!(required_op("screen"), Some(Op::ReadScreen));
    }

    /// F4: a small inline image encodes normally; an oversized one (user-supplied
    /// OSC 1337) is `truncated` with an EMPTY payload — but its real `nbytes` is
    /// still reported, so a consumer learns it exists without the multi-MB blowup on
    /// every `image read` / styled frame.
    #[test]
    fn oversized_image_payload_is_truncated_not_encoded() {
        use aterm_core::grid::extra::{ImageData, ImageFormat};
        let small = ImageData { bytes: vec![1, 2, 3, 4], format: ImageFormat::Png, cols: 1, rows: 1 };
        let (fmt, b64) = image_payload(&small);
        assert_eq!(fmt, "png");
        assert!(!b64.is_empty(), "small image must carry its payload");

        let big = ImageData {
            bytes: vec![0u8; MAX_IMAGE_PAYLOAD_BYTES + 1],
            format: ImageFormat::Png,
            cols: 80,
            rows: 24,
        };
        let (fmt, b64) = image_payload(&big);
        assert_eq!(fmt, "truncated", "oversized image must be marked truncated");
        assert!(b64.is_empty(), "oversized image must NOT be base64-encoded");
        // The line form still reports the real size (so the consumer can decide).
        let line = image_read_line(0, 0, 0, 0, &big);
        assert!(line.contains(&format!("truncated {}", MAX_IMAGE_PAYLOAD_BYTES + 1)), "{line}");
        // And the JSON form is well-formed with an empty payload + real nbytes.
        let js = styled_image_json(0, 0, &big);
        assert!(js.contains("\"format\":\"truncated\"") && js.contains("\"b64\":\"\""), "{js}");
        assert!(js.contains(&format!("\"nbytes\":{}", MAX_IMAGE_PAYLOAD_BYTES + 1)), "{js}");
    }

    /// LOSSLESS FIDELITY (F1/F2/F3): the styled frame carries inline IMAGES (not
    /// blank cells), DEC double-width/height LINE SIZES, and the text SELECTION —
    /// the three fields the renderer consumes that were once dropped. A human sees
    /// all three; an outer agent watching the frame now does too.
    #[test]
    fn screen_styled_frame_carries_images_line_sizes_and_selection() {
        use aterm_core::terminal::Terminal;
        let mut t = Terminal::new(3, 10);
        // F1: an inline image (OSC 1337 File=, 2x1) — PNG magic + 4 NULs.
        t.process(b"\x1b]1337;File=inline=1;width=2;height=1:iVBORw0KGgoAAAAA\x1b\\");
        // F2: make row 1 double-width (DECDWL, ESC # 6).
        t.process(b"\x1b[2;1H\x1b#6");
        let frame = styled_frame_payload(&t);
        assert!(
            frame.contains("\"images\":[{\"row\":0,\"col\":0,\"cols\":2,\"rows\":1,\"format\":\"png\",\"nbytes\":12,\"b64\":\"iVBORw0KGgoAAAAA\"}]"),
            "image must be in the frame, not a blank cell: {frame}"
        );
        assert!(frame.contains("\"double_width\""), "double-width line size lost: {frame}");
        // F3: select a region, assert it surfaces.
        {
            let sel = t.text_selection_mut();
            sel.start_selection(0, 1, SelectionSide::Left, SelectionType::Simple);
            sel.update_selection(0, 5, SelectionSide::Right);
            sel.complete_selection();
        }
        let frame = styled_frame_payload(&t);
        assert!(
            frame.contains("\"selection\":{\"start_row\":0,\"start_col\":1,"),
            "selection must surface in the frame: {frame}"
        );
        // And no-selection / no-image stays null / empty (the cheap common case).
        let plain = Terminal::new(2, 4);
        let pf = styled_frame_payload(&plain);
        assert!(pf.contains("\"selection\":null"), "no selection -> null: {pf}");
        assert!(pf.contains("\"images\":[]"), "no images -> empty: {pf}");
    }

    /// An inline iTerm2 image (OSC 1337 `File=`) read back as STRUCTURED base64,
    /// deduplicated across its covered cells. The base64 INPUT below is a
    /// hand-computed literal (PNG magic + 4 NUL bytes), so the OUTPUT matching it
    /// proves `b64_encode` independently. `image read` is the headless,
    /// framebuffer-free path.
    #[test]
    fn image_read_returns_payload_and_dedups() {
        use aterm_core::terminal::Terminal;
        // 12 raw bytes = PNG magic (8) + 4×0x00; standard base64 = "iVBORw0KGgoAAAAA".
        let term = Arc::new(Mutex::new(Terminal::new(3, 10)));
        term_lock(&term)
            .process(b"\x1b]1337;File=inline=1;width=2;height=1:iVBORw0KGgoAAAAA\x1b\\");
        let out = cmd_image_read(&term, "");
        let mut lines = out.lines();
        assert_eq!(lines.next().unwrap(), "OK 1", "expected one deduped image: {out}");
        let line = lines.next().unwrap();
        // <row> <col> <img_cols> <img_rows> <cell_row> <cell_col> <format> <nbytes> <b64>
        assert_eq!(line, "0 0 2 1 0 0 png 12 iVBORw0KGgoAAAAA", "got: {line}");
        assert!(lines.next().is_none(), "image must be deduped to one line: {out}");
    }

    /// `image read` on a screen with no images is `OK 0`.
    #[test]
    fn image_read_empty_screen_is_ok_zero() {
        use aterm_core::terminal::Terminal;
        let term = Arc::new(Mutex::new(Terminal::new(3, 10)));
        assert_eq!(cmd_image_read(&term, ""), "OK 0\n");
    }

    /// Cell addressing: `image read <r> <c>` returns the covering tile, with the
    /// tile coords of the queried cell; a cell with no image is `ERR none`.
    #[test]
    fn image_read_cell_addressing_and_none() {
        use aterm_core::terminal::Terminal;
        let term = Arc::new(Mutex::new(Terminal::new(3, 10)));
        term_lock(&term)
            .process(b"\x1b]1337;File=inline=1;width=2;height=1:iVBORw0KGgoAAAAA\x1b\\");
        // Cell (0,1) is the right tile of the 2-wide image: cell_col == 1.
        let out = cmd_image_read(&term, "0 1");
        assert_eq!(out, "OK 1\n0 0 2 1 0 1 png 12 iVBORw0KGgoAAAAA\n", "got: {out}");
        // A cell with no image -> ERR none.
        assert_eq!(cmd_image_read(&term, "0 5"), "ERR none\n");
        // Out of grid -> ERR out of range.
        assert_eq!(cmd_image_read(&term, "9 9"), "ERR out of range\n");
    }

    /// `image` (incl. `image read`) is ReadScreen-gated and therefore allowed
    /// cross-session (the read path is matched before the rasterize fail-closed).
    #[test]
    fn image_read_is_readscreen() {
        assert_eq!(required_op("image"), Some(Op::ReadScreen));
    }

    /// ITEM 5b: the cross-process forward DECISION — Owner-only, presents the
    /// per-op edge token, rewrites the child selector to `@.`, and fails closed on
    /// an Edge scope, an unknown child, or a relaunched (nonce-mismatched) child.
    #[test]
    fn proxy_forward_plan_owner_only_op_scoped_and_nonce_guarded() {
        use aterm_session::{EdgeToken, LaunchNonce, SessionId};
        let dir = std::env::temp_dir().join(format!("aterm-fwd-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let store = crate::session_store::new_store();
        let child = SessionId::generate();
        let nonce = LaunchNonce::generate();
        let entry = crate::proxy::ProxyEntry {
            nonce,
            read: EdgeToken::generate(),
            write: EdgeToken::generate(),
            signal: EdgeToken::generate(),
        };
        let read_hex = entry.read.to_hex();
        let write_hex = entry.write.to_hex();
        crate::proxy::register_child(child.clone(), entry);
        // The published socket MUST live directly in `dir` (the runtime sock dir);
        // `confine_proxy_sock` rejects anything else. The forward dials the confined
        // (canonical-dir-rooted) path.
        let child_sock = dir.join("aterm-child.sock");
        let child_sock_str = child_sock.to_string_lossy().into_owned();
        let confined = std::fs::canonicalize(&dir).unwrap().join("aterm-child.sock");
        let confined_str = confined.to_string_lossy().into_owned();
        crate::proxy::write_graph_entry(&dir, &child, &child_sock_str, &nonce);

        let line = format!("@{} screen", child.as_str());
        // Owner forwards: read verb → READ token, selector rewritten to @.
        let plan = proxy_forward_plan(&line, Scope::Owner, &store, &dir).expect("owner forwards");
        assert_eq!(plan.0, confined_str);
        assert_eq!(plan.1, format!("TOKEN {read_hex} @. screen\n"));
        // A write verb selects the WRITE token (op→token mapping).
        let kline = format!("@{} key up", child.as_str());
        let kplan = proxy_forward_plan(&kline, Scope::Owner, &store, &dir).expect("write forwards");
        assert_eq!(kplan.1, format!("TOKEN {write_hex} @. key up\n"));

        // M1: the selector-SECOND `subscribe` grammar is forwarded too (the bug
        // was that only @-first lines were), presenting the READ token, rewriting
        // the child selector to `@.`, and preserving the streams + flags.
        let sline = format!("subscribe @{} cells,bytes every-frame", child.as_str());
        let splan = proxy_forward_plan(&sline, Scope::Owner, &store, &dir).expect("subscribe forwards");
        assert_eq!(splan.0, confined_str);
        assert_eq!(splan.1, format!("TOKEN {read_hex} subscribe @. cells,bytes every-frame\n"));

        // Edge scope cannot escalate to a child.
        assert!(
            proxy_forward_plan(&line, Scope::Edge(EdgeToken::generate()), &store, &dir)
                .is_none(),
            "edge scope must not forward",
        );
        // A subscribe with a comma-list of targets stays local (not a single child).
        let mixed = format!("subscribe @.,@{} cells", child.as_str());
        assert!(proxy_forward_plan(&mixed, Scope::Owner, &store, &dir).is_none());
        // An unregistered child id → no plan.
        let other = format!("@{} screen", SessionId::generate().as_str());
        assert!(proxy_forward_plan(&other, Scope::Owner, &store, &dir).is_none());

        // A relaunch (graph entry under a NEW nonce) fails closed.
        crate::proxy::write_graph_entry(&dir, &child, &child_sock_str, &LaunchNonce::generate());
        assert!(
            proxy_forward_plan(&line, Scope::Owner, &store, &dir).is_none(),
            "nonce mismatch must fail closed",
        );

        // CONFINEMENT: a graph entry redirected to a socket OUTSIDE our runtime dir
        // (a hostile same-uid overwrite that copies the readable nonce) fails closed —
        // the parent never dials it nor presents the edge token. Restore the correct
        // nonce so ONLY the out-of-dir path is what trips the gate.
        crate::proxy::write_graph_entry(&dir, &child, "/tmp/evil-attacker.sock", &nonce);
        assert!(
            proxy_forward_plan(&line, Scope::Owner, &store, &dir).is_none(),
            "out-of-dir socket path must fail closed (token must not leak)",
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// CONFORMANCE (Tier-1) of the REAL router to `dispatch_complete_model`'s
    /// invariant `ForwardableRemoteAlwaysForwarded`: for an Owner reaching a
    /// registered REMOTE child, `proxy_forward_plan` forwards a verb IFF that verb
    /// is forwardable — exactly the op-bearing verbs (`required_op` ⇒ read/write/
    /// signal). This is the code↔model binding the abstract `ty` proof needs: it
    /// drives the real decision function over every verb CLASS and checks it
    /// matches the modeled predicate. Drop the subscribe forward arm (M1), or let
    /// any forwardable verb fall to the local path, and this fails.
    #[test]
    fn proxy_forward_plan_conforms_to_dispatch_model() {
        use aterm_session::{EdgeToken, LaunchNonce, SessionId};
        let dir = std::env::temp_dir().join(format!("aterm-conform-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let store = crate::session_store::new_store();
        let child = SessionId::generate();
        let nonce = LaunchNonce::generate();
        crate::proxy::register_child(
            child.clone(),
            crate::proxy::ProxyEntry {
                nonce,
                read: EdgeToken::generate(),
                write: EdgeToken::generate(),
                signal: EdgeToken::generate(),
            },
        );
        // The published socket must live directly in `dir` — `confine_proxy_sock`
        // (the proxy forward's anti-token-capture guard) rejects any out-of-dir path.
        let child_sock = dir.join("aterm-child.sock").to_string_lossy().into_owned();
        crate::proxy::write_graph_entry(&dir, &child, &child_sock, &nonce);
        let sid = child.as_str();

        // Representative verb lines across EVERY class: forwardable read-side,
        // forwardable write-side, signal, and NON-forwardable (owner-only / global).
        let cases: &[&str] = &[
            "screen", "text", "cell 0 0", "search x", "modes", "image read", "cast", "scroll up",
            "key up", "ctrl c", "feed 03", "feed-bin 4", "paste hi", "resize 10 20", "focus in",
            "send hi", "signal int", // forwardable (read/write/signal)
            "grant x", "revoke x", "whoami", "sessions", "version", "bogus", // NOT forwardable
        ];
        for verb_line in cases {
            let verb = verb_line.split_whitespace().next().unwrap();
            let line = format!("@{sid} {verb_line}");
            let forwarded = proxy_forward_plan(&line, Scope::Owner, &store, &dir).is_some();
            let forwardable = required_op(verb).is_some();
            assert_eq!(
                forwarded, forwardable,
                "router ≠ model for `{verb}`: forwarded={forwarded} forwardable={forwardable}"
            );
        }
        // `subscribe` is selector-SECOND; it MUST forward (read-side) — the exact M1
        // case the generic @-first parse missed. (Not in the loop: different grammar.)
        let sub = format!("subscribe @{sid} cells,bytes");
        assert!(
            proxy_forward_plan(&sub, Scope::Owner, &store, &dir).is_some(),
            "subscribe @<child> must forward (the M1 regression guard)"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `key ctrl+<c>` and `ctrl <c>` build the IDENTICAL event, so both drive
    /// the encoder through the same seam and write the same control byte to the
    /// PTY (`ctrl+u` -> 0x15). This is the load-bearing invariant of the fix.
    #[test]
    fn parse_ctrl_eq_inline_key() {
        for c in ['u', 'c', 'a', 'd', 'l', 'w'] {
            assert_eq!(
                parse_key(&format!("ctrl+{c}")),
                parse_ctrl(&c.to_string()),
                "key ctrl+{c} must equal ctrl {c}",
            );
        }
    }

    /// `parse_ctrl` lower-cases and CTRL-modifies exactly one letter; else None.
    #[test]
    fn parse_ctrl_grammar() {
        use aterm_types::keyboard::{Key, Modifiers};
        assert_eq!(
            parse_ctrl("C"),
            Some(InputEvent::Key {
                key: Key::Character('c'),
                mods: Modifiers::CTRL,
                base_layout: None,
                event_type: aterm_types::keyboard::KeyEventType::Press,
            }),
        );
        assert_eq!(parse_ctrl(""), None);
        assert_eq!(parse_ctrl("ab"), None);
    }

    /// `parse_mouse` — the additive `mods=`/`count=`/`side=`/`block=` grammar, the
    /// load-bearing half of the mouse-convergence claim (kills a/b/i + the
    /// ambient-state read for block-select).
    #[test]
    fn parse_mouse_grammar() {
        use aterm_core::selection::SelectionSide;
        use aterm_types::mouse::{MouseButton, ALT_MASK, SHIFT_MASK};
        // Bare press: empty mods, count 1, left side, simple (block=false).
        assert_eq!(
            parse_mouse("press left 5 9"),
            Ok(InputEvent::MouseButton {
                button: MouseButton::Left, pressed: true, row: 5, col: 9,
                mods: 0, click_count: 1, side: SelectionSide::Left, block: false,
            }),
        );
        // Full grammar, tokens in any position.
        assert_eq!(
            parse_mouse("count=2 press left side=right 5 9 mods=shift+alt block=1"),
            Ok(InputEvent::MouseButton {
                button: MouseButton::Left, pressed: true, row: 5, col: 9,
                mods: SHIFT_MASK | ALT_MASK, click_count: 2, side: SelectionSide::Right, block: true,
            }),
        );
        // count clamps to 1..=3.
        let Ok(InputEvent::MouseButton { click_count, .. }) = parse_mouse("press left 0 0 count=9") else {
            panic!("press parses")
        };
        assert_eq!(click_count, 3);
        // move: bare = hover code 3; with a button = its X10 drag code.
        assert_eq!(
            parse_mouse("move 7 3"),
            Ok(InputEvent::MouseMove { buttons: 3, row: 7, col: 3, mods: 0, side: SelectionSide::Left }),
        );
        let Ok(InputEvent::MouseMove { buttons, .. }) = parse_mouse("move left 7 3") else {
            panic!("drag move parses")
        };
        assert_eq!(buttons, MouseButton::Left.code());
        // wheel actions default to lines=1.
        assert_eq!(
            parse_mouse("wheelup left 2 4"),
            Ok(InputEvent::Wheel { dir_up: true, lines: 1, row: 2, col: 4, mods: 0 }),
        );
        // errors.
        assert!(parse_mouse("press left").is_err(), "missing row/col");
        assert!(parse_mouse("press banana 1 1").is_err(), "bad button");
        assert!(parse_mouse("jump left 1 1").is_err(), "bad action");
    }

    /// The control socket follows the ACTIVE tab: `resolve_active` snapshots
    /// whatever the shared `ActiveHandle` currently points at, so after the GUI
    /// updates it on a tab switch, the next request targets the new session.
    #[test]
    fn resolve_active_follows_handle_updates() {
        use aterm_session::sink::SinkWriter;
        use aterm_session::{EdgeTable, LaunchNonce, SessionId};
        let term_a = Arc::new(Mutex::new(Terminal::new(24, 80)));
        let term_b = Arc::new(Mutex::new(Terminal::new(24, 80)));
        let ctx = Arc::new(crate::SessionCtx {
            sink: Arc::new(SinkWriter::new(11)),
            edges: std::sync::Mutex::new(EdgeTable::new()),
            self_id: SessionId::generate(),
            nonce: LaunchNonce::generate(),
            cast: Arc::new(std::sync::Mutex::new(crate::cast::CastRecorder::new(80, 24))),
            temporal: Arc::new(std::sync::Mutex::new(crate::temporal::TemporalRecorder::new())),
            byte_fanout: Arc::new(crate::cast::ByteFanout::new()),
        });
        let active: ActiveHandle = Arc::new(Mutex::new(ActiveSession {
            term: term_a.clone(),
            master: 11,
            id: 0,
            ctx: ctx.clone(),
        }));

        let (t, m, id, _ctx) = resolve_active(&active);
        assert!(Arc::ptr_eq(&t, &term_a) && m == 11 && id == 0, "tab 0 active");

        // GUI switches to a new tab (sync_active_session).
        {
            let mut g = active.lock().unwrap();
            g.term = term_b.clone();
            g.master = 22;
            g.id = 3;
        }
        let (t, m, id, _ctx) = resolve_active(&active);
        assert!(
            Arc::ptr_eq(&t, &term_b) && m == 22 && id == 3,
            "resolve_active must track the switch to tab 3",
        );
    }

    /// A live, PIPE-backed session: a real `SinkWriter` over the WRITE end of a
    /// `pipe(2)` (so `seam_egress`/`cmd_send` bytes are readable from `rx`), its own
    /// `Terminal`, and a `SessionHandle` registered under `local_id`. The read end
    /// is returned separately so the test can drain the bytes that reached THIS
    /// session's PTY. Mirrors the production wiring (term+sink+ctx are the SAME
    /// `Arc`s the registry hands back) so the cross-session resolve is exercised
    /// for real, not stubbed.
    fn pipe_session(local_id: u64) -> (crate::session_store::SessionHandle, std::fs::File) {
        use aterm_session::sink::SinkWriter;
        use aterm_session::{EdgeTable, LaunchNonce, SessionId};
        use crate::session_store::{SessionHandle, SessionState};
        let mut fds = [0i32; 2];
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0, "pipe(2)");
        let (rd, wr) = (fds[0], fds[1]);
        let rx = unsafe { std::fs::File::from_raw_fd(rd) };
        let sid = SessionId::generate();
        let nonce = LaunchNonce::generate();
        let ctx = Arc::new(crate::SessionCtx {
            sink: Arc::new(SinkWriter::new(wr)),
            edges: std::sync::Mutex::new(EdgeTable::new()),
            self_id: sid.clone(),
            nonce,
            cast: Arc::new(std::sync::Mutex::new(crate::cast::CastRecorder::new(80, 24))),
            temporal: Arc::new(std::sync::Mutex::new(crate::temporal::TemporalRecorder::new())),
            byte_fanout: Arc::new(crate::cast::ByteFanout::new()),
        });
        let handle = SessionHandle {
            sid,
            nonce,
            local_id,
            parent: None,
            state: SessionState::Alive,
            title: String::new(),
            term: Arc::new(Mutex::new(Terminal::new(24, 80))),
            master: wr, // the write end doubles as the "master" for this headless test
            ctx,
        };
        (handle, rx)
    }

    /// Read whatever bytes are buffered in a pipe WITHOUT blocking when empty:
    /// flips the read end non-blocking, then `read`s once. Used to assert a sink
    /// got (or did NOT get) bytes.
    fn drain_pipe(rx: &std::fs::File) -> Vec<u8> {
        use std::os::fd::AsRawFd;
        let fd = rx.as_raw_fd();
        let fl = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        unsafe { libc::fcntl(fd, libc::F_SETFL, fl | libc::O_NONBLOCK) };
        let mut buf = [0u8; 4096];
        let n = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
        if n > 0 { buf[..n as usize].to_vec() } else { Vec::new() }
    }

    /// THE follow-up's core claim: a cross-session input verb resolves the
    /// `@<selector>` TARGET the SAME way `send`/`feed` do and drives the source-blind
    /// seam against THAT session — bytes land in the TARGET's sink, never self's,
    /// and the self path is unchanged. Round-trips `key`/`feed`/`select`/`send`.
    #[test]
    fn cross_session_input_reaches_target_sink_not_self() {
        use crate::session_store::new_store;
        let store = new_store();
        let (h_self, self_rx) = pipe_session(1);
        let (h_target, target_rx) = pipe_session(2);
        // Both terms are left in the default keyboard mode (a `key enter` is a bare
        // CR). Mouse tracking is exercised separately in
        // `cross_session_mouse_reports_or_scrolls_target`.
        store.write().unwrap().register(h_self.clone());
        store.write().unwrap().register(h_target.clone());

        let self_tuple: Target =
            (h_self.term.clone(), h_self.master, h_self.local_id, h_self.ctx.clone());

        // Resolve `@2` (the target's local id) EXACTLY as `handle()` does.
        let sel = Selector::parse("2");
        assert!(matches!(sel, Selector::Local(2)), "@2 parses to Local(2)");
        let (term, master, session, ctx) =
            resolve_target(&self_tuple, &store, &sel).expect("@2 resolves");
        assert!(Arc::ptr_eq(&term, &h_target.term), "resolved the TARGET term");
        assert_eq!(session, 2);
        let _ = master;
        let ctx: &SessionCtx = &ctx;

        // `key enter` cross-session: CR reaches the TARGET sink, nothing to self.
        assert_eq!(cross_input(&term, ctx, parse_key("enter"), "ERR\n"), "OK\n");
        assert_eq!(drain_pipe(&target_rx), b"\r", "key bytes hit the TARGET pty");
        assert!(drain_pipe(&self_rx).is_empty(), "self pty must be untouched");

        // `feed 03` (Ctrl-C) cross-session: `cmd_feed(&ctx.sink, ..)` is ALREADY the
        // resolved-target path — assert it stays correct alongside the new arms.
        assert_eq!(cmd_feed(&ctx.sink, "03"), "OK 1 bytes\n");
        assert_eq!(drain_pipe(&target_rx), b"\x03", "feed bytes hit the TARGET pty");
        assert!(drain_pipe(&self_rx).is_empty(), "feed must not touch self");

        // `send` (the other always-cross writer) still writes to the resolved sink.
        // `send` is RAW (no implicit CR unless a literal trailing `\n` is given).
        let _ = cmd_send(&ctx.sink, "ls");
        assert_eq!(drain_pipe(&target_rx), b"ls", "send bytes hit the TARGET pty");
        assert!(drain_pipe(&self_rx).is_empty(), "send must not touch self");

        // `select` is ALREADY cross-correct and is left untouched (it has no
        // `is_cross` guard): it mutates the RESOLVED `term`'s selection and repaints
        // by the RESOLVED `session` id. `cmd_select` needs an `EventLoopProxy` (not
        // buildable off the main thread), so we exercise the SAME engine selection
        // path it uses on the resolved target — `text_selection_mut()` over the rows
        // we just drove in — and read it back via `cmd_selection` on the TARGET term,
        // proving the resolved target is the one being selected (and that it emits no
        // pty bytes, the read-side contract).
        {
            term_lock(&term).process(b"hello world");
            let mut t = term_lock(&term);
            let sel = t.text_selection_mut();
            sel.start_selection(0, 0, SelectionSide::Left, SelectionType::Simple);
            sel.update_selection(0, 4, SelectionSide::Right);
            sel.complete_selection();
        }
        let reply = cmd_selection(&term);
        assert!(reply.starts_with("OK ") && reply.contains("hello"), "TARGET selection: {reply}");
        assert!(drain_pipe(&target_rx).is_empty(), "select must not write pty bytes");
        assert_eq!(session, 2, "select repaints by the RESOLVED target id");
    }

    /// Cross-session `resize` applies `echo_to_window:false` to the TARGET term+pty
    /// ONLY (never the active window). It exercises ALL THREE per-session artifacts
    /// `apply_term_resize` touches (main.rs:2453-2463): the engine grid, the PTY
    /// winsize (over a REAL `openpty` master, asserted via `TIOCGWINSZ` — a pipe fd
    /// would make the `TIOCSWINSZ` ioctl silently no-op), and the target's own
    /// asciicast geometry record (so a cross resize is indistinguishable from a self
    /// resize in the target's `cast` timeline). Out-of-range requests reuse the
    /// shared `parse_resize` errors and mutate nothing.
    #[test]
    fn cross_session_resize_targets_term_only() {
        use aterm_session::sink::SinkWriter;
        use aterm_session::{EdgeTable, LaunchNonce, SessionId};

        // A REAL pty pair so `aterm_pty::resize`'s TIOCSWINSZ actually takes effect
        // (a plain pipe is not a tty and the ioctl would no-op).
        let mut master = 0i32;
        let mut slave = 0i32;
        assert_eq!(
            unsafe {
                libc::openpty(
                    &mut master,
                    &mut slave,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            },
            0,
            "openpty"
        );
        let sid = SessionId::generate();
        let nonce = LaunchNonce::generate();
        let ctx = Arc::new(crate::SessionCtx {
            sink: Arc::new(SinkWriter::new(master)),
            edges: std::sync::Mutex::new(EdgeTable::new()),
            self_id: sid,
            nonce,
            cast: Arc::new(std::sync::Mutex::new(crate::cast::CastRecorder::new(80, 24))),
            temporal: Arc::new(std::sync::Mutex::new(crate::temporal::TemporalRecorder::new())),
            byte_fanout: Arc::new(crate::cast::ByteFanout::new()),
        });
        let term = Arc::new(Mutex::new(Terminal::new(24, 80)));
        let before = ctx.cast.lock().unwrap().event_count();

        assert_eq!(cross_resize(&term, master, &ctx, "10 40"), "OK\n");
        // (1) engine grid.
        {
            let t = term_lock(&term);
            assert_eq!((t.rows(), t.cols()), (10, 40), "TARGET grid resized");
        }
        // (2) PTY winsize — the half a pipe-fd test could not prove.
        {
            let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
            assert_eq!(
                unsafe { libc::ioctl(master, libc::TIOCGWINSZ, &mut ws) },
                0,
                "TIOCGWINSZ"
            );
            assert_eq!((ws.ws_row, ws.ws_col), (10, 40), "TARGET pty winsize updated");
        }
        // (3) asciicast record — one new `[t,"r","40x10"]` on the target timeline,
        // matching the self path's `apply_term_resize`.
        assert_eq!(
            ctx.cast.lock().unwrap().event_count(),
            before + 1,
            "cross resize recorded into the TARGET's cast"
        );

        // Out-of-range reuses parse_resize's exact string; nothing is mutated (no
        // extra cast event, grid unchanged).
        assert_eq!(cross_resize(&term, master, &ctx, "65535 65535"), "ERR out of range\n");
        {
            let t = term_lock(&term);
            assert_eq!((t.rows(), t.cols()), (10, 40), "grid unchanged after reject");
        }
        assert_eq!(
            ctx.cast.lock().unwrap().event_count(),
            before + 1,
            "rejected resize records nothing"
        );

        unsafe {
            libc::close(slave);
            libc::close(master);
        }
    }

    /// Cross-session `mouse` against the TARGET, BOTH tracking states (the
    /// `cross_mouse_apply` core, proxy-free):
    ///   * tracking ON  -> the seam writes a mouse REPORT to the TARGET sink (and
    ///     nothing to self); `cross_mouse_apply` returns `Ok(false)` (no viewport
    ///     move, so no repaint).
    ///   * tracking OFF -> a WHEEL falls back to `scroll_display` on the TARGET term
    ///     (viewport moves, `Ok(true)` = repaint) and emits NO pty bytes; a plain
    ///     PRESS is a deliberate no-op (`Ok(false)`, sink empty, offset unchanged).
    #[test]
    fn cross_session_mouse_reports_or_scrolls_target() {
        let (_h_self, self_rx) = pipe_session(1);
        let (h_target, target_rx) = pipe_session(2);
        let term = &h_target.term;
        let ctx: &SessionCtx = &h_target.ctx;

        // Build scrollback so a wheel can actually move the viewport.
        for i in 0..60 {
            term_lock(term).process(format!("line {i}\r\n").as_bytes());
        }
        drain_pipe(&target_rx); // engine query replies, if any, are not sink writes — clear anyway

        // ── Tracking ON (SGR mouse, DEC 1000 + 1006) ──
        term_lock(term).process(b"\x1b[?1000h\x1b[?1006h");
        assert!(term_lock(term).mouse_tracking_enabled(), "DEC 1000 enabled tracking");
        assert_eq!(cross_mouse_apply(term, ctx, "press left 1 1"), Ok(false), "report, no viewport move");
        let report = drain_pipe(&target_rx);
        assert!(!report.is_empty() && report.starts_with(b"\x1b["), "SGR press report to TARGET: {report:?}");
        assert!(drain_pipe(&self_rx).is_empty(), "self sink untouched by a cross mouse");

        // ── Tracking OFF (DEC 1000 / 1006 reset) ──
        term_lock(term).process(b"\x1b[?1006l\x1b[?1000l");
        assert!(!term_lock(term).mouse_tracking_enabled(), "tracking reset");
        term_lock(term).scroll_to_bottom();
        assert_eq!(term_lock(term).grid().display_offset(), 0, "at live tail");

        // Wheel-up: scroll_display fallback moves the TARGET viewport into history,
        // emits no pty bytes, and asks for a repaint.
        assert_eq!(cross_mouse_apply(term, ctx, "wheelup left 2 4 count=3"), Ok(true), "wheel => repaint");
        assert!(term_lock(term).grid().display_offset() > 0, "wheel moved TARGET viewport");
        assert!(drain_pipe(&target_rx).is_empty(), "wheel fallback emits no pty bytes");

        // A plain press under tracking-off: deliberate no-op (no selection UI for a
        // background tab) — sink empty, offset unchanged, no repaint.
        let off_before = term_lock(term).grid().display_offset();
        assert_eq!(cross_mouse_apply(term, ctx, "press left 1 1"), Ok(false), "press no-op");
        assert!(drain_pipe(&target_rx).is_empty(), "press fallback emits no pty bytes");
        assert_eq!(term_lock(term).grid().display_offset(), off_before, "press did not move viewport");
    }

    /// Cross-session `scroll` moves the TARGET term's viewport directly (no seam, no
    /// pty bytes) and reports `OK <offset> <max>` — the SAME wire shape as the self
    /// path. With history present, `scroll top` jumps to the oldest line.
    #[test]
    fn cross_session_scroll_moves_target_viewport() {
        let (h_target, rx) = pipe_session(2);
        // Generate scrollback: print more lines than the 24-row screen.
        for i in 0..60 {
            term_lock(&h_target.term).process(format!("line {i}\r\n").as_bytes());
        }
        let reply = cross_scroll(&h_target.term, "top");
        assert!(reply.starts_with("OK "), "scroll reply shape: {reply}");
        assert!(term_lock(&h_target.term).grid().display_offset() > 0, "viewport moved into history");
        assert!(drain_pipe(&rx).is_empty(), "scroll emits no pty bytes");
        // `scroll bottom` returns to the live tail (offset 0).
        let _ = cross_scroll(&h_target.term, "bottom");
        assert_eq!(term_lock(&h_target.term).grid().display_offset(), 0, "back to live bottom");
    }

    /// The exact bytes the `paste` verb puts on the wire: the seam applies
    /// `format_paste` to the verb's `paste_text(rest)` transform. (Phase 0.5: the
    /// verb itself now posts an `InputEvent::Paste` to the seam, which a headless
    /// unit test can't drive — but the OBSERVABLE bytes are exactly this, so we
    /// assert on the same `format_paste` output the seam produces.)
    fn paste_to_pipe(term: &Arc<Mutex<Terminal>>, rest: &str) -> Vec<u8> {
        term_lock(term).format_paste(&paste_text(rest))
    }

    /// A paste planted with ESC[201~ must not terminate the bracket guard:
    /// the engine sanitizer strips ESC, so the only ESC[201~ on the wire is
    /// the final guard and the planted "[201~" is inert text.
    #[test]
    fn paste_verb_cannot_escape_bracket_guard() {
        let term = Arc::new(Mutex::new(Terminal::new(24, 80)));
        term.lock().unwrap().process(b"\x1b[?2004h");
        let got = paste_to_pipe(&term, "safe\x1b[201~rm -rf ~");
        assert_eq!(got, b"\x1b[200~safe[201~rm -rf ~\x1b[201~");
    }

    /// The literal trailing `\n` still ends the paste with a line break,
    /// which the engine sends as CR exactly like a real paste.
    #[test]
    fn paste_verb_trailing_newline_becomes_cr() {
        let term = Arc::new(Mutex::new(Terminal::new(24, 80)));
        assert_eq!(paste_to_pipe(&term, "echo hi\\n"), b"echo hi\r");
    }

    /// Mint a fresh `SessionCtx` for the auth/gate tests (no real PTY needed; the
    /// sink wraps a harmless `-1` fd and is never written by these tests).
    fn test_ctx() -> Arc<crate::SessionCtx> {
        use aterm_session::sink::SinkWriter;
        use aterm_session::{EdgeTable, LaunchNonce, SessionId};
        Arc::new(crate::SessionCtx {
            sink: Arc::new(SinkWriter::new(-1)),
            edges: std::sync::Mutex::new(EdgeTable::new()),
            self_id: SessionId::generate(),
            nonce: LaunchNonce::generate(),
            cast: Arc::new(std::sync::Mutex::new(crate::cast::CastRecorder::new(80, 24))),
            temporal: Arc::new(std::sync::Mutex::new(crate::temporal::TemporalRecorder::new())),
            byte_fanout: Arc::new(crate::cast::ByteFanout::new()),
        })
    }

    /// The exact decision the op-scope gate at the top of `handle()` makes. Kept in
    /// lockstep with the inline `match (scope, required_op(verb))` so the gate is
    /// testable without an `EventLoopProxy` (which can't be built off the main
    /// thread): the deny path returns BEFORE any proxy/queue use, so the decision is
    /// all that matters. Mirrors the three exhaustive arms verbatim.
    fn gate_allows(scope: Scope, verb: &str, active: &SessionCtx) -> bool {
        // Mirrors the SELF-path op-scope gate in `handle()`: Owner passes everything
        // (no lookup); an Edge is permitted iff its token is authorized against the
        // NOW-ACTIVE session for the verb's op (`cross_session_authorized`) — NOT
        // op-match alone, so an edge cannot drive a session it was never granted on
        // after the active handle swings (tab/window switch).
        matches!(scope, Scope::Owner) || cross_session_authorized(scope, verb, active)
    }

    /// An `Edge` scope carrying a throwaway, UNGRANTED token. Since authority is
    /// re-derived from the token per request (`decide_edge`/`authorize`), an ungranted
    /// token is denied for EVERY op against any real session — so the deny-side / body-
    /// guard tests need no op (the scope no longer caches one).
    fn edge() -> Scope {
        Scope::Edge(EdgeToken::generate())
    }

    /// An `Edge` scope whose token is GRANTED on `ctx` (so the gate /
    /// `cross_session_authorized` permit it against THAT session for `op` verbs) —
    /// mirrors a real connection whose token was authorized against its session at
    /// connect time.
    fn edge_granted(op: Op, ctx: &SessionCtx) -> Scope {
        let tok = {
            let mut tbl = ctx.edges.lock().unwrap_or_else(|p| p.into_inner());
            tbl.grant(SessionId::new("s-test-controller"), ctx.self_id.clone(), op, ctx.nonce)
        };
        Scope::Edge(tok)
    }

    /// `required_op` is the single source of truth for which `Op` each verb needs;
    /// the design 7.2 read != write != signal split must hold exactly.
    #[test]
    fn required_op_classifies_each_verb() {
        // Read-side: observers + the controller's own view-state controls.
        let read_verbs = [
            "text", "cursor", "cell", "search", "dims", "lines", "line", "modes", "title", "cwd",
            "blocks", "blocktext", "wait", "colors", "selection", "copy", "scroll", "select",
            "image", "cast",
        ];
        for v in read_verbs {
            assert_eq!(required_op(v), Some(Op::ReadScreen), "{v} read");
        }
        // Write-side: bytes/geometry the driven program observes (`feed-bin` is the
        // binary twin of `feed`, classified here for the cross-process forward).
        for v in ["send", "key", "ctrl", "feed", "feed-bin", "mouse", "paste", "resize", "focus"] {
            assert_eq!(required_op(v), Some(Op::WriteInput), "{v} write");
        }
        // Signal is its own out-of-band class.
        assert_eq!(required_op("signal"), Some(Op::Signal));
        // Owner-only privilege verbs + any unknown verb default-deny (None).
        for v in ["grant", "revoke", "whoami", "bogus", ""] {
            assert_eq!(required_op(v), None, "{v} default-deny");
        }
    }

    /// The Owner-path regression invariant: an Owner passes EVERY verb (the gate's
    /// `(Owner, _)` arm short-circuits before any lookup), so the existing aterm-ctl
    /// client is byte-identical. A `ReadScreen` Edge is denied for write/signal/
    /// privilege verbs but allowed for read-side verbs.
    #[test]
    fn op_scope_gate_owner_full_power_edge_read_only() {
        // The active session the gate evaluates against; the edges below are GRANTED
        // on it (a real connection's token is authorized against its session).
        let ctx = test_ctx();
        let owner = Scope::Owner;
        let edge_read = edge_granted(Op::ReadScreen, &ctx);

        // Owner: every verb is permitted, including grant/revoke/whoami and image.
        let all_verbs = [
            "text", "image", "scroll", "select", "feed", "signal", "send", "resize", "grant",
            "revoke", "whoami",
        ];
        for v in all_verbs {
            assert!(gate_allows(owner, v, &ctx), "Owner must pass {v}");
        }

        // ReadScreen Edge (granted on ctx): read-side verbs pass; write/signal denied.
        assert!(gate_allows(edge_read, "text", &ctx), "read edge: text");
        assert!(gate_allows(edge_read, "image", &ctx), "read edge: image");
        assert!(gate_allows(edge_read, "select", &ctx), "read edge: select");
        assert!(!gate_allows(edge_read, "feed", &ctx), "read edge: NOT feed");
        assert!(!gate_allows(edge_read, "signal", &ctx), "read edge: NOT signal");
        assert!(!gate_allows(edge_read, "send", &ctx), "read edge: NOT send");

        // No Edge — regardless of op — may grant/revoke/whoami (Owner-only, None-op).
        for op in [Op::ReadScreen, Op::WriteInput, Op::Signal] {
            let e = edge_granted(op, &ctx);
            assert!(!gate_allows(e, "grant", &ctx), "no edge may grant");
            assert!(!gate_allows(e, "revoke", &ctx), "no edge may revoke");
            assert!(!gate_allows(e, "whoami", &ctx), "no edge may whoami");
            assert!(!gate_allows(e, "bogus", &ctx), "no edge: unknown verb");
        }

        // A WriteInput edge mirrors the split: it may write but not read or signal.
        let edge_write = edge_granted(Op::WriteInput, &ctx);
        assert!(gate_allows(edge_write, "feed", &ctx), "write edge: feed");
        assert!(!gate_allows(edge_write, "text", &ctx), "write edge: NOT read");
        assert!(!gate_allows(edge_write, "signal", &ctx), "write edge: NOT signal");
    }

    /// grant/revoke enforce Owner-only INSIDE the body too (defense in depth beyond
    /// the gate): an Edge scope is rejected even if the body is reached directly.
    /// `whoami` has no body guard (the gate already keeps it Owner-only); its body
    /// reports the edge's EFFECTIVE op re-derived from the presented token against the
    /// CURRENT session — an ungranted token therefore reads `edge unauthorized`.
    #[test]
    fn privilege_verbs_reject_edge_scope_in_body() {
        let ctx = test_ctx();
        let edge = edge();
        assert_eq!(
            cmd_grant(&ctx, edge, "s-deadbeef read-screen"),
            "ERR denied\n"
        );
        assert_eq!(cmd_revoke(&ctx, edge, &"0".repeat(64)), "ERR denied\n");
        // whoami re-derives the op from the token: an UNGRANTED token holds no
        // authority against this session, so it reports `edge unauthorized` (never an
        // over-stated op). The gate still keeps grant/revoke Owner-only.
        let who = cmd_whoami(&ctx, edge);
        assert!(
            who.trim_end().ends_with("edge unauthorized"),
            "ungranted edge: {who}"
        );
        // A GRANTED ReadScreen token reports its real effective op.
        let granted = edge_granted(Op::ReadScreen, &ctx);
        let who_granted = cmd_whoami(&ctx, granted);
        assert!(
            who_granted.trim_end().ends_with("edge read-screen"),
            "granted edge: {who_granted}"
        );
    }

    /// REGRESSION (introspection integrity): `whoami` must report the EFFECTIVE op
    /// against the session active RIGHT NOW, re-derived from the presented token — NOT
    /// a cached connect-time op. A token granted on session B, presented on a
    /// connection whose `@.` has swung to session A, must read `edge unauthorized` (it
    /// holds no authority over A), never over-state "edge read-screen". Mirrors the
    /// gate's per-request `authorize`, so whoami can never claim power the gate denies.
    #[test]
    fn whoami_reports_unauthorized_after_active_session_swings() {
        let ctx_b = test_ctx(); // session active when the edge connected
        let ctx_a = test_ctx(); // a DIFFERENT session `@.` later swings to
        let edge_b = edge_granted(Op::ReadScreen, &ctx_b);

        // Against its OWN granted session B, whoami reports the real op.
        assert!(
            cmd_whoami(&ctx_b, edge_b).trim_end().ends_with("edge read-screen"),
            "whoami on granted session B",
        );
        // After the active handle swings to A, the SAME token authorizes nothing.
        assert!(
            cmd_whoami(&ctx_a, edge_b).trim_end().ends_with("edge unauthorized"),
            "whoami must not over-state authority on swung-to session A",
        );
    }

    /// An Owner can mint an edge with `grant`, that edge then authenticates as an
    /// `Edge(op)` via `edge_scope_from_first_line`, and `revoke` invalidates it —
    /// the full mint -> authorize -> revoke fabric round-trip through the verbs.
    #[test]
    fn grant_then_edge_handshake_then_revoke_roundtrip() {
        let ctx = test_ctx();
        let owner = Scope::Owner;

        // Owner mints a ReadScreen edge from some source session into THIS session.
        let reply = cmd_grant(&ctx, owner, "s-source01 read-screen");
        let hex = reply.strip_prefix("OK ").and_then(|s| s.strip_suffix('\n')).expect("OK <hex>");
        assert_eq!(hex.len(), 64, "edge token is 64 hex chars");

        // The bearer presents it as the handshake hex => resolves to Edge(ReadScreen).
        let line = format!("AUTH {hex}");
        let (op, _tok, inline) = edge_scope_from_first_line(&line, &ctx).expect("edge authenticates");
        assert_eq!(op, Op::ReadScreen);
        assert_eq!(inline, None);

        // A folded TOKEN form preserves the inline verb.
        let line2 = format!("TOKEN {hex} text");
        let (op2, _tok2, inline2) =
            edge_scope_from_first_line(&line2, &ctx).expect("edge authenticates");
        assert_eq!(op2, Op::ReadScreen);
        assert_eq!(inline2.as_deref(), Some("text"));

        // Owner revokes it; the same hex no longer authenticates (fail closed).
        assert_eq!(cmd_revoke(&ctx, owner, hex), "OK\n");
        assert!(
            edge_scope_from_first_line(&line, &ctx).is_none(),
            "revoked => fail closed"
        );
        // A second revoke reports no-such-edge.
        assert_eq!(cmd_revoke(&ctx, owner, hex), "ERR no such edge\n");

        // whoami as Owner reports this session's identity + the owner scope.
        let who = cmd_whoami(&ctx, owner);
        assert!(who.starts_with("OK s-"), "whoami: {who}");
        assert!(who.trim_end().ends_with("owner"), "whoami scope: {who}");
    }

    /// `resize 65535 65535` asks for a ~4.3-billion-cell allocation; the parse
    /// must reject anything outside 1..=MAX_GRID_ROWS/COLS. (RES-1: the verb now
    /// forwards a `Wake::Resize` to the geometry-owning main thread; the pure
    /// `parse_resize` is the validator the verb gates on.)
    #[test]
    fn resize_rejects_out_of_range() {
        for req in ["65535 65535", "4097 80", "24 4097", "0 80", "24 0"] {
            assert_eq!(parse_resize(req), Err("ERR out of range\n".to_string()));
        }
    }

    /// `cwd` surfaces the OSC 7-reported working directory (empty until set).
    #[test]
    fn cwd_verb_reports_working_directory() {
        let term = Arc::new(Mutex::new(Terminal::new(24, 80)));
        assert_eq!(cmd_cwd(&term), "OK \n");
        // OSC 7: a program reports its cwd as a file:// URI.
        term.lock()
            .unwrap()
            .process(b"\x1b]7;file://localhost/Users/ayates/x\x07");
        let out = cmd_cwd(&term);
        assert!(out.contains("/Users/ayates/x"), "cwd not surfaced: {out}");
    }

    /// `cast` serializes the session's asciicast recorder behind the read-verb
    /// framing `OK <nbytes>\n<body>`, where `<nbytes>` is the exact body length
    /// and the body is valid asciicast v2 (header line + recorded events).
    #[test]
    fn cast_verb_frames_asciicast_body() {
        let ctx = test_ctx();
        // Empty recording: header-only body, framed with its true byte length.
        let reply = cmd_cast(&ctx);
        let (hdr_line, body) = reply.split_once('\n').expect("OK <n>\\n<body>");
        let n: usize = hdr_line.strip_prefix("OK ").expect("OK prefix").parse().expect("nbytes");
        assert_eq!(n, body.len(), "framed length must equal the body length");
        let header = body.lines().next().expect("a header line");
        assert!(header.contains("\"version\": 2"), "not asciicast v2: {header}");

        // Fold in an output burst, then the body grows by one well-formed event.
        {
            let mut rec = ctx.cast.lock().unwrap();
            let t = rec.now();
            rec.record_output(t, b"hi there\r\n");
        }
        let reply2 = cmd_cast(&ctx);
        let (hdr2, body2) = reply2.split_once('\n').unwrap();
        let n2: usize = hdr2.strip_prefix("OK ").unwrap().parse().unwrap();
        assert_eq!(n2, body2.len());
        assert!(body2.lines().count() >= 2, "expected header + >=1 event: {body2}");
        let event = body2.lines().nth(1).unwrap();
        assert!(event.starts_with('[') && event.contains("\"o\""), "bad event: {event}");
    }

    /// `blocks` surfaces the OSC 133/633 shell-integration command blocks so an
    /// AI can navigate by command: exit codes, output row range, command text.
    #[test]
    fn blocks_verb_surfaces_command_blocks() {
        let term = Arc::new(Mutex::new(Terminal::new(24, 80)));
        // No shell integration yet -> no blocks.
        assert_eq!(cmd_blocks(&term, ""), "OK 0\n");
        // Two command blocks via OSC 133 (+ OSC 633;E commandline): exit 0, exit 1.
        // Each OSC mark is BEL-terminated so the surrounding text isn't swallowed.
        term.lock().unwrap().process(
            b"\x1b]133;A\x07$ \x1b]633;E;echo hi\x07\x1b]133;B\x07echo hi\n\x1b]133;C\x07hi\n\x1b]133;D;0\x07\
\x1b]133;A\x07$ \x1b]633;E;false\x07\x1b]133;B\x07false\n\x1b]133;C\x07\x1b]133;D;1\x07",
        );
        let out = cmd_blocks(&term, "");
        assert!(out.starts_with("OK 2\n"), "expected 2 blocks: {out}");
        assert!(out.contains("exit=0") && out.contains("cmdline=echo%20hi"), "block 1 wrong: {out}");
        assert!(out.contains("exit=1") && out.contains("cmdline=false"), "block 2 wrong: {out}");
        // `blocks 1` returns only the most recent (the failed one).
        let last = cmd_blocks(&term, "1");
        assert!(last.starts_with("OK 1\n") && last.contains("exit=1"), "last block wrong: {last}");
        // `blocktext 0` reads block 0's OUTPUT directly (no coordinate math).
        let txt = cmd_blocktext(&term, "0");
        assert!(txt.starts_with("OK ") && txt.contains("hi"), "block 0 output wrong: {txt}");
        assert_eq!(cmd_blocktext(&term, "99"), "ERR no such block\n");
    }

    /// `wait` blocks until the in-flight command completes, then reports it; with
    /// no new completion it times out.
    #[test]
    fn wait_verb_blocks_until_command_completes() {
        let term = Arc::new(Mutex::new(Terminal::new(24, 80)));
        // No command in flight -> a short wait times out.
        assert_eq!(cmd_wait(&term, "0"), "OK timeout\n");
        // Start a command (executing), then complete it from another thread.
        term.lock()
            .unwrap()
            .process(b"\x1b]133;A\x07$ \x1b]133;B\x07sleep\n\x1b]133;C\x07");
        let bg = term.clone();
        let h = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(40));
            bg.lock().unwrap().process(b"\x1b]133;D;0\x07");
        });
        let resp = cmd_wait(&term, "5000");
        h.join().unwrap();
        assert!(
            resp.starts_with("OK complete ") && resp.contains("exit=0"),
            "wait should report the completed command: {resp}"
        );
    }

    /// `cell` appends `link=<url>` for an OSC 8 hyperlinked cell, and nothing
    /// for a plain cell (positional fields unchanged for non-link cells).
    #[test]
    fn cell_verb_surfaces_osc8_hyperlink() {
        let term = Arc::new(Mutex::new(Terminal::new(24, 80)));
        // OSC 8 open (target https://example.com), one glyph 'X', OSC 8 close.
        term.lock()
            .unwrap()
            .process(b"\x1b]8;;https://example.com\x1b\\X\x1b]8;;\x1b\\");
        let linked = cmd_cell(&term, "0 0");
        assert!(
            linked.contains("link=https://example.com"),
            "linked cell missing hyperlink: {linked}"
        );
        let plain = cmd_cell(&term, "0 5");
        assert!(!plain.contains("link="), "plain cell has a stray link: {plain}");
    }

    /// `colors` reports the theme and reflects OSC 10/11/12 dynamic changes.
    #[test]
    fn colors_verb_reports_theme() {
        let term = Arc::new(Mutex::new(Terminal::new(24, 80)));
        let out = cmd_colors(&term);
        assert!(
            out.starts_with("OK fg=") && out.contains(" bg=") && out.contains(" cursor="),
            "unexpected colors format: {out}"
        );
        // OSC 11 sets the background; the verb must reflect it.
        term.lock().unwrap().process(b"\x1b]11;#102030\x07");
        assert!(
            cmd_colors(&term).contains("bg=102030"),
            "bg not updated: {}",
            cmd_colors(&term)
        );
    }

    /// `modes` exposes IRM / DECAWM / DECOM, which a driving client needs to
    /// predict how typed input and printed output land.
    #[test]
    fn modes_verb_exposes_insert_wrap_origin() {
        let term = Arc::new(Mutex::new(Terminal::new(24, 80)));
        let out = cmd_modes(&term);
        assert!(out.contains("insert_mode=false"), "{out}");
        assert!(out.contains("auto_wrap=true"), "{out}");
        assert!(out.contains("origin_mode=false"), "{out}");
        // IRM on (ESC[4h), auto-wrap off (ESC[?7l), origin on (ESC[?6h).
        term.lock().unwrap().process(b"\x1b[4h\x1b[?7l\x1b[?6h");
        let out2 = cmd_modes(&term);
        assert!(
            out2.contains("insert_mode=true")
                && out2.contains("auto_wrap=false")
                && out2.contains("origin_mode=true"),
            "{out2}"
        );
    }

    /// An in-range resize parses to the requested `(rows, cols)` (RES-1: the
    /// engine/PTY/window resize then happens on the main thread via
    /// `Wake::Resize`, which a headless unit test cannot drive — so we verify the
    /// validated geometry the verb forwards).
    #[test]
    fn resize_parses_in_range() {
        assert_eq!(parse_resize("30 100"), Ok((30, 100)));
        assert_eq!(parse_resize(""), Err("ERR usage: resize <r> <c>\n".to_string()));
        assert_eq!(parse_resize("x y"), Err("ERR bad args\n".to_string()));
    }

    /// `tab` parses each form to its `TabAction`; the actual App mutation happens on
    /// the main thread via `Wake::TabCmd` (a headless unit test cannot drive it), so
    /// we verify the action the verb forwards. An unknown / missing arg is `None` (the
    /// verb then replies with the usage error).
    #[test]
    fn tab_parses_each_form() {
        assert_eq!(parse_tab("new"), Some(TabAction::New));
        assert_eq!(parse_tab("next"), Some(TabAction::Next));
        assert_eq!(parse_tab("prev"), Some(TabAction::Prev));
        assert_eq!(parse_tab("0"), Some(TabAction::Select(0)));
        assert_eq!(parse_tab("3"), Some(TabAction::Select(3)));
        // Surrounding whitespace is tolerated (the rest-of-line may carry it).
        assert_eq!(parse_tab("  2 "), Some(TabAction::Select(2)));
        // `close` (active tab) and `close <N>` (a specific tab).
        assert_eq!(parse_tab("close"), Some(TabAction::Close(None)));
        assert_eq!(parse_tab("close 1"), Some(TabAction::Close(Some(1))));
        assert_eq!(parse_tab("  close 0 "), Some(TabAction::Close(Some(0))));
        // `move <from> <to>` reorders.
        assert_eq!(parse_tab("move 2 0"), Some(TabAction::Move { from: 2, to: 0 }));
        assert_eq!(parse_tab("move 0 3"), Some(TabAction::Move { from: 0, to: 3 }));
        // Unknown / empty / negative => None (usage error).
        assert_eq!(parse_tab(""), None);
        assert_eq!(parse_tab("bogus"), None);
        assert_eq!(parse_tab("-1"), None);
        // Malformed close/move => None.
        assert_eq!(parse_tab("close x"), None);
        assert_eq!(parse_tab("close 1 2"), None);
        assert_eq!(parse_tab("move 1"), None);
        assert_eq!(parse_tab("move 1 x"), None);
        assert_eq!(parse_tab("move 1 2 3"), None);
        // A trailing word after a keyword is rejected (not silently swallowed).
        assert_eq!(parse_tab("new x"), None);
        assert_eq!(parse_tab("next y"), None);
    }

    /// `tab` is classed as a WRITE verb (it DRIVES the GUI), so a `ReadScreen` edge
    /// cannot run it and a `WriteInput` edge can — same as `send`/`key`/`resize`.
    #[test]
    fn tab_is_write_classified() {
        assert_eq!(required_op("tab"), Some(Op::WriteInput));
    }

    /// The combining-aware grapheme content of a single cell, taken via the
    /// SELECTION path (`select` that one cell + `selection_to_string`) — the
    /// fidelity ground truth the pixels also render. Used by the I-1 test to
    /// prove `text`/`cell`/`search` agree with selection.
    fn selection_of_cell(term: &Arc<Mutex<Terminal>>, row: i32, col: u16) -> String {
        let mut t = term_lock(term);
        let sel = t.text_selection_mut();
        sel.start_selection(row, col, SelectionSide::Left, SelectionType::Simple);
        sel.update_selection(row, col, SelectionSide::Right);
        sel.complete_selection();
        t.selection_to_string().unwrap_or_default()
    }

    /// I-1 FIDELITY: `text`/`cell`/`search` must return the SAME grapheme content
    /// (base char + combining marks + complex cluster) the SELECTION path returns
    /// — the renderer consumes that same content via combining_row/cluster_row, so
    /// this also proves text/cell/search agree with the rendered pixels. The old
    /// code read only the resolved base `RenderCell.ch`, silently dropping an NFD
    /// accent and a ZWJ emoji family; this test would fail against that code.
    #[test]
    fn read_paths_preserve_combining_and_zwj_clusters() {
        let term = Arc::new(Mutex::new(Terminal::new(24, 80)));
        // Row 0: an NFD accent "é" = 'e' + U+0301 in one cell.
        // Row 1: a ZWJ family "👨‍👩‍👧" = man + ZWJ + woman + ZWJ + girl in one
        // (wide) cell. Both are folded into a single grid cell with the trailing
        // codepoints stored as combining marks (the same path the renderer reads).
        term.lock()
            .unwrap()
            .process("e\u{0301}\r\n\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}".as_bytes());

        // Ground truth from the selection path.
        let accent_sel = selection_of_cell(&term, 0, 0);
        let family_sel = selection_of_cell(&term, 1, 0);
        assert_eq!(accent_sel, "e\u{0301}", "selection ground truth (accent)");
        assert_eq!(
            family_sel, "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}",
            "selection ground truth (ZWJ family)"
        );

        // ---- cell verb: grapheme token (pct-encoded) decodes to the selection.
        let accent_cell = cmd_cell(&term, "0 0");
        let accent_tok = accent_cell
            .strip_prefix("OK ")
            .and_then(|s| s.split(' ').next())
            .expect("cell OK token");
        assert_eq!(
            pct_decode(accent_tok), accent_sel,
            "cell grapheme must equal selection (accent): {accent_cell}"
        );
        let family_cell = cmd_cell(&term, "1 0");
        let family_tok = family_cell
            .strip_prefix("OK ")
            .and_then(|s| s.split(' ').next())
            .expect("cell OK token");
        assert_eq!(
            pct_decode(family_tok), family_sel,
            "cell grapheme must equal selection (ZWJ family): {family_cell}"
        );
        // The old `ch as u32` field would have been a bare codepoint, NOT the
        // multi-codepoint cluster — assert the cluster is really present.
        assert!(
            pct_decode(family_tok).chars().count() >= 5,
            "ZWJ family must keep all 5 codepoints, got {family_tok}"
        );

        // ---- text verb: the row line equals the full-row selection.
        let text = cmd_text(&term);
        let lines: Vec<&str> = text.lines().collect();
        // lines[0] is the "OK <n>" header; row r is lines[r+1].
        assert_eq!(lines[1], accent_sel, "text row 0 must equal selection: {text}");
        assert_eq!(lines[2], family_sel, "text row 1 must equal selection: {text}");

        // ---- search verb: searching the cluster finds it, and the located cell
        // reads back (via cell) the same grapheme the selection shows.
        let s = cmd_search(&term, "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}");
        assert!(s.starts_with("OK 1"), "search must find the ZWJ family once: {s}");
        let hit = s.lines().nth(1).expect("a match row");
        let mut parts = hit.split(' ');
        let abs_row: u64 = parts.next().unwrap().parse().expect("abs row");
        // The absolute row resolves (via abs_row_text, the `line`/`text` space)
        // to a row whose text contains the faithful cluster.
        let row_text = match abs_row_text(&term_lock(&term), abs_row) {
            AbsRow::Text(t) => t,
            AbsRow::Evicted => panic!("search row {abs_row} unexpectedly evicted"),
            AbsRow::OutOfRange => panic!("search row {abs_row} out of range"),
        };
        assert!(
            row_text.contains(&family_sel),
            "search's absolute row must resolve to the cluster: {row_text:?}"
        );
    }

    /// Decode the percent-encoding `pct_encode` produces (test helper).
    fn pct_decode(s: &str) -> String {
        let bytes = s.as_bytes();
        let mut out = Vec::with_capacity(bytes.len());
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'%' && i + 2 < bytes.len() {
                let hi = (bytes[i + 1] as char).to_digit(16);
                let lo = (bytes[i + 2] as char).to_digit(16);
                if let (Some(hi), Some(lo)) = (hi, lo) {
                    out.push((hi * 16 + lo) as u8);
                    i += 3;
                    continue;
                }
            }
            out.push(bytes[i]);
            i += 1;
        }
        String::from_utf8(out).expect("valid utf8")
    }

    /// SEARCH-1: a term scrolled OFF the visible screen into scrollback is still
    /// found, the match's ABSOLUTE row resolves (via `line`) to the same content,
    /// and a regex query returns the expected matches. Proves the real
    /// `TerminalSearch` (scrollback + visible, regex) replaced the naive
    /// visible-only substring scan.
    #[test]
    fn search_finds_scrollback_and_regex() {
        // Small grid so content scrolls into history quickly.
        let term = Arc::new(Mutex::new(Terminal::new(4, 40)));
        // Print a unique needle, then enough lines to push it off-screen.
        term.lock().unwrap().process(b"NEEDLE_alpha\r\n");
        for i in 0..20 {
            term.lock().unwrap().process(format!("filler line {i}\r\n").as_bytes());
        }
        // The needle is no longer on the visible 4-row screen.
        let visible = cmd_text(&term);
        assert!(
            !visible.contains("NEEDLE_alpha"),
            "needle should have scrolled off-screen: {visible}"
        );
        // But search (which indexes scrollback) finds it.
        let s = cmd_search(&term, "NEEDLE_alpha");
        assert!(s.starts_with("OK 1"), "scrolled-off needle must be found: {s}");
        let hit = s.lines().nth(1).expect("match row");
        let abs_row: u64 = hit.split(' ').next().unwrap().parse().expect("abs row");
        // The `line` verb resolves that absolute row to the needle's content.
        let line_out = cmd_line(&term, &abs_row.to_string());
        assert!(
            line_out.contains("NEEDLE_alpha"),
            "line {abs_row} must resolve to the needle (got {line_out})"
        );

        // Regex: a single-token pattern + the `regex` flag. `fill[a-z]+` matches
        // every "filler" row (the pattern carries no spaces, so it stays one
        // token; the trailing `regex` is parsed as a flag).
        let rx = cmd_search(&term, "fill[a-z]+ regex");
        assert!(
            rx.starts_with("OK "),
            "regex search should succeed (regex feature enabled): {rx}"
        );
        let count: usize = rx
            .lines()
            .next()
            .and_then(|h| h.strip_prefix("OK "))
            .and_then(|n| n.split(' ').next())
            .and_then(|n| n.parse().ok())
            .expect("count");
        assert!(count >= 2, "regex `fill[a-z]+` should match many filler rows: {rx}");

        // Case sensitivity: default is insensitive, `case` flips it.
        let ci = cmd_search(&term, "needle_alpha");
        assert!(ci.starts_with("OK 1"), "case-insensitive default must match: {ci}");
        let cs = cmd_search(&term, "needle_alpha case");
        assert!(cs.starts_with("OK 0"), "case-sensitive must NOT match lowercased: {cs}");
    }

    // ---- P1.2: cross-session @selector addressing --------------------------------

    use crate::session_store::{self, SessionHandle, SessionState};
    use aterm_session::{decide_edge, EdgeTable, LaunchNonce};

    /// Build a registered session: a fresh `Terminal` (optionally pre-fed `seed`
    /// bytes so its `text` read is distinctive), a fresh fabric identity, and a sink
    /// over `master` (a pipe write-end in the write tests, else `-1`). Returns the
    /// handle (carrying the shared `Arc`s) so a test can register it AND assert on
    /// the same live engine.
    fn registered_session(local_id: u64, master: i32, seed: &[u8]) -> SessionHandle {
        let sid = SessionId::generate();
        let nonce = LaunchNonce::generate();
        let term = Arc::new(Mutex::new(Terminal::new(24, 80)));
        if !seed.is_empty() {
            term.lock().unwrap().process(seed);
        }
        let ctx = Arc::new(crate::SessionCtx {
            sink: Arc::new(SinkWriter::new(master)),
            edges: std::sync::Mutex::new(EdgeTable::new()),
            self_id: sid.clone(),
            nonce,
            cast: Arc::new(std::sync::Mutex::new(crate::cast::CastRecorder::new(80, 24))),
            temporal: Arc::new(std::sync::Mutex::new(crate::temporal::TemporalRecorder::new())),
            byte_fanout: Arc::new(crate::cast::ByteFanout::new()),
        });
        SessionHandle {
            sid,
            nonce,
            local_id,
            parent: None,
            state: SessionState::Alive,
            title: format!("tab-{local_id}"),
            term,
            master,
            ctx,
        }
    }

    /// (a) `@.` (explicit self) resolves to the EXACT self tuple, so a read of `@.`
    /// is byte-identical to the verbatim self read (zero-change guarantee).
    #[test]
    fn at_dot_selector_resolves_self_verbatim() {
        let store = session_store::new_store();
        let self_h = registered_session(0, -1, b"hello-self\r\n");
        store.write().unwrap().register(self_h.clone());

        let self_tuple: Target = (
            self_h.term.clone(),
            self_h.master,
            self_h.local_id,
            self_h.ctx.clone(),
        );

        // `@.` and `@` (empty body) both name self.
        for body in ["", "."] {
            let sel = Selector::parse(body);
            let (t, m, id, _ctx) = resolve_target(&self_tuple, &store, &sel).expect("self resolves");
            assert!(Arc::ptr_eq(&t, &self_h.term), "@{body} is the same Arc as self");
            assert_eq!(m, self_h.master);
            assert_eq!(id, self_h.local_id);
        }

        // The read through `@.` equals the verbatim self read — byte-for-byte.
        let sel = Selector::parse(".");
        let (t, _, _, _) = resolve_target(&self_tuple, &store, &sel).unwrap();
        assert_eq!(cmd_text(&t), cmd_text(&self_h.term), "@. text == self text");
    }

    /// (b) A SECOND registered session is readable via `@<local>` and `@<sid>` by an
    /// Owner connection, and returns ITS state (its text), not self's.
    #[test]
    fn owner_reads_a_sibling_via_local_and_sid_selectors() {
        let store = session_store::new_store();
        let self_h = registered_session(0, -1, b"I-am-self\r\n");
        let peer_h = registered_session(7, -1, b"I-am-peer\r\n");
        store.write().unwrap().register(self_h.clone());
        store.write().unwrap().register(peer_h.clone());

        let self_tuple: Target = (
            self_h.term.clone(),
            self_h.master,
            self_h.local_id,
            self_h.ctx.clone(),
        );

        let self_text = cmd_text(&self_h.term);
        let peer_text = cmd_text(&peer_h.term);
        assert_ne!(self_text, peer_text, "the two sessions read distinctly");

        // By process-local id.
        let by_local = resolve_target(&self_tuple, &store, &Selector::parse("7")).expect("by local");
        assert!(Arc::ptr_eq(&by_local.0, &peer_h.term), "resolved the peer term");
        assert_eq!(cmd_text(&by_local.0), peer_text, "@7 returns the PEER's state");
        assert_ne!(cmd_text(&by_local.0), self_text, "@7 is NOT self's state");

        // By stable SessionId.
        let by_sid =
            resolve_target(&self_tuple, &store, &Selector::parse(peer_h.sid.as_str())).expect("by sid");
        assert!(Arc::ptr_eq(&by_sid.0, &peer_h.term), "@s-... resolved the peer term");
        assert_eq!(cmd_text(&by_sid.0), peer_text, "@s-... returns the PEER's state");

        // Owner is authorized to read the sibling (same trust domain).
        assert!(
            cross_session_authorized(Scope::Owner, "text", &peer_h.ctx),
            "Owner may read a sibling",
        );

        // An unknown selector fails closed (no such session).
        assert!(resolve_target(&self_tuple, &store, &Selector::parse("999")).is_none());
        assert!(resolve_target(&self_tuple, &store, &Selector::parse("s-nope")).is_none());
    }

    /// (c) An Edge connection WITHOUT an authorizing edge into the target is DENIED
    /// `@other` (fail-closed); WITH a granted edge for the right op it is ALLOWED,
    /// and the op-split holds (a read edge cannot write the target).
    #[test]
    fn edge_cross_session_is_fail_closed_without_edge_and_allowed_with_one() {
        let peer_h = registered_session(7, -1, b"peer\r\n");

        // A connection presenting some token NOT recorded in the peer's table.
        let stray = EdgeToken::generate();
        let read_scope = Scope::Edge(stray);
        assert!(
            !cross_session_authorized(read_scope, "text", &peer_h.ctx),
            "no edge in the target table => DENY (fail-closed)",
        );

        // The peer (as Owner of its own table) grants a ReadScreen edge from the
        // controller's source id into itself, returning the bearer token.
        let src = SessionId::new("s-controller");
        let granted = {
            let mut tbl = peer_h.ctx.edges.lock().unwrap();
            tbl.grant(src.clone(), peer_h.ctx.self_id.clone(), Op::ReadScreen, peer_h.ctx.nonce)
        };

        // The bearer presenting THAT token is now authorized to READ the peer...
        let auth_read = Scope::Edge(granted);
        assert!(
            cross_session_authorized(auth_read, "text", &peer_h.ctx),
            "a granted ReadScreen edge authorizes a cross-session read",
        );
        // ...but the op-split denies a WRITE through a read edge.
        assert!(
            !cross_session_authorized(auth_read, "send", &peer_h.ctx),
            "a ReadScreen edge may NOT write the target (read != write)",
        );

        // A restarted target (nonce mismatch) fails the SAME edge closed (the
        // confused-deputy guard). Simulate by checking decide_edge against a fresh
        // nonce, which is what a relaunched session would publish.
        let restarted_nonce = LaunchNonce::generate();
        let tbl = peer_h.ctx.edges.lock().unwrap();
        assert_eq!(
            decide_edge(&tbl, &granted, &peer_h.ctx.self_id, Op::ReadScreen, &restarted_nonce),
            aterm_session::EdgeDecision::Deny,
            "an edge bound to the old nonce fails closed across a restart",
        );
    }

    /// (d) A WRITE verb (`send @<local>`) reaches the TARGET's master only when
    /// authorized: an authorized write lands the bytes on the peer's pipe; the
    /// op-gate denies an unauthorized (read-edge) write before any byte is sent.
    #[test]
    fn cross_session_send_reaches_target_master_only_when_authorized() {
        // The peer's "master" is a pipe; we read back what `send` writes.
        let mut fds = [0i32; 2];
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        let (read_fd, write_fd) = (fds[0], fds[1]);

        let store = session_store::new_store();
        let self_h = registered_session(0, -1, b"");
        let peer_h = registered_session(7, write_fd, b"");
        store.write().unwrap().register(self_h.clone());
        store.write().unwrap().register(peer_h.clone());

        let self_tuple: Target = (
            self_h.term.clone(),
            self_h.master,
            self_h.local_id,
            self_h.ctx.clone(),
        );

        // An Owner (cross-session authorized) resolves the peer and writes to it.
        let target = resolve_target(&self_tuple, &store, &Selector::parse("7")).expect("peer");
        assert!(cross_session_authorized(Scope::Owner, "send", &peer_h.ctx), "owner write ok");
        assert_eq!(cmd_send(&target.3.sink, "echo-into-peer"), "OK\n");

        // A read-only Edge is denied the SAME write BEFORE any byte is sent (op-gate).
        let read_scope = Scope::Edge(EdgeToken::generate());
        assert!(
            !cross_session_authorized(read_scope, "send", &peer_h.ctx),
            "a read edge may not write the peer",
        );

        // Read back: only the authorized write's bytes reached the peer's master.
        unsafe { libc::close(write_fd) };
        let mut buf = Vec::new();
        let mut reader = unsafe { std::fs::File::from_raw_fd(read_fd) };
        reader.read_to_end(&mut buf).expect("read peer pipe");
        assert_eq!(buf, b"echo-into-peer", "exactly the authorized write reached the PEER");
    }

    /// The `sessions` verb lists the registry: a single-session store yields exactly
    /// one line == the lone session (the zero-regression base case); a family yields
    /// one line per registered session with parent + state.
    #[test]
    fn sessions_verb_lists_the_registry() {
        let store = session_store::new_store();
        let root = registered_session(0, -1, b"");
        let root_ctx = root.ctx.clone();
        store.write().unwrap().register(root.clone());

        // Base case: one session, one data line.
        let one = cmd_sessions(&root_ctx, &store);
        let mut lines = one.lines();
        assert_eq!(lines.next(), Some("OK 1"), "header counts one session");
        let only = lines.next().expect("one data line");
        assert!(only.starts_with("0 "), "local id 0 first: {only}");
        assert!(only.contains(root.sid.as_str()), "carries the sid: {only}");
        assert!(only.contains(" - alive "), "no parent, alive: {only}");
        assert_eq!(lines.next(), None, "exactly one data line");

        // Family case: a child links to the root and the listing is sorted by local.
        let mut child = registered_session(1, -1, b"");
        child.parent = Some(root.sid.clone());
        store.write().unwrap().register(child.clone());
        let two = cmd_sessions(&root_ctx, &store);
        let mut l = two.lines();
        assert_eq!(l.next(), Some("OK 2"));
        assert!(l.next().unwrap().starts_with("0 "), "root first");
        let child_line = l.next().unwrap();
        assert!(child_line.starts_with("1 "), "child second");
        assert!(child_line.contains(root.sid.as_str()), "child names its parent sid");
    }

    /// A malformed / cross-session selector on a SELF-SCOPED verb (sessions/grant/
    /// revoke/whoami) is rejected — those verbs can never be redirected to act on
    /// another session's table. (The selector PARSE itself is total + fail-closed:
    /// an unknown id resolves to None, not a wrong session.)
    #[test]
    fn self_scoped_verbs_reject_a_target_selector() {
        // A non-self selector is `Local`/`Sid`, which the handle() guard rejects for
        // these verbs. Here we assert the parse classification the guard relies on.
        assert!(matches!(Selector::parse("."), Selector::SelfTok));
        assert!(matches!(Selector::parse(""), Selector::SelfTok));
        assert!(matches!(Selector::parse("7"), Selector::Local(7)));
        assert!(matches!(Selector::parse("s-abc"), Selector::Sid(_)));
    }

    // ── P1.3 subscribe wiring ────────────────────────────────────────────────

    /// Build an [`ActiveHandle`] over a registered session's tuple, so `@.` self
    /// subscribe follows the active tab the same way a self read does.
    fn active_for(h: &SessionHandle) -> ActiveHandle {
        Arc::new(Mutex::new(ActiveSession {
            term: h.term.clone(),
            master: h.master,
            id: h.local_id,
            ctx: h.ctx.clone(),
        }))
    }

    /// Accumulate pushed bytes from a `UnixStream` until `pred` is satisfied by the
    /// accumulated text or a generous deadline passes (so a correctly-silent
    /// producer doesn't hang the test). Frames may arrive split or coalesced across
    /// `read`s — accumulating and matching on substrings makes the assertion robust
    /// to that and to parallel-test scheduling jitter.
    fn read_until(s: &UnixStream, mut acc: String, pred: impl Fn(&str) -> bool) -> String {
        use std::io::Read;
        let mut s = s.try_clone().expect("clone client end");
        s.set_read_timeout(Some(std::time::Duration::from_millis(200))).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        let mut buf = [0u8; 8192];
        while !pred(&acc) && std::time::Instant::now() < deadline {
            match s.read(&mut buf) {
                Ok(n) if n > 0 => acc.push_str(&String::from_utf8_lossy(&buf[..n])),
                Ok(_) => break, // EOF
                Err(_) => {}    // timeout: loop until the deadline
            }
        }
        acc
    }

    /// Read whatever bytes arrive within a short window (for the NEGATIVE assertion
    /// that nothing was pushed). Returns the accumulated text (possibly empty).
    fn read_quiet(s: &UnixStream) -> String {
        use std::io::Read;
        let mut s = s.try_clone().expect("clone client end");
        s.set_read_timeout(Some(std::time::Duration::from_millis(300))).unwrap();
        let mut buf = [0u8; 8192];
        match s.read(&mut buf) {
            Ok(n) if n > 0 => String::from_utf8_lossy(&buf[..n]).into_owned(),
            _ => String::new(),
        }
    }

    /// (a) SELF subscribe to `screen`: a write to the term pushes a sid-tagged DELTA
    /// carrying the live text; a subsequent PURE viewport scroll (which never bumps
    /// `content_seq`) pushes NOTHING. End-to-end through `run_subscribe` (auth +
    /// flip) and the push loop over a real socket, with the production notify hook
    /// (the registry `notify`) driving the wake.
    #[test]
    fn subscribe_self_screen_delta_on_write_none_on_scroll() {
        let store = session_store::new_store();
        let h = registered_session(0, -1, b"");
        store.write().unwrap().register(h.clone());
        let active = active_for(&h);
        let registry = subscribe::new_registry();

        let (client, server) = UnixStream::pair().unwrap();
        let (store_t, active_t, reg_t) = (store.clone(), active.clone(), registry.clone());
        let join = std::thread::spawn(move || {
            let mut w = server;
            run_subscribe("subscribe @. screen", &active_t, &store_t, &reg_t, Scope::Owner, &mut w);
        });

        // The ack confirms the flip to push mode (accumulate past any immediate
        // catch-up frame that may coalesce into the same read).
        let acc = read_until(&client, String::new(), |s| s.contains("OK subscribe 1\n"));
        assert!(acc.contains("OK subscribe 1\n"), "subscribe ack: {acc:?}");

        // Produce real content, then fire the SAME notify the GUI's Wake::Output
        // hook fires. The push loop re-reads the latest grid and emits a DELTA.
        crate::term_lock(&h.term).process(b"hello-live");
        registry.lock().unwrap().notify(0);
        let frame = read_until(&client, acc, |s| s.contains("hello-live"));
        assert!(frame.contains("DELTA 0 seq="), "screen delta pushed: {frame:?}");
        assert!(frame.contains("hello-live"), "delta carries live text: {frame:?}");

        // A PURE viewport scroll does not bump content_seq -> no further DELTA even
        // though we notify (a coalesced/spurious wake reads unchanged content).
        crate::term_lock(&h.term).scroll_display(1);
        registry.lock().unwrap().notify(0);
        let none = read_quiet(&client);
        assert!(!none.contains("DELTA"), "viewport scroll pushes no delta: {none:?}");

        // Drop the client: the loop's next write fails and it returns (deregister).
        drop(client);
        crate::term_lock(&h.term).process(b"x");
        registry.lock().unwrap().notify(0);
        join.join().expect("push loop ends cleanly on a dead client");
        assert_eq!(registry.lock().unwrap().watched_sessions(), 0, "deregistered on drop");
    }

    /// (b)-deny FAIL-CLOSED: a scoped `Edge` connection that subscribes to a SIBLING
    /// it has NO authorizing edge for gets `ERR denied\n` and never enters push mode
    /// (no partial subscription, no registry entry). Uses a buffer writer since the
    /// denial path returns before the push loop.
    #[test]
    fn subscribe_sibling_without_edge_is_fail_closed() {
        let store = session_store::new_store();
        let self_h = registered_session(0, -1, b"");
        let sib = registered_session(2, -1, b"sibling-screen");
        store.write().unwrap().register(self_h.clone());
        store.write().unwrap().register(sib.clone());
        let active = active_for(&self_h);
        let registry = subscribe::new_registry();

        // An Edge scope carrying a throwaway token with NO grant on the sibling's
        // table => decide_edge denies => fail closed.
        let scope = Scope::Edge(EdgeToken::generate());
        let mut out: Vec<u8> = Vec::new();
        run_subscribe("subscribe @2 screen", &active, &store, &registry, scope, &mut out);
        assert_eq!(String::from_utf8_lossy(&out), "ERR denied\n", "cross subscribe fail-closed");
        assert_eq!(registry.lock().unwrap().watched_sessions(), 0, "no registration on denial");
    }

    /// REGRESSION (capability escape): a SELF (`@.`) subscribe must re-verify the edge
    /// against the session active RIGHT NOW — not treat `@.` as unconditionally
    /// allowed. The one global ActiveHandle retargets `@.` to the new frontmost tab on
    /// every switch (`sync_active_session`); a `ReadScreen` edge granted on session B
    /// must NOT be able to read whatever session A became frontmost. (Pre-fix the SELF
    /// target skipped the gate entirely — `is_cross && ...` — so any edge read A.)
    #[test]
    fn subscribe_self_edge_denied_after_active_session_swings() {
        let store = session_store::new_store();
        let a = registered_session(0, -1, b"secret-of-A"); // frontmost; what `@.` resolves to
        let b = registered_session(2, -1, b""); // a DIFFERENT session, where the edge was granted
        store.write().unwrap().register(a.clone());
        let active = active_for(&a);
        let registry = subscribe::new_registry();

        // A ReadScreen edge GRANTED on B's table (op matches subscribe) — but no grant
        // against the now-active A.
        let tok = {
            let mut edges = b.ctx.edges.lock().unwrap();
            edges.grant(a.sid.clone(), b.sid.clone(), Op::ReadScreen, b.nonce)
        };
        let scope = Scope::Edge(tok);
        // Positive control: the edge legitimately authorizes subscribe on its OWN B.
        assert!(
            cross_session_authorized(scope, "subscribe", &b.ctx),
            "B's edge authorizes subscribe against its own session",
        );

        // A SELF subscribe while A is frontmost is DENIED — no partial push, no entry.
        let mut out: Vec<u8> = Vec::new();
        run_subscribe("subscribe @. screen", &active, &store, &registry, scope, &mut out);
        assert_eq!(
            String::from_utf8_lossy(&out),
            "ERR denied\n",
            "B's edge must NOT subscribe to the swung-to active session A",
        );
        assert_eq!(registry.lock().unwrap().watched_sessions(), 0, "no registration on denial");
    }

    /// (b)-allow: the SAME scoped `Edge`, once it holds a `ReadScreen` grant on the
    /// sibling's edge table (minted by the owner, presented as the connection token),
    /// subscribes to the sibling and receives pushed sid-tagged DELTAs. Mirrors the
    /// cross-session read authorization path exactly (`decide_edge` against the
    /// TARGET table + nonce).
    #[test]
    fn subscribe_sibling_with_read_edge_pushes_deltas() {
        let store = session_store::new_store();
        let self_h = registered_session(0, -1, b"");
        let sib = registered_session(2, -1, b"");
        store.write().unwrap().register(self_h.clone());
        store.write().unwrap().register(sib.clone());
        let active = active_for(&self_h);
        let registry = subscribe::new_registry();

        // Owner mints a ReadScreen edge (self -> sibling). The bearer presents that
        // token as the connection's Edge scope.
        let tok = {
            let mut edges = sib.ctx.edges.lock().unwrap();
            edges.grant(self_h.sid.clone(), sib.sid.clone(), Op::ReadScreen, sib.nonce)
        };
        // Sanity: the gate would PERMIT this exact (token, target) pair.
        assert!(
            cross_session_authorized(Scope::Edge(tok), "subscribe", &sib.ctx),
            "minted edge authorizes the cross subscribe",
        );

        let (client, server) = UnixStream::pair().unwrap();
        let (store_t, active_t, reg_t) = (store.clone(), active.clone(), registry.clone());
        let scope = Scope::Edge(tok);
        let join = std::thread::spawn(move || {
            let mut w = server;
            run_subscribe("subscribe @2 screen", &active_t, &store_t, &reg_t, scope, &mut w);
        });

        let ack = read_until(&client, String::new(), |s| s.contains("OK subscribe 1\n"));
        assert!(ack.contains("OK subscribe 1\n"), "edge subscribe authorized: {ack:?}");

        crate::term_lock(&sib.term).process(b"from-sibling");
        registry.lock().unwrap().notify(2);
        let frame = read_until(&client, ack, |s| s.contains("from-sibling"));
        assert!(frame.contains("DELTA 2 seq="), "sibling delta tagged with its sid: {frame:?}");
        assert!(frame.contains("from-sibling"), "carries the sibling's screen: {frame:?}");

        drop(client);
        crate::term_lock(&sib.term).process(b"x");
        registry.lock().unwrap().notify(2);
        join.join().expect("push loop ends on a dead client");
    }

    /// (d) MULTIPLEX: one connection subscribing to `@0,@2` receives DELTAs tagged
    /// with EACH session's own sid, so the client demultiplexes by the leading token.
    #[test]
    fn subscribe_multiplex_two_sids_tags_each() {
        let store = session_store::new_store();
        let a = registered_session(0, -1, b"");
        let b = registered_session(2, -1, b"");
        store.write().unwrap().register(a.clone());
        store.write().unwrap().register(b.clone());
        let active = active_for(&a);
        let registry = subscribe::new_registry();

        let (client, server) = UnixStream::pair().unwrap();
        let (store_t, active_t, reg_t) = (store.clone(), active.clone(), registry.clone());
        let join = std::thread::spawn(move || {
            let mut w = server;
            // `@0` is self (active), `@2` a sibling; Owner reaches both.
            run_subscribe("subscribe @0,@2 screen", &active_t, &store_t, &reg_t, Scope::Owner, &mut w);
        });
        let ack = read_until(&client, String::new(), |s| s.contains("OK subscribe 2\n"));
        assert!(ack.contains("OK subscribe 2\n"), "two targets acked: {ack:?}");

        crate::term_lock(&a.term).process(b"AAA");
        crate::term_lock(&b.term).process(b"BBB");
        registry.lock().unwrap().notify(0);
        registry.lock().unwrap().notify(2);
        // Accumulate until BOTH sids' deltas (with their text) have shown up.
        let seen = read_until(&client, ack, |s| {
            s.contains("AAA") && s.contains("BBB")
        });
        assert!(seen.contains("DELTA 0 ") && seen.contains("AAA"), "sid 0 frame: {seen:?}");
        assert!(seen.contains("DELTA 2 ") && seen.contains("BBB"), "sid 2 frame: {seen:?}");

        drop(client);
        crate::term_lock(&a.term).process(b"x");
        registry.lock().unwrap().notify(0);
        join.join().expect("multiplex push loop ends on a dead client");
    }

    /// (c) A STALLED subscriber (its socket buffer full, never drained) cannot block
    /// or backpressure the PRODUCER: the producing session's `content_seq` keeps
    /// advancing freely while a subscription is registered and never `wait`ed on.
    /// This is the registry-level guarantee the GUI's one-line notify hook relies on
    /// — `notify` is a single-slot `try_send`, O(1) and infallible.
    #[test]
    fn stalled_subscriber_never_blocks_producer_content_seq() {
        let store = session_store::new_store();
        let h = registered_session(0, -1, b"");
        store.write().unwrap().register(h.clone());
        let registry = subscribe::new_registry();

        // Register a subscriber for session 0 and NEVER wait() on it (wedged).
        let _wedged = subscribe::SubscriberSet::register(&registry, &[0]);

        let before = crate::term_lock(&h.term).content_seq();
        let start = std::time::Instant::now();
        // Drive a flood of producer output + the matching notify hook. If a stalled
        // subscriber could backpressure, this would stall; it must stay fast.
        for _ in 0..2000 {
            crate::term_lock(&h.term).process(b"x");
            registry.lock().unwrap().notify(0);
        }
        let after = crate::term_lock(&h.term).content_seq();
        assert!(after > before, "producer content_seq advanced past a stalled subscriber");
        assert!(start.elapsed() < std::time::Duration::from_secs(5), "producer not blocked");
    }

    // ── wf3: --json read mode + edges/family/feed-bin/ready verbs ─────────────

    /// A minimal JSON spot-checker: confirms a string is a balanced JSON object
    /// (braces/brackets nest, strings are closed) so the schema tests assert on
    /// shape without a full parser dependency. Returns the contained substring of
    /// the FIRST top-level object for convenience.
    fn assert_balanced_json(s: &str) {
        let mut depth: i32 = 0;
        let mut in_str = false;
        let mut esc = false;
        for c in s.chars() {
            if in_str {
                if esc {
                    esc = false;
                } else if c == '\\' {
                    esc = true;
                } else if c == '"' {
                    in_str = false;
                }
                continue;
            }
            match c {
                '"' => in_str = true,
                '{' | '[' => depth += 1,
                '}' | ']' => depth -= 1,
                _ => {}
            }
            assert!(depth >= 0, "JSON brace underflow in {s:?}");
        }
        assert_eq!(depth, 0, "unbalanced JSON braces in {s:?}");
        assert!(!in_str, "unterminated JSON string in {s:?}");
    }

    /// `json_escape` produces RFC 8259-valid string bodies: quotes/backslashes and
    /// the whitespace controls get two-char escapes, other C0 bytes get `\u00XX`,
    /// and ordinary (incl. non-ASCII) text is verbatim.
    #[test]
    fn json_escape_handles_quotes_controls_and_unicode() {
        assert_eq!(json_escape("plain"), "plain");
        assert_eq!(json_escape("a\"b\\c"), "a\\\"b\\\\c");
        assert_eq!(json_escape("tab\tnl\ncr\r"), "tab\\tnl\\ncr\\r");
        assert_eq!(json_escape("\u{0001}"), "\\u0001");
        // Non-ASCII is emitted verbatim (JSON strings are UTF-8).
        assert_eq!(json_escape("café 日本"), "café 日本");
    }

    /// `text --json` emits the documented schema: a `rows` array whose entries are
    /// the SAME grapheme-faithful tail-trimmed lines the TEXT form emits, plus a
    /// `cursor` object, `dims`, and the `seq` (content_seq). The text path is
    /// byte-identical when the flag is absent.
    #[test]
    fn text_json_mode_matches_text_rows_and_carries_cursor_seq() {
        let term = Arc::new(Mutex::new(Terminal::new(24, 80)));
        term.lock().unwrap().process(b"line-zero\r\nsecond \"quoted\"");

        // Text form (the byte-identical baseline) and JSON form.
        let text = cmd_text(&term);
        let json = cmd_text_json(&term);

        // Framing: `OK 1\n<json>\n` (one body line), and the body is balanced JSON.
        assert!(json.starts_with("OK 1\n"), "json framing: {json}");
        let body = json.strip_prefix("OK 1\n").unwrap().trim_end();
        assert_balanced_json(body);

        // Rows carry the same visible content as the text form's data lines.
        let text_rows: Vec<&str> = text.lines().skip(1).collect();
        assert!(body.contains("\"rows\":["), "has rows array: {body}");
        assert!(body.contains(&format!("\"{}\"", text_rows[0])), "row0 present: {body}");
        // The quote in row 1 is escaped in JSON, NOT in text.
        assert!(text_rows[1].contains("second \"quoted\""), "text keeps raw quote");
        assert!(body.contains("second \\\"quoted\\\""), "json escapes the quote: {body}");

        // cursor + dims + seq members are present and consistent with the verbs.
        let c = term.lock().unwrap().cursor();
        assert!(body.contains(&format!("\"row\":{}", c.row)), "cursor row: {body}");
        assert!(body.contains("\"dims\":{\"rows\":24,\"cols\":80}"), "dims: {body}");
        assert!(body.contains("\"seq\":"), "carries content_seq: {body}");
    }

    /// The `--json`/`json` flag is parsed off `rest` additively: a line without it
    /// is byte-identical, and the flag is stripped from the remainder so the verb's
    /// own positional parse runs unchanged (e.g. `blocks 1 --json`).
    #[test]
    fn take_json_flag_is_additive_and_strips_the_flag() {
        assert_eq!(take_json_flag(""), (false, String::new()));
        assert_eq!(take_json_flag("1"), (false, "1".to_string()));
        assert_eq!(take_json_flag("--json"), (true, String::new()));
        assert_eq!(take_json_flag("1 --json"), (true, "1".to_string()));
        assert_eq!(take_json_flag("json 1"), (true, "1".to_string()));
    }

    /// `cursor --json` / `dims --json` / `blocks --json` round-trip the same data as
    /// their text forms in a balanced-JSON body.
    #[test]
    fn cursor_dims_blocks_json_schemas() {
        let term = Arc::new(Mutex::new(Terminal::new(24, 80)));
        // cursor: row/col/visible/style.
        let cj = cmd_cursor_json(&term);
        let cbody = cj.strip_prefix("OK 1\n").unwrap().trim_end();
        assert_balanced_json(cbody);
        assert!(cbody.contains("\"row\":0") && cbody.contains("\"style\":\"blinking_block\""), "{cbody}");

        // dims: rows/cols/pixels (cell size (8,16) for the test).
        let dj = cmd_dims_json(&term, (8, 16));
        let dbody = dj.strip_prefix("OK 1\n").unwrap().trim_end();
        assert_balanced_json(dbody);
        assert!(dbody.contains("\"rows\":24,\"cols\":80,\"pixel_w\":640,\"pixel_h\":384"), "{dbody}");

        // blocks: two OSC-133 blocks -> a `blocks` array; absent rows are JSON null.
        term.lock().unwrap().process(
            b"\x1b]133;A\x07$ \x1b]633;E;echo hi\x07\x1b]133;B\x07echo hi\n\x1b]133;C\x07hi\n\x1b]133;D;0\x07",
        );
        let bj = cmd_blocks_json(&term, "");
        let bbody = bj.strip_prefix("OK 1\n").unwrap().trim_end();
        assert_balanced_json(bbody);
        assert!(bbody.contains("\"blocks\":[{"), "blocks array: {bbody}");
        assert!(bbody.contains("\"exit\":0") && bbody.contains("\"cmdline\":\"echo hi\""), "{bbody}");
        assert!(bbody.contains("\"state\":\"complete\""), "{bbody}");
    }

    /// `edges`/`grants` lists this session's inbound EdgeTable rows as
    /// `<src> <dst> <op>`, sorted, WITHOUT ever leaking the bearer token; the JSON
    /// form carries the same triples. An empty table is `OK 0`.
    #[test]
    fn edges_verb_lists_capability_edges_without_tokens() {
        let ctx = test_ctx();
        assert_eq!(cmd_edges(&ctx), "OK 0\n", "no edges yet");

        // Mint two edges into this session (the data `grant` records).
        let tok = {
            let mut tbl = ctx.edges.lock().unwrap();
            tbl.grant(SessionId::new("s-src-a"), ctx.self_id.clone(), Op::ReadScreen, ctx.nonce);
            tbl.grant(SessionId::new("s-src-b"), ctx.self_id.clone(), Op::WriteInput, ctx.nonce)
        };
        let out = cmd_edges(&ctx);
        let mut lines = out.lines();
        assert_eq!(lines.next(), Some("OK 2"), "header counts both edges: {out}");
        // Sorted by (src, op): s-src-a read-screen, then s-src-b write-input.
        let l1 = lines.next().unwrap();
        let l2 = lines.next().unwrap();
        assert!(l1.starts_with("s-src-a ") && l1.ends_with(" read-screen"), "edge 1: {l1}");
        assert!(l2.starts_with("s-src-b ") && l2.ends_with(" write-input"), "edge 2: {l2}");
        // The dst column is always THIS session.
        assert!(l1.contains(ctx.self_id.as_str()), "dst is self: {l1}");
        // The secret token NEVER appears in the listing.
        assert!(!out.contains(&tok.to_hex()), "edge token must not leak: {out}");

        // JSON form: same triples, balanced, no token.
        let j = cmd_edges_json(&ctx);
        let body = j.strip_prefix("OK 1\n").unwrap().trim_end();
        assert_balanced_json(body);
        assert!(body.contains("\"src\":\"s-src-a\"") && body.contains("\"op\":\"read-screen\""), "{body}");
        assert!(!body.contains(&tok.to_hex()), "json must not leak the token: {body}");
    }

    /// `family` emits the hierarchy for a node: a `self` line, a `parent` line
    /// (`-` for a root), and a `child` line per direct child (sorted by local id).
    /// An explicit `<sid>` argument is Owner-only; the no-arg form is scoped to the
    /// resolved session.
    #[test]
    fn family_verb_emits_parent_and_children() {
        let store = session_store::new_store();
        let root = registered_session(0, -1, b"");
        let root_ctx = root.ctx.clone();
        let mut child_a = registered_session(1, -1, b"");
        child_a.parent = Some(root.sid.clone());
        let mut child_b = registered_session(2, -1, b"");
        child_b.parent = Some(root.sid.clone());
        store.write().unwrap().register(root.clone());
        store.write().unwrap().register(child_a.clone());
        store.write().unwrap().register(child_b.clone());

        // No-arg form from the ROOT's ctx (Owner): self=root, parent=-, two children.
        let out = cmd_family(&root_ctx, &store, Scope::Owner, "");
        let mut lines = out.lines();
        assert_eq!(lines.next(), Some("OK"), "header: {out}");
        let self_line = lines.next().unwrap();
        assert!(self_line.starts_with(&format!("self {} ", root.sid.as_str())), "self: {self_line}");
        assert_eq!(lines.next(), Some("parent - - -"), "root has no parent: {out}");
        let kids: Vec<&str> = lines.collect();
        assert_eq!(kids.len(), 2, "two children: {out}");
        assert!(kids[0].starts_with(&format!("child {} ", child_a.sid.as_str())), "child a: {out}");
        assert!(kids[1].starts_with(&format!("child {} ", child_b.sid.as_str())), "child b: {out}");

        // Explicit `<sid>` of a child (Owner): self=child, parent=root, no children.
        let cout = cmd_family(&root_ctx, &store, Scope::Owner, child_a.sid.as_str());
        assert!(cout.contains(&format!("self {} ", child_a.sid.as_str())), "child self: {cout}");
        assert!(cout.contains(&format!("parent {} ", root.sid.as_str())), "child parent=root: {cout}");
        assert!(!cout.contains("\nchild "), "a leaf has no children: {cout}");

        // An explicit sid is OWNER-ONLY: a scoped Edge is denied (cannot enumerate
        // arbitrary trees).
        let edge = Scope::Edge(EdgeToken::generate());
        assert_eq!(
            cmd_family(&root_ctx, &store, edge, child_a.sid.as_str()),
            "ERR denied\n",
            "explicit-sid family is owner-only",
        );
        // The no-arg form (resolved session) is allowed for an Edge (already gated).
        assert!(cmd_family(&root_ctx, &store, edge, "").starts_with("OK\n"), "no-arg edge ok");

        // An unknown sid fails closed.
        assert_eq!(cmd_family(&root_ctx, &store, Scope::Owner, "s-nope"), "ERR no such session\n");
    }

    /// `feed-bin` FRAMING: the request-line parse extracts the optional selector and
    /// the declared length, rejects malformed/oversize lines, and recognizes the
    /// verb (with or without an `@<sel>` prefix).
    #[test]
    fn feed_bin_framing_parse() {
        // Bare form.
        assert!(matches!(parse_feed_bin("feed-bin 4"), Some((None, 4))));
        assert!(is_feed_bin_line("feed-bin 4"));
        // Cross-session form.
        assert!(matches!(parse_feed_bin("@7 feed-bin 10"), Some((Some(Selector::Local(7)), 10))));
        assert!(is_feed_bin_line("@7 feed-bin 10"));
        // Not feed-bin.
        assert!(!is_feed_bin_line("feed 0a"));
        assert!(!is_feed_bin_line("@7 feed 0a"));
        // Malformed: missing length, non-numeric, oversize -> None (fail closed).
        assert!(parse_feed_bin("feed-bin").is_none());
        assert!(parse_feed_bin("feed-bin xx").is_none());
        assert!(parse_feed_bin(&format!("feed-bin {}", MAX_FEED_BIN + 1)).is_none());
        // Exactly the cap is allowed.
        assert!(matches!(parse_feed_bin(&format!("feed-bin {MAX_FEED_BIN}")), Some((None, n)) if n == MAX_FEED_BIN));
    }

    /// `feed-bin <n>\n<bytes>` end-to-end: an Owner connection's length-prefixed
    /// payload lands the EXACT raw bytes on the resolved target's PTY (binary-clean,
    /// no hex), replies `OK <n> bytes`, and leaves the stream correctly framed for
    /// the NEXT request. Mirrors the production `serve` wiring: a `BufReader` over a
    /// pipe holding `feed-bin 3\n\x00\x01\x02` then a following line.
    #[test]
    fn feed_bin_writes_raw_bytes_and_keeps_stream_framed() {
        let store = session_store::new_store();
        let (h_self, self_rx) = pipe_session(1);
        store.write().unwrap().register(h_self.clone());
        let active = active_for_handle(&h_self);

        // The control INPUT stream: the request line + 3 raw bytes (incl. NUL, which
        // a line-delimited `send` could never carry) + a trailing line to prove the
        // stream stays framed after the payload.
        let input: Vec<u8> = b"feed-bin 3\n\x00\x01\x02next-line\n".to_vec();
        let mut reader = BufReader::new(std::io::Cursor::new(input));
        let mut out: Vec<u8> = Vec::new();

        // Read the (already-known) first line, then run the binary frame.
        let line = read_request_line(&mut reader).expect("the feed-bin request line");
        assert_eq!(line, "feed-bin 3");
        assert!(is_feed_bin_line(&line));
        let keep = run_feed_bin(&line, &mut reader, &active, &store, Scope::Owner, &mut out);
        assert!(keep, "connection kept after a clean frame");
        assert_eq!(String::from_utf8_lossy(&out), "OK 3 bytes\n", "reply framing");

        // The raw bytes (incl. the NUL) reached the TARGET's PTY verbatim.
        assert_eq!(drain_pipe(&self_rx), b"\x00\x01\x02", "raw binary bytes hit the pty");

        // The stream is still framed: the NEXT request line is intact.
        let next = read_request_line(&mut reader).expect("the following line survives");
        assert_eq!(next, "next-line", "stream stays framed past the binary payload");
    }

    /// `feed-bin` AUTH: a ReadScreen Edge is DENIED the write — but the N payload
    /// bytes are still CONSUMED so the stream stays framed (the denial reads-and-
    /// discards), and NOTHING reaches the PTY.
    #[test]
    fn feed_bin_read_edge_is_denied_but_consumes_payload() {
        let store = session_store::new_store();
        let (h_self, self_rx) = pipe_session(1);
        store.write().unwrap().register(h_self.clone());
        let active = active_for_handle(&h_self);

        let input: Vec<u8> = b"feed-bin 3\nABCafter\n".to_vec();
        let mut reader = BufReader::new(std::io::Cursor::new(input));
        let mut out: Vec<u8> = Vec::new();

        let line = read_request_line(&mut reader).unwrap();
        // A read-only Edge: WriteInput is required for feed/feed-bin -> denied.
        let scope = Scope::Edge(EdgeToken::generate());
        let keep = run_feed_bin(&line, &mut reader, &active, &store, scope, &mut out);
        assert!(keep, "denied frame keeps the connection");
        assert_eq!(String::from_utf8_lossy(&out), "ERR denied\n", "read edge denied feed-bin");
        // No bytes reached the pty.
        assert!(drain_pipe(&self_rx).is_empty(), "denied feed-bin writes nothing");
        // But the 3 payload bytes WERE consumed: the next line is correctly framed.
        let next = read_request_line(&mut reader).unwrap();
        assert_eq!(next, "after", "denial still consumes the payload (stream framed)");
    }

    /// REGRESSION (capability escape): `feed-bin`'s SELF path must re-verify the edge
    /// against the session active RIGHT NOW — not op-match alone. The one global
    /// ActiveHandle retargets `@.`/self to the new frontmost tab on every switch
    /// (`sync_active_session`); a `WriteInput` edge granted on session B must NOT inject
    /// raw bytes into whatever session A became frontmost. (Pre-fix the SELF branch
    /// matched `Edge(WriteInput, _)` on the op alone and let B's token write into A.)
    #[test]
    fn feed_bin_self_edge_denied_after_active_session_swings() {
        let store = session_store::new_store();
        // Session A is FRONTMOST — what `@.`/self resolves to via the active handle.
        let (h_a, a_rx) = pipe_session(1);
        // Session B is where the edge was legitimately granted (a DIFFERENT session).
        let (h_b, _b_rx) = pipe_session(2);
        store.write().unwrap().register(h_a.clone());
        let active = active_for_handle(&h_a);

        // A WriteInput edge GRANTED on B's table: the op matches feed-bin, but it holds
        // no grant against the now-active A. (Scope is Copy, reused for both asserts.)
        let edge_b = edge_granted(Op::WriteInput, &h_b.ctx);
        // Positive control: the SAME edge legitimately drives its OWN session B.
        assert!(
            cross_session_authorized(edge_b, "feed", &h_b.ctx),
            "B's edge authorizes feed-bin against its own session",
        );

        let input: Vec<u8> = b"feed-bin 3\nXYZafter\n".to_vec();
        let mut reader = BufReader::new(std::io::Cursor::new(input));
        let mut out: Vec<u8> = Vec::new();
        let line = read_request_line(&mut reader).unwrap();
        let keep = run_feed_bin(&line, &mut reader, &active, &store, edge_b, &mut out);

        assert!(keep, "denied frame keeps the connection");
        assert_eq!(
            String::from_utf8_lossy(&out),
            "ERR denied\n",
            "B's edge must NOT feed-bin the swung-to active session A",
        );
        assert!(drain_pipe(&a_rx).is_empty(), "nothing reached A's pty");
        // The denial still CONSUMED the N payload bytes, so the stream stays framed.
        let next = read_request_line(&mut reader).unwrap();
        assert_eq!(next, "after", "denial still consumes the payload (stream framed)");
    }

    /// `feed-bin` with a malformed length replies `ERR usage` WITHOUT consuming any
    /// payload (the next line is whatever followed the bad request line verbatim).
    #[test]
    fn feed_bin_bad_length_does_not_consume_payload() {
        let store = session_store::new_store();
        let (h_self, _self_rx) = pipe_session(1);
        store.write().unwrap().register(h_self.clone());
        let active = active_for_handle(&h_self);

        let input: Vec<u8> = b"feed-bin notanumber\nfollowing\n".to_vec();
        let mut reader = BufReader::new(std::io::Cursor::new(input));
        let mut out: Vec<u8> = Vec::new();
        let line = read_request_line(&mut reader).unwrap();
        let keep = run_feed_bin(&line, &mut reader, &active, &store, Scope::Owner, &mut out);
        assert!(keep);
        assert!(String::from_utf8_lossy(&out).starts_with("ERR usage"), "bad length usage: {out:?}");
        // Nothing consumed: the line right after the bad request is intact.
        let next = read_request_line(&mut reader).unwrap();
        assert_eq!(next, "following", "a parse error consumes no payload");
    }

    /// `read_request_line` yields the SAME line shape the old `lines()` iterator did:
    /// strips the `\n` (and a CRLF `\r`), yields a final unterminated line at EOF,
    /// and drops a runaway line past the cap (returns None).
    #[test]
    fn read_request_line_strips_newline_and_cr() {
        let input: Vec<u8> = b"plain\r\ncrlf\r\nlast-no-nl".to_vec();
        let mut reader = BufReader::new(std::io::Cursor::new(input));
        assert_eq!(read_request_line(&mut reader).as_deref(), Some("plain"));
        assert_eq!(read_request_line(&mut reader).as_deref(), Some("crlf"));
        assert_eq!(read_request_line(&mut reader).as_deref(), Some("last-no-nl"), "EOF yields the tail");
        assert_eq!(read_request_line(&mut reader), None, "then EOF");
    }

    /// `ready` reports the target's readiness: a fresh OSC-133 prompt is `prompt`,
    /// an in-flight command times out (not ready), a completed command is `prompt`
    /// again, and a session marked `Exited` in the registry fails closed.
    #[test]
    fn ready_verb_reports_prompt_executing_and_exit() {
        let store = session_store::new_store();
        let h = registered_session(0, -1, b"");
        store.write().unwrap().register(h.clone());
        let term = &h.term;

        // A fresh prompt (OSC 133 A) -> PromptOnly -> ready immediately.
        term.lock().unwrap().process(b"\x1b]133;A\x07$ ");
        assert_eq!(cmd_ready(term, &store, 0, "2000"), "OK ready prompt\n", "fresh prompt is ready");

        // An executing command -> NOT ready -> a short timeout returns `OK timeout`.
        term.lock().unwrap().process(b"\x1b]133;B\x07sleep\n\x1b]133;C\x07");
        assert_eq!(cmd_ready(term, &store, 0, "0"), "OK timeout\n", "executing is not ready");

        // The command completes -> Complete -> ready again (prompt-end).
        term.lock().unwrap().process(b"\x1b]133;D;0\x07");
        assert_eq!(cmd_ready(term, &store, 0, "2000"), "OK ready prompt\n", "completed is ready");

        // A session marked Exited never becomes ready -> ERR exited (fail closed).
        store.write().unwrap().set_state(0, session_store::SessionState::Exited);
        assert_eq!(cmd_ready(term, &store, 0, "2000"), "ERR exited\n", "exited fails closed");
    }

    /// `ready` for a PLAIN shell (no OSC-133 integration) settles on a stable
    /// `content_seq`: with no in-flight block it returns `OK ready idle` once output
    /// has stopped changing across the settle window.
    #[test]
    fn ready_verb_settles_on_idle_without_shell_integration() {
        let store = session_store::new_store();
        let h = registered_session(0, -1, b"some output\r\n");
        store.write().unwrap().register(h.clone());
        // No OSC-133 at all: the settle heuristic fires (content_seq holds steady).
        assert_eq!(cmd_ready(&h.term, &store, 0, "2000"), "OK ready idle\n", "idle plain shell");
    }

    /// `edges`/`grants`/`family`/`ready` are classified as READ-side (`ReadScreen`)
    /// in `required_op`, so a ReadScreen edge may run them but a WriteInput edge may
    /// not — the same read != write split every other read verb honors.
    #[test]
    fn new_read_verbs_are_read_scoped() {
        let ctx = test_ctx();
        let read = edge_granted(Op::ReadScreen, &ctx);
        let write = edge_granted(Op::WriteInput, &ctx);
        for v in ["edges", "grants", "family", "ready"] {
            assert_eq!(required_op(v), Some(Op::ReadScreen), "{v} is read-side");
            assert!(gate_allows(read, v, &ctx), "read edge may {v}");
            assert!(!gate_allows(write, v, &ctx), "write edge may NOT {v}");
        }
    }

    /// REGRESSION (integration audit): the SELF-path op-scope gate must re-verify an
    /// Edge against the session that is active RIGHT NOW — not op-match alone. The one
    /// global ActiveHandle is retargeted to the new frontmost active tab on every tab
    /// switch / cross-window focus change (`sync_active_session`); an edge granted on
    /// session B must NOT be able to drive whatever session A became frontmost after
    /// the swing (a confused-deputy authority escape: e.g. a WriteInput edge injecting
    /// keystrokes into, or resizing, an arbitrary foreground session). Owner keeps
    /// full self-power regardless of which session is active.
    #[test]
    fn self_path_edge_denied_after_active_session_swings() {
        let ctx_b = test_ctx(); // the session active when the edge connected
        let ctx_a = test_ctx(); // a DIFFERENT session the active handle later swings to
        let edge_b = edge_granted(Op::WriteInput, &ctx_b);

        // While B is active, the edge drives its OWN granted session (legitimate).
        assert!(gate_allows(edge_b, "send", &ctx_b), "edge drives its granted session B");

        // After the active handle SWINGS to A, the SAME edge is DENIED on the SELF
        // path — it holds no grant against A. (Pre-fix this passed on op-match alone.)
        assert!(!gate_allows(edge_b, "send", &ctx_a), "edge must NOT drive swung-to session A");
        assert!(!gate_allows(edge_b, "resize", &ctx_a), "incl. resize (whole-window effect)");

        // Owner is unaffected — full self-power against whichever session is active.
        assert!(gate_allows(Scope::Owner, "send", &ctx_a), "owner drives the active session");
    }

    /// Build an [`ActiveHandle`] over a `pipe_session` handle (the cross-session
    /// feed-bin tests resolve `@.`/self through the active handle the same way the
    /// production `serve` loop does).
    fn active_for_handle(h: &crate::session_store::SessionHandle) -> ActiveHandle {
        Arc::new(Mutex::new(ActiveSession {
            term: h.term.clone(),
            master: h.master,
            id: h.local_id,
            ctx: h.ctx.clone(),
        }))
    }
}
