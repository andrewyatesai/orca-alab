//! orca-daemon — a pure-Rust replacement for the Node terminal daemon
//! (`src/main/daemon/`). It owns PTYs via `orca-pty`, runs a real `aterm` engine
//! per session via `orca-terminal`, and speaks the existing NDJSON Unix-socket
//! protocol (`src/main/daemon/types.ts`), so the Electron client can drive it
//! unchanged and the napi PTY→engine hop disappears.
//!
//! Exposed as a lib (not just a bin) so integration tests and the differential
//! parity harness can drive `rpc::dispatch_request` against a `registry::Registry`
//! directly. See docs/rust-migration/move-1-orca-daemon-extraction.md.

#[cfg(unix)]
pub mod connection;
pub mod pending_output;
pub mod protocol;
pub mod registry;
pub mod resolver_health;
pub mod rpc;
// token.rs reads /dev/urandom and sets 0600 perms via std::os::unix — unix-only,
// like the socket transport it guards. The Windows daemon keeps the Node path.
#[cfg(unix)]
pub mod token;

#[cfg(unix)]
use connection::handle_connection;
#[cfg(unix)]
use registry::Registry;
use std::io;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(unix)]
use std::os::unix::net::UnixListener;
#[cfg(unix)]
use std::sync::Arc;
#[cfg(unix)]
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

/// The socket transport is unix-only for now: the Node daemon uses a named pipe on
/// Windows, and Rust std offers neither named pipes nor AF_UNIX there without
/// `unsafe` (this crate forbids it). The RPC/registry/engine core above still
/// builds and is tested on Windows; the transport twin lands with its own port.
/// Signature matches the unix `serve` so `main.rs` calls it unchanged; `token_path`
/// is accepted (and ignored) because no listener is bound to gate.
#[cfg(not(unix))]
pub fn serve(socket_path: &str, _token_path: Option<&str>) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        format!("orca-daemon socket transport is not implemented on this platform (socket {socket_path})"),
    ))
}
