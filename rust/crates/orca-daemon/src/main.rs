//! orca-daemon (spike) — a pure-Rust replacement for the Node terminal daemon
//! (`src/main/daemon/`). It owns PTYs via `orca-pty` and speaks the existing
//! NDJSON Unix-socket protocol (`src/main/daemon/types.ts`), so the Electron
//! client can drive it unchanged and the napi PTY→engine hop disappears.
//!
//! Sub-step 1 of Move 1 (see docs/rust-migration/move-1-orca-daemon-extraction.md):
//! `hello` handshake at protocol v18 + `createOrAttach`/`write`/`resize`/`kill`/
//! `ping` + `data`/`exit` streaming. The remaining RPCs, persistence, and the
//! parity gate are sub-steps 2–4.
//!
//! Usage: `orca-daemon <socket-path>`  (or `ORCA_DAEMON_SOCKET=<path> orca-daemon`)

mod connection;
mod protocol;
mod registry;
mod rpc;

use connection::handle_connection;
use registry::Registry;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::sync::Arc;
use std::thread;

fn main() {
    let socket_path = std::env::args()
        .nth(1)
        .or_else(|| std::env::var("ORCA_DAEMON_SOCKET").ok())
        .unwrap_or_else(|| {
            eprintln!("usage: orca-daemon <socket-path>");
            std::process::exit(2);
        });

    // Fresh bind: clear a stale socket file left by a crashed prior daemon.
    let _ = std::fs::remove_file(&socket_path);
    let listener = match UnixListener::bind(&socket_path) {
        Ok(listener) => listener,
        Err(e) => {
            eprintln!("orca-daemon: bind {socket_path} failed: {e}");
            std::process::exit(1);
        }
    };
    // 0o600: a local RPC channel only the owner may connect to (parity with the
    // Node daemon's chmod of its socket).
    let _ = std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o600));

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
}
