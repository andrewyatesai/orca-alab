// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! Cross-process `@grandchild` PROXY forward (Item 5b) — the layer that turns the
//! per-process control socket into a UNIFIED address space spanning the recursion
//! tree, so an outer aterm can reach a session inside an inner aterm it spawned.
//!
//! ## How a hop works
//!
//! When `handle`/`serve` cannot resolve a `@<sid>` selector in THIS process's
//! store, it consults the [`ProxyTable`] — the per-op capability tokens this aterm
//! minted for each child when it spawned it (Item 4's `ChildProvision`). The
//! child's live socket path is discovered from the on-disk graph entry the inner
//! aterm wrote at bind time ([`read_graph_entry`]). The forward then:
//!
//! 1. `UnixStream::connect`s the child's socket,
//! 2. presents `TOKEN <edge-hex> <rewritten-verb>` — the child authorizes it
//!    against the edges it installed from its injected env (Item 4), so the op the
//!    parent granted is exactly the op the verb needs, and
//! 3. RELAYS bytes transparently in both directions until either side closes.
//!
//! The relay is format-agnostic — it never parses framing — so it carries the
//! styled `screen` JSON, the `subscribe cells/bytes` push streams, `feed-bin`
//! binary payloads, and every other verb verbatim. Authority is the parent's
//! per-op edge over the child it spawned (presented on the dial), so the child
//! authorizes the EXACT op the verb needs.
//!
//! ## Scope: one hop
//!
//! The shipped path forwards DIRECT children only — the child's own selector is
//! inlined to `@.` so it runs the verb on itself, and a child is never in its own
//! proxy table, so no cycle can form. Transitive `@<grandchild>` forwarding (which
//! would need a `via=<n>` hop guard) is NOT implemented; a grandchild selector
//! simply does not resolve here and falls through to a local `ERR no such session`.
//!
//! ## Identity binding
//!
//! The tokens bind to the child's launch NONCE (recorded in the graph entry): a
//! child relaunch under a fresh nonce makes the graph nonce mismatch the table
//! the parent retained, so a stale forward fails closed at discovery rather than
//! dialing a re-launched stranger.

use std::collections::HashMap;
use std::io::{BufRead, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::{Arc, RwLock};

use aterm_session::{EdgeToken, LaunchNonce, Op, SessionId};

/// The capability this aterm holds over ONE child it spawned: the child's launch
/// nonce (to validate the graph entry) plus the three per-op edge tokens minted
/// at spawn (Item 4). The child's socket PATH is not stored here — it is
/// discovered live from the graph entry, since the child binds it only once it
/// (an inner aterm) actually starts.
#[derive(Clone)]
pub struct ProxyEntry {
    pub nonce: LaunchNonce,
    pub read: EdgeToken,
    pub write: EdgeToken,
    pub signal: EdgeToken,
}

impl ProxyEntry {
    /// The edge token to present for `op` (read/write/signal). `DeriveLoop` has no
    /// provisioned edge, so it is refused (returns the read token's `None` peer).
    #[must_use]
    pub fn token_for(&self, op: Op) -> Option<&EdgeToken> {
        match op {
            Op::ReadScreen => Some(&self.read),
            Op::WriteInput => Some(&self.write),
            Op::Signal => Some(&self.signal),
            _ => None,
        }
    }
}

/// This aterm's map of spawned children → the capability it holds over each.
/// Shared between the spawn path (which inserts) and the control server (which
/// reads to forward). Empty until this aterm spawns a child.
pub type ProxyTable = Arc<RwLock<HashMap<SessionId, ProxyEntry>>>;

/// A fresh, empty proxy table.
#[must_use]
pub fn new_proxy_table() -> ProxyTable {
    Arc::new(RwLock::new(HashMap::new()))
}

/// The process-wide proxy table: ONE per aterm process (the spawn path inserts a
/// child's capability; the control server reads it to forward). A singleton avoids
/// threading the handle through every `spawn_session`/`serve` caller; correctness-
/// wise a process has exactly one recursion fabric.
static PROXIES: std::sync::OnceLock<ProxyTable> = std::sync::OnceLock::new();

/// The process-wide [`ProxyTable`] (lazily initialized, cloned Arc).
#[must_use]
pub fn proxies() -> ProxyTable {
    PROXIES.get_or_init(new_proxy_table).clone()
}

/// Record the capability this aterm holds over a child it just spawned.
pub fn register_child(child: SessionId, entry: ProxyEntry) {
    proxies()
        .write()
        .unwrap_or_else(|p| p.into_inner())
        .insert(child, entry);
}

/// Look up the capability for a child by session id (cloned out).
#[must_use]
pub fn lookup_child(sid: &SessionId) -> Option<ProxyEntry> {
    proxies()
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .get(sid)
        .cloned()
}

/// Drop the capability for a child (its session closed) so the process-wide table
/// does not grow for the process lifetime as tabs open and close.
pub fn deregister_child(child: &SessionId) {
    proxies()
        .write()
        .unwrap_or_else(|p| p.into_inner())
        .remove(child);
}

/// The graph-entry filename for a child session id, under `<sock_dir>/graph/`.
fn graph_path(sock_dir: &Path, sid: &SessionId) -> std::path::PathBuf {
    sock_dir.join("graph").join(sid.as_str())
}

/// Write the discovery graph entry an inner aterm publishes at bind time so its
/// parent can reach it: `<sock_dir>/graph/<self-sid>` (0600) with two lines
/// `sock <abs-path>\nnonce <hex>\n`. Edge tokens are NEVER written here — they
/// travel only via the injected env. Best-effort: a write failure just means the
/// parent cannot reach us by proxy (direct per-instance reach still works).
pub fn write_graph_entry(sock_dir: &Path, sid: &SessionId, sock_path: &str, nonce: &LaunchNonce) {
    use std::os::unix::fs::OpenOptionsExt;
    let dir = sock_dir.join("graph");
    // 0700 + owner-verified, like the sibling `images/` subdir (control_auth).
    if crate::control_auth::ensure_private_dir(&dir).is_err() {
        return;
    }
    let path = graph_path(sock_dir, sid);
    let body = format!("sock {sock_path}\nnonce {}\n", nonce.to_hex());
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&path)
    {
        let _ = f.write_all(body.as_bytes());
    }
}

/// Remove this session's graph entry (best-effort) on graceful exit so a dead
/// session's socket path is not left for a parent to dial. (A leftover is harmless
/// anyway — the nonce guard fails a stale dial closed — so this is hygiene.)
pub fn remove_graph_entry(sock_dir: &Path, sid: &SessionId) {
    let _ = std::fs::remove_file(graph_path(sock_dir, sid));
}

/// Sweep dead discovery entries: remove any `graph/<sid>` whose recorded socket no
/// longer has a live listener — a crashed session that never ran its graceful
/// `remove_graph_entry`. Mirrors `control_auth::sweep_stale_instances` for the
/// sibling per-instance files; best-effort (the nonce guard already fails a stale
/// dial closed, so this only keeps the dir bounded). Called at spawn.
pub fn sweep_stale_graph(sock_dir: &Path) {
    let Ok(entries) = std::fs::read_dir(sock_dir.join("graph")) else {
        return;
    };
    for ent in entries.flatten() {
        let path = ent.path();
        if let Ok(body) = std::fs::read_to_string(&path) {
            if let Some((sock, _nonce)) = parse_graph_entry(&body) {
                if !crate::control_auth::socket_is_live(&sock) {
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
    }
}

/// Write the parent→child edge-token SECRETS to a 0600 file under
/// `<sock_dir>/edges/<child-sid>` (audit finding F1) and return its absolute path,
/// or `None` if the private dir / file cannot be created. The bearer tokens live
/// ONLY here (a 0600 file in the 0700 socket dir), never in inheritable env — so a
/// same-uid peer that cannot read 0600 files cannot obtain them. Three lines:
/// `read <hex>` / `write <hex>` / `signal <hex>`.
///
/// LIFECYCLE (F1, revised): the file PERSISTS for the parent session — it is NOT
/// consumed on the child's first read ([`read_edge_tokens`] is now repeatable).
/// The reason is the SAME-SHELL relaunch case: the child inherits the file PATH in
/// `ATERM_EDGE_TOKENS` pinned in its shell env, so a child aterm that exits and is
/// re-launched in the same shell must be able to re-read the same secrets to
/// re-install the parent edges; a consume-on-read deleted the file after the first
/// launch, breaking every subsequent relaunch (the outer's `@child` proxy answered
/// `ERR auth`). The secret window therefore widens from "write→first-read" to the
/// parent's session lifetime — which matches the EXISTING per-launch AUTH token
/// file (`aterm-<pid>.token`), also 0600 in the same 0700 same-uid dir for the
/// whole session, so the trust boundary (same-uid + 0600) is unchanged. The PARENT
/// owns the file and removes it on session/child teardown ([`remove_edge_tokens`]);
/// crash leftovers are swept at the next spawn ([`sweep_stale_edges`]). Inheritance
/// across a NEW aterm hop is still blocked — `ATERM_EDGE_TOKENS` stays deny-listed,
/// so only a same-shell relaunch (which re-inherits the pinned path) re-reads it.
pub fn write_edge_tokens(
    sock_dir: &Path,
    child_sid: &SessionId,
    read_hex: &str,
    write_hex: &str,
    signal_hex: &str,
) -> Option<String> {
    use std::os::unix::fs::OpenOptionsExt;
    let dir = sock_dir.join("edges");
    if crate::control_auth::ensure_private_dir(&dir).is_err() {
        return None;
    }
    let path = dir.join(child_sid.as_str());
    let body = format!("read {read_hex}\nwrite {write_hex}\nsignal {signal_hex}\n");
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&path)
        .ok()?;
    f.write_all(body.as_bytes()).ok()?;
    Some(path.to_string_lossy().into_owned())
}

/// Read the three edge-token hexes `(read, write, signal)` from the 0600 file at
/// `path` (written by [`write_edge_tokens`]), or `None` if absent / malformed.
/// Same-uid + 0600 is the access gate (the path is non-secret; the file is not).
///
/// REPEATABLE (F1, revised): this read is non-destructive and may run any number
/// of times for the parent session's lifetime — a child re-launched in the SAME
/// shell re-reads the same file to re-install the parent edges. The parent owns the
/// file's removal ([`remove_edge_tokens`] on teardown, [`sweep_stale_edges`] for
/// crash leftovers); the reader never deletes it.
pub fn read_edge_tokens(path: &str) -> Option<(String, String, String)> {
    let body = std::fs::read_to_string(path).ok()?;
    let (mut r, mut w, mut s) = (None, None, None);
    for line in body.lines() {
        if let Some(v) = line.strip_prefix("read ") {
            r = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("write ") {
            w = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("signal ") {
            s = Some(v.trim().to_string());
        }
    }
    Some((r?, w?, s?))
}

/// The edge-token filename for a child session id, under `<sock_dir>/edges/`.
fn edge_path(sock_dir: &Path, child_sid: &SessionId) -> std::path::PathBuf {
    sock_dir.join("edges").join(child_sid.as_str())
}

/// Remove the parent→child edge-token file the PARENT wrote ([`write_edge_tokens`])
/// once the spawned child session is torn down — the parent owns the file (it lives
/// in the parent's own 0700 socket dir) and is responsible for its removal, since
/// the file now PERSISTS for the session rather than being consumed on the child's
/// first read (so a same-shell child relaunch can re-read it). Best-effort, run on
/// graceful session/child teardown ([`crate::main`]'s `Session::drop`).
///
/// There is deliberately NO liveness-based sweep: a freshly-provisioned child has no
/// discovery entry UNTIL it launches (often much later, or never), so "no live graph
/// entry" cannot distinguish a still-needed fresh file from an orphan — a sweep on
/// that signal would clobber the very file a not-yet-launched (or about-to-relaunch)
/// child must read. A file orphaned by a CRASHED parent is cryptographically inert:
/// its tokens authorize only against the dead child's exact `(sid, nonce)`, both
/// random and never reissued, so a leftover can never authorize anything again.
pub fn remove_edge_tokens(sock_dir: &Path, child_sid: &SessionId) {
    let _ = std::fs::remove_file(edge_path(sock_dir, child_sid));
}

/// Read a child's discovery entry: `(sock_path, nonce)` or `None` if absent /
/// malformed. PURE parse split out for testing.
pub fn read_graph_entry(sock_dir: &Path, sid: &SessionId) -> Option<(String, LaunchNonce)> {
    let body = std::fs::read_to_string(graph_path(sock_dir, sid)).ok()?;
    parse_graph_entry(&body)
}

/// Parse a graph-entry body (`sock <path>\nnonce <hex>\n`).
fn parse_graph_entry(body: &str) -> Option<(String, LaunchNonce)> {
    let mut sock: Option<String> = None;
    let mut nonce: Option<LaunchNonce> = None;
    for line in body.lines() {
        if let Some(p) = line.strip_prefix("sock ") {
            sock = Some(p.trim().to_string());
        } else if let Some(h) = line.strip_prefix("nonce ") {
            nonce = LaunchNonce::from_hex(h.trim());
        }
    }
    Some((sock?, nonce?))
}

/// Build the first line to present on the child socket: `TOKEN <edge-hex> <verb>`,
/// where `verb` is the caller's already-rewritten verb line (the direct child's
/// own selector inlined to `@.`). The shipped path forwards DIRECT children only
/// (one hop) — the child is never in its own proxy table, so no cycle can form.
#[must_use]
pub fn forward_first_line(edge_hex: &str, verb: &str) -> String {
    format!("TOKEN {edge_hex} {verb}\n")
}

/// Connect to `child_sock`, present `first_line`, and RELAY bytes transparently in
/// both directions until either side closes. Format-agnostic: carries any verb's
/// framing (status lines, `OK <n>` bodies, subscribe push frames, binary). The
/// `client_prebuffered` bytes (anything the server's `BufReader` already read past
/// the request line) are forwarded to the child FIRST so nothing is lost.
///
/// Returns `Ok(())` on a clean close, or an `io::Error` if the dial / handshake
/// failed before any relay (so the caller can answer `ERR`).
pub fn connect_and_relay(
    child_sock: &str,
    first_line: &str,
    client: &UnixStream,
    client_prebuffered: &[u8],
) -> std::io::Result<()> {
    let child = UnixStream::connect(child_sock)?;
    // Present the handshake + folded, rewritten verb.
    (&child).write_all(first_line.as_bytes())?;
    if !client_prebuffered.is_empty() {
        (&child).write_all(client_prebuffered)?;
    }
    (&child).flush()?;
    // Past this point the verb HAS been delivered to the child. A relay-stage
    // failure (e.g. a `try_clone` under fd exhaustion) tears the connection down
    // but must NOT be reported as "forward failed" — that would be a false
    // negative for an op that already reached the child. Only a connect/handshake
    // error (the `?`s above, before any byte was delivered) surfaces as `Err` so
    // the caller can honestly answer `ERR forward`.
    let _ = relay_bidirectional(client, &child);
    Ok(())
}

/// Pump bytes both ways between two connected streams until EITHER closes. One
/// direction runs on a spawned thread; the other on the caller's. When either
/// direction ends, BOTH halves of BOTH sockets are shut down so the paired pump's
/// reader — which may be parked on a CLONE of the same local socket (where a mere
/// `Shutdown::Write` would NOT deliver EOF) — always unblocks. This is what keeps
/// a half-open teardown from leaking the worker thread + its fds.
fn relay_bidirectional(client: &UnixStream, child: &UnixStream) -> std::io::Result<()> {
    let mut c2s_r = client.try_clone()?;
    let mut c2s_w = child.try_clone()?;
    let mut s2c_r = child.try_clone()?;
    let mut s2c_w = client.try_clone()?;
    let w_client = client.try_clone()?;
    let w_child = child.try_clone()?;
    // child -> client on a worker; client -> child here.
    let worker = std::thread::spawn(move || {
        let _ = copy_until_eof(&mut s2c_r, &mut s2c_w);
        let _ = w_client.shutdown(std::net::Shutdown::Both);
        let _ = w_child.shutdown(std::net::Shutdown::Both);
    });
    let _ = copy_until_eof(&mut c2s_r, &mut c2s_w);
    let _ = client.shutdown(std::net::Shutdown::Both);
    let _ = child.shutdown(std::net::Shutdown::Both);
    let _ = worker.join();
    Ok(())
}

/// Copy `reader` → `writer` in 32 KiB chunks until EOF or error.
fn copy_until_eof<R: Read, W: Write>(reader: &mut R, writer: &mut W) -> std::io::Result<()> {
    let mut buf = [0u8; 32 * 1024];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            return Ok(());
        }
        writer.write_all(&buf[..n])?;
        writer.flush()?;
    }
}

/// Drain whatever a server-side `BufReader` already buffered past the request line,
/// so [`connect_and_relay`] can forward it to the child before the raw relay. A
/// freshly-handshaked connection usually has none.
///
/// Uses [`BufReader::buffer`] — the bytes ALREADY in the internal buffer — and
/// NEVER `fill_buf()`: `fill_buf` performs a blocking read when the buffer is
/// empty, which is the COMMON case for a one-line forward request (the client
/// sent `TOKEN <hex> @<sid> <verb>\n` and is now blocked awaiting the reply). A
/// `fill_buf` there would deadlock — the client never sends more, so the drain
/// would park forever before the relay even starts. `buffer()` returns the
/// pipelined leftovers when present and an empty slice otherwise, no syscall.
#[must_use]
pub fn drain_buffered<R: Read>(reader: &mut std::io::BufReader<R>) -> Vec<u8> {
    let buffered = reader.buffer().to_vec();
    let n = buffered.len();
    reader.consume(n);
    buffered
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufReader;

    #[test]
    fn graph_entry_roundtrips_through_disk() {
        let dir = std::env::temp_dir().join(format!("aterm-graph-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let sid = SessionId::generate();
        let nonce = LaunchNonce::generate();
        write_graph_entry(&dir, &sid, "/run/user/1000/aterm/aterm-42.sock", &nonce);
        let (sock, got_nonce) = read_graph_entry(&dir, &sid).expect("entry exists");
        assert_eq!(sock, "/run/user/1000/aterm/aterm-42.sock");
        assert!(got_nonce.ct_eq(&nonce));
        // A different sid has no entry.
        assert!(read_graph_entry(&dir, &SessionId::generate()).is_none());
        remove_graph_entry(&dir, &sid);
        assert!(read_graph_entry(&dir, &sid).is_none(), "removed");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_graph_entry_rejects_malformed() {
        assert!(parse_graph_entry("sock /a/b.sock\nnonce deadbeef\n").is_none(), "short nonce");
        assert!(parse_graph_entry("nonce {}\n".replace("{}", &LaunchNonce::generate().to_hex()).as_str()).is_none(), "no sock");
        let good = format!("sock /x.sock\nnonce {}\n", LaunchNonce::generate().to_hex());
        assert!(parse_graph_entry(&good).is_some());
    }

    #[test]
    fn token_for_maps_op_to_its_edge() {
        let e = ProxyEntry {
            nonce: LaunchNonce::generate(),
            read: EdgeToken::generate(),
            write: EdgeToken::generate(),
            signal: EdgeToken::generate(),
        };
        assert!(e.token_for(Op::ReadScreen).unwrap().ct_eq(&e.read));
        assert!(e.token_for(Op::WriteInput).unwrap().ct_eq(&e.write));
        assert!(e.token_for(Op::Signal).unwrap().ct_eq(&e.signal));
        assert!(e.token_for(Op::DeriveLoop).is_none());
    }

    #[test]
    fn forward_first_line_presents_token_and_verb() {
        assert_eq!(forward_first_line("abcd", "@. screen"), "TOKEN abcd @. screen\n");
    }

    /// The relay carries an arbitrary request → response (incl. a multi-line body
    /// and raw non-UTF-8 bytes) transparently, presenting the TOKEN handshake to
    /// the "child" socket. A throwaway UnixListener stands in for the inner aterm.
    #[test]
    fn connect_and_relay_pipes_handshake_and_response() {
        let dir = std::env::temp_dir().join(format!("aterm-relay-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let sock = dir.join("child.sock");
        let sock_s = sock.to_string_lossy().into_owned();
        let _ = std::fs::remove_file(&sock);
        let listener = std::os::unix::net::UnixListener::bind(&sock).expect("bind child");

        // The fake child: read the TOKEN line, then reply with a framed body that
        // includes a raw 0xff byte, then echo one more line, then close.
        let child = std::thread::spawn(move || {
            let (mut conn, _) = listener.accept().expect("accept");
            let mut rdr = BufReader::new(conn.try_clone().unwrap());
            let mut first = String::new();
            rdr.read_line(&mut first).unwrap();
            assert!(first.starts_with("TOKEN tok-hex screen"), "handshake: {first:?}");
            conn.write_all(b"OK 1\n{\"k\":1}\n").unwrap();
            conn.write_all(&[0xffu8, b'\n']).unwrap();
            conn.flush().unwrap();
            // Read whatever the client sends next, echo it, then hang up.
            let mut more = String::new();
            let _ = rdr.read_line(&mut more);
            let _ = conn.write_all(more.as_bytes());
            let _ = conn.shutdown(std::net::Shutdown::Both);
        });

        // The "client" is one end of a socketpair; the relay drives the other end.
        let (client_app, client_relay) = UnixStream::pair().expect("pair");
        let first_line = forward_first_line("tok-hex", "screen");
        let relay = std::thread::spawn(move || {
            connect_and_relay(&sock_s, &first_line, &client_relay, &[]).expect("relay");
        });

        // The app side: read the child's framed response through the relay.
        let mut app_rdr = BufReader::new(client_app.try_clone().unwrap());
        let mut status = String::new();
        app_rdr.read_line(&mut status).unwrap();
        assert_eq!(status, "OK 1\n");
        let mut body = String::new();
        app_rdr.read_line(&mut body).unwrap();
        assert_eq!(body, "{\"k\":1}\n");
        let mut raw = Vec::new();
        // The 0xff + newline byte survives the relay verbatim.
        let mut byte = [0u8; 2];
        std::io::Read::read_exact(&mut app_rdr, &mut byte).unwrap();
        raw.extend_from_slice(&byte);
        assert_eq!(raw, vec![0xff, b'\n']);

        // Send a follow-up line; the child echoes it back through the relay.
        (&client_app).write_all(b"ping\n").unwrap();
        (&client_app).flush().unwrap();
        let mut echo = String::new();
        app_rdr.read_line(&mut echo).unwrap();
        assert_eq!(echo, "ping\n");

        // Close BOTH client handles (the raw stream AND `app_rdr`'s cloned fd) so
        // the relay's client→child reader hits EOF and the relay thread returns.
        let _ = client_app.shutdown(std::net::Shutdown::Both);
        drop(app_rdr);
        drop(client_app);
        let _ = relay.join();
        let _ = child.join();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// F1 (revised): edge-token secrets round-trip through the 0600 file, the file
    /// is owner-only (0600), and the read is REPEATABLE — it PERSISTS for the
    /// session so a child re-launched in the same shell can re-read it. The PARENT
    /// removes it on teardown via `remove_edge_tokens` (keyed by child sid).
    #[test]
    fn edge_tokens_file_is_0600_and_read_is_repeatable() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("aterm-edges-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let sid = SessionId::generate();
        let (r, w, s) = ("aa".repeat(32), "bb".repeat(32), "cc".repeat(32));
        let path = write_edge_tokens(&dir, &sid, &r, &w, &s).expect("write");
        // 0600 — owner read/write only (no group/other bits).
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "edge-token file must be 0600, got {mode:o}");
        // The CORE of this bug fix: two reads of the same file BOTH succeed (the
        // same-shell relaunch re-reads it; consume-on-read previously broke this).
        let first = read_edge_tokens(&path);
        let second = read_edge_tokens(&path);
        assert_eq!(first, Some((r.clone(), w.clone(), s.clone())), "first read");
        assert_eq!(second, Some((r, w, s)), "second read still succeeds (persists)");
        // The parent owns removal, keyed by child sid; after it the file is gone.
        remove_edge_tokens(&dir, &sid);
        assert!(read_edge_tokens(&path).is_none(), "removed by owning parent");
        // A different child's sid is a no-op (removes only its own file).
        remove_edge_tokens(&dir, &SessionId::generate());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// REGRESSION: `drain_buffered` must NOT block when the BufReader has no
    /// buffered bytes and the peer is silent — the common one-line-forward case.
    /// (The bug was `fill_buf()`, which blocks on an empty buffer and hung every
    /// forward before the relay started.) If it regresses, this test HANGS — the
    /// correct, loud failure mode under a test timeout.
    #[test]
    fn drain_buffered_never_blocks_on_empty_buffer() {
        let (a, _b) = UnixStream::pair().expect("pair"); // _b stays open + silent
        let mut r = std::io::BufReader::new(a);
        assert!(drain_buffered(&mut r).is_empty(), "empty buffer drains to nothing, no block");
    }

    /// `drain_buffered` returns exactly the bytes PIPELINED past the request line
    /// (so the relay forwards them first) and consumes them from the buffer.
    #[test]
    fn drain_buffered_returns_pipelined_leftovers() {
        use std::io::BufRead;
        let (a, b) = UnixStream::pair().expect("pair");
        // Peer sends a request line + pipelined trailing bytes in one write.
        (&b).write_all(b"verb line\nLEFTOVER").unwrap();
        drop(b);
        let mut r = std::io::BufReader::new(a);
        let mut line = String::new();
        r.read_line(&mut line).unwrap(); // consume the request line
        assert_eq!(line, "verb line\n");
        // fill the buffer (a real serve loop's next read would); then drain it.
        let _ = r.fill_buf().unwrap();
        assert_eq!(drain_buffered(&mut r), b"LEFTOVER");
        // Second drain is empty (consumed).
        assert!(drain_buffered(&mut r).is_empty());
    }
}
