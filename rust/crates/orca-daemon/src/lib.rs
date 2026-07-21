//! orca-daemon — a pure-Rust replacement for the Node terminal daemon
//! (`src/main/daemon/`). It owns PTYs via `orca-pty`, runs a real `aterm` engine
//! per session via `orca-terminal`, and speaks the existing NDJSON Unix-socket
//! protocol (`src/main/daemon/types.ts`), so the Electron client can drive it
//! unchanged and the napi PTY→engine hop disappears.
//!
//! Exposed as a lib (not just a bin) so integration tests and the differential
//! parity harness can drive `rpc::dispatch_request` against a `registry::Registry`
//! directly. See docs/rust-migration/move-1-orca-daemon-extraction.md.

// The connection logic is transport-generic (see connection::DaemonStream), so it
// compiles on every platform; each `serve` below supplies its own socket type.
pub mod connection;
pub mod pending_output;
pub mod process_query;
pub mod protocol;
pub mod registry;
pub mod resolver_health;
pub mod rpc;
pub mod shell_ready_barrier;
#[cfg(unix)]
pub mod termination_signals;
pub mod token;
pub mod utf8_stream_decoder;

#[cfg(any(unix, windows))]
use connection::handle_connection;
#[cfg(any(unix, windows))]
use registry::Registry;
use std::io;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(unix)]
use std::os::unix::net::UnixListener;
#[cfg(any(unix, windows))]
use std::sync::Arc;
#[cfg(any(unix, windows))]
use std::thread;

/// Bind the Unix socket at `socket_path` and serve connections forever. Each
/// accepted socket is handled on its own thread (a control RPC socket or an event
/// stream socket, distinguished by its `hello` role). A stale socket file from a
/// crashed prior daemon is cleared first; the socket is chmod'd 0o600 (owner-only,
/// parity with the Node daemon).
///
/// `token_path`: when `Some` (the live app), the daemon self-generates an auth
/// token, publishes it to that file (0600) for the client to read, and rejects
/// every `hello` whose token doesn't match. When `None` (parity harness /
/// standalone), any token is accepted.
#[cfg(unix)]
pub fn serve(socket_path: &str, token_path: Option<&str>) -> io::Result<()> {
    let _ = std::fs::remove_file(socket_path);
    let listener = UnixListener::bind(socket_path)?;
    let _ = std::fs::set_permissions(socket_path, std::fs::Permissions::from_mode(0o600));

    let expected_token: Option<Arc<str>> = match token_path {
        Some(path) => {
            let generated = token::generate_token()?;
            token::write_token_file(path, &generated)?;
            Some(Arc::from(generated.as_str()))
        }
        None => None,
    };

    eprintln!(
        "orca-daemon listening at {socket_path} (protocol v{}, auth={})",
        protocol::PROTOCOL_VERSION,
        if expected_token.is_some() {
            "on"
        } else {
            "off"
        }
    );
    let registry = Arc::new(Registry::new());
    registry.set_socket_path(socket_path);
    // Why: logout/shutdown SIGTERMs the detached daemon; without this, PTY children ignoring SIGHUP orphan (#7936).
    termination_signals::install(registry.clone());
    for incoming in listener.incoming() {
        match incoming {
            Ok(stream) => {
                let registry = registry.clone();
                let expected = expected_token.clone();
                thread::spawn(move || handle_connection(stream, registry, expected));
            }
            Err(e) => eprintln!("orca-daemon: accept error: {e}"),
        }
    }
    Ok(())
}

/// Windows transport: the client dials the exact pipe path the spawner passes us
/// (`\\?\pipe\orca-terminal-host-v…`), so this mirrors the unix `serve` with a
/// named-pipe listener instead of a `UnixListener`. The unsafe winapi FFI is
/// isolated in `orca-winpipe` so this crate stays `unsafe_code = "forbid"`. Each
/// accepted pipe instance is handled on its own thread via the shared,
/// transport-generic `handle_connection`.
///
/// Cross-compile-verified for x86_64-pc-windows (lib + tests); the wire protocol,
/// handshake, and threading model are identical to the unix path. End-to-end
/// runtime on a real Windows host has not yet been exercised.
#[cfg(windows)]
pub fn serve(socket_path: &str, token_path: Option<&str>) -> io::Result<()> {
    // bind() pre-creates the first pipe instance, and accept() pre-arms the next
    // one on every connection — so a dialing client practically never sees
    // ERROR_PIPE_BUSY (the JS clients retry the residual window).
    let mut listener = orca_winpipe::NamedPipeListener::bind(socket_path)?;

    let expected_token: Option<Arc<str>> = match token_path {
        Some(path) => {
            let generated = token::generate_token()?;
            token::write_token_file(path, &generated)?;
            Some(Arc::from(generated.as_str()))
        }
        None => None,
    };

    eprintln!(
        "orca-daemon listening at {socket_path} (protocol v{}, auth={})",
        protocol::PROTOCOL_VERSION,
        if expected_token.is_some() { "on" } else { "off" }
    );
    let registry = Arc::new(Registry::new());
    registry.set_socket_path(socket_path);
    loop {
        match listener.accept() {
            Ok(stream) => {
                let registry = registry.clone();
                let expected = expected_token.clone();
                thread::spawn(move || handle_connection(stream, registry, expected));
            }
            Err(e) => eprintln!("orca-daemon: accept error: {e}"),
        }
    }
}

/// Fallback for any other platform: no socket transport. Signature matches the
/// real `serve` so `main.rs` calls it unchanged.
#[cfg(not(any(unix, windows)))]
pub fn serve(socket_path: &str, _token_path: Option<&str>) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        format!("orca-daemon socket transport is not implemented on this platform (socket {socket_path})"),
    ))
}
