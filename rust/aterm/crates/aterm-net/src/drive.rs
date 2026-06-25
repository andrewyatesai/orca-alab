// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Andrew Yates

//! The network drive — both ends of "one aterm drives another over the network",
//! composed from the tested pieces: [`tls`](crate::tls) (channel) +
//! [`verify_capability`]/[`present_capability`] (the channel-bound capability) +
//! [`relay`](crate::tls::relay) (the byte bridge).
//!
//! * **Listener** ([`accept_and_relay`], [`serve`]) — the host being driven.
//!   Accepts TLS with an operator cert, verifies the dialer's channel-bound
//!   capability, then relays the connection to its **local control socket**. The
//!   remote driver thereafter speaks the ordinary control protocol; the TLS
//!   capability is the network-specific gate that replaces the local same-uid
//!   `SO_PEERCRED` check (which has no network analog).
//! * **Driver** ([`dial_and_relay`]) — the host doing the driving. Dials a pinned
//!   endpoint, presents the channel-bound capability, then relays a local control
//!   client to the remote. The network analog of `proxy::connect_and_relay`.
//!
//! **Secure-default-OFF**: nothing here runs unless a caller explicitly stands up
//! a listener with an operator-provided cert+key and a capability lookup. There is
//! no implicit bind, no default port, no ambient authority.

use std::io;
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use aterm_session::EdgeToken;
use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ServerConfig};

use crate::tls::{self, TlsTransport};
use crate::{Granted, present_capability, verify_capability};

/// What happened on one accepted connection — surfaced to the caller's logger so
/// the network listener has an auditable trail (mirrors the local socket's
/// `log_denial`). Never carries a secret.
#[derive(Clone, Debug)]
pub enum NetEvent {
    /// A dialer verified and was relayed to the local control socket.
    Relayed(Granted),
    /// A connection was refused (bad handshake, denied capability, dial error).
    /// The string is a non-sensitive reason for the audit log.
    Rejected(String),
}

/// Total wall-clock deadline on the UNAUTHENTICATED phase of an accepted
/// connection — the TLS handshake plus the `AUTH` line read. Enforced two ways
/// (see [`accept_and_relay_inner`]): a per-syscall `SO_RCVTIMEO` floor bounds a
/// fully-idle read, and a watchdog force-closes the socket at this total deadline
/// so a peer that DRIBBLES bytes (which would keep resetting the per-syscall
/// timer) is still cut off. Neither pins a thread past this bound; the deadline is
/// lifted once the capability verifies, before the long-lived relay.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// Maximum connections [`serve`] keeps in flight at once (handshaking + relaying).
/// A flood beyond this is refused rather than allowed to spawn unbounded threads
/// and file descriptors. Bounds total resource use to `MAX_INFLIGHT × (2 threads
/// + 2 fds)`.
const MAX_INFLIGHT: usize = 64;

/// How often [`serve`]'s accept loop wakes to re-check its `running` flag, so the
/// kill-switch is observed promptly instead of only when the next peer connects.
const ACCEPT_POLL: Duration = Duration::from_millis(200);

/// Decrements the in-flight counter when a connection's handler thread ends
/// (including on panic), so a refused/finished connection always frees its slot.
struct InFlightGuard(Arc<AtomicUsize>);
impl Drop for InFlightGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

// Unauthenticated-phase watchdog state. A per-syscall `SO_RCVTIMEO` only bounds an
// IDLE stall — a peer that dribbles one byte just under the timeout keeps resetting
// it. So a watchdog thread enforces a TOTAL WALL-CLOCK deadline: after it elapses it
// force-closes the socket unless the handshake+AUTH already finished. The tri-state
// is claimed by exactly one side (CAS), so the watchdog never shuts a socket the
// relay is about to use.
const WD_RUNNING: u8 = 0;
const WD_AUTHED: u8 = 1;
const WD_FIRED: u8 = 2;
/// How often the watchdog wakes to check whether the unauth phase finished early.
const WD_STEP: Duration = Duration::from_millis(50);

/// Listener side, ONE connection: complete the TLS handshake on `tcp` with the
/// operator `config`, verify the dialer's channel-bound capability via `lookup`,
/// and on success relay the connection to the local control socket from
/// `connect_local`. The capability is checked BEFORE the local socket is dialed,
/// so an unauthorized peer never reaches it.
///
/// `lookup(src, op)` returns the [`EdgeToken`] the host minted for that driver+op
/// (`None` ⇒ no such grant ⇒ denied). `connect_local` dials the host's own
/// control socket (e.g. `UnixStream::connect(sock_path)`).
///
/// # Errors
/// On a TLS/handshake failure, a denied capability, or a local-socket dial error
/// — in every case before (or without) any relay.
pub fn accept_and_relay<F, G>(
    tcp: TcpStream,
    config: Arc<ServerConfig>,
    lookup: F,
    connect_local: G,
) -> io::Result<Granted>
where
    F: FnOnce(&str, &str) -> Option<EdgeToken>,
    G: FnOnce() -> io::Result<UnixStream>,
{
    accept_and_relay_inner(tcp, config, lookup, connect_local, HANDSHAKE_TIMEOUT)
}

/// [`accept_and_relay`] with the unauthenticated-phase deadline injected (tests
/// use a short value to exercise the slow-loris timeout without a real wait).
fn accept_and_relay_inner<F, G>(
    tcp: TcpStream,
    config: Arc<ServerConfig>,
    lookup: F,
    connect_local: G,
    handshake_timeout: Duration,
) -> io::Result<Granted>
where
    F: FnOnce(&str, &str) -> Option<EdgeToken>,
    G: FnOnce() -> io::Result<UnixStream>,
{
    // Per-syscall floor: bounds a single fully-idle read/write (defense in depth).
    tcp.set_read_timeout(Some(handshake_timeout))?;
    tcp.set_write_timeout(Some(handshake_timeout))?;

    // Total wall-clock deadline on the whole unauthenticated phase (handshake +
    // AUTH read): a watchdog force-closes a clone of the socket once the deadline
    // elapses, so a peer that DRIBBLES bytes (resetting the per-syscall timer) is
    // still cut off. Exactly one of {authed, fired} wins the CAS, so the watchdog
    // never shuts a socket the relay will use.
    let state = Arc::new(AtomicU8::new(WD_RUNNING));
    let watchdog = {
        let wd_sock = tcp.try_clone()?;
        let wd_state = Arc::clone(&state);
        std::thread::spawn(move || {
            let start = Instant::now();
            while start.elapsed() < handshake_timeout {
                if wd_state.load(Ordering::Acquire) != WD_RUNNING {
                    return; // finished early — nothing to cut off
                }
                std::thread::sleep(WD_STEP);
            }
            // Deadline reached: claim FIRED iff still running, then force-close so
            // the blocked handshake/AUTH read errors out.
            if wd_state
                .compare_exchange(WD_RUNNING, WD_FIRED, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                let _ = wd_sock.shutdown(std::net::Shutdown::Both);
            }
        })
    };

    // The unauthenticated phase. Any error here (incl. the watchdog's force-close)
    // tears the connection down.
    let unauth = (|| -> io::Result<(TlsTransport<rustls::ServerConnection>, Granted)> {
        let mut transport = tls::accept(tcp, config)?;
        let exporter = transport.exporter().to_vec();
        let granted = verify_capability(transport.stream(), &exporter, lookup)?;
        Ok((transport, granted))
    })();
    // Claim AUTHED iff the watchdog has not already fired; then join it (so its
    // socket clone is dropped before we touch the connection again).
    let authed_in_time = state
        .compare_exchange(WD_RUNNING, WD_AUTHED, Ordering::AcqRel, Ordering::Acquire)
        .is_ok();
    let _ = watchdog.join();

    let (mut transport, granted) = match unauth {
        Ok(v) if authed_in_time => v,
        Ok(_) => return Err(io::Error::other("handshake deadline exceeded")),
        Err(e) => return Err(e),
    };

    // Authenticated: drop the handshake deadline before the (long-lived) relay,
    // which installs its own non-blocking poll on the same socket.
    {
        let sock = transport.stream().get_mut();
        sock.set_read_timeout(None)?;
        sock.set_write_timeout(None)?;
    }
    // NOW (and only now) dial the local control socket and bridge the two.
    let local = connect_local()?;
    tls::relay(transport, local)?;
    Ok(granted)
}

/// Serve a bound `TcpListener` until `running` is cleared: one thread per
/// connection, each running [`accept_and_relay`]. A per-connection failure is
/// reported via `on_event` (audit) and never stops the loop — a hostile peer
/// cannot take the listener down by failing a handshake.
///
/// **DoS bounds.** At most [`MAX_INFLIGHT`] connections are handled concurrently;
/// a flood beyond that is refused (logged, dropped) rather than allowed to spawn
/// unbounded threads/fds. Each accepted connection's unauthenticated phase is
/// deadline-bounded inside [`accept_and_relay`] ([`HANDSHAKE_TIMEOUT`]), so a
/// slow-loris cannot pin a handler thread. (An AUTHENTICATED peer that then idles
/// holds one slot until it closes; the cap bounds the worst case — these are
/// token-holders the operator already trusts.)
///
/// **Shutdown.** The listener is set non-blocking and the accept loop polls
/// `running` every [`ACCEPT_POLL`], so clearing it stops the loop promptly (a
/// plain blocking `accept` would only notice on the next connection).
///
/// `lookup` and `connect_local` are cloned per connection, so wrap shared state in
/// `Arc` on the caller side.
pub fn serve<F, G, E>(
    listener: &TcpListener,
    config: &Arc<ServerConfig>,
    lookup: F,
    connect_local: G,
    running: &Arc<AtomicBool>,
    on_event: E,
) where
    F: Fn(&str, &str) -> Option<EdgeToken> + Send + Sync + Clone + 'static,
    G: Fn() -> io::Result<UnixStream> + Send + Sync + Clone + 'static,
    E: Fn(NetEvent) + Send + Sync + Clone + 'static,
{
    // Non-blocking accept so the `running` kill-switch is observed on a timer, not
    // only when the next peer connects.
    let _ = listener.set_nonblocking(true);
    let inflight = Arc::new(AtomicUsize::new(0));

    while running.load(Ordering::Relaxed) {
        let tcp = match listener.accept() {
            Ok((s, _)) => s,
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(ACCEPT_POLL);
                continue;
            }
            // A single accept error is not fatal — but back off, so a persistent
            // error that is NOT WouldBlock (e.g. EMFILE/ENFILE under fd pressure)
            // cannot busy-spin this loop at 100% CPU until an fd frees.
            Err(_) => {
                std::thread::sleep(ACCEPT_POLL);
                continue;
            }
        };
        // An accepted socket does NOT inherit the listener's non-blocking flag on
        // POSIX, but make it explicit: `accept_and_relay` needs blocking mode for
        // its SO_RCVTIMEO handshake deadline to take effect.
        let _ = tcp.set_nonblocking(false);

        // Concurrency cap: reserve a slot; if we are already at the ceiling, roll
        // back and refuse rather than spawn unbounded work.
        if inflight.fetch_add(1, Ordering::SeqCst) >= MAX_INFLIGHT {
            inflight.fetch_sub(1, Ordering::SeqCst);
            on_event(NetEvent::Rejected("listener at capacity".to_owned()));
            continue; // dropping `tcp` closes the connection
        }
        let guard = InFlightGuard(Arc::clone(&inflight));

        let config = Arc::clone(config);
        let lookup = lookup.clone();
        let connect_local = connect_local.clone();
        let on_event = on_event.clone();
        std::thread::spawn(move || {
            let _slot = guard; // released (slot freed) when this thread ends
            let ev = match accept_and_relay(tcp, config, lookup, connect_local) {
                Ok(granted) => NetEvent::Relayed(granted),
                Err(e) => NetEvent::Rejected(e.to_string()),
            };
            on_event(ev);
        });
    }
}

/// Driver side: dial the pinned remote `addr` over TLS (the cert is pinned by
/// `config` — see [`tls::client_config`]), present the channel-bound capability
/// for `(src, op)` keyed by `token`, then relay the local control client `local`
/// to the remote control server. The network analog of
/// `proxy::connect_and_relay`: past a successful present, bytes flow both ways
/// until either side closes.
///
/// `server_name` is the TLS SNI (identity is by cert fingerprint, not name, so any
/// stable value works).
///
/// **Identity pinning — what this enforces, and what it does not.** The server
/// CERTIFICATE fingerprint IS enforced: `config` (from [`tls::client_config`])
/// rejects the handshake unless the peer presents the pinned cert AND proves key
/// possession, so a redirected/MITM endpoint fails before any secret crosses the
/// wire. The session-NONCE half of the rebind guard
/// ([`RemoteEndpoint::matches`](crate::RemoteEndpoint::matches)) is NOT enforced
/// here — it needs a launch-identity exchange (the listener echoing its current
/// `LaunchNonce`) that no shipping dialer yet drives. Until then a caller that
/// wants the rebind guard MUST obtain the live nonce out of band and check
/// `matches` before calling this. (Channel binding still makes a stale capability
/// useless against a relaunched session: a different session ⇒ a different TLS
/// exporter ⇒ the tag fails to verify — so this is a defense-in-depth gap, not an
/// auth bypass.)
///
/// # Errors
/// On a connect/TLS/handshake failure (incl. cert-pin mismatch) or a denied
/// capability (before any relay). A relay-stage I/O error after a successful
/// present is returned too, but by then the capability HAS been accepted and bytes
/// may have flowed.
pub fn dial_and_relay<A: ToSocketAddrs>(
    addr: A,
    server_name: ServerName<'static>,
    config: Arc<ClientConfig>,
    src: &str,
    op: &str,
    token: &EdgeToken,
    local: UnixStream,
) -> io::Result<()> {
    let tcp = TcpStream::connect(addr)?;
    let mut transport: TlsTransport<_> = tls::connect(tcp, server_name, config)?;
    let exporter = transport.exporter().to_vec();
    present_capability(transport.stream(), &exporter, src, op, token)?;
    tls::relay(transport, local)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tls::{cert_fingerprint, client_config, server_config};
    use std::io::{Read, Write};
    use std::sync::Mutex;

    const TEST_CERT_DER: &[u8] = include_bytes!("testdata/cert.der");
    const TEST_KEY_DER: &[u8] = include_bytes!("testdata/key.pkcs8.der");

    fn name() -> ServerName<'static> {
        ServerName::try_from("aterm-net-test").unwrap()
    }

    /// Echo whatever arrives on `s` back to it, until EOF. Stands in for the
    /// host's local control socket.
    fn spawn_echo(mut s: UnixStream) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            let mut buf = [0u8; 1024];
            loop {
                match s.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if s.write_all(&buf[..n]).and_then(|()| s.flush()).is_err() {
                            break;
                        }
                    }
                }
            }
        })
    }

    #[test]
    fn driver_dials_listener_presents_capability_and_relays_to_the_local_socket() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let scfg = server_config(TEST_CERT_DER.to_vec(), TEST_KEY_DER.to_vec()).unwrap();
        let ccfg = client_config(cert_fingerprint(TEST_CERT_DER));
        let token = EdgeToken::generate();

        // Host: its "local control socket" is an echo over a UnixStream pair.
        let (svc_a, svc_b) = UnixStream::pair().unwrap();
        let echo = spawn_echo(svc_b);
        let svc_a = Arc::new(Mutex::new(Some(svc_a)));

        let host = std::thread::spawn({
            let scfg = Arc::clone(&scfg);
            let svc_a = Arc::clone(&svc_a);
            move || {
                let (tcp, _) = listener.accept().unwrap();
                accept_and_relay(
                    tcp,
                    scfg,
                    |src, op| (src == "driver-1" && op == "drive").then_some(token),
                    || Ok(svc_a.lock().unwrap().take().unwrap()),
                )
            }
        });

        // Driver: relays its local control client (the test holds the peer).
        let (drv_local, mut drv_client) = UnixStream::pair().unwrap();
        let driver = std::thread::spawn(move || {
            dial_and_relay(addr, name(), ccfg, "driver-1", "drive", &token, drv_local)
        });

        // Drive a control exchange end-to-end: client -> driver -> TLS -> host ->
        // local socket (echo) -> back.
        drv_client.write_all(b"screen\n").unwrap();
        drv_client.flush().unwrap();
        let mut got = [0u8; 7];
        drv_client.read_exact(&mut got).unwrap();
        assert_eq!(&got, b"screen\n", "the remote drive round-tripped a control verb");

        // Close the local client -> both relays tear down, both ends return.
        drv_client.shutdown(std::net::Shutdown::Both).unwrap();
        drop(drv_client);

        let granted = host.join().unwrap().unwrap();
        assert_eq!(granted, Granted { src: "driver-1".into(), op: "drive".into() });
        driver.join().unwrap().ok();
        echo.join().ok();
    }

    #[test]
    fn a_stalled_pre_auth_peer_is_dropped_by_the_handshake_deadline() {
        // A peer that opens TCP and then sends NOTHING must not pin the handler
        // thread: the unauthenticated-phase deadline fires and accept_and_relay
        // returns an error WITHOUT ever dialing the local socket.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let scfg = server_config(TEST_CERT_DER.to_vec(), TEST_KEY_DER.to_vec()).unwrap();
        let token = EdgeToken::generate();
        let dialed_local = Arc::new(AtomicBool::new(false));

        let host = std::thread::spawn({
            let dialed_local = Arc::clone(&dialed_local);
            move || {
                let (tcp, _) = listener.accept().unwrap();
                // 300ms deadline (vs the 10s default) so the test is fast.
                accept_and_relay_inner(
                    tcp,
                    scfg,
                    |_s, _o| Some(token),
                    || {
                        dialed_local.store(true, Ordering::SeqCst);
                        UnixStream::pair().map(|(a, _b)| a)
                    },
                    Duration::from_millis(300),
                )
            }
        });

        // Connect, then stall (send nothing, hold the socket open).
        let _stalled = TcpStream::connect(addr).unwrap();
        let started = Instant::now();
        let res = host.join().unwrap();
        assert!(res.is_err(), "a stalled pre-auth peer must be dropped, not relayed");
        assert!(
            !dialed_local.load(Ordering::SeqCst),
            "a peer that never authenticated must NEVER dial the local socket"
        );
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the handshake deadline must fire promptly (~300ms), not hang"
        );
    }

    #[test]
    fn a_dribbling_pre_auth_peer_is_cut_off_by_the_wall_clock_deadline() {
        // The HARDER slow-loris: a peer that keeps the per-syscall timer alive by
        // dribbling bytes must still be cut off by the TOTAL wall-clock deadline.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let scfg = server_config(TEST_CERT_DER.to_vec(), TEST_KEY_DER.to_vec()).unwrap();
        let token = EdgeToken::generate();
        let dialed_local = Arc::new(AtomicBool::new(false));

        let host = std::thread::spawn({
            let dialed_local = Arc::clone(&dialed_local);
            move || {
                let (tcp, _) = listener.accept().unwrap();
                accept_and_relay_inner(
                    tcp,
                    scfg,
                    |_s, _o| Some(token),
                    || {
                        dialed_local.store(true, Ordering::SeqCst);
                        UnixStream::pair().map(|(a, _b)| a)
                    },
                    Duration::from_millis(300),
                )
            }
        });

        // A valid TLS record HEADER (handshake, len=4096) so rustls keeps waiting
        // for the body — then dribble it 1 byte / 80ms, well under the 300ms
        // per-syscall timeout, so ONLY the wall-clock watchdog can stop it.
        let mut peer = TcpStream::connect(addr).unwrap();
        let started = Instant::now();
        let dribbler = std::thread::spawn(move || {
            let _ = peer.write_all(&[0x16, 0x03, 0x03, 0x10, 0x00]);
            let _ = peer.flush();
            for _ in 0..40 {
                if peer.write_all(&[0x00]).and_then(|()| peer.flush()).is_err() {
                    break; // server force-closed -> our writes start failing
                }
                std::thread::sleep(Duration::from_millis(80));
            }
        });

        let res = host.join().unwrap();
        let elapsed = started.elapsed();
        assert!(res.is_err(), "a dribbling pre-auth peer must be cut off, not relayed");
        assert!(
            !dialed_local.load(Ordering::SeqCst),
            "a dribbling unauthenticated peer must NEVER dial the local socket"
        );
        assert!(
            elapsed >= Duration::from_millis(200),
            "the cut-off must be the wall-clock watchdog (~300ms), not an instant reject ({elapsed:?})"
        );
        assert!(
            elapsed < Duration::from_secs(3),
            "the wall-clock deadline must bound the dribble, not run unbounded ({elapsed:?})"
        );
        let _ = dribbler.join();
    }

    #[test]
    fn a_denied_capability_never_reaches_the_local_socket() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let scfg = server_config(TEST_CERT_DER.to_vec(), TEST_KEY_DER.to_vec()).unwrap();
        let ccfg = client_config(cert_fingerprint(TEST_CERT_DER));
        let host_token = EdgeToken::generate();
        let driver_token = EdgeToken::generate(); // WRONG token -> forgery

        // If the capability is denied, `connect_local` must NEVER be called.
        let dialed_local = Arc::new(AtomicBool::new(false));
        let host = std::thread::spawn({
            let scfg = Arc::clone(&scfg);
            let dialed_local = Arc::clone(&dialed_local);
            move || {
                let (tcp, _) = listener.accept().unwrap();
                accept_and_relay(
                    tcp,
                    scfg,
                    |src, op| (src == "driver-1" && op == "drive").then_some(host_token),
                    || {
                        dialed_local.store(true, Ordering::SeqCst);
                        UnixStream::pair().map(|(a, _b)| a)
                    },
                )
            }
        });

        let (drv_local, _drv_client) = UnixStream::pair().unwrap();
        let driver = std::thread::spawn(move || {
            dial_and_relay(addr, name(), ccfg, "driver-1", "drive", &driver_token, drv_local)
        });

        assert!(host.join().unwrap().is_err(), "a forged capability is rejected");
        assert!(driver.join().unwrap().is_err(), "the dialer sees the denial");
        assert!(
            !dialed_local.load(Ordering::SeqCst),
            "a denied capability must NEVER dial the local control socket"
        );
    }
}
