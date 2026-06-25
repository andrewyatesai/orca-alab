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
use std::sync::{Arc, Mutex};

use aterm_containment::log_denial;
use aterm_core::terminal::{CursorStyle, Terminal};
use aterm_session::{EdgeToken, Op, SessionId, decide_edge};
use winit::event_loop::EventLoopProxy;

use crate::control_auth::{self, AuthOutcome};
use crate::input::{Egress, InputEvent, InputOutcome, ScrollIntent, Source, seam_egress};
use crate::session_store::Store;
use crate::subscribe::{self, Streams, Subscribers};
use crate::{SessionCtx, Wake, term_lock};

/// Read-only screen introspection serializers (the SACRED AI-reads-the-screen
/// path). Child module of `control`; verbs are dispatched as
/// `control_query::cmd_*` from [`handle`]. The file lives flat at
/// `src/control_query.rs` (sibling of `control.rs`), so `#[path]` points at it.
#[path = "control_query.rs"]
mod control_query;
// Re-export the two query serializers that out-of-module callers reach through
// the stable `crate::control::NAME` path (`crate::subscribe`), so the path keeps
// resolving after the move.
pub(crate) use control_query::{styled_frame_payload, visible_row};

/// Input-injection verbs + their parsers (key/ctrl/send/feed/signal/mouse/paste/
/// focus/resize/scroll/tab). Child module of `control`; dispatched as
/// `control_input::cmd_*` from [`handle`]. The file lives flat at
/// `src/control_input.rs` (sibling of `control.rs`), so `#[path]` points at it.
#[path = "control_input.rs"]
mod control_input;
// Re-export the parsers that out-of-module callers reach through the stable
// `crate::control::NAME` path (`crate::input`), so the path keeps resolving.
pub(crate) use control_input::{parse_ctrl, parse_key, parse_mouse};

/// Selection / copy / block verbs (`select`/`selection`/`copy`/`blocks`/
/// `blocktext`/`wait`). Child module of `control`; dispatched as
/// `control_selection::cmd_*` from [`handle`]. The file lives flat at
/// `src/control_selection.rs` (sibling of `control.rs`), so `#[path]` points at it.
#[path = "control_selection.rs"]
mod control_selection;
// Re-export `pbcopy` (GUI OSC-52 path, `main.rs`) and the smart-selection
// gesture helpers (`app_mouse.rs`'s double/triple-click), which both reach
// through the stable `crate::control::NAME` path, so those paths keep resolving.
pub(crate) use control_selection::{
    clipboard_command, pbcopy, select_line, select_word, word_cols,
};

/// Media-capture verbs (`image`/`image read`/`window`/`chrome`). Child module of
/// `control`; dispatched as `control_media::cmd_*` from [`handle`]. The file
/// lives flat at `src/control_media.rs` (sibling of `control.rs`), so `#[path]`
/// points at it.
#[path = "control_media.rs"]
mod control_media;
// Re-export `image_payload`: `control_query::styled_image_json` reaches it through
// `super::image_payload`, which now resolves to this sibling module's serializer.
pub(crate) use control_media::image_payload;

#[path = "control_app_fed.rs"]
mod control_app_fed;
/// Session-graph + capability-authority verbs (`sessions`/`family`/`ready`/
/// `cast`/`edges`/`grant`/`revoke`/`whoami`). Child module of `control`;
/// dispatched as `control_session::cmd_*` from [`handle`]. The file lives flat at
/// `src/control_session.rs` (sibling of `control.rs`), so `#[path]` points at it.
#[path = "control_session.rs"]
mod control_session;

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
pub(crate) enum Scope {
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
        | "grants" | "family" | "ready" | "await" | "metrics" => Some(Op::ReadScreen),
        // write-side: bytes/geometry the driven PROGRAM observes. `feed-bin` is the
        // length-prefixed binary twin of `feed`; the local path intercepts it via
        // `is_feed_bin_line` before this table is consulted, but the cross-process
        // forward (Item 5b) classifies it here. `tab` DRIVES the GUI (opens/switches
        // the front window's tabs, mutating `App` on the event loop), so it is classed
        // with the other write verbs rather than the read-side observers.
        "send" | "key" | "ctrl" | "feed" | "feed-bin" | "mouse" | "paste" | "resize" | "focus"
        | "tab" | "metric" => Some(Op::WriteInput),
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
#[allow(
    clippy::too_many_arguments,
    reason = "the control server's full set of independent collaborators (active handle, store, subscribers, proxy, image queue, socket plan, cell size); a config struct would only move the list"
)]
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
                eprintln!("aterm-gui: could not provision control-socket token; socket disabled");
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
        eprintln!(
            "aterm-gui: control socket listening at {sock_path} (token-gated, same-uid only)"
        );
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
                    stream,
                    &active,
                    &store,
                    &subscribers,
                    &proxy,
                    &queue,
                    cell_size,
                    &token,
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
    if store
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .by_sid(sid)
        .is_some()
    {
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
        return Some((
            sock_path,
            crate::proxy::forward_first_line(&tok.to_hex(), &rewritten),
        ));
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
    Some((
        sock_path,
        crate::proxy::forward_first_line(&tok.to_hex(), &rewritten),
    ))
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
    if let Some(verb) = inline_verb
        && !verb.is_empty()
    {
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
                &verb,
                &term,
                master,
                sid,
                &ctx,
                store,
                scope,
                proxy,
                queue,
                cell_size,
                sock_dir,
                subscribers,
            );
            if writer.write_all(resp.as_bytes()).is_err() {
                return;
            }
            let _ = writer.flush();
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
            &line,
            &term,
            master,
            sid,
            &ctx,
            store,
            scope,
            proxy,
            queue,
            cell_size,
            sock_dir,
            subscribers,
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
                return if buf.is_empty() {
                    None
                } else {
                    Some(decode_request_line(buf))
                };
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
    let authorized = matches!(scope, Scope::Owner) || cross_session_authorized(scope, "feed", &ctx);
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

    // D3 self-feed floor: `feed-bin` reaches the PTY HERE — it is intercepted in
    // `serve` BEFORE `handle()`, so it bypasses the verb-dispatch floor. Apply the
    // SAME per-session injection bucket on the SELF path so a raw client cannot
    // drive a feedback storm via `feed-bin` (the cross path is gated by the edge
    // above, and targets a different session anyway).
    if matches!(&selector, None | Some(Selector::SelfTok))
        && !crate::inject_floor::allow(self_session, payload.len().max(1))
    {
        let _ = writer.write_all(b"ERR rate (self-feed floor)\n");
        let _ = writer.flush();
        return true;
    }

    control_input::write_pty(&ctx.sink, &payload);
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
            let _ =
                writer.write_all(b"ERR usage: subscribe @<sel>[,<sel>] <streams> [since=<seq>]\n");
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
    if writer
        .write_all(format!("OK subscribe {}\n", targets.len()).as_bytes())
        .is_err()
    {
        return;
    }
    if writer.flush().is_err() {
        return;
    }
    subscribe::push_loop(
        subscribers,
        store,
        &targets,
        streams,
        since,
        non_coalesced,
        writer,
    );
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
fn resolve_target(self_tuple: &Target, store: &Store, sel: &Selector) -> Option<Target> {
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
            decide_edge(
                &table,
                &presented,
                &target_ctx.self_id,
                need,
                &target_ctx.nonce,
            )
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
fn cross_mouse_apply(
    term: &Arc<Mutex<Terminal>>,
    ctx: &SessionCtx,
    rest: &str,
) -> Result<bool, String> {
    let ev = parse_mouse(rest)?;
    match seam_egress(term, &ctx.sink, &ev) {
        // Tracking ON but the PTY write failed: honest error, not a false OK.
        Egress::Reported(crate::input::Delivery::Failed) => Err("ERR write failed\n".to_string()),
        // Tracking ON: the seam already wrote the report to the target sink.
        Egress::Reported(_) => Ok(false),
        // Tracking OFF: only a wheel has a meaningful background fallback — move the
        // target viewport. A plain press/release/move is a deliberate no-op (no
        // controller-side selection UI for a background tab).
        Egress::TrackingOff {
            wheel_lines,
            wheel_up,
        } if wheel_lines > 0 => {
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
    let (rows, cols) = match control_input::parse_resize(rest) {
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
    subscribers: &Subscribers,
) -> String {
    // Tolerate CRLF clients; the protocol itself is bare-LF terminated.
    let line = line.strip_suffix('\r').unwrap_or(line);

    // P1.2: parse an OPTIONAL leading `@<selector>` BEFORE the verb split. Absence
    // (the first token does not start with '@') is the verbatim self path below —
    // byte-identical to the pre-P1.2 wire form.
    let (selector, line) = match line.split_once(' ') {
        Some((first, tail)) if first.starts_with('@') => (Some(Selector::parse(&first[1..])), tail),
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
                "sessions" => control_session::cmd_sessions(self_ctx, store),
                "grant" => control_session::cmd_grant(self_ctx, scope, rest),
                "revoke" => control_session::cmd_revoke(self_ctx, scope, rest),
                "whoami" => control_session::cmd_whoami(self_ctx, scope),
                _ => unreachable!(),
            };
        }
        _ => {}
    }

    // Resolve the dispatch target. No selector (or `@.`) => the verbatim self
    // tuple (zero regression). Otherwise resolve the sibling from the registry.
    let self_tuple: Target = (
        self_term.clone(),
        self_master,
        self_session,
        self_ctx.clone(),
    );
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
    } else if !matches!(scope, Scope::Owner) && !cross_session_authorized(scope, verb, ctx) {
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
    if rest.contains("json")
        && let (true, body) = take_json_flag(rest)
    {
        let json = match verb {
            "text" => Some(control_query::cmd_text_json(term)),
            // `screen` is ALWAYS styled JSON; accept `screen --json` for symmetry.
            "screen" => Some(control_query::cmd_screen_styled_json(term)),
            "cursor" => Some(control_query::cmd_cursor_json(term)),
            "dims" => Some(control_query::cmd_dims_json(term, cell_size)),
            "blocks" => Some(control_selection::cmd_blocks_json(term, &body)),
            "edges" | "grants" => Some(control_session::cmd_edges_json(ctx)),
            _ => None,
        };
        if let Some(out) = json {
            return out;
        }
    }

    // D3: the un-bypassable SELF-FEED FLOOR. Every self-targeted input-injection
    // verb passes a per-session token bucket FIRST, so a raw client cannot drive
    // an output->observe->write feedback storm by looping `feed @.` (the L2
    // `SelfGovernor` only binds drivers that link `aterm-agent`; this floor binds
    // everyone). Generous cap; legitimate driving never trips it. The floor scopes
    // to SELF: a cross-session write targets a DIFFERENT session (so it cannot
    // self-loop) and is separately authority-gated by that session's edge token.
    // `feed-bin` is NOT listed here — it is intercepted before this dispatch and
    // passes the SAME floor in `run_feed_bin`.
    if !is_cross && matches!(verb, "send" | "key" | "ctrl" | "feed" | "mouse" | "paste") {
        let nbytes = rest.len().max(1);
        if !crate::inject_floor::allow(self_session, nbytes) {
            return "ERR rate (self-feed floor)\n".to_string();
        }
    }

    match verb {
        "text" => control_query::cmd_text(term),
        // The LOSSLESS styled-screen read (keystone): full per-cell colour +
        // resolved decorations + cursor + dims + seq as one JSON frame. Always
        // styled-JSON (no plaintext variant) — `--json` is implied.
        "screen" => control_query::cmd_screen_styled_json(term),
        "cursor" => control_query::cmd_cursor(term),
        "cell" => control_query::cmd_cell(term, rest),
        "search" => control_query::cmd_search(term, rest),
        // `edges`/`grants`: list this session's inbound capability edges (the
        // EdgeTable rows). A pure observer of the AUTHORITY surface, so it is gated
        // as `ReadScreen` like every other read verb; cross-session reads a sibling's
        // table through the same `@<selector>` resolution + gate.
        "edges" | "grants" => control_session::cmd_edges(ctx),
        // `family [<sid>]`: the session HIERARCHY (parent + children) for a target,
        // from the registry's parent links. The no-arg form walks from the RESOLVED
        // (gated) session; an EXPLICIT `<sid>` argument walks an ARBITRARY node, so it
        // is Owner-only (a scoped Edge may not enumerate trees it has no edge into) —
        // the `scope` guard mirrors the `sessions` verb's Owner gate.
        "family" => control_session::cmd_family(ctx, store, scope, rest),
        // `ready [timeout_ms]`: block until the target is Alive AND idle (at an
        // OSC-133 prompt, or the kernel idle-settle window), so an agent can chain
        // sessions without busy-polling. Read-side (observes lifecycle/blocks).
        "ready" => control_session::cmd_ready(term, store, session, rest, subscribers),
        // `await <idle|seq|match|block>`: block until the Observation Kernel (L0)
        // latches the predicate. The event-driven, no-silent-loss successor to the
        // OSC-133-only `ready`/`wait`; works for alt-screen agent TUIs (Claude).
        // Registers a subscriber so it wakes on output (content predicates) AND at
        // the idle deadline — no fixed-interval poll.
        "await" => control_session::cmd_await(term, store, session, rest, subscribers),
        "send" => control_input::cmd_send(&ctx.sink, rest),
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
        "key" => control_input::cmd_key(proxy, rest),
        "ctrl" if is_cross => cross_input(
            term,
            ctx,
            parse_ctrl(rest),
            "ERR usage: ctrl <single-letter>\n",
        ),
        "ctrl" => control_input::cmd_ctrl(proxy, rest),
        "feed" => control_input::cmd_feed(&ctx.sink, rest),
        "signal" => control_input::cmd_signal(master, rest),
        "mouse" if is_cross => cross_mouse(term, ctx, session, proxy, rest),
        "mouse" => control_input::cmd_mouse(proxy, rest),
        "paste" if is_cross => cross_input(
            term,
            ctx,
            Some(InputEvent::Paste(control_input::paste_text(rest))),
            "ERR\n",
        ),
        "paste" => control_input::cmd_paste(proxy, rest),
        "focus" if is_cross => match control_input::parse_focus(rest) {
            Some(focused) => cross_input(term, ctx, Some(InputEvent::Focus(focused)), "ERR\n"),
            None => "ERR usage: focus <in|out>\n".to_string(),
        },
        "focus" => control_input::cmd_focus(proxy, rest),
        // `image` rides the shared renderer + event loop, which act on the ACTIVE tab;
        // cross-session pixel capture (offscreen render of a background session) is a
        // later P1.2 deliverable, so it stays fail-closed here rather than silently
        // capturing the WRONG (active) session.
        // `image read [...]` reads the STRUCTURED inline-image payloads from the
        // (target) terminal model — headless-safe and cross-session-correct, so it
        // is matched BEFORE the framebuffer-rasterize arms (which stay fail-closed
        // cross-session). `term` is already the resolved target for cross reads.
        "image" if rest.split_whitespace().next() == Some("read") => control_media::cmd_image_read(
            term,
            rest.strip_prefix("read").unwrap_or(rest).trim_start(),
        ),
        "image" if is_cross => "ERR cross-session image unsupported\n".to_string(),
        "image" => control_media::cmd_image(proxy, queue, rest, sock_dir),
        // `window` captures the FRONT window's ENTIRE on-screen pixels (OS chrome +
        // content) to a PNG — the introspection an AI needs to SEE the whole window,
        // which `image` (terminal-content framebuffer only) cannot. Like `image` it
        // rides the event loop to read AppKit + the window number on the MAIN thread,
        // acting on the ACTIVE/front window; the on-screen window is a window-level
        // (not per-session) surface, so a cross-session `@<sel>` would capture the
        // SAME front window — meaningless. Keep it fail-closed for `@<sel>` like
        // `image`/`chrome`. The confined PNG path is validated EXACTLY like `image`.
        "window" if is_cross => "ERR cross-session window unsupported\n".to_string(),
        "window" => control_media::cmd_window(proxy, rest, sock_dir),
        // `chrome` reports the frontmost window's NATIVE macOS UI (the NSToolbar
        // items + app menu bar). It rides the event loop to read AppKit on the MAIN
        // thread, which acts on the ACTIVE/front window; the chrome (a per-process
        // menu bar + the front window's toolbar) is a window/app-level surface, not a
        // per-session one, so a cross-session `@<sel>` would report the SAME front
        // window's chrome — meaningless. Keep it fail-closed for `@<sel>` like `image`.
        "chrome" if is_cross => "ERR cross-session chrome unsupported\n".to_string(),
        "chrome" => control_media::cmd_chrome(proxy),
        // `tab` DRIVES the FRONT window's native tabs (open/switch/cycle). Like
        // `chrome`/`image` it rides the event loop to mutate `App` on the MAIN thread
        // and is a window-level (not per-session) op, so a cross-session `@<sel>` is
        // meaningless — keep it fail-closed for `@<sel>`.
        "tab" if is_cross => "ERR cross-session tab unsupported\n".to_string(),
        "tab" => control_input::cmd_tab(proxy, rest),
        // Cross-session `resize` does NOT go through the seam: `seam_egress` emits no
        // bytes for `Resize`, and `App::input`'s Resize arm resizes the WINDOW (every
        // tab + the GPU swapchain). A background target has no window to echo to, so we
        // replicate ONLY the term+PTY pair (`echo_to_window: false` semantics) on the
        // TARGET, never the active window/framebuffer.
        "resize" if is_cross => cross_resize(term, master, ctx, rest),
        "resize" => control_input::cmd_resize(proxy, rest),
        // Cross-session `scroll` also bypasses the seam (`ScrollView` emits no bytes;
        // the viewport move lives in `App::input`). It applies the `ScrollIntent`
        // DIRECTLY to the TARGET term's viewport and reports `OK <offset> <max>` — the
        // SAME wire shape as the self path's `cmd_scroll`. `select` is already
        // cross-correct (mutates the target term + fires a repaint keyed by target id).
        "scroll" if is_cross => cross_scroll(term, rest),
        "scroll" => control_input::cmd_scroll(term, proxy, rest),
        "dims" => control_query::cmd_dims(term, cell_size),
        // `metrics` -> live render/latency counters (process-global; the active tab's
        // grid supplies rows/cols). Lets a driving AI MEASURE responsiveness directly
        // rather than scraping the $ATERM_TRACE_LATENCY stderr log. Read-side.
        "metrics" => control_query::cmd_metrics(term, rest),
        // `metric <name> <value>` -> push an app-fed sample (AI token spend, build
        // progress, …) shown by the app-fed HUD panel. Write-class.
        "metric" => control_app_fed::cmd_metric(rest),
        "lines" => control_query::cmd_lines(term),
        "line" => control_query::cmd_line(term, rest),
        "modes" => control_query::cmd_modes(term),
        "title" => control_query::cmd_title(term),
        "cwd" => control_query::cmd_cwd(term),
        "blocks" => control_selection::cmd_blocks(term, rest),
        "blocktext" => control_selection::cmd_blocktext(term, rest),
        "wait" => control_selection::cmd_wait(term, rest),
        "colors" => control_query::cmd_colors(term),
        "select" => control_selection::cmd_select(term, proxy, session, rest),
        "selection" => control_selection::cmd_selection(term),
        "copy" => control_selection::cmd_copy(term),
        // `cast` reads the TARGET session's own asciicast recorder (its recorded
        // program-output history), not the shared renderer, so it is correct
        // cross-session — no `is_cross` guard.
        "cast" => control_session::cmd_cast(ctx),
        // `sessions`/`grant`/`revoke`/`whoami` are handled SELF-SCOPED above.
        _ => "ERR unknown verb\n".to_string(),
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
    if proxy
        .send_event(Wake::Input {
            batch,
            src,
            reply: None,
        })
        .is_err()
    {
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
        .send_event(Wake::Input {
            batch,
            src,
            reply: Some(tx),
        })
        .is_err()
    {
        return Err("ERR event loop closed\n".to_string());
    }
    rx.recv().map_err(|_| "ERR event loop closed\n".to_string())
}

#[cfg(test)]
mod tests {
    use super::control_input::{
        cmd_feed, cmd_send, parse_resize, parse_tab, paste_text, take_mods,
    };
    use super::control_media::{
        MAX_IMAGE_PAYLOAD_BYTES, cmd_image_read, image_payload, image_read_line,
    };
    use super::control_query::{
        AbsRow, abs_row_text, cmd_cell, cmd_colors, cmd_cursor_json, cmd_cwd, cmd_dims_json,
        cmd_line, cmd_modes, cmd_screen_styled_json, cmd_search, cmd_text, cmd_text_json,
        styled_image_json,
    };
    use super::control_selection::{
        cmd_blocks, cmd_blocks_json, cmd_blocktext, cmd_selection, cmd_wait,
    };
    use super::control_session::{
        cmd_cast, cmd_edges, cmd_edges_json, cmd_family, cmd_grant, cmd_ready, cmd_revoke,
        cmd_sessions, cmd_whoami,
    };
    use super::*;
    use crate::TabAction;
    use crate::input::InputEvent;
    use aterm_core::selection::{SelectionSide, SelectionType};
    use aterm_session::sink::SinkWriter;
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
            Some(InputEvent::Key {
                key: Key::Named(Nk::ArrowUp),
                mods: Modifiers::empty(),
                base_layout: None,
                event_type: press
            }),
        );
        assert_eq!(
            parse_key("f5 mods=ctrl"),
            Some(InputEvent::Key {
                key: Key::Named(Nk::F5),
                mods: Modifiers::CTRL,
                base_layout: None,
                event_type: press
            }),
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
            Some(InputEvent::Key {
                key: Key::Character('u'),
                mods: Modifiers::CTRL,
                base_layout: None,
                event_type: press
            }),
        );
        // alt+/shift+/super+ and stacked prefixes.
        assert_eq!(
            parse_key("alt+x"),
            Some(InputEvent::Key {
                key: Key::Character('x'),
                mods: Modifiers::ALT,
                base_layout: None,
                event_type: press
            }),
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
            Some(InputEvent::Key {
                key: Key::Named(Nk::ArrowUp),
                mods: Modifiers::CTRL,
                base_layout: None,
                event_type: press
            }),
        );
        // The literal `+` key (no recognized modifier before it) survives.
        assert_eq!(
            parse_key("+"),
            Some(InputEvent::Key {
                key: Key::Character('+'),
                mods: Modifiers::empty(),
                base_layout: None,
                event_type: press
            }),
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
        let ev = |t| {
            Some(InputEvent::Key {
                key: Key::Named(Nk::ArrowUp),
                mods: Modifiers::empty(),
                base_layout: None,
                event_type: t,
            })
        };
        assert_eq!(parse_key("up"), ev(KeyEventType::Press));
        assert_eq!(parse_key("up type=press"), ev(KeyEventType::Press));
        assert_eq!(parse_key("up type=repeat"), ev(KeyEventType::Repeat));
        assert_eq!(parse_key("up type=release"), ev(KeyEventType::Release));
        assert_eq!(parse_key("up type=up"), ev(KeyEventType::Release));
        // Additive with mods=, position-independent.
        assert_eq!(
            parse_key("type=release mods=ctrl up"),
            Some(InputEvent::Key {
                key: Key::Named(Nk::ArrowUp),
                mods: Modifiers::CTRL,
                base_layout: None,
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
                key: Key::Character('q'),
                mods: Modifiers::empty(),
                base_layout: Some('a'),
                event_type: KeyEventType::Press,
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
        assert!(
            frame.contains("\"underline_style\":\"curly\""),
            "curly underline lost: {frame}"
        );
        assert!(
            frame.contains("\"overline\":true"),
            "overline lost: {frame}"
        );
        assert!(
            frame.contains("\"underline_color\":\"ff0000\""),
            "underline colour lost: {frame}"
        );
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
        assert!(
            frame.contains("\"dims\":{\"rows\":3,\"cols\":10}"),
            "{frame}"
        );
        // 3 rows × 10 cols = 30 cells, each carries exactly one "glyph" key.
        let glyphs = frame.matches("\"glyph\"").count();
        assert_eq!(glyphs, 30, "expected 30 cells with no trim, got {glyphs}");
        assert!(
            frame.contains(&format!("\"seq\":{}", t.content_seq())),
            "{frame}"
        );
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
        assert!(
            frame.contains("\"glyph\":\"e\u{0301}\""),
            "combining grapheme lost: {frame}"
        );
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
        let body = out
            .strip_prefix("OK 1\n")
            .unwrap()
            .strip_suffix('\n')
            .unwrap();
        assert!(
            !body.contains('\n'),
            "styled frame must be single-line JSON: {body}"
        );
        assert!(
            body.starts_with("{\"seq\":") && body.ends_with('}'),
            "{body}"
        );
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
        let small = ImageData {
            bytes: vec![1, 2, 3, 4],
            format: ImageFormat::Png,
            cols: 1,
            rows: 1,
            z_index: 0,
        };
        let (fmt, b64) = image_payload(&small);
        assert_eq!(fmt, "png");
        assert!(!b64.is_empty(), "small image must carry its payload");

        let big = ImageData {
            bytes: vec![0u8; MAX_IMAGE_PAYLOAD_BYTES + 1],
            format: ImageFormat::Png,
            cols: 80,
            rows: 24,
            z_index: 0,
        };
        let (fmt, b64) = image_payload(&big);
        assert_eq!(fmt, "truncated", "oversized image must be marked truncated");
        assert!(b64.is_empty(), "oversized image must NOT be base64-encoded");
        // The line form still reports the real size (so the consumer can decide).
        let line = image_read_line(0, 0, 0, 0, &big);
        assert!(
            line.contains(&format!("truncated {}", MAX_IMAGE_PAYLOAD_BYTES + 1)),
            "{line}"
        );
        // And the JSON form is well-formed with an empty payload + real nbytes.
        let js = styled_image_json(0, 0, &big);
        assert!(
            js.contains("\"format\":\"truncated\"") && js.contains("\"b64\":\"\""),
            "{js}"
        );
        assert!(
            js.contains(&format!("\"nbytes\":{}", MAX_IMAGE_PAYLOAD_BYTES + 1)),
            "{js}"
        );
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
        assert!(
            frame.contains("\"double_width\""),
            "double-width line size lost: {frame}"
        );
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
        assert!(
            pf.contains("\"selection\":null"),
            "no selection -> null: {pf}"
        );
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
        assert_eq!(
            lines.next().unwrap(),
            "OK 1",
            "expected one deduped image: {out}"
        );
        let line = lines.next().unwrap();
        // <row> <col> <img_cols> <img_rows> <cell_row> <cell_col> <format> <nbytes> <b64>
        assert_eq!(line, "0 0 2 1 0 0 png 12 iVBORw0KGgoAAAAA", "got: {line}");
        assert!(
            lines.next().is_none(),
            "image must be deduped to one line: {out}"
        );
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
        assert_eq!(
            out, "OK 1\n0 0 2 1 0 1 png 12 iVBORw0KGgoAAAAA\n",
            "got: {out}"
        );
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
        let confined = std::fs::canonicalize(&dir)
            .unwrap()
            .join("aterm-child.sock");
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
        let splan =
            proxy_forward_plan(&sline, Scope::Owner, &store, &dir).expect("subscribe forwards");
        assert_eq!(splan.0, confined_str);
        assert_eq!(
            splan.1,
            format!("TOKEN {read_hex} subscribe @. cells,bytes every-frame\n")
        );

        // Edge scope cannot escalate to a child.
        assert!(
            proxy_forward_plan(&line, Scope::Edge(EdgeToken::generate()), &store, &dir).is_none(),
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
            "screen",
            "text",
            "cell 0 0",
            "search x",
            "modes",
            "image read",
            "cast",
            "scroll up",
            "key up",
            "ctrl c",
            "feed 03",
            "feed-bin 4",
            "paste hi",
            "resize 10 20",
            "focus in",
            "send hi",
            "signal int", // forwardable (read/write/signal)
            "grant x",
            "revoke x",
            "whoami",
            "sessions",
            "version",
            "bogus", // NOT forwardable
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
        use aterm_types::mouse::{ALT_MASK, MouseButton, SHIFT_MASK};
        // Bare press: empty mods, count 1, left side, simple (block=false).
        assert_eq!(
            parse_mouse("press left 5 9"),
            Ok(InputEvent::MouseButton {
                button: MouseButton::Left,
                pressed: true,
                row: 5,
                col: 9,
                mods: 0,
                click_count: 1,
                side: SelectionSide::Left,
                block: false,
                px_off: crate::input::PixelOffset::CELL_ORIGIN,
            }),
        );
        // Full grammar, tokens in any position.
        assert_eq!(
            parse_mouse("count=2 press left side=right 5 9 mods=shift+alt block=1"),
            Ok(InputEvent::MouseButton {
                button: MouseButton::Left,
                pressed: true,
                row: 5,
                col: 9,
                mods: SHIFT_MASK | ALT_MASK,
                click_count: 2,
                side: SelectionSide::Right,
                block: true,
                px_off: crate::input::PixelOffset::CELL_ORIGIN,
            }),
        );
        // count clamps to 1..=3.
        let Ok(InputEvent::MouseButton { click_count, .. }) = parse_mouse("press left 0 0 count=9")
        else {
            panic!("press parses")
        };
        assert_eq!(click_count, 3);
        // move: bare = hover code 3; with a button = its X10 drag code.
        assert_eq!(
            parse_mouse("move 7 3"),
            Ok(InputEvent::MouseMove {
                buttons: 3,
                row: 7,
                col: 3,
                mods: 0,
                side: SelectionSide::Left,
                px_off: crate::input::PixelOffset::CELL_ORIGIN,
            }),
        );
        let Ok(InputEvent::MouseMove { buttons, .. }) = parse_mouse("move left 7 3") else {
            panic!("drag move parses")
        };
        assert_eq!(buttons, MouseButton::Left.code());
        // wheel actions default to lines=1.
        assert_eq!(
            parse_mouse("wheelup left 2 4"),
            Ok(InputEvent::Wheel {
                dir_up: true,
                lines: 1,
                row: 2,
                col: 4,
                mods: 0,
                px_off: crate::input::PixelOffset::CELL_ORIGIN,
            }),
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
            cast: Arc::new(std::sync::Mutex::new(crate::cast::CastRecorder::new(
                80, 24,
            ))),
            temporal: Arc::new(std::sync::Mutex::new(
                crate::temporal::TemporalRecorder::new(),
            )),
            byte_fanout: Arc::new(crate::cast::ByteFanout::new()),
        });
        let active: ActiveHandle = Arc::new(Mutex::new(ActiveSession {
            term: term_a.clone(),
            master: 11,
            id: 0,
            ctx: ctx.clone(),
        }));

        let (t, m, id, _ctx) = resolve_active(&active);
        assert!(
            Arc::ptr_eq(&t, &term_a) && m == 11 && id == 0,
            "tab 0 active"
        );

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
        use crate::session_store::{SessionHandle, SessionState};
        use aterm_session::sink::SinkWriter;
        use aterm_session::{EdgeTable, LaunchNonce, SessionId};
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
            cast: Arc::new(std::sync::Mutex::new(crate::cast::CastRecorder::new(
                80, 24,
            ))),
            temporal: Arc::new(std::sync::Mutex::new(
                crate::temporal::TemporalRecorder::new(),
            )),
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
        if n > 0 {
            buf[..n as usize].to_vec()
        } else {
            Vec::new()
        }
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

        let self_tuple: Target = (
            h_self.term.clone(),
            h_self.master,
            h_self.local_id,
            h_self.ctx.clone(),
        );

        // Resolve `@2` (the target's local id) EXACTLY as `handle()` does.
        let sel = Selector::parse("2");
        assert!(matches!(sel, Selector::Local(2)), "@2 parses to Local(2)");
        let (term, master, session, ctx) =
            resolve_target(&self_tuple, &store, &sel).expect("@2 resolves");
        assert!(
            Arc::ptr_eq(&term, &h_target.term),
            "resolved the TARGET term"
        );
        assert_eq!(session, 2);
        let _ = master;
        let ctx: &SessionCtx = &ctx;

        // `key enter` cross-session: CR reaches the TARGET sink, nothing to self.
        assert_eq!(cross_input(&term, ctx, parse_key("enter"), "ERR\n"), "OK\n");
        assert_eq!(
            drain_pipe(&target_rx),
            b"\r",
            "key bytes hit the TARGET pty"
        );
        assert!(
            drain_pipe(&self_rx).is_empty(),
            "self pty must be untouched"
        );

        // `feed 03` (Ctrl-C) cross-session: `cmd_feed(&ctx.sink, ..)` is ALREADY the
        // resolved-target path — assert it stays correct alongside the new arms.
        assert_eq!(cmd_feed(&ctx.sink, "03"), "OK 1 bytes\n");
        assert_eq!(
            drain_pipe(&target_rx),
            b"\x03",
            "feed bytes hit the TARGET pty"
        );
        assert!(drain_pipe(&self_rx).is_empty(), "feed must not touch self");

        // `send` (the other always-cross writer) still writes to the resolved sink.
        // `send` is RAW (no implicit CR unless a literal trailing `\n` is given).
        let _ = cmd_send(&ctx.sink, "ls");
        assert_eq!(
            drain_pipe(&target_rx),
            b"ls",
            "send bytes hit the TARGET pty"
        );
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
        assert!(
            reply.starts_with("OK ") && reply.contains("hello"),
            "TARGET selection: {reply}"
        );
        assert!(
            drain_pipe(&target_rx).is_empty(),
            "select must not write pty bytes"
        );
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
            cast: Arc::new(std::sync::Mutex::new(crate::cast::CastRecorder::new(
                80, 24,
            ))),
            temporal: Arc::new(std::sync::Mutex::new(
                crate::temporal::TemporalRecorder::new(),
            )),
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
            assert_eq!(
                (ws.ws_row, ws.ws_col),
                (10, 40),
                "TARGET pty winsize updated"
            );
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
        assert_eq!(
            cross_resize(&term, master, &ctx, "65535 65535"),
            "ERR out of range\n"
        );
        {
            let t = term_lock(&term);
            assert_eq!(
                (t.rows(), t.cols()),
                (10, 40),
                "grid unchanged after reject"
            );
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
        assert!(
            term_lock(term).mouse_tracking_enabled(),
            "DEC 1000 enabled tracking"
        );
        assert_eq!(
            cross_mouse_apply(term, ctx, "press left 1 1"),
            Ok(false),
            "report, no viewport move"
        );
        let report = drain_pipe(&target_rx);
        assert!(
            !report.is_empty() && report.starts_with(b"\x1b["),
            "SGR press report to TARGET: {report:?}"
        );
        assert!(
            drain_pipe(&self_rx).is_empty(),
            "self sink untouched by a cross mouse"
        );

        // ── Tracking OFF (DEC 1000 / 1006 reset) ──
        term_lock(term).process(b"\x1b[?1006l\x1b[?1000l");
        assert!(!term_lock(term).mouse_tracking_enabled(), "tracking reset");
        term_lock(term).scroll_to_bottom();
        assert_eq!(term_lock(term).grid().display_offset(), 0, "at live tail");

        // Wheel-up: scroll_display fallback moves the TARGET viewport into history,
        // emits no pty bytes, and asks for a repaint.
        assert_eq!(
            cross_mouse_apply(term, ctx, "wheelup left 2 4 count=3"),
            Ok(true),
            "wheel => repaint"
        );
        assert!(
            term_lock(term).grid().display_offset() > 0,
            "wheel moved TARGET viewport"
        );
        assert!(
            drain_pipe(&target_rx).is_empty(),
            "wheel fallback emits no pty bytes"
        );

        // A plain press under tracking-off: deliberate no-op (no selection UI for a
        // background tab) — sink empty, offset unchanged, no repaint.
        let off_before = term_lock(term).grid().display_offset();
        assert_eq!(
            cross_mouse_apply(term, ctx, "press left 1 1"),
            Ok(false),
            "press no-op"
        );
        assert!(
            drain_pipe(&target_rx).is_empty(),
            "press fallback emits no pty bytes"
        );
        assert_eq!(
            term_lock(term).grid().display_offset(),
            off_before,
            "press did not move viewport"
        );
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
        assert!(
            term_lock(&h_target.term).grid().display_offset() > 0,
            "viewport moved into history"
        );
        assert!(drain_pipe(&rx).is_empty(), "scroll emits no pty bytes");
        // `scroll bottom` returns to the live tail (offset 0).
        let _ = cross_scroll(&h_target.term, "bottom");
        assert_eq!(
            term_lock(&h_target.term).grid().display_offset(),
            0,
            "back to live bottom"
        );
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
            cast: Arc::new(std::sync::Mutex::new(crate::cast::CastRecorder::new(
                80, 24,
            ))),
            temporal: Arc::new(std::sync::Mutex::new(
                crate::temporal::TemporalRecorder::new(),
            )),
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
            tbl.grant(
                SessionId::new("s-test-controller"),
                ctx.self_id.clone(),
                op,
                ctx.nonce,
            )
        };
        Scope::Edge(tok)
    }

    /// `required_op` is the single source of truth for which `Op` each verb needs;
    /// the design 7.2 read != write != signal split must hold exactly.
    #[test]
    fn required_op_classifies_each_verb() {
        // Read-side: observers + the controller's own view-state controls.
        let read_verbs = [
            "text",
            "cursor",
            "cell",
            "search",
            "dims",
            "lines",
            "line",
            "modes",
            "title",
            "cwd",
            "blocks",
            "blocktext",
            "wait",
            "colors",
            "selection",
            "copy",
            "scroll",
            "select",
            "image",
            "cast",
        ];
        for v in read_verbs {
            assert_eq!(required_op(v), Some(Op::ReadScreen), "{v} read");
        }
        // Write-side: bytes/geometry the driven program observes (`feed-bin` is the
        // binary twin of `feed`, classified here for the cross-process forward).
        for v in [
            "send", "key", "ctrl", "feed", "feed-bin", "mouse", "paste", "resize", "focus",
            "metric",
        ] {
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
        assert!(
            !gate_allows(edge_read, "signal", &ctx),
            "read edge: NOT signal"
        );
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
        assert!(
            !gate_allows(edge_write, "text", &ctx),
            "write edge: NOT read"
        );
        assert!(
            !gate_allows(edge_write, "signal", &ctx),
            "write edge: NOT signal"
        );
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
            cmd_whoami(&ctx_b, edge_b)
                .trim_end()
                .ends_with("edge read-screen"),
            "whoami on granted session B",
        );
        // After the active handle swings to A, the SAME token authorizes nothing.
        assert!(
            cmd_whoami(&ctx_a, edge_b)
                .trim_end()
                .ends_with("edge unauthorized"),
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
        let hex = reply
            .strip_prefix("OK ")
            .and_then(|s| s.strip_suffix('\n'))
            .expect("OK <hex>");
        assert_eq!(hex.len(), 64, "edge token is 64 hex chars");

        // The bearer presents it as the handshake hex => resolves to Edge(ReadScreen).
        let line = format!("AUTH {hex}");
        let (op, _tok, inline) =
            edge_scope_from_first_line(&line, &ctx).expect("edge authenticates");
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
        let n: usize = hdr_line
            .strip_prefix("OK ")
            .expect("OK prefix")
            .parse()
            .expect("nbytes");
        assert_eq!(n, body.len(), "framed length must equal the body length");
        let header = body.lines().next().expect("a header line");
        assert!(
            header.contains("\"version\": 2"),
            "not asciicast v2: {header}"
        );

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
        assert!(
            body2.lines().count() >= 2,
            "expected header + >=1 event: {body2}"
        );
        let event = body2.lines().nth(1).unwrap();
        assert!(
            event.starts_with('[') && event.contains("\"o\""),
            "bad event: {event}"
        );
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
        assert!(
            out.contains("exit=0") && out.contains("cmdline=echo%20hi"),
            "block 1 wrong: {out}"
        );
        assert!(
            out.contains("exit=1") && out.contains("cmdline=false"),
            "block 2 wrong: {out}"
        );
        // `blocks 1` returns only the most recent (the failed one).
        let last = cmd_blocks(&term, "1");
        assert!(
            last.starts_with("OK 1\n") && last.contains("exit=1"),
            "last block wrong: {last}"
        );
        // `blocktext 0` reads block 0's OUTPUT directly (no coordinate math).
        let txt = cmd_blocktext(&term, "0");
        assert!(
            txt.starts_with("OK ") && txt.contains("hi"),
            "block 0 output wrong: {txt}"
        );
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
        assert!(
            !plain.contains("link="),
            "plain cell has a stray link: {plain}"
        );
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
        assert_eq!(
            parse_resize(""),
            Err("ERR usage: resize <r> <c>\n".to_string())
        );
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
        assert_eq!(
            parse_tab("move 2 0"),
            Some(TabAction::Move { from: 2, to: 0 })
        );
        assert_eq!(
            parse_tab("move 0 3"),
            Some(TabAction::Move { from: 0, to: 3 })
        );
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
            pct_decode(accent_tok),
            accent_sel,
            "cell grapheme must equal selection (accent): {accent_cell}"
        );
        let family_cell = cmd_cell(&term, "1 0");
        let family_tok = family_cell
            .strip_prefix("OK ")
            .and_then(|s| s.split(' ').next())
            .expect("cell OK token");
        assert_eq!(
            pct_decode(family_tok),
            family_sel,
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
        assert_eq!(
            lines[1], accent_sel,
            "text row 0 must equal selection: {text}"
        );
        assert_eq!(
            lines[2], family_sel,
            "text row 1 must equal selection: {text}"
        );

        // ---- search verb: searching the cluster finds it, and the located cell
        // reads back (via cell) the same grapheme the selection shows.
        let s = cmd_search(&term, "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}");
        assert!(
            s.starts_with("OK 1"),
            "search must find the ZWJ family once: {s}"
        );
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
            term.lock()
                .unwrap()
                .process(format!("filler line {i}\r\n").as_bytes());
        }
        // The needle is no longer on the visible 4-row screen.
        let visible = cmd_text(&term);
        assert!(
            !visible.contains("NEEDLE_alpha"),
            "needle should have scrolled off-screen: {visible}"
        );
        // But search (which indexes scrollback) finds it.
        let s = cmd_search(&term, "NEEDLE_alpha");
        assert!(
            s.starts_with("OK 1"),
            "scrolled-off needle must be found: {s}"
        );
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
        assert!(
            count >= 2,
            "regex `fill[a-z]+` should match many filler rows: {rx}"
        );

        // Case sensitivity: default is insensitive, `case` flips it.
        let ci = cmd_search(&term, "needle_alpha");
        assert!(
            ci.starts_with("OK 1"),
            "case-insensitive default must match: {ci}"
        );
        let cs = cmd_search(&term, "needle_alpha case");
        assert!(
            cs.starts_with("OK 0"),
            "case-sensitive must NOT match lowercased: {cs}"
        );
    }

    // ---- P1.2: cross-session @selector addressing --------------------------------

    use crate::session_store::{self, SessionHandle, SessionState};
    use aterm_session::{EdgeTable, LaunchNonce, decide_edge};

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
            cast: Arc::new(std::sync::Mutex::new(crate::cast::CastRecorder::new(
                80, 24,
            ))),
            temporal: Arc::new(std::sync::Mutex::new(
                crate::temporal::TemporalRecorder::new(),
            )),
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
            let (t, m, id, _ctx) =
                resolve_target(&self_tuple, &store, &sel).expect("self resolves");
            assert!(
                Arc::ptr_eq(&t, &self_h.term),
                "@{body} is the same Arc as self"
            );
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
        let by_local =
            resolve_target(&self_tuple, &store, &Selector::parse("7")).expect("by local");
        assert!(
            Arc::ptr_eq(&by_local.0, &peer_h.term),
            "resolved the peer term"
        );
        assert_eq!(
            cmd_text(&by_local.0),
            peer_text,
            "@7 returns the PEER's state"
        );
        assert_ne!(cmd_text(&by_local.0), self_text, "@7 is NOT self's state");

        // By stable SessionId.
        let by_sid = resolve_target(&self_tuple, &store, &Selector::parse(peer_h.sid.as_str()))
            .expect("by sid");
        assert!(
            Arc::ptr_eq(&by_sid.0, &peer_h.term),
            "@s-... resolved the peer term"
        );
        assert_eq!(
            cmd_text(&by_sid.0),
            peer_text,
            "@s-... returns the PEER's state"
        );

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
            tbl.grant(
                src.clone(),
                peer_h.ctx.self_id.clone(),
                Op::ReadScreen,
                peer_h.ctx.nonce,
            )
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
            decide_edge(
                &tbl,
                &granted,
                &peer_h.ctx.self_id,
                Op::ReadScreen,
                &restarted_nonce
            ),
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
        assert!(
            cross_session_authorized(Scope::Owner, "send", &peer_h.ctx),
            "owner write ok"
        );
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
        assert_eq!(
            buf, b"echo-into-peer",
            "exactly the authorized write reached the PEER"
        );
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
        assert!(
            child_line.contains(root.sid.as_str()),
            "child names its parent sid"
        );
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
        s.set_read_timeout(Some(std::time::Duration::from_millis(200)))
            .unwrap();
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
        s.set_read_timeout(Some(std::time::Duration::from_millis(300)))
            .unwrap();
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
            run_subscribe(
                "subscribe @. screen",
                &active_t,
                &store_t,
                &reg_t,
                Scope::Owner,
                &mut w,
            );
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
        assert!(
            frame.contains("DELTA 0 seq="),
            "screen delta pushed: {frame:?}"
        );
        assert!(
            frame.contains("hello-live"),
            "delta carries live text: {frame:?}"
        );

        // A PURE viewport scroll does not bump content_seq -> no further DELTA even
        // though we notify (a coalesced/spurious wake reads unchanged content).
        crate::term_lock(&h.term).scroll_display(1);
        registry.lock().unwrap().notify(0);
        let none = read_quiet(&client);
        assert!(
            !none.contains("DELTA"),
            "viewport scroll pushes no delta: {none:?}"
        );

        // Drop the client: the loop's next write fails and it returns (deregister).
        drop(client);
        crate::term_lock(&h.term).process(b"x");
        registry.lock().unwrap().notify(0);
        join.join()
            .expect("push loop ends cleanly on a dead client");
        assert_eq!(
            registry.lock().unwrap().watched_sessions(),
            0,
            "deregistered on drop"
        );
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
        run_subscribe(
            "subscribe @2 screen",
            &active,
            &store,
            &registry,
            scope,
            &mut out,
        );
        assert_eq!(
            String::from_utf8_lossy(&out),
            "ERR denied\n",
            "cross subscribe fail-closed"
        );
        assert_eq!(
            registry.lock().unwrap().watched_sessions(),
            0,
            "no registration on denial"
        );
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
        run_subscribe(
            "subscribe @. screen",
            &active,
            &store,
            &registry,
            scope,
            &mut out,
        );
        assert_eq!(
            String::from_utf8_lossy(&out),
            "ERR denied\n",
            "B's edge must NOT subscribe to the swung-to active session A",
        );
        assert_eq!(
            registry.lock().unwrap().watched_sessions(),
            0,
            "no registration on denial"
        );
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
            edges.grant(
                self_h.sid.clone(),
                sib.sid.clone(),
                Op::ReadScreen,
                sib.nonce,
            )
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
            run_subscribe(
                "subscribe @2 screen",
                &active_t,
                &store_t,
                &reg_t,
                scope,
                &mut w,
            );
        });

        let ack = read_until(&client, String::new(), |s| s.contains("OK subscribe 1\n"));
        assert!(
            ack.contains("OK subscribe 1\n"),
            "edge subscribe authorized: {ack:?}"
        );

        crate::term_lock(&sib.term).process(b"from-sibling");
        registry.lock().unwrap().notify(2);
        let frame = read_until(&client, ack, |s| s.contains("from-sibling"));
        assert!(
            frame.contains("DELTA 2 seq="),
            "sibling delta tagged with its sid: {frame:?}"
        );
        assert!(
            frame.contains("from-sibling"),
            "carries the sibling's screen: {frame:?}"
        );

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
            run_subscribe(
                "subscribe @0,@2 screen",
                &active_t,
                &store_t,
                &reg_t,
                Scope::Owner,
                &mut w,
            );
        });
        let ack = read_until(&client, String::new(), |s| s.contains("OK subscribe 2\n"));
        assert!(
            ack.contains("OK subscribe 2\n"),
            "two targets acked: {ack:?}"
        );

        crate::term_lock(&a.term).process(b"AAA");
        crate::term_lock(&b.term).process(b"BBB");
        registry.lock().unwrap().notify(0);
        registry.lock().unwrap().notify(2);
        // Accumulate until BOTH sids' deltas (with their text) have shown up.
        let seen = read_until(&client, ack, |s| s.contains("AAA") && s.contains("BBB"));
        assert!(
            seen.contains("DELTA 0 ") && seen.contains("AAA"),
            "sid 0 frame: {seen:?}"
        );
        assert!(
            seen.contains("DELTA 2 ") && seen.contains("BBB"),
            "sid 2 frame: {seen:?}"
        );

        drop(client);
        crate::term_lock(&a.term).process(b"x");
        registry.lock().unwrap().notify(0);
        join.join()
            .expect("multiplex push loop ends on a dead client");
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
        assert!(
            after > before,
            "producer content_seq advanced past a stalled subscriber"
        );
        assert!(
            start.elapsed() < std::time::Duration::from_secs(5),
            "producer not blocked"
        );
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
        term.lock()
            .unwrap()
            .process(b"line-zero\r\nsecond \"quoted\"");

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
        assert!(
            body.contains(&format!("\"{}\"", text_rows[0])),
            "row0 present: {body}"
        );
        // The quote in row 1 is escaped in JSON, NOT in text.
        assert!(
            text_rows[1].contains("second \"quoted\""),
            "text keeps raw quote"
        );
        assert!(
            body.contains("second \\\"quoted\\\""),
            "json escapes the quote: {body}"
        );

        // cursor + dims + seq members are present and consistent with the verbs.
        let c = term.lock().unwrap().cursor();
        assert!(
            body.contains(&format!("\"row\":{}", c.row)),
            "cursor row: {body}"
        );
        assert!(
            body.contains("\"dims\":{\"rows\":24,\"cols\":80}"),
            "dims: {body}"
        );
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
        assert!(
            cbody.contains("\"row\":0") && cbody.contains("\"style\":\"blinking_block\""),
            "{cbody}"
        );

        // dims: rows/cols/pixels (cell size (8,16) for the test).
        let dj = cmd_dims_json(&term, (8, 16));
        let dbody = dj.strip_prefix("OK 1\n").unwrap().trim_end();
        assert_balanced_json(dbody);
        assert!(
            dbody.contains("\"rows\":24,\"cols\":80,\"pixel_w\":640,\"pixel_h\":384"),
            "{dbody}"
        );

        // blocks: two OSC-133 blocks -> a `blocks` array; absent rows are JSON null.
        term.lock().unwrap().process(
            b"\x1b]133;A\x07$ \x1b]633;E;echo hi\x07\x1b]133;B\x07echo hi\n\x1b]133;C\x07hi\n\x1b]133;D;0\x07",
        );
        let bj = cmd_blocks_json(&term, "");
        let bbody = bj.strip_prefix("OK 1\n").unwrap().trim_end();
        assert_balanced_json(bbody);
        assert!(bbody.contains("\"blocks\":[{"), "blocks array: {bbody}");
        assert!(
            bbody.contains("\"exit\":0") && bbody.contains("\"cmdline\":\"echo hi\""),
            "{bbody}"
        );
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
            tbl.grant(
                SessionId::new("s-src-a"),
                ctx.self_id.clone(),
                Op::ReadScreen,
                ctx.nonce,
            );
            tbl.grant(
                SessionId::new("s-src-b"),
                ctx.self_id.clone(),
                Op::WriteInput,
                ctx.nonce,
            )
        };
        let out = cmd_edges(&ctx);
        let mut lines = out.lines();
        assert_eq!(
            lines.next(),
            Some("OK 2"),
            "header counts both edges: {out}"
        );
        // Sorted by (src, op): s-src-a read-screen, then s-src-b write-input.
        let l1 = lines.next().unwrap();
        let l2 = lines.next().unwrap();
        assert!(
            l1.starts_with("s-src-a ") && l1.ends_with(" read-screen"),
            "edge 1: {l1}"
        );
        assert!(
            l2.starts_with("s-src-b ") && l2.ends_with(" write-input"),
            "edge 2: {l2}"
        );
        // The dst column is always THIS session.
        assert!(l1.contains(ctx.self_id.as_str()), "dst is self: {l1}");
        // The secret token NEVER appears in the listing.
        assert!(
            !out.contains(&tok.to_hex()),
            "edge token must not leak: {out}"
        );

        // JSON form: same triples, balanced, no token.
        let j = cmd_edges_json(&ctx);
        let body = j.strip_prefix("OK 1\n").unwrap().trim_end();
        assert_balanced_json(body);
        assert!(
            body.contains("\"src\":\"s-src-a\"") && body.contains("\"op\":\"read-screen\""),
            "{body}"
        );
        assert!(
            !body.contains(&tok.to_hex()),
            "json must not leak the token: {body}"
        );
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
        assert!(
            self_line.starts_with(&format!("self {} ", root.sid.as_str())),
            "self: {self_line}"
        );
        assert_eq!(
            lines.next(),
            Some("parent - - -"),
            "root has no parent: {out}"
        );
        let kids: Vec<&str> = lines.collect();
        assert_eq!(kids.len(), 2, "two children: {out}");
        assert!(
            kids[0].starts_with(&format!("child {} ", child_a.sid.as_str())),
            "child a: {out}"
        );
        assert!(
            kids[1].starts_with(&format!("child {} ", child_b.sid.as_str())),
            "child b: {out}"
        );

        // Explicit `<sid>` of a child (Owner): self=child, parent=root, no children.
        let cout = cmd_family(&root_ctx, &store, Scope::Owner, child_a.sid.as_str());
        assert!(
            cout.contains(&format!("self {} ", child_a.sid.as_str())),
            "child self: {cout}"
        );
        assert!(
            cout.contains(&format!("parent {} ", root.sid.as_str())),
            "child parent=root: {cout}"
        );
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
        assert!(
            cmd_family(&root_ctx, &store, edge, "").starts_with("OK\n"),
            "no-arg edge ok"
        );

        // An unknown sid fails closed.
        assert_eq!(
            cmd_family(&root_ctx, &store, Scope::Owner, "s-nope"),
            "ERR no such session\n"
        );
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
        assert!(matches!(
            parse_feed_bin("@7 feed-bin 10"),
            Some((Some(Selector::Local(7)), 10))
        ));
        assert!(is_feed_bin_line("@7 feed-bin 10"));
        // Not feed-bin.
        assert!(!is_feed_bin_line("feed 0a"));
        assert!(!is_feed_bin_line("@7 feed 0a"));
        // Malformed: missing length, non-numeric, oversize -> None (fail closed).
        assert!(parse_feed_bin("feed-bin").is_none());
        assert!(parse_feed_bin("feed-bin xx").is_none());
        assert!(parse_feed_bin(&format!("feed-bin {}", MAX_FEED_BIN + 1)).is_none());
        // Exactly the cap is allowed.
        assert!(
            matches!(parse_feed_bin(&format!("feed-bin {MAX_FEED_BIN}")), Some((None, n)) if n == MAX_FEED_BIN)
        );
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
        assert_eq!(
            String::from_utf8_lossy(&out),
            "OK 3 bytes\n",
            "reply framing"
        );

        // The raw bytes (incl. the NUL) reached the TARGET's PTY verbatim.
        assert_eq!(
            drain_pipe(&self_rx),
            b"\x00\x01\x02",
            "raw binary bytes hit the pty"
        );

        // The stream is still framed: the NEXT request line is intact.
        let next = read_request_line(&mut reader).expect("the following line survives");
        assert_eq!(
            next, "next-line",
            "stream stays framed past the binary payload"
        );
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
        assert_eq!(
            String::from_utf8_lossy(&out),
            "ERR denied\n",
            "read edge denied feed-bin"
        );
        // No bytes reached the pty.
        assert!(
            drain_pipe(&self_rx).is_empty(),
            "denied feed-bin writes nothing"
        );
        // But the 3 payload bytes WERE consumed: the next line is correctly framed.
        let next = read_request_line(&mut reader).unwrap();
        assert_eq!(
            next, "after",
            "denial still consumes the payload (stream framed)"
        );
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
        assert_eq!(
            next, "after",
            "denial still consumes the payload (stream framed)"
        );
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
        assert!(
            String::from_utf8_lossy(&out).starts_with("ERR usage"),
            "bad length usage: {out:?}"
        );
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
        assert_eq!(
            read_request_line(&mut reader).as_deref(),
            Some("last-no-nl"),
            "EOF yields the tail"
        );
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
        assert_eq!(
            cmd_ready(term, &store, 0, "2000", &subscribe::new_registry()),
            "OK ready prompt\n",
            "fresh prompt is ready"
        );

        // An executing command -> NOT ready -> a short timeout returns `OK timeout`.
        term.lock()
            .unwrap()
            .process(b"\x1b]133;B\x07sleep\n\x1b]133;C\x07");
        assert_eq!(
            cmd_ready(term, &store, 0, "0", &subscribe::new_registry()),
            "OK timeout\n",
            "executing is not ready"
        );

        // The command completes -> Complete -> ready again (prompt-end).
        term.lock().unwrap().process(b"\x1b]133;D;0\x07");
        assert_eq!(
            cmd_ready(term, &store, 0, "2000", &subscribe::new_registry()),
            "OK ready prompt\n",
            "completed is ready"
        );

        // A session marked Exited never becomes ready -> ERR exited (fail closed).
        store
            .write()
            .unwrap()
            .set_state(0, session_store::SessionState::Exited);
        assert_eq!(
            cmd_ready(term, &store, 0, "2000", &subscribe::new_registry()),
            "ERR exited\n",
            "exited fails closed"
        );
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
        assert_eq!(
            cmd_ready(&h.term, &store, 0, "2000", &subscribe::new_registry()),
            "OK ready idle\n",
            "idle plain shell"
        );
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
        assert!(
            gate_allows(edge_b, "send", &ctx_b),
            "edge drives its granted session B"
        );

        // After the active handle SWINGS to A, the SAME edge is DENIED on the SELF
        // path — it holds no grant against A. (Pre-fix this passed on op-match alone.)
        assert!(
            !gate_allows(edge_b, "send", &ctx_a),
            "edge must NOT drive swung-to session A"
        );
        assert!(
            !gate_allows(edge_b, "resize", &ctx_a),
            "incl. resize (whole-window effect)"
        );

        // Owner is unaffected — full self-power against whichever session is active.
        assert!(
            gate_allows(Scope::Owner, "send", &ctx_a),
            "owner drives the active session"
        );
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
