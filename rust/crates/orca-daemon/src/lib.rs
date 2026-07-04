//! orca-daemon (spike) — a pure-Rust replacement for the Node terminal daemon
//! (`src/main/daemon/`). It owns PTYs via `orca-pty`, runs a real `aterm` engine
//! per session via `orca-terminal`, and speaks the existing NDJSON Unix-socket
//! protocol (`src/main/daemon/types.ts`), so the Electron client can drive it
//! unchanged and the napi PTY→engine hop disappears.
//!
//! Exposed as a lib (not just a bin) so integration tests and the differential
//! parity harness can drive `rpc::dispatch_request` against a `registry::Registry`
//! directly. See docs/rust-migration/move-1-orca-daemon-extraction.md.

pub mod connection;
pub mod protocol;
pub mod registry;
pub mod rpc;
pub mod token;

use connection::handle_connection;
use registry::Registry;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::sync::Arc;
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
        "orca-daemon (spike) listening at {socket_path} (protocol v{}, auth={})",
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
