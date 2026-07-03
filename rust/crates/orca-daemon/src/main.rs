//! orca-daemon (spike) entry point. The daemon logic lives in the lib (see
//! `lib.rs` + docs/rust-migration/move-1-orca-daemon-extraction.md); this only
//! resolves the socket path and starts serving.
//!
//! Usage: `orca-daemon <socket-path>`  (or `ORCA_DAEMON_SOCKET=<path> orca-daemon`)

use std::process::exit;

fn main() {
    let socket_path = std::env::args()
        .nth(1)
        .or_else(|| std::env::var("ORCA_DAEMON_SOCKET").ok())
        .unwrap_or_else(|| {
            eprintln!("usage: orca-daemon <socket-path>");
            exit(2);
        });
    if let Err(e) = orca_daemon::serve(&socket_path) {
        eprintln!("orca-daemon: serve {socket_path} failed: {e}");
        exit(1);
    }
}
