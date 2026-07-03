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
pub fn serve(socket_path: &str) -> io::Result<()> {
    let _ = std::fs::remove_file(socket_path);
    let listener = UnixListener::bind(socket_path)?;
    let _ = std::fs::set_permissions(socket_path, std::fs::Permissions::from_mode(0o600));
    eprintln!(
        "orca-daemon (spike) listening at {socket_path} (protocol v{})",
        protocol::PROTOCOL_VERSION
    );
    let registry = Arc::new(Registry::new());
    for incoming in listener.incoming() {
        match incoming {
            Ok(stream) => {
                let registry = registry.clone();
                thread::spawn(move || handle_connection(stream, registry));
            }
            Err(e) => eprintln!("orca-daemon: accept error: {e}"),
        }
    }
    Ok(())
}
