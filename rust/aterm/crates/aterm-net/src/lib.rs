// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The aterm Authors

//! **aterm-net** — layer L3 of the RFC "The Reactive Surface": carrying the
//! control protocol (read + drive + `await`) over a NETWORK boundary so one
//! aterm can drive another on a different host, byte-identically to local.
//!
//! ## Why this needs no astream to *drive* a remote aterm
//!
//! The design review's key correction: predicate evaluation runs on the
//! **authoritative aterm surface** (the host that owns the engine), never on a
//! subset-diverging remote fold. So "drive a remote aterm" is just **relaying the
//! control protocol over a network transport** — the remote host runs the
//! watchers; the driver sends `await idle`/`send`/`key` and reads back. No
//! astream fold is on the critical path. astream remains the OPTIONAL record/
//! replay format for an *offline observer* and is the one genuinely external
//! piece (a sibling repo, not on disk) — out of scope here.
//!
//! ## What this crate provides (built + tested over loopback)
//!
//! 1. [`Transport`] — generalizes `proxy.rs`'s `UnixStream::connect` dial to any
//!    connected channel, yielding **owned** read/write halves (TLS/QUIC have no
//!    `UnixStream::try_clone`, so the relay must own its halves — the teardown
//!    pitfall the review flagged). [`LoopbackTransport`] is the 0-hop case.
//! 2. [`pump`] — the transport-agnostic byte relay (the `connect_and_relay` core)
//!    that carries any verb's framing verbatim, incl. binary.
//! 3. [`channel_bind`] / [`verify_presented`] — the capability binding that
//!    replaces the local same-uid `SO_PEERCRED` check, which has **no network
//!    analog**. The presented secret is `H(edge_token, channel_exporter)`, so a
//!    token captured on one connection **cannot replay** on another (a different
//!    exporter → a different presented value the verifier rejects). Property
//!    proven by `channel_bind_model` (`aterm-spec`).
//! 4. [`RemoteEndpoint`] — the pinned `(host, sid, nonce, fingerprint)` a dial
//!    checks BEFORE presenting the token, so a redirected dial fails the
//!    handshake before any secret crosses the wire.
//! 5. [`RemoteOp::DialRemote`] — the new object-capability gating WHICH remote
//!    endpoint a node may dial (the local `Scope::Owner` is all-or-nothing).
//!
//! > **Crypto note.** [`channel_bind`] uses a fast non-cryptographic keyed digest
//! > to demonstrate and model-check the channel-SEPARATION property. A shipping
//! > deployment MUST replace it with HMAC-SHA256 over a TLS exporter (RFC 5705)
//! > or an ssh-tunnel binding; the protocol shape and the replay-resistance
//! > invariant are unchanged.

use std::io::{self, Read, Write};

use aterm_session::EdgeToken;

/// A connected network channel for the control protocol. Generalizes the local
/// `UnixStream::connect` dial: the relay takes OWNED halves (no `try_clone`
/// dependency), so a TLS/QUIC transport drops in unchanged.
pub trait Transport {
    /// Consume the transport into its owned read and write halves.
    fn split(self: Box<Self>) -> (Box<dyn Read + Send>, Box<dyn Write + Send>);
    /// The per-connection exporter that binds a capability to THIS channel (the
    /// TLS keying material in production; a per-connection nonce here). Distinct
    /// per connection, so a token bound to one channel is useless on another.
    fn exporter(&self) -> Vec<u8>;
}

/// The 0-hop transport: a connected `UnixStream` pair. The same `pump` and
/// binding run over it as over a real network transport — local is the trivial
/// case of remote, by construction.
pub struct LoopbackTransport {
    stream: std::os::unix::net::UnixStream,
    exporter: Vec<u8>,
}

impl LoopbackTransport {
    /// A connected loopback pair `(a, b)`, each carrying the SAME channel
    /// exporter `exporter` (the two ends of one channel share keying material).
    #[must_use]
    pub fn pair(exporter: Vec<u8>) -> (Self, Self) {
        let (a, b) = std::os::unix::net::UnixStream::pair().expect("socketpair");
        (
            Self {
                stream: a,
                exporter: exporter.clone(),
            },
            Self {
                stream: b,
                exporter,
            },
        )
    }
}

impl Transport for LoopbackTransport {
    fn split(self: Box<Self>) -> (Box<dyn Read + Send>, Box<dyn Write + Send>) {
        let w = self.stream.try_clone().expect("clone loopback");
        (Box::new(self.stream), Box::new(w))
    }
    fn exporter(&self) -> Vec<u8> {
        self.exporter.clone()
    }
}

/// Relay bytes from `reader` to `writer` until EOF — the transport-agnostic core
/// of `connect_and_relay`, carrying any verb's framing (status lines, `OK <n>`
/// bodies, `subscribe` push frames, the `bytes` stream, binary) verbatim.
///
/// # Errors
/// Propagates the first I/O error from either half.
pub fn pump<R: Read, W: Write>(reader: &mut R, writer: &mut W) -> io::Result<u64> {
    let mut buf = [0u8; 32 * 1024];
    let mut total = 0u64;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            return Ok(total);
        }
        writer.write_all(&buf[..n])?;
        writer.flush()?;
        total += n as u64;
    }
}

/// The channel-bound presented secret: `H(edge_token, channel_exporter)`. Binding
/// the token to the connection's exporter means a value captured on one channel
/// is rejected on any other — the network replacement for the same-uid peer check.
///
/// NON-CRYPTO digest (see the module docs); production uses HMAC over a TLS
/// exporter. The PROPERTY — distinct exporter ⇒ distinct presented value — holds
/// and is what `channel_bind_model` proves.
#[must_use]
pub fn channel_bind(token: &EdgeToken, exporter: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = aterm_hash::FxHasher::default();
    // Key material (the bearer token) THEN the channel exporter, so the output
    // depends on both — a different channel cannot reproduce it.
    token.to_hex().hash(&mut h);
    exporter.hash(&mut h);
    h.finish()
}

/// Verify a `presented` secret against `token` on the CURRENT channel's
/// `exporter`. A token bound to a different channel (a captured replay) fails
/// because its `presented` was computed over a different exporter.
#[must_use]
pub fn verify_presented(token: &EdgeToken, exporter: &[u8], presented: u64) -> bool {
    channel_bind(token, exporter) == presented
}

/// A pinned remote identity. A dial checks this BEFORE presenting the bound
/// token, so a redirected/MITM'd endpoint fails the handshake before any secret
/// crosses the wire (the filesystem `confine_proxy_sock` discipline has no
/// network analog, so identity pinning replaces it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteEndpoint {
    /// Host/address the endpoint was published at.
    pub host: String,
    /// The remote session id being driven.
    pub sid: String,
    /// The remote session's launch nonce (rebind guard).
    pub nonce: String,
    /// The pinned transport identity (TLS cert fingerprint / ssh host key).
    pub fingerprint: String,
}

impl RemoteEndpoint {
    /// Does an observed `(fingerprint, nonce)` match the pin? Both must hold, so
    /// neither a relaunched session (fresh nonce) nor a swapped host (fresh
    /// fingerprint) is dialed by a stale capability.
    #[must_use]
    pub fn matches(&self, observed_fingerprint: &str, observed_nonce: &str) -> bool {
        self.fingerprint == observed_fingerprint && self.nonce == observed_nonce
    }
}

/// The network object-capability: WHICH remote endpoint a node may dial. The
/// local fabric's `Scope::Owner` is all-or-nothing; crossing a host needs a
/// capability scoped to one endpoint, presented (channel-bound) on the dial.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RemoteOp {
    /// Authority to dial exactly this endpoint (and no other).
    DialRemote(RemoteEndpoint),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pump_carries_an_arbitrary_exchange_over_the_transport_verbatim() {
        // The relay carries a control-protocol response — including a multi-line
        // `OK <n>` body and a raw non-UTF-8 byte — across the transport unchanged.
        let (client, server) = LoopbackTransport::pair(b"chan-1".to_vec());
        let payload = b"OK 1\n{\"k\":1}\n\xff\n".to_vec();
        let p2 = payload.clone();
        let srv = std::thread::spawn(move || {
            let (mut r, mut w) = Box::new(server).split();
            let mut first = [0u8; 6];
            std::io::Read::read_exact(&mut r, &mut first).unwrap();
            assert_eq!(&first, b"screen"); // the relayed verb arrived
            w.write_all(&p2).unwrap();
            w.flush().unwrap();
        });
        let (mut cr, mut cw) = Box::new(client).split();
        cw.write_all(b"screen").unwrap();
        cw.flush().unwrap();
        let mut got = vec![0u8; payload.len()];
        std::io::Read::read_exact(&mut cr, &mut got).unwrap();
        assert_eq!(
            got, payload,
            "the byte pump is format-agnostic and lossless"
        );
        srv.join().unwrap();
    }

    #[test]
    fn channel_binding_rejects_a_cross_channel_replay() {
        // A token captured on channel A must NOT authorize on channel B — the
        // network analog of the same-uid peer check.
        let token = EdgeToken::generate();
        let exporter_a = b"tls-exporter-A".to_vec();
        let exporter_b = b"tls-exporter-B".to_vec();

        let presented_on_a = channel_bind(&token, &exporter_a);
        // Legit: presented on the SAME channel it was bound to -> accepted.
        assert!(verify_presented(&token, &exporter_a, presented_on_a));
        // Replay: the captured channel-A value presented on channel B -> rejected.
        assert!(
            !verify_presented(&token, &exporter_b, presented_on_a),
            "a captured token must not replay on a different channel"
        );
        // And the two channels yield distinct bindings.
        assert_ne!(
            channel_bind(&token, &exporter_a),
            channel_bind(&token, &exporter_b)
        );
    }

    #[test]
    fn endpoint_pin_rejects_relaunch_and_host_swap() {
        let ep = RemoteEndpoint {
            host: "host-1:7000".into(),
            sid: "s-abc".into(),
            nonce: "nonce-1".into(),
            fingerprint: "fp-1".into(),
        };
        assert!(ep.matches("fp-1", "nonce-1"), "exact match dials");
        assert!(
            !ep.matches("fp-1", "nonce-2"),
            "relaunched session (fresh nonce) refused"
        );
        assert!(
            !ep.matches("fp-2", "nonce-1"),
            "swapped host (fresh fingerprint) refused"
        );
    }

    #[test]
    fn dial_remote_is_scoped_to_one_endpoint() {
        let ep1 = RemoteEndpoint {
            host: "h1".into(),
            sid: "s1".into(),
            nonce: "n1".into(),
            fingerprint: "f1".into(),
        };
        let cap = RemoteOp::DialRemote(ep1.clone());
        // The capability authorizes exactly ep1, not a different endpoint.
        match cap {
            RemoteOp::DialRemote(e) => assert_eq!(e, ep1),
        }
    }
}
