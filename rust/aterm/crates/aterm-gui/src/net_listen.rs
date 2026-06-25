// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! Secure-default-OFF network front-end for the control socket.
//!
//! aterm never opens a network port on its own. ONLY when an operator explicitly
//! sets all three of:
//!
//! * `ATERM_NET_LISTEN` — the bind address (e.g. `0.0.0.0:7100`),
//! * `ATERM_NET_CERT`    — path to the server certificate (DER),
//! * `ATERM_NET_KEY`     — path to its PKCS#8 private key (DER),
//!
//! does [`maybe_spawn`] stand up a TLS listener. Each accepted connection must
//! present a capability **channel-bound** to the TLS session
//! ([`aterm_net::channel_bind`]) keyed by THIS instance's control token; only then
//! is it relayed to the local control socket, where it authenticates again with
//! the ordinary `AUTH <token>` handshake. The TLS capability is the network-
//! specific gate that replaces the local same-uid `SO_PEERCRED` check — which has
//! no network analog — and, being bound to the TLS exporter, resists an active
//! MITM (a relay that terminates one TLS leg holds a different exporter, so a
//! captured tag never transfers).
//!
//! Missing any of the three env vars ⇒ this is a no-op. A malformed cert/key or a
//! failed bind is logged and the listener simply does not start — never a panic,
//! never a fallback to an unauthenticated port.

use std::net::TcpListener;
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use aterm_net::drive::NetEvent;
use aterm_session::EdgeToken;
// Canonical names (also deny-listed in `ENV_DENY_VARS`, so a nested aterm never
// inherits them — see env_sanitize). Single source of truth.
use aterm_types::domain::{ENV_NET_CERT as ENV_CERT, ENV_NET_KEY as ENV_KEY, ENV_NET_LISTEN as ENV_LISTEN};

/// The single op the network capability authorizes: driving this instance's
/// control socket. A remote driver presents `channel_bind(token, exporter)` for
/// this op; `src` is informational (logged), authority comes from the token.
const NET_OP: &str = "drive";

/// If `ATERM_NET_LISTEN`/`_CERT`/`_KEY` are all set, bind a TLS listener and spawn
/// the serve loop relaying authorized remote drivers into the local control
/// socket at `sock_path`. Otherwise (the default) do nothing.
///
/// `token_hex` is this instance's 64-char control token; it is the HMAC key for
/// the network capability binding, so a remote driver must hold the same token it
/// would use for the local `AUTH` handshake — no new secret, no weaker gate.
pub fn maybe_spawn(token_hex: &str, sock_path: &str) {
    let (Some(listen), Some(cert_path), Some(key_path)) = (
        std::env::var_os(ENV_LISTEN),
        std::env::var_os(ENV_CERT),
        std::env::var_os(ENV_KEY),
    ) else {
        return; // secure default: no network port
    };
    let Some(addr) = listen.to_str() else {
        eprintln!("aterm-gui: {ENV_LISTEN} is not valid UTF-8; network drive disabled");
        return;
    };

    // The control token IS the channel-binding key. 64-char hex => 32 bytes.
    let Some(token) = EdgeToken::from_hex(token_hex) else {
        eprintln!("aterm-gui: control token is not 32-byte hex; network drive disabled");
        return;
    };

    let cert = match std::fs::read(&cert_path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("aterm-gui: {ENV_CERT} unreadable ({e}); network drive disabled");
            return;
        }
    };
    let key = match std::fs::read(&key_path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("aterm-gui: {ENV_KEY} unreadable ({e}); network drive disabled");
            return;
        }
    };
    let config = match aterm_net::tls::server_config(cert, key) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("aterm-gui: server cert/key rejected ({e}); network drive disabled");
            return;
        }
    };
    let listener = match TcpListener::bind(addr) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("aterm-gui: network drive bind failed at {addr}: {e}");
            return;
        }
    };

    let sock_path = sock_path.to_owned();
    let addr_owned = addr.to_owned();
    eprintln!(
        "aterm-gui: network drive listening at {addr_owned} \
         (TLS, channel-bound capability, relays to the local control socket)"
    );
    std::thread::spawn(move || {
        // Process-lifetime listener: aterm has no partial "stop just the network
        // drive" teardown, so this flag stays true for the life of the process.
        // It is a REAL kill-switch (`serve` polls it on a timer — see ACCEPT_POLL),
        // not dead control; it is simply never cleared here.
        let running = Arc::new(AtomicBool::new(true));
        aterm_net::drive::serve(
            &listener,
            &config,
            // Authority is the channel-bound token; `op` must be the drive op.
            move |_src, op| (op == NET_OP).then_some(token),
            // Authorized connections are bridged to the local control socket.
            move || UnixStream::connect(&sock_path),
            &running,
            move |ev| match ev {
                NetEvent::Relayed(g) => {
                    eprintln!(
                        "aterm-gui: network drive relayed a verified peer (src={}, op={})",
                        g.src, g.op
                    );
                }
                NetEvent::Rejected(why) => {
                    eprintln!("aterm-gui: network drive rejected a connection: {why}");
                }
            },
        );
    });
}
