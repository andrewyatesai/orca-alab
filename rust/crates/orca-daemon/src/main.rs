//! orca-daemon entry point. The daemon logic lives in the lib (see
//! `lib.rs` + docs/rust-migration/move-1-orca-daemon-extraction.md); this only
//! resolves the socket + token paths and starts serving.
//!
//! Args mirror the Node `daemon-entry` so `daemon-spawner`/`daemon-init` can
//! launch this as a drop-in:
//!   orca-daemon --socket <path> [--token <path>]
//! A bare positional socket path is also accepted (token-less, for the parity
//! harness / standalone runs): `orca-daemon <socket-path>`.

use std::process::exit;

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let mut socket_path: Option<String> = None;
    let mut token_path: Option<String> = None;
    let mut i = 0;
    while i < argv.len() {
        match argv[i].as_str() {
            "--socket" => {
                socket_path = argv.get(i + 1).cloned();
                i += 2;
            }
            "--token" => {
                token_path = argv.get(i + 1).cloned();
                i += 2;
            }
            other => {
                if socket_path.is_none() {
                    socket_path = Some(other.to_string());
                }
                i += 1;
            }
        }
    }

    let socket_path = socket_path
        .or_else(|| std::env::var("ORCA_DAEMON_SOCKET").ok())
        .unwrap_or_else(|| {
            eprintln!("usage: orca-daemon (--socket <path> [--token <path>] | <socket-path>)");
            exit(2);
        });

    if let Err(e) = orca_daemon::serve(&socket_path, token_path.as_deref()) {
        eprintln!("orca-daemon: serve {socket_path} failed: {e}");
        exit(1);
    }
}
