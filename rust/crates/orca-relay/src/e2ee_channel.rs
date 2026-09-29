//! E2EE channel handshake + transport, ported from
//! `src/main/runtime/rpc/e2ee-channel.ts`.
//!
//! Sits between the WebSocket transport and the RPC handler: it owns the
//! handshake state machine and transparent encrypt/decrypt so the handler only
//! sees plaintext, identical to the Unix-socket path. Crypto is `orca-crypto`
//! (NaCl box). To stay pure/testable, the channel is a reducer: every input
//! returns a list of [`E2eeEffect`]s the transport owner executes (send /
//! deliver / close), and the WebSocket, the handshake timer, and the nonce RNG
//! are injected at the edge — no IO here.
//!
//! Teardown is modeled in [`ChannelState`], not in a side-band flag. The TS
//! original survives the same structural hole (its `destroy()` also leaves
//! `state` at `awaiting_hello`) only because its owner nulls the delivery
//! callbacks and drops the channel from a map. A pure reducer has neither, so
//! `Closed` is absorbing here and every fatal `Error` latches it: the reducer
//! enforces its own termination instead of trusting the caller to stop calling.

use crate::base64;
use orca_crypto::{
    decrypt_bytes, derive_shared_box, encrypt_bytes_with_nonce, SharedBox, NONCE_BYTES,
    PUBLIC_KEY_BYTES,
};
use serde_json::Value;
use std::collections::{BTreeSet, VecDeque};
use zeroize::Zeroizing;

const MAX_CONSECUTIVE_DECRYPT_FAILURES: u32 = 5;
/// Handshake watchdog the owner arms a timer for; on fire it calls
/// [`E2eeChannel::on_handshake_timeout`].
pub const HANDSHAKE_TIMEOUT_MS: u64 = 10_000;
/// Owner-side backpressure cap for buffered binary frames (the channel emits
/// the frame; the owner drops it if its socket is over this).
pub const MAX_BINARY_BUFFERED_AMOUNT: usize = 8 * 1024 * 1024;
/// Recent-nonce window for the v1 replay guard (TS `MobileE2EEV1ReplayGuard`).
/// Bounds worst-case memory (~400 KB) so a peer cannot grow the set unbounded.
const MAX_TRACKED_INBOUND_NONCES: usize = 8192;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChannelState {
    AwaitingHello,
    AwaitingAuth,
    Ready,
    /// Absorbing. Reached by `destroy()` and by every fatal error the reducer
    /// emits; no transition leaves it and no input produces an effect from it.
    Closed,
}

/// An inbound message off the transport: a text frame or a binary frame.
pub enum RawMessage<'a> {
    Text(&'a str),
    Binary(&'a [u8]),
}

/// Side effects the transport owner executes. Pure substitute for the TS
/// `ws.send` / `onReady` / `onError` / message-handler callbacks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum E2eeEffect {
    /// Send a text frame on the socket (plaintext control or encrypted payload).
    SendText(String),
    /// Send a binary frame (encrypted payload).
    SendBinary(Vec<u8>),
    /// Deliver a decrypted text message to the RPC handler.
    DeliverText(String),
    /// Deliver a decrypted binary message to the RPC handler.
    DeliverBinary(Vec<u8>),
    /// Handshake completed; the channel is authenticated and ready.
    Ready,
    /// Fatal: the owner should close the socket with this code/reason. The
    /// channel has already latched `Closed`, so the close is belt-and-braces.
    Error { code: u16, reason: String },
}

fn error(code: u16, reason: &str) -> E2eeEffect {
    E2eeEffect::Error { code, reason: reason.to_string() }
}

/// Where a *decrypted* payload goes. Split out as a pure function so "plaintext
/// never leaves the channel before authentication" is a postcondition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PlaintextRoute {
    Auth,
    DeliverText,
    DeliverBinary,
    ProtocolError,
}

// Trust contract: inert unless verification runs; checked under `targo trust`.
// Postcondition — no delivery route exists outside `Ready`. This is the
// "no Deliver* before Ready" safety property; routing through one total pure
// function means a later opcode cannot re-derive it by hand and get it wrong.
fn plaintext_route(state: ChannelState, is_binary: bool) -> PlaintextRoute {
    match (state, is_binary) {
        (ChannelState::Ready, true) => PlaintextRoute::DeliverBinary,
        (ChannelState::Ready, false) => PlaintextRoute::DeliverText,
        (ChannelState::AwaitingAuth, false) => PlaintextRoute::Auth,
        _ => PlaintextRoute::ProtocolError,
    }
}

// Postcondition — `Closed` is absorbing, and otherwise the requested advance is
// honored verbatim. Every `self.state = …` write goes through here, so the
// terminality of `destroy()` holds at the assignment level independently of the
// message-path guard: even a caller that reached a transition directly cannot
// move a closed channel.
fn next_state(before: ChannelState, advance: ChannelState) -> ChannelState {
    if before == ChannelState::Closed {
        ChannelState::Closed
    } else {
        advance
    }
}

// Postcondition — input is accepted in exactly the non-closed states. The gate
// in front of the whole message path; a new opcode added inside `dispatch_open`
// is behind it by construction.
fn accepts_input(state: ChannelState) -> bool {
    state != ChannelState::Closed
}

// Postcondition — replies are sealed only from `Ready`, so neither a destroyed
// channel nor an unauthenticated one can have owner data encrypted to its peer.
fn may_encrypt_reply(state: ChannelState) -> bool {
    state == ChannelState::Ready
}

/// v1 replay guard, ported from `src/main/runtime/rpc/mobile-e2ee-v1-replay-guard.ts`.
///
/// v1 framing is a random-nonce NaCl box with no sequence counter (unlike v2's
/// monotonic per-direction counter), so a verbatim replayed ciphertext decrypts
/// cleanly and would re-run the method. Nonce-uniqueness is the only in-band
/// defense that needs no wire or client change. `BTreeSet` rather than a hash
/// set: no RNG seeding, deterministic, and O(log n) on the frame hot path.
struct V1ReplayGuard {
    seen: BTreeSet<[u8; NONCE_BYTES]>,
    order: VecDeque<[u8; NONCE_BYTES]>,
}

impl V1ReplayGuard {
    fn new() -> Self {
        Self { seen: BTreeSet::new(), order: VecDeque::new() }
    }

    /// `true` when the bundle's nonce is new. `false` on a replay or a frame too
    /// short to carry a nonce (which `decrypt_bytes` would reject anyway).
    fn accept(&mut self, bundle: &[u8]) -> bool {
        let Some(slice) = bundle.get(..NONCE_BYTES) else {
            return false;
        };
        let Ok(nonce) = <[u8; NONCE_BYTES]>::try_from(slice) else {
            return false;
        };
        if !self.seen.insert(nonce) {
            return false;
        }
        self.order.push_back(nonce);
        if self.order.len() > MAX_TRACKED_INBOUND_NONCES {
            if let Some(oldest) = self.order.pop_front() {
                self.seen.remove(&oldest);
            }
        }
        true
    }

    fn clear(&mut self) {
        self.seen.clear();
        self.order.clear();
    }
}

type ValidateToken = Box<dyn Fn(&str) -> bool>;
type NonceSource = Box<dyn FnMut() -> [u8; NONCE_BYTES]>;

pub struct E2eeChannel {
    state: ChannelState,
    shared_box: Option<SharedBox>,
    consecutive_failures: u32,
    replay_guard: V1ReplayGuard,
    // Long-lived NaCl secret; wiped on drop, and on close so a torn-down channel
    // has nothing left to derive a shared box from.
    server_secret_key: Zeroizing<Vec<u8>>,
    device_token: Option<String>,
    validate_token: ValidateToken,
    next_nonce: NonceSource,
}

impl E2eeChannel {
    /// `validate_token` and `next_nonce` are the injected boundaries: token
    /// authorization and a unique-nonce source (OS RNG in production, a counter
    /// in tests). `server_secret_key` is our 32-byte NaCl secret key.
    pub fn new(server_secret_key: Vec<u8>, validate_token: ValidateToken, next_nonce: NonceSource) -> Self {
        Self {
            state: ChannelState::AwaitingHello,
            shared_box: None,
            consecutive_failures: 0,
            replay_guard: V1ReplayGuard::new(),
            server_secret_key: Zeroizing::new(server_secret_key),
            device_token: None,
            validate_token,
            next_nonce,
        }
    }

    pub fn device_token(&self) -> Option<&str> {
        self.device_token.as_deref()
    }

    /// The one door in. Keep it a guard plus a delegation: the closed check must
    /// stay ahead of *every* branch, including opcodes added later, so the
    /// dispatch body lives in `dispatch_open` where it cannot run when closed.
    pub fn handle_raw_message(&mut self, raw: RawMessage) -> Vec<E2eeEffect> {
        if !accepts_input(self.state) {
            return Vec::new();
        }
        self.dispatch_open(raw)
    }

    fn dispatch_open(&mut self, raw: RawMessage) -> Vec<E2eeEffect> {
        if self.state == ChannelState::AwaitingHello {
            return match raw {
                RawMessage::Text(text) => self.handle_hello(text),
                RawMessage::Binary(_) => self.fatal(4001, "Invalid handshake message"),
            };
        }
        // Defensive: unreachable while `Closed` is absorbing (the only other key
        // clear), but a keyless channel must never fall through to decryption.
        if self.shared_box.is_none() {
            return Vec::new();
        }
        match raw {
            RawMessage::Binary(bytes) => {
                let Some(plaintext) = self.open_frame(bytes) else {
                    return self.track_decrypt_failure();
                };
                self.route_plaintext(plaintext, true)
            }
            RawMessage::Text(text) => {
                let Some(bundle) = base64::decode(text) else {
                    return self.track_decrypt_failure();
                };
                let Some(plaintext) = self.open_frame(&bundle) else {
                    return self.track_decrypt_failure();
                };
                self.route_plaintext(plaintext, false)
            }
        }
    }

    fn route_plaintext(&mut self, plaintext: Vec<u8>, is_binary: bool) -> Vec<E2eeEffect> {
        match plaintext_route(self.state, is_binary) {
            PlaintextRoute::DeliverBinary => vec![E2eeEffect::DeliverBinary(plaintext)],
            PlaintextRoute::DeliverText => {
                vec![E2eeEffect::DeliverText(String::from_utf8_lossy(&plaintext).into_owned())]
            }
            PlaintextRoute::Auth => {
                let text = String::from_utf8_lossy(&plaintext).into_owned();
                self.handle_auth(&text)
            }
            PlaintextRoute::ProtocolError => {
                self.fatal(4001, "Invalid binary message before authentication")
            }
        }
    }

    /// Encrypt a text reply (the TS `encryptedReply`). `None` unless the channel
    /// is authenticated and live, so late streaming emits racing teardown — and
    /// any emit before authentication — are no-ops.
    pub fn encrypt_text_reply(&mut self, response: &str) -> Option<E2eeEffect> {
        if !may_encrypt_reply(self.state) {
            return None;
        }
        self.encrypt_text(response).map(E2eeEffect::SendText)
    }

    /// Encrypt a binary reply. The owner still applies its buffered-amount
    /// backpressure ([`MAX_BINARY_BUFFERED_AMOUNT`]) before sending.
    pub fn encrypt_binary_reply(&mut self, response: &[u8]) -> Option<E2eeEffect> {
        if !may_encrypt_reply(self.state) {
            return None;
        }
        // Key check before the nonce draw so a keyless channel does not burn the
        // nonce source; this borrow ends here, so it cannot overlap `next_nonce`.
        self.shared_box.as_ref()?;
        let nonce = (self.next_nonce)();
        let shared = self.shared_box.as_ref()?;
        encrypt_bytes_with_nonce(response, shared, &nonce).map(E2eeEffect::SendBinary)
    }

    /// Called when the owner's handshake timer fires. Fatal unless the handshake
    /// already completed (the TS "clear timer on ready") or the channel is
    /// closed. Firing it latches `Closed`, so a re-armed or duplicate timer
    /// cannot re-emit 4002 against a channel the owner already tore down.
    pub fn on_handshake_timeout(&mut self) -> Vec<E2eeEffect> {
        if self.state == ChannelState::Ready || self.state == ChannelState::Closed {
            return Vec::new();
        }
        self.fatal(4002, "E2EE handshake timeout")
    }

    /// Tear down. Terminal: after this no input on any sequence produces an
    /// effect or advances state, and no reply can be sealed.
    pub fn destroy(&mut self) {
        self.close();
    }

    /// The single teardown. Latches the absorbing state and drops every secret.
    fn close(&mut self) {
        self.state = next_state(self.state, ChannelState::Closed);
        self.shared_box = None;
        self.device_token = None;
        self.replay_guard.clear();
        // Drop the long-lived NaCl secret too: with no key material left, a hello
        // that somehow reached `handle_hello` could not re-derive a shared box.
        self.server_secret_key = Zeroizing::new(Vec::new());
    }

    /// Emit a fatal error *and* enforce it. The TS owner destroys the channel on
    /// every `onError`; the reducer has no owner, so it closes itself.
    fn fatal(&mut self, code: u16, reason: &str) -> Vec<E2eeEffect> {
        self.close();
        vec![error(code, reason)]
    }

    fn handle_hello(&mut self, raw: &str) -> Vec<E2eeEffect> {
        let Ok(value) = serde_json::from_str::<Value>(raw) else {
            return self.fatal(4001, "Invalid handshake message");
        };
        let is_hello = value.get("type").and_then(Value::as_str) == Some("e2ee_hello");
        let public_key_b64 =
            value.get("publicKeyB64").and_then(Value::as_str).filter(|key| !key.is_empty());
        let Some(public_key_b64) = public_key_b64.filter(|_| is_hello) else {
            return self.fatal(4001, "Invalid e2ee_hello");
        };
        let Some(client_public_key) =
            base64::decode(public_key_b64).filter(|key| key.len() == PUBLIC_KEY_BYTES)
        else {
            return self.fatal(4001, "Invalid public key");
        };
        let Some(shared) = derive_shared_box(&self.server_secret_key, &client_public_key) else {
            return self.fatal(4001, "Invalid public key");
        };
        self.shared_box = Some(shared);
        self.state = next_state(self.state, ChannelState::AwaitingAuth);
        // e2ee_ready is plaintext: the client needs it to know key exchange
        // succeeded before it can send encrypted authentication.
        vec![E2eeEffect::SendText(r#"{"type":"e2ee_ready"}"#.to_string())]
    }

    fn handle_auth(&mut self, plaintext: &str) -> Vec<E2eeEffect> {
        let token = serde_json::from_str::<Value>(plaintext)
            .ok()
            .filter(|value| value.get("type").and_then(Value::as_str) == Some("e2ee_auth"))
            .and_then(|value| value.get("deviceToken").and_then(Value::as_str).map(str::to_string))
            .filter(|token| !token.is_empty());
        let Some(token) = token else {
            return self.fail_auth("bad_auth", "Invalid e2ee_auth");
        };
        if !(self.validate_token)(&token) {
            return self.fail_auth("unauthorized", "Unauthorized");
        }
        self.device_token = Some(token);
        self.state = next_state(self.state, ChannelState::Ready);
        let mut effects = Vec::new();
        effects.extend(self.encrypt_text(r#"{"type":"e2ee_authenticated"}"#).map(E2eeEffect::SendText));
        effects.push(E2eeEffect::Ready);
        effects
    }

    /// A rejected authentication ends the channel: the reducer previously left
    /// `AwaitingAuth` intact with the shared key held, so the peer could retry
    /// forever — and because a wrong-token frame decrypts cleanly, the decrypt
    /// cap could never fire against it. The client already treats an auth
    /// failure as socket-fatal and reconnects with a fresh keypair.
    fn fail_auth(&mut self, code: &str, reason: &str) -> Vec<E2eeEffect> {
        let control = serde_json::json!({ "type": "e2ee_error", "error": { "code": code } });
        // Seal before closing: `close()` drops the shared key, and the peer needs
        // a decryptable e2ee_error to show "re-pair" rather than an opaque hangup.
        let sealed = self.encrypt_text(&control.to_string()).map(E2eeEffect::SendText);
        let mut effects = Vec::new();
        effects.extend(sealed);
        effects.extend(self.fatal(4001, reason));
        effects
    }

    /// Encrypt `plaintext` to the base64 text-frame body. `None` if there is no
    /// shared key. Private: control frames (`e2ee_authenticated`, `e2ee_error`)
    /// legitimately seal outside `Ready`, unlike the public reply API.
    fn encrypt_text(&mut self, plaintext: &str) -> Option<String> {
        // Key check before the nonce draw so a keyless channel does not burn the
        // nonce source; this borrow ends here, so it cannot overlap `next_nonce`.
        self.shared_box.as_ref()?;
        let nonce = (self.next_nonce)();
        let shared = self.shared_box.as_ref()?;
        let bundle = encrypt_bytes_with_nonce(plaintext.as_bytes(), shared, &nonce)?;
        Some(base64::encode_standard(&bundle))
    }

    /// Replay-guarded open: nonce-uniqueness first (v1 carries no sequence
    /// counter), then the box. A success resets the failure budget.
    fn open_frame(&mut self, bundle: &[u8]) -> Option<Vec<u8>> {
        if !self.replay_guard.accept(bundle) {
            return None;
        }
        let shared = self.shared_box.as_ref()?;
        let plaintext = decrypt_bytes(bundle, shared)?;
        self.consecutive_failures = 0;
        Some(plaintext)
    }

    fn track_decrypt_failure(&mut self) -> Vec<E2eeEffect> {
        // TS parity (`trackDecryptFailure`): a wrong key cannot recover on this
        // socket, so a pre-auth failure is fatal at once. The free probe budget
        // the port handed out here was a pre-auth key-confirmation oracle.
        if self.state == ChannelState::AwaitingAuth {
            return self.fatal(4001, "Unauthorized");
        }
        self.consecutive_failures += 1;
        if self.consecutive_failures >= MAX_CONSECUTIVE_DECRYPT_FAILURES {
            self.fatal(4003, "Too many decryption failures")
        } else {
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orca_crypto::key_pair_from_seed;

    fn nonce_source() -> NonceSource {
        let mut counter: u8 = 0;
        Box::new(move || {
            counter = counter.wrapping_add(1);
            let mut nonce = [0u8; NONCE_BYTES];
            nonce[0] = counter;
            nonce
        })
    }

    struct Ctx {
        channel: E2eeChannel,
        server_public: [u8; 32],
        client_secret: [u8; 32],
        client_public: [u8; 32],
    }

    fn setup() -> Ctx {
        let server = key_pair_from_seed(&[1u8; 32]).unwrap();
        let client = key_pair_from_seed(&[2u8; 32]).unwrap();
        let channel =
            E2eeChannel::new(server.secret_key.to_vec(), Box::new(|t| t == "valid-token"), nonce_source());
        Ctx {
            channel,
            server_public: server.public_key,
            client_secret: client.secret_key,
            client_public: client.public_key,
        }
    }

    fn client_box(ctx: &Ctx) -> SharedBox {
        derive_shared_box(&ctx.client_secret, &ctx.server_public).unwrap()
    }

    fn attacker_box() -> SharedBox {
        derive_shared_box(
            &key_pair_from_seed(&[8u8; 32]).unwrap().secret_key,
            &key_pair_from_seed(&[9u8; 32]).unwrap().public_key,
        )
        .unwrap()
    }

    fn client_encrypt_text(shared: &SharedBox, plaintext: &str, nonce_byte: u8) -> String {
        let mut nonce = [0u8; NONCE_BYTES];
        nonce[0] = nonce_byte;
        base64::encode_standard(&encrypt_bytes_with_nonce(plaintext.as_bytes(), shared, &nonce).unwrap())
    }

    fn hello_frame(public_key: &[u8]) -> String {
        format!(r#"{{"type":"e2ee_hello","publicKeyB64":"{}"}}"#, base64::encode_standard(public_key))
    }

    fn auth_frame(shared: &SharedBox, token: &str, nonce_byte: u8) -> String {
        client_encrypt_text(
            shared,
            &format!(r#"{{"type":"e2ee_auth","deviceToken":"{token}"}}"#),
            nonce_byte,
        )
    }

    fn do_handshake(ctx: &mut Ctx) -> SharedBox {
        let shared = client_box(ctx);
        let hello = hello_frame(&ctx.client_public);
        ctx.channel.handle_raw_message(RawMessage::Text(&hello));
        let auth = client_encrypt_text(&shared, r#"{"type":"e2ee_auth","deviceToken":"valid-token"}"#, 1);
        ctx.channel.handle_raw_message(RawMessage::Text(&auth));
        shared
    }

    fn parse(text: &str) -> Value {
        serde_json::from_str(text).unwrap()
    }

    fn send_text(effect: &E2eeEffect) -> &str {
        match effect {
            E2eeEffect::SendText(text) => text,
            other => panic!("expected SendText, got {other:?}"),
        }
    }

    fn decrypt_send_text(effect: &E2eeEffect, shared: &SharedBox) -> String {
        let bundle = base64::decode(send_text(effect)).unwrap();
        String::from_utf8(decrypt_bytes(&bundle, shared).unwrap()).unwrap()
    }

    fn is_error(effect: &E2eeEffect) -> bool {
        matches!(effect, E2eeEffect::Error { .. })
    }

    fn is_delivery(effect: &E2eeEffect) -> bool {
        matches!(effect, E2eeEffect::DeliverText(_) | E2eeEffect::DeliverBinary(_))
    }

    #[test]
    fn server_secret_key_is_held_in_a_zeroizing_container() {
        let ctx = setup();
        // Compile-time pin: the long-lived secret must live in a zeroize-on-drop
        // container (not a bare Vec), while still holding the usable 32-byte key.
        let key: &Zeroizing<Vec<u8>> = &ctx.channel.server_secret_key;
        assert_eq!(key.len(), PUBLIC_KEY_BYTES);
    }

    #[test]
    fn completes_handshake_with_valid_encrypted_auth() {
        let mut ctx = setup();
        let shared = client_box(&ctx);

        let hello = hello_frame(&ctx.client_public);
        let hello_effects = ctx.channel.handle_raw_message(RawMessage::Text(&hello));
        let auth = client_encrypt_text(&shared, r#"{"type":"e2ee_auth","deviceToken":"valid-token"}"#, 1);
        let auth_effects = ctx.channel.handle_raw_message(RawMessage::Text(&auth));

        assert_eq!(parse(send_text(&hello_effects[0])), parse(r#"{"type":"e2ee_ready"}"#));
        assert_eq!(parse(&decrypt_send_text(&auth_effects[0], &shared)), parse(r#"{"type":"e2ee_authenticated"}"#));
        assert!(auth_effects.contains(&E2eeEffect::Ready));
        assert!(!hello_effects.iter().any(is_error) && !auth_effects.iter().any(is_error));
        assert_eq!(ctx.channel.device_token(), Some("valid-token"));
    }

    #[test]
    fn does_not_authenticate_from_plaintext_hello_alone() {
        let mut ctx = setup();
        let hello = hello_frame(&ctx.client_public);
        let effects = ctx.channel.handle_raw_message(RawMessage::Text(&hello));

        assert!(!effects.contains(&E2eeEffect::Ready));
        assert_eq!(parse(send_text(&effects[0])), parse(r#"{"type":"e2ee_ready"}"#));
    }

    #[test]
    fn rejects_invalid_encrypted_token() {
        let mut ctx = setup();
        let shared = client_box(&ctx);
        let hello = hello_frame(&ctx.client_public);
        ctx.channel.handle_raw_message(RawMessage::Text(&hello));
        let auth = client_encrypt_text(&shared, r#"{"type":"e2ee_auth","deviceToken":"bad-token"}"#, 1);
        let effects = ctx.channel.handle_raw_message(RawMessage::Text(&auth));

        assert!(effects.contains(&error(4001, "Unauthorized")));
        assert!(!effects.contains(&E2eeEffect::Ready));
    }

    #[test]
    fn rejects_malformed_json() {
        let mut ctx = setup();
        let effects = ctx.channel.handle_raw_message(RawMessage::Text("not json"));
        assert_eq!(effects, vec![error(4001, "Invalid handshake message")]);
    }

    #[test]
    fn rejects_missing_fields() {
        let mut ctx = setup();
        let effects = ctx.channel.handle_raw_message(RawMessage::Text(r#"{"type":"e2ee_hello"}"#));
        assert_eq!(effects, vec![error(4001, "Invalid e2ee_hello")]);
    }

    #[test]
    fn rejects_invalid_public_key_length() {
        let mut ctx = setup();
        let hello = format!(r#"{{"type":"e2ee_hello","publicKeyB64":"{}"}}"#, base64::encode_standard(b"short"));
        let effects = ctx.channel.handle_raw_message(RawMessage::Text(&hello));
        assert_eq!(effects, vec![error(4001, "Invalid public key")]);
    }

    #[test]
    fn times_out_if_no_hello_received() {
        let mut ctx = setup();
        assert_eq!(ctx.channel.on_handshake_timeout(), vec![error(4002, "E2EE handshake timeout")]);
    }

    #[test]
    fn clears_timeout_after_successful_handshake() {
        let mut ctx = setup();
        do_handshake(&mut ctx);
        assert_eq!(ctx.channel.on_handshake_timeout(), Vec::new());
    }

    #[test]
    fn decrypts_and_forwards_messages() {
        let mut ctx = setup();
        let shared = do_handshake(&mut ctx);
        let request = r#"{"id":"rpc-1","method":"status.get"}"#;
        let frame = client_encrypt_text(&shared, request, 9);
        let effects = ctx.channel.handle_raw_message(RawMessage::Text(&frame));
        assert_eq!(effects, vec![E2eeEffect::DeliverText(request.to_string())]);
    }

    #[test]
    fn provides_encrypted_reply_function() {
        let mut ctx = setup();
        let shared = do_handshake(&mut ctx);
        let frame = client_encrypt_text(&shared, r#"{"id":"rpc-1","method":"status.get"}"#, 9);
        ctx.channel.handle_raw_message(RawMessage::Text(&frame));

        let reply = ctx.channel.encrypt_text_reply(r#"{"id":"rpc-1","ok":true}"#).unwrap();
        assert_eq!(decrypt_send_text(&reply, &shared), r#"{"id":"rpc-1","ok":true}"#);
    }

    #[test]
    fn decrypts_and_forwards_binary_messages_after_authentication() {
        let mut ctx = setup();
        let shared = do_handshake(&mut ctx);
        let mut nonce = [0u8; NONCE_BYTES];
        nonce[0] = 7;
        let frame = encrypt_bytes_with_nonce(&[1, 2, 3], &shared, &nonce).unwrap();
        let effects = ctx.channel.handle_raw_message(RawMessage::Binary(&frame));
        assert_eq!(effects, vec![E2eeEffect::DeliverBinary(vec![1, 2, 3])]);
    }

    #[test]
    fn silently_drops_messages_with_wrong_key() {
        let mut ctx = setup();
        do_handshake(&mut ctx);
        let frame = client_encrypt_text(&attacker_box(), "attack", 3);
        let effects = ctx.channel.handle_raw_message(RawMessage::Text(&frame));
        assert_eq!(effects, Vec::new());
    }

    #[test]
    fn closes_after_too_many_consecutive_decrypt_failures() {
        let mut ctx = setup();
        do_handshake(&mut ctx);
        let bad = attacker_box();

        for i in 0..4 {
            let frame = client_encrypt_text(&bad, "bad", i + 10);
            assert_eq!(ctx.channel.handle_raw_message(RawMessage::Text(&frame)), Vec::new());
        }
        let frame = client_encrypt_text(&bad, "bad", 20);
        assert_eq!(
            ctx.channel.handle_raw_message(RawMessage::Text(&frame)),
            vec![error(4003, "Too many decryption failures")]
        );
    }

    #[test]
    fn resets_failure_count_on_successful_decrypt() {
        let mut ctx = setup();
        let shared = do_handshake(&mut ctx);
        let bad = attacker_box();

        for i in 0..4 {
            let frame = client_encrypt_text(&bad, "bad", i + 10);
            ctx.channel.handle_raw_message(RawMessage::Text(&frame));
        }
        let good = client_encrypt_text(&shared, "good", 30);
        ctx.channel.handle_raw_message(RawMessage::Text(&good));
        for i in 0..4 {
            let frame = client_encrypt_text(&bad, "bad", i + 40);
            assert_eq!(ctx.channel.handle_raw_message(RawMessage::Text(&frame)), Vec::new());
        }
    }

    #[test]
    fn destroy_clears_state_and_stops_forwarding() {
        let mut ctx = setup();
        let shared = do_handshake(&mut ctx);
        ctx.channel.destroy();
        let frame = client_encrypt_text(&shared, "after destroy", 5);
        assert_eq!(ctx.channel.handle_raw_message(RawMessage::Text(&frame)), Vec::new());
    }

    #[test]
    fn does_not_emit_when_reply_fires_after_destroy() {
        let mut ctx = setup();
        do_handshake(&mut ctx);
        ctx.channel.destroy();
        assert_eq!(ctx.channel.encrypt_text_reply("late streaming frame"), None);
    }

    // ---- Regression tests for the post-destroy resurrection class ----------
    // Each of these fails against the pre-fix reducer, where `destroy()` left
    // `state == AwaitingHello` and `handle_raw_message` routed on `state` before
    // reading the (message-path-invisible) `destroyed` flag.

    #[test]
    fn destroyed_channel_does_not_resurrect_from_a_later_hello() {
        // The core attack: tear down before the handshake, then hand the channel
        // a well-formed hello. Pre-fix this re-derived the shared box, emitted a
        // plaintext e2ee_ready, and walked back to AwaitingAuth.
        let mut ctx = setup();
        let shared = client_box(&ctx);
        ctx.channel.destroy();

        let hello = hello_frame(&ctx.client_public);
        assert_eq!(ctx.channel.handle_raw_message(RawMessage::Text(&hello)), Vec::new());
        let auth = auth_frame(&shared, "valid-token", 1);
        assert_eq!(ctx.channel.handle_raw_message(RawMessage::Text(&auth)), Vec::new());
        assert_eq!(ctx.channel.device_token(), None);
    }

    #[test]
    fn destroyed_channel_delivers_no_payload_after_a_replayed_handshake() {
        // Captured frames from an earlier session against the same server key
        // reproduce the identical shared box, so the whole handshake replays with
        // no token knowledge. Pre-fix this reached DeliverText/DeliverBinary.
        let mut ctx = setup();
        let shared = client_box(&ctx);
        let hello = hello_frame(&ctx.client_public);
        let auth = auth_frame(&shared, "valid-token", 1);
        let text = client_encrypt_text(&shared, r#"{"id":"rpc-1","method":"terminal.send"}"#, 2);
        let mut nonce = [0u8; NONCE_BYTES];
        nonce[0] = 3;
        let binary = encrypt_bytes_with_nonce(&[1, 2, 3], &shared, &nonce).unwrap();

        ctx.channel.destroy();
        let mut effects = Vec::new();
        effects.extend(ctx.channel.handle_raw_message(RawMessage::Text(&hello)));
        effects.extend(ctx.channel.handle_raw_message(RawMessage::Text(&auth)));
        effects.extend(ctx.channel.handle_raw_message(RawMessage::Text(&text)));
        effects.extend(ctx.channel.handle_raw_message(RawMessage::Binary(&binary)));

        assert_eq!(effects, Vec::new());
        assert!(!effects.iter().any(is_delivery));
        assert!(!effects.contains(&E2eeEffect::Ready));
    }

    #[test]
    fn destroy_is_terminal_for_every_input_kind() {
        // Structural pin for requirement "no effect on any input sequence": the
        // guard sits in front of the whole message path, not per branch.
        let mut ctx = setup();
        let shared = client_box(&ctx);
        let hello = hello_frame(&ctx.client_public);
        let auth = auth_frame(&shared, "valid-token", 1);
        ctx.channel.destroy();

        let inputs: Vec<String> =
            vec![hello, auth, "not json".to_string(), r#"{"type":"e2ee_hello"}"#.to_string()];
        for input in &inputs {
            assert_eq!(ctx.channel.handle_raw_message(RawMessage::Text(input)), Vec::new());
        }
        assert_eq!(ctx.channel.handle_raw_message(RawMessage::Binary(&[0, 1, 2])), Vec::new());
        assert_eq!(ctx.channel.on_handshake_timeout(), Vec::new());
        assert_eq!(ctx.channel.encrypt_text_reply("late"), None);
        assert_eq!(ctx.channel.encrypt_binary_reply(&[1, 2, 3]), None);
        assert_eq!(ctx.channel.device_token(), None);
        // Idempotent, and still terminal after a second teardown.
        ctx.channel.destroy();
        assert_eq!(ctx.channel.handle_raw_message(RawMessage::Text(&inputs[0])), Vec::new());
    }

    #[test]
    fn destroyed_channel_emits_no_error_on_hello_path_garbage() {
        // Pre-fix a torn-down channel still answered garbage with 4001s. Silence,
        // not a new Error: "Unauthorized" on an innocent peer path is what fires
        // the desktop's "re-pair your phone" prompt.
        let mut ctx = setup();
        ctx.channel.destroy();
        assert_eq!(ctx.channel.handle_raw_message(RawMessage::Binary(&[0, 1, 2])), Vec::new());
        assert_eq!(ctx.channel.handle_raw_message(RawMessage::Text("not json")), Vec::new());
        assert_eq!(
            ctx.channel.handle_raw_message(RawMessage::Text(r#"{"type":"e2ee_hello"}"#)),
            Vec::new()
        );
    }

    #[test]
    fn destroyed_channel_cannot_seal_a_binary_reply() {
        // encrypt_binary_reply had zero coverage at HEAD, and gated on the key
        // rather than the state — so it came back to life with the shared box.
        let mut ctx = setup();
        ctx.channel.destroy();
        let hello = hello_frame(&ctx.client_public);
        ctx.channel.handle_raw_message(RawMessage::Text(&hello));
        assert_eq!(ctx.channel.encrypt_binary_reply(&[1, 2, 3]), None);
        assert_eq!(ctx.channel.encrypt_text_reply("late streaming frame"), None);
    }

    #[test]
    fn unauthenticated_channel_cannot_seal_replies() {
        // ECDH is unauthenticated, so any peer that opens a socket reaches
        // AwaitingAuth. Owner data must not be sealed to it before Ready.
        let mut ctx = setup();
        let hello = hello_frame(&ctx.client_public);
        ctx.channel.handle_raw_message(RawMessage::Text(&hello));
        assert_eq!(ctx.channel.encrypt_text_reply("pty output"), None);
        assert_eq!(ctx.channel.encrypt_binary_reply(&[1, 2, 3]), None);
    }

    // ---- Regression tests for the unbounded auth-retry / oracle class -------

    #[test]
    fn wrong_token_is_terminal_and_permits_no_retry() {
        // Pre-fix fail_auth mutated no channel field, so the post-failure vector
        // equalled the post-hello one and guesses were free and unlimited.
        let mut ctx = setup();
        let shared = client_box(&ctx);
        let hello = hello_frame(&ctx.client_public);
        ctx.channel.handle_raw_message(RawMessage::Text(&hello));

        let first = ctx.channel.handle_raw_message(RawMessage::Text(&auth_frame(&shared, "guess-1", 1)));
        assert!(first.contains(&error(4001, "Unauthorized")));

        for i in 0..8u8 {
            let retry = auth_frame(&shared, "guess-again", i + 2);
            assert_eq!(ctx.channel.handle_raw_message(RawMessage::Text(&retry)), Vec::new());
        }
        // Even the *correct* token cannot revive the channel.
        let good = auth_frame(&shared, "valid-token", 40);
        let effects = ctx.channel.handle_raw_message(RawMessage::Text(&good));
        assert_eq!(effects, Vec::new());
        assert!(!effects.contains(&E2eeEffect::Ready));
        assert_eq!(ctx.channel.device_token(), None);
    }

    #[test]
    fn rejected_auth_still_sends_a_decryptable_error_frame_before_closing() {
        // Wire contract: the peer distinguishes "re-pair" from an opaque hangup
        // by the sealed e2ee_error. Closing must not clear the key before it.
        let mut ctx = setup();
        let shared = client_box(&ctx);
        let hello = hello_frame(&ctx.client_public);
        ctx.channel.handle_raw_message(RawMessage::Text(&hello));

        let effects = ctx.channel.handle_raw_message(RawMessage::Text(&auth_frame(&shared, "nope", 1)));
        assert_eq!(
            parse(&decrypt_send_text(&effects[0], &shared)),
            parse(r#"{"type":"e2ee_error","error":{"code":"unauthorized"}}"#)
        );
        assert_eq!(effects[1], error(4001, "Unauthorized"));
        assert_eq!(effects.len(), 2);
    }

    #[test]
    fn malformed_auth_reports_bad_auth_then_closes() {
        let mut ctx = setup();
        let shared = client_box(&ctx);
        let hello = hello_frame(&ctx.client_public);
        ctx.channel.handle_raw_message(RawMessage::Text(&hello));

        let effects = ctx.channel.handle_raw_message(RawMessage::Text(&client_encrypt_text(&shared, "{}", 1)));
        assert_eq!(
            parse(&decrypt_send_text(&effects[0], &shared)),
            parse(r#"{"type":"e2ee_error","error":{"code":"bad_auth"}}"#)
        );
        assert_eq!(effects[1], error(4001, "Invalid e2ee_auth"));
        let retry = auth_frame(&shared, "valid-token", 2);
        assert_eq!(ctx.channel.handle_raw_message(RawMessage::Text(&retry)), Vec::new());
    }

    #[test]
    fn pre_auth_decrypt_failure_is_immediately_fatal() {
        // TS parity: the port handed out four silent probes in AwaitingAuth, which
        // is a key-confirmation oracle for a peer unsure of the server public key.
        let mut ctx = setup();
        let hello = hello_frame(&ctx.client_public);
        ctx.channel.handle_raw_message(RawMessage::Text(&hello));

        let probe = client_encrypt_text(&attacker_box(), "{}", 1);
        assert_eq!(
            ctx.channel.handle_raw_message(RawMessage::Text(&probe)),
            vec![error(4001, "Unauthorized")]
        );
        let second = client_encrypt_text(&attacker_box(), "{}", 2);
        assert_eq!(ctx.channel.handle_raw_message(RawMessage::Text(&second)), Vec::new());
    }

    #[test]
    fn second_hello_cannot_rekey_an_established_channel() {
        // No conforming client sends two hellos on one socket; a second one is a
        // rekey attempt and must not produce another e2ee_ready.
        let mut ctx = setup();
        let hello = hello_frame(&ctx.client_public);
        ctx.channel.handle_raw_message(RawMessage::Text(&hello));

        let effects = ctx.channel.handle_raw_message(RawMessage::Text(&hello));
        assert!(!effects.iter().any(|e| matches!(e, E2eeEffect::SendText(t) if t.contains("e2ee_ready"))));
        assert!(effects.iter().any(is_error));
        assert_eq!(ctx.channel.handle_raw_message(RawMessage::Text(&hello)), Vec::new());
    }

    // ---- Regression tests for the non-latching limits ----------------------

    #[test]
    fn decrypt_failure_cap_latches_closed() {
        // Pre-fix the cap re-emitted 4003 forever against a fully live channel;
        // the 6th failure and a subsequent *valid* frame both went through.
        let mut ctx = setup();
        let shared = do_handshake(&mut ctx);
        let bad = attacker_box();
        for i in 0..4 {
            ctx.channel.handle_raw_message(RawMessage::Text(&client_encrypt_text(&bad, "bad", i + 10)));
        }
        assert_eq!(
            ctx.channel.handle_raw_message(RawMessage::Text(&client_encrypt_text(&bad, "bad", 20))),
            vec![error(4003, "Too many decryption failures")]
        );
        assert_eq!(
            ctx.channel.handle_raw_message(RawMessage::Text(&client_encrypt_text(&bad, "bad", 21))),
            Vec::new()
        );
        let good = client_encrypt_text(&shared, r#"{"id":"rpc-2"}"#, 22);
        assert_eq!(ctx.channel.handle_raw_message(RawMessage::Text(&good)), Vec::new());
    }

    #[test]
    fn handshake_timeout_is_terminal_and_does_not_re_fire() {
        // Pre-fix on_handshake_timeout mutated nothing: a re-armed or duplicate
        // timer re-emitted 4002, and the channel stayed fully revivable.
        let mut ctx = setup();
        assert_eq!(ctx.channel.on_handshake_timeout(), vec![error(4002, "E2EE handshake timeout")]);
        assert_eq!(ctx.channel.on_handshake_timeout(), Vec::new());
        let hello = hello_frame(&ctx.client_public);
        assert_eq!(ctx.channel.handle_raw_message(RawMessage::Text(&hello)), Vec::new());
    }

    #[test]
    fn replayed_ciphertext_is_not_redelivered() {
        // v1 has no sequence counter, so nonce-uniqueness is the only in-band
        // replay defense; the port dropped the TS guard entirely.
        let mut ctx = setup();
        let shared = do_handshake(&mut ctx);
        let request = r#"{"id":"rpc-1","method":"terminal.send"}"#;
        let frame = client_encrypt_text(&shared, request, 9);
        assert_eq!(
            ctx.channel.handle_raw_message(RawMessage::Text(&frame)),
            vec![E2eeEffect::DeliverText(request.to_string())]
        );
        // Byte-identical replay: dropped, and counted against the failure budget.
        assert_eq!(ctx.channel.handle_raw_message(RawMessage::Text(&frame)), Vec::new());
    }

    #[test]
    fn replayed_binary_ciphertext_is_not_redelivered() {
        let mut ctx = setup();
        let shared = do_handshake(&mut ctx);
        let mut nonce = [0u8; NONCE_BYTES];
        nonce[0] = 7;
        let frame = encrypt_bytes_with_nonce(&[1, 2, 3], &shared, &nonce).unwrap();
        assert_eq!(
            ctx.channel.handle_raw_message(RawMessage::Binary(&frame)),
            vec![E2eeEffect::DeliverBinary(vec![1, 2, 3])]
        );
        assert_eq!(ctx.channel.handle_raw_message(RawMessage::Binary(&frame)), Vec::new());
    }

    #[test]
    fn replay_window_evicts_oldest_and_stays_bounded() {
        let mut guard = V1ReplayGuard::new();
        let nonce_at = |i: usize| {
            let mut nonce = [0u8; NONCE_BYTES];
            nonce[..8].copy_from_slice(&(i as u64).to_be_bytes());
            nonce
        };
        for i in 0..=MAX_TRACKED_INBOUND_NONCES {
            assert!(guard.accept(&nonce_at(i)));
        }
        assert_eq!(guard.order.len(), MAX_TRACKED_INBOUND_NONCES);
        assert_eq!(guard.seen.len(), MAX_TRACKED_INBOUND_NONCES);
        // The oldest fell out of the window; the newest is still tracked.
        assert!(guard.accept(&nonce_at(0)));
        assert!(!guard.accept(&nonce_at(MAX_TRACKED_INBOUND_NONCES)));
        // Frames too short to carry a nonce are rejected, not indexed.
        assert!(!guard.accept(&[0u8; NONCE_BYTES - 1]));
    }

    // ---- Properties the Trust contracts state, pinned under stock cargo ----

    #[test]
    fn plaintext_never_routes_to_delivery_before_ready() {
        for state in [ChannelState::AwaitingHello, ChannelState::AwaitingAuth, ChannelState::Closed] {
            for is_binary in [true, false] {
                let route = plaintext_route(state, is_binary);
                assert!(route != PlaintextRoute::DeliverText && route != PlaintextRoute::DeliverBinary);
            }
        }
        assert_eq!(plaintext_route(ChannelState::Ready, false), PlaintextRoute::DeliverText);
        assert_eq!(plaintext_route(ChannelState::Ready, true), PlaintextRoute::DeliverBinary);
        assert_eq!(plaintext_route(ChannelState::AwaitingAuth, false), PlaintextRoute::Auth);
        assert_eq!(plaintext_route(ChannelState::AwaitingAuth, true), PlaintextRoute::ProtocolError);
    }

    #[test]
    fn closed_state_is_absorbing_in_the_transition_table() {
        let all = [
            ChannelState::AwaitingHello,
            ChannelState::AwaitingAuth,
            ChannelState::Ready,
            ChannelState::Closed,
        ];
        for advance in all {
            assert_eq!(next_state(ChannelState::Closed, advance), ChannelState::Closed);
        }
        for before in [ChannelState::AwaitingHello, ChannelState::AwaitingAuth, ChannelState::Ready] {
            assert_eq!(next_state(before, ChannelState::Ready), ChannelState::Ready);
        }
        for state in all {
            assert_eq!(accepts_input(state), state != ChannelState::Closed);
            assert_eq!(may_encrypt_reply(state), state == ChannelState::Ready);
        }
    }
}
