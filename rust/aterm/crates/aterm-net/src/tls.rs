// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Andrew Yates

//! TLS 1.3 transport (rustls + the `ring` provider) for the L3 network drive.
//!
//! TLS gives three things the local Unix socket got for free: (1) an
//! authenticated, encrypted channel; (2) server identity via **certificate
//! fingerprint pinning** (a custom [`ServerCertVerifier`] — no CA/PKI, the dialer
//! pins the exact cert the endpoint record names); and (3) the **RFC 5705
//! keying-material exporter** — 32 bytes unique to this TLS session, symmetric on
//! both ends, that [`channel_bind`](crate::channel_bind) keys the capability HMAC
//! over. The exporter is what makes the capability resist an active MITM: a relay
//! that terminates one TLS leg holds a *different* exporter, so a captured tag
//! never transfers.
//!
//! The listener uses an **operator-provided** cert+key (standard server practice;
//! aterm does not mint certs). Tests use a self-signed fixture under
//! `src/testdata/`.

use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{WebPkiSupportedAlgorithms, verify_tls12_signature, verify_tls13_signature};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime};
use rustls::{
    ClientConfig, ClientConnection, DigitallySignedStruct, ServerConfig, ServerConnection,
    SignatureScheme, StreamOwned,
};
use sha2::{Digest, Sha256};

/// RFC 5705 exporter label — namespaces our keying material so it cannot collide
/// with any other exporter use on the same connection.
const EXPORTER_LABEL: &[u8] = b"EXPORTER-aterm-net-capability-v1";
/// The exporter (and the channel-binding HMAC) length.
pub const EXPORTER_LEN: usize = 32;

/// Install the `ring` crypto provider as the process default, once. Idempotent —
/// a second call (or a pre-installed provider) is ignored.
pub fn init_crypto() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        // Ignore the error: another caller may have already installed a provider.
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

/// SHA-256 of a certificate's DER — the fingerprint an endpoint record pins.
#[must_use]
pub fn cert_fingerprint(cert_der: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(cert_der);
    h.finalize().into()
}

fn io_err(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}

/// A server [`ServerConfig`] from an operator-provided cert chain + key (both
/// DER). The key is PKCS#8.
///
/// # Errors
/// If the cert/key are malformed or incompatible.
pub fn server_config(cert_der: Vec<u8>, key_pkcs8_der: Vec<u8>) -> io::Result<Arc<ServerConfig>> {
    init_crypto();
    let certs = vec![CertificateDer::from(cert_der)];
    let key = PrivateKeyDer::try_from(key_pkcs8_der).map_err(io_err)?;
    // TLS 1.3 ONLY: a TLS 1.2 peer can negotiate a non-EMS master secret (RFC
    // 7627), whose RFC 5705 exporter is not bound to the full handshake
    // transcript — which would weaken the channel-binding the capability HMAC
    // keys over. Pinning 1.3 keeps the exporter transcript-bound.
    let cfg = ServerConfig::builder_with_protocol_versions(&[&rustls::version::TLS13])
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(io_err)?;
    Ok(Arc::new(cfg))
}

/// A client [`ClientConfig`] that pins the server cert to `fingerprint` (SHA-256
/// of its DER) and otherwise verifies the TLS 1.3 handshake signature with the
/// `ring` provider — so the peer must BOTH present the pinned cert AND prove it
/// holds the matching private key.
#[must_use]
pub fn client_config(fingerprint: [u8; 32]) -> Arc<ClientConfig> {
    init_crypto();
    let verifier = PinnedServerVerifier {
        pin: fingerprint,
        supported: rustls::crypto::ring::default_provider().signature_verification_algorithms,
    };
    // TLS 1.3 ONLY (matches `server_config`): no downgrade to a non-EMS 1.2
    // session whose exporter would not be transcript-bound.
    let cfg = ClientConfig::builder_with_protocol_versions(&[&rustls::version::TLS13])
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth();
    Arc::new(cfg)
}

/// Verifies the server cert by SHA-256 FINGERPRINT pinning (no CA/PKI), and the
/// handshake signature via the provider's algorithms (proving key possession).
#[derive(Debug)]
struct PinnedServerVerifier {
    pin: [u8; 32],
    supported: WebPkiSupportedAlgorithms,
}

impl ServerCertVerifier for PinnedServerVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        // Constant-time-ish fingerprint compare (32 bytes, fixed length).
        if cert_fingerprint(end_entity.as_ref()) == self.pin {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::InvalidCertificate(
                rustls::CertificateError::ApplicationVerificationFailure,
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(message, cert, dss, &self.supported)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, cert, dss, &self.supported)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.supported.supported_schemes()
    }
}

/// A connected TLS stream plus its channel exporter — the input to
/// [`channel_bind`](crate::channel_bind). Generic over the rustls connection role
/// (`ServerConnection` / `ClientConnection`).
pub struct TlsTransport<C> {
    stream: StreamOwned<C, TcpStream>,
    exporter: [u8; EXPORTER_LEN],
}

impl<C> TlsTransport<C> {
    /// The RFC 5705 channel exporter — 32 bytes unique to this TLS session,
    /// identical on both ends, that the capability HMAC keys over.
    #[must_use]
    pub fn exporter(&self) -> &[u8] {
        &self.exporter
    }

    /// The connected TLS stream (Read + Write) for the post-handshake relay.
    pub fn stream(&mut self) -> &mut StreamOwned<C, TcpStream> {
        &mut self.stream
    }

    /// Consume into the raw TLS stream.
    pub fn into_stream(self) -> StreamOwned<C, TcpStream> {
        self.stream
    }
}

/// Server side: accept a TLS connection on an already-accepted `tcp`, completing
/// the handshake so the exporter is available.
///
/// # Errors
/// On a TLS or I/O failure during the handshake (`complete_io` errors on EOF).
pub fn accept(
    tcp: TcpStream,
    config: Arc<ServerConfig>,
) -> io::Result<TlsTransport<ServerConnection>> {
    let mut conn = ServerConnection::new(config).map_err(io_err)?;
    let mut tcp = tcp;
    while conn.is_handshaking() {
        conn.complete_io(&mut tcp)?;
    }
    let exporter: [u8; EXPORTER_LEN] = conn
        .export_keying_material([0u8; EXPORTER_LEN], EXPORTER_LABEL, None)
        .map_err(io_err)?;
    Ok(TlsTransport {
        stream: StreamOwned::new(conn, tcp),
        exporter,
    })
}

/// Client side: connect TLS over an already-connected `tcp` to a server whose cert
/// is pinned by `config`, completing the handshake so the exporter is available.
/// `server_name` is the SNI/name (any value; identity is by fingerprint, not name).
///
/// # Errors
/// On a TLS (incl. fingerprint mismatch) or I/O failure during the handshake.
pub fn connect(
    tcp: TcpStream,
    server_name: ServerName<'static>,
    config: Arc<ClientConfig>,
) -> io::Result<TlsTransport<ClientConnection>> {
    let mut conn = ClientConnection::new(config, server_name).map_err(io_err)?;
    let mut tcp = tcp;
    while conn.is_handshaking() {
        conn.complete_io(&mut tcp)?;
    }
    let exporter: [u8; EXPORTER_LEN] = conn
        .export_keying_material([0u8; EXPORTER_LEN], EXPORTER_LABEL, None)
        .map_err(io_err)?;
    Ok(TlsTransport {
        stream: StreamOwned::new(conn, tcp),
        exporter,
    })
}

/// Poll granularity for the TLS side of [`relay`]. A TLS connection has no owned
/// read/write halves (the rustls `Connection` is shared state), so the relay
/// guards it with a `Mutex` and reads with this timeout: short enough that the
/// other direction is never starved of the lock for long, long enough not to spin
/// hot. A control protocol (await/screen/key) tolerates this latency.
const RELAY_POLL: Duration = Duration::from_millis(20);

/// Full-duplex relay between an authenticated TLS stream and a local `UnixStream`
/// (the control socket), until either side closes. This is the network analog of
/// `proxy.rs`'s local splice: the listener bridges the verified remote driver to
/// its own control socket; the dialer bridges its local control client to the
/// remote.
///
/// TLS cannot be split into owned read/write halves the way a `UnixStream` can
/// (the rustls `Connection` mixes both directions), so the TLS side is shared
/// behind a `Mutex`. To keep the two directions from starving each other for that
/// lock, the TLS socket is **non-blocking**: the downloader holds the lock only
/// for the instant of a read syscall (never blocking *inside* it) and sleeps
/// [`RELAY_POLL`] *outside* the lock, leaving it free for the uploader. The local
/// side splits with `try_clone` and carries a read timeout so the uploader polls
/// the teardown flag. `rustls`'s `Stream::write` buffers all plaintext before any
/// socket send, so a non-blocking would-block never drops data.
///
/// # Errors
/// On setup failure (nonblocking/timeout/clone) or an unexpected mid-stream I/O
/// error (a normal peer close — EOF / `close_notify` / reset — is not an error).
pub fn relay<C, S>(transport: TlsTransport<C>, local: UnixStream) -> io::Result<()>
where
    C: std::ops::DerefMut
        + std::ops::Deref<Target = rustls::ConnectionCommon<S>>
        + Send
        + 'static,
    S: rustls::SideData + 'static,
{
    let mut stream = transport.into_stream();
    stream.get_mut().set_nonblocking(true)?;
    let tls = Arc::new(Mutex::new(stream));
    let done = Arc::new(AtomicBool::new(false));

    // A read timeout lets the uploader poll `done` for teardown without relying on
    // a cross-thread shutdown to unblock a blocking read.
    local.set_read_timeout(Some(RELAY_POLL))?;
    let mut local_up = local.try_clone()?; // read local -> write TLS
    let mut local_down = local; // write local <- read TLS
    // Surfaces the first UNEXPECTED error from either direction (not a normal close).
    let err_slot: Arc<Mutex<Option<io::Error>>> = Arc::new(Mutex::new(None));

    // Uploader: local -> TLS. Reads the local socket (timeout-polled, no lock
    // held); grabs the TLS lock only to buffer+flush, which is fast for the small
    // messages of a control protocol.
    let up = {
        let tls = Arc::clone(&tls);
        let done = Arc::clone(&done);
        let err_slot = Arc::clone(&err_slot);
        std::thread::spawn(move || {
            let mut buf = [0u8; 16 * 1024];
            while !done.load(Ordering::Relaxed) {
                match local_up.read(&mut buf) {
                    Ok(0) => break, // local EOF
                    Ok(n) => {
                        let mut g = tls.lock().unwrap();
                        // write() buffers all plaintext (never drops it); flush()
                        // pushes it to the socket. A non-blocking would-block on
                        // flush means the bytes are queued in rustls and a later
                        // flush completes them — not an error.
                        if let Err(e) = g.write_all(&buf[..n]) {
                            drop(g);
                            record_err(&err_slot, e);
                            break;
                        }
                        match g.flush() {
                            Ok(()) => {}
                            Err(e) if is_would_block(&e) => {}
                            Err(e) => {
                                drop(g);
                                record_err(&err_slot, e);
                                break;
                            }
                        }
                    }
                    Err(e) if is_would_block(&e) => continue, // poll `done`
                    Err(e) if is_normal_close(&e) => break,
                    Err(e) => {
                        record_err(&err_slot, e);
                        break;
                    }
                }
            }
            done.store(true, Ordering::Relaxed);
        })
    };

    // Downloader: TLS -> local (this thread). The non-blocking read returns at
    // once; on would-block we release the lock and sleep, so the uploader is free
    // to take it.
    let mut buf = [0u8; 16 * 1024];
    while !done.load(Ordering::Relaxed) {
        let n = {
            let mut g = tls.lock().unwrap();
            match g.read(&mut buf) {
                Ok(n) => Ok(n),
                Err(e) if is_would_block(&e) => Ok(usize::MAX), // no data yet
                Err(e) => Err(e),
            }
        }; // lock released here
        match n {
            Ok(usize::MAX) => {
                std::thread::sleep(RELAY_POLL); // outside the lock
                continue;
            }
            Ok(0) => break, // TLS EOF / close_notify
            Ok(n) => {
                if let Err(e) = local_down.write_all(&buf[..n]).and_then(|()| local_down.flush()) {
                    record_err(&err_slot, e);
                    break;
                }
            }
            Err(e) => {
                record_err(&err_slot, e);
                break;
            }
        }
    }
    done.store(true, Ordering::Relaxed);
    // Unblock the uploader promptly (besides its read-timeout poll).
    let _ = local_down.shutdown(std::net::Shutdown::Both);
    let _ = up.join();

    match Arc::try_unwrap(err_slot)
        .map(Mutex::into_inner)
        .map(Result::unwrap)
    {
        Ok(Some(e)) => Err(e),
        _ => Ok(()),
    }
}

/// Record the first UNEXPECTED error from a relay direction (a normal peer close
/// is not recorded — it is the expected end of the relay).
fn record_err(slot: &Mutex<Option<io::Error>>, e: io::Error) {
    if !is_normal_close(&e) {
        let mut g = slot.lock().unwrap();
        if g.is_none() {
            *g = Some(e);
        }
    }
}

/// A read with no data yet on a non-blocking/timeout socket (not a true block).
fn is_would_block(e: &io::Error) -> bool {
    matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut)
}

/// A peer closing the connection — expected during teardown, not a relay failure.
fn is_normal_close(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::BrokenPipe
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::NotConnected
            | io::ErrorKind::UnexpectedEof
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Granted, present_capability, verify_capability};
    use aterm_session::EdgeToken;
    use std::net::{TcpListener, TcpStream};

    const TEST_CERT_DER: &[u8] = include_bytes!("testdata/cert.der");
    const TEST_KEY_DER: &[u8] = include_bytes!("testdata/key.pkcs8.der");

    fn test_server_name() -> ServerName<'static> {
        ServerName::try_from("aterm-net-test").unwrap()
    }

    #[test]
    fn tls_handshake_exporters_match_and_a_wrong_pin_is_rejected() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let scfg = server_config(TEST_CERT_DER.to_vec(), TEST_KEY_DER.to_vec()).unwrap();

        // Server thread: accept TLS, return the exporter.
        let srv = std::thread::spawn(move || {
            let (tcp, _) = listener.accept().unwrap();
            let t = accept(tcp, scfg).unwrap();
            t.exporter().to_vec()
        });

        // Client: pin the REAL fingerprint -> handshake succeeds, exporter matches.
        let pin = cert_fingerprint(TEST_CERT_DER);
        let ccfg = client_config(pin);
        let tcp = TcpStream::connect(addr).unwrap();
        let ct = connect(tcp, test_server_name(), ccfg).unwrap();
        let client_exporter = ct.exporter().to_vec();

        let server_exporter = srv.join().unwrap();
        assert_eq!(
            client_exporter, server_exporter,
            "RFC 5705 exporter must be identical on both ends"
        );
        assert_eq!(client_exporter.len(), EXPORTER_LEN);
        assert_ne!(client_exporter, [0u8; EXPORTER_LEN], "exporter is real key material");
    }

    #[test]
    fn end_to_end_tls_capability_handshake_then_relays_a_control_exchange() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let scfg = server_config(TEST_CERT_DER.to_vec(), TEST_KEY_DER.to_vec()).unwrap();
        let token = EdgeToken::generate(); // Copy: both ends use the same token
        let pin = cert_fingerprint(TEST_CERT_DER);

        // The "service" behind the listener's control socket: a byte echo. The
        // relay bridges the verified TLS peer to `svc_a`; `svc_b` echoes.
        let (svc_a, mut svc_b) = UnixStream::pair().unwrap();
        let echo = std::thread::spawn(move || {
            let mut buf = [0u8; 1024];
            loop {
                match svc_b.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if svc_b.write_all(&buf[..n]).and_then(|()| svc_b.flush()).is_err() {
                            break;
                        }
                    }
                }
            }
        });

        // Listener: accept TLS, verify the channel-bound capability, then relay.
        let srv = std::thread::spawn(move || {
            let (tcp, _) = listener.accept().unwrap();
            let mut t = accept(tcp, scfg).unwrap();
            let exporter = t.exporter().to_vec();
            let granted = verify_capability(t.stream(), &exporter, |src, op| {
                (src == "driver-1" && op == "drive").then_some(token)
            })
            .unwrap();
            assert_eq!(granted, Granted { src: "driver-1".into(), op: "drive".into() });
            relay(t, svc_a).unwrap();
        });

        // Dialer: connect TLS (pinned), present the capability, exchange, close.
        let ccfg = client_config(pin);
        let tcp = TcpStream::connect(addr).unwrap();
        let mut ct = connect(tcp, test_server_name(), ccfg).unwrap();
        let exporter = ct.exporter().to_vec();
        present_capability(ct.stream(), &exporter, "driver-1", "drive", &token).unwrap();

        // Drive a control round-trip THROUGH the relay (TLS -> svc_a -> echo -> back).
        ct.stream().write_all(b"ping\n").unwrap();
        ct.stream().flush().unwrap();
        let mut got = [0u8; 5];
        ct.stream().read_exact(&mut got).unwrap();
        assert_eq!(&got, b"ping\n", "the relay carried the control exchange round-trip");

        // Clean close_notify -> the listener's relay sees a clean EOF and returns.
        {
            let s = ct.stream();
            s.conn.send_close_notify();
            let _ = s.flush();
        }
        drop(ct);
        srv.join().unwrap();
        let _ = echo.join();
    }

    #[test]
    fn a_wrong_fingerprint_pin_rejects_the_handshake() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let scfg = server_config(TEST_CERT_DER.to_vec(), TEST_KEY_DER.to_vec()).unwrap();
        let srv = std::thread::spawn(move || {
            if let Ok((tcp, _)) = listener.accept() {
                let _ = accept(tcp, scfg); // expected to fail (client aborts)
            }
        });
        // Pin a DIFFERENT fingerprint -> the client must refuse the server cert.
        let mut wrong = cert_fingerprint(TEST_CERT_DER);
        wrong[0] ^= 0xff;
        let ccfg = client_config(wrong);
        let tcp = TcpStream::connect(addr).unwrap();
        let res = connect(tcp, test_server_name(), ccfg);
        assert!(res.is_err(), "a mismatched fingerprint pin must reject the handshake");
        let _ = srv.join();
    }
}
