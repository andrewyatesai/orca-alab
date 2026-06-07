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

use crate::base64;
use orca_crypto::{
    decrypt_bytes, derive_shared_box, encrypt_bytes_with_nonce, SharedBox, NONCE_BYTES,
    PUBLIC_KEY_BYTES,
};
use serde_json::Value;

const MAX_CONSECUTIVE_DECRYPT_FAILURES: u32 = 5;
/// Handshake watchdog the owner arms a timer for; on fire it calls
/// [`E2eeChannel::on_handshake_timeout`].
pub const HANDSHAKE_TIMEOUT_MS: u64 = 10_000;
/// Owner-side backpressure cap for buffered binary frames (the channel emits
/// the frame; the owner drops it if its socket is over this).
pub const MAX_BINARY_BUFFERED_AMOUNT: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChannelState {
    AwaitingHello,
    AwaitingAuth,
    Ready,
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
    /// Fatal: the owner should close the socket with this code/reason.
    Error { code: u16, reason: String },
}

fn error(code: u16, reason: &str) -> E2eeEffect {
    E2eeEffect::Error { code, reason: reason.to_string() }
}

type ValidateToken = Box<dyn Fn(&str) -> bool>;
type NonceSource = Box<dyn FnMut() -> [u8; NONCE_BYTES]>;

pub struct E2eeChannel {
    state: ChannelState,
    shared_box: Option<SharedBox>,
    consecutive_failures: u32,
    handshake_complete: bool,
    destroyed: bool,
    server_secret_key: Vec<u8>,
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
            handshake_complete: false,
            destroyed: false,
            server_secret_key,
            device_token: None,
            validate_token,
            next_nonce,
        }
    }

    pub fn device_token(&self) -> Option<&str> {
        self.device_token.as_deref()
    }

    pub fn handle_raw_message(&mut self, raw: RawMessage) -> Vec<E2eeEffect> {
        if self.state == ChannelState::AwaitingHello {
            return match raw {
                RawMessage::Text(text) => self.handle_hello(text),
                RawMessage::Binary(_) => vec![error(4001, "Invalid handshake message")],
            };
        }
        if self.shared_box.is_none() {
            return Vec::new();
        }
        match raw {
            RawMessage::Binary(bytes) => match self.decrypt_binary(bytes) {
                None => self.track_decrypt_failure(),
                Some(plaintext) => {
                    self.consecutive_failures = 0;
                    if self.state == ChannelState::Ready {
                        vec![E2eeEffect::DeliverBinary(plaintext)]
                    } else {
                        vec![error(4001, "Invalid binary message before authentication")]
                    }
                }
            },
            RawMessage::Text(text) => match self.decrypt_text(text) {
                None => self.track_decrypt_failure(),
                Some(plaintext) => {
                    self.consecutive_failures = 0;
                    if self.state == ChannelState::AwaitingAuth {
                        self.handle_auth(&plaintext)
                    } else {
                        vec![E2eeEffect::DeliverText(plaintext)]
                    }
                }
            },
        }
    }

    /// Encrypt a text reply (the TS `encryptedReply`). `None` once the channel
    /// is destroyed (shared key cleared) so late streaming emits are no-ops.
    pub fn encrypt_text_reply(&mut self, response: &str) -> Option<E2eeEffect> {
        self.encrypt_text(response).map(E2eeEffect::SendText)
    }

    /// Encrypt a binary reply. The owner still applies its buffered-amount
    /// backpressure ([`MAX_BINARY_BUFFERED_AMOUNT`]) before sending.
    pub fn encrypt_binary_reply(&mut self, response: &[u8]) -> Option<E2eeEffect> {
        // Nonce first so the `?` borrow of `shared_box` doesn't overlap the
        // mutable `next_nonce` call; a wasted nonce on the no-key path is fine.
        let nonce = (self.next_nonce)();
        let shared = self.shared_box.as_ref()?;
        encrypt_bytes_with_nonce(response, shared, &nonce).map(E2eeEffect::SendBinary)
    }

    /// Called when the owner's handshake timer fires. Errors unless the
    /// handshake already completed (the TS "clear timer on ready") or the
    /// channel was destroyed.
    pub fn on_handshake_timeout(&mut self) -> Vec<E2eeEffect> {
        if self.handshake_complete || self.destroyed {
            Vec::new()
        } else {
            vec![error(4002, "E2EE handshake timeout")]
        }
    }

    /// Tear down: clear the shared key so subsequent traffic and late replies
    /// become no-ops.
    pub fn destroy(&mut self) {
        self.destroyed = true;
        self.shared_box = None;
        self.device_token = None;
    }

    fn handle_hello(&mut self, raw: &str) -> Vec<E2eeEffect> {
        let Ok(value) = serde_json::from_str::<Value>(raw) else {
            return vec![error(4001, "Invalid handshake message")];
        };
        let is_hello = value.get("type").and_then(Value::as_str) == Some("e2ee_hello");
        let public_key_b64 =
            value.get("publicKeyB64").and_then(Value::as_str).filter(|key| !key.is_empty());
        let Some(public_key_b64) = public_key_b64.filter(|_| is_hello) else {
            return vec![error(4001, "Invalid e2ee_hello")];
        };
        let Some(client_public_key) =
            base64::decode(public_key_b64).filter(|key| key.len() == PUBLIC_KEY_BYTES)
        else {
            return vec![error(4001, "Invalid public key")];
        };
        let Some(shared) = derive_shared_box(&self.server_secret_key, &client_public_key) else {
            return vec![error(4001, "Invalid public key")];
        };
        self.shared_box = Some(shared);
        self.state = ChannelState::AwaitingAuth;
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
        self.state = ChannelState::Ready;
        self.handshake_complete = true;
        let mut effects = Vec::new();
        effects.extend(self.encrypt_text(r#"{"type":"e2ee_authenticated"}"#).map(E2eeEffect::SendText));
        effects.push(E2eeEffect::Ready);
        effects
    }

    fn fail_auth(&mut self, code: &str, reason: &str) -> Vec<E2eeEffect> {
        let control = serde_json::json!({ "type": "e2ee_error", "error": { "code": code } });
        let json = control.to_string();
        let mut effects = Vec::new();
        effects.extend(self.encrypt_text(&json).map(E2eeEffect::SendText));
        effects.push(error(4001, reason));
        effects
    }

    /// Encrypt `plaintext` to the base64 text-frame body. `None` if there is no
    /// shared key.
    fn encrypt_text(&mut self, plaintext: &str) -> Option<String> {
        // Nonce first so the `?` borrow of `shared_box` doesn't overlap the
        // mutable `next_nonce` call; a wasted nonce on the no-key path is fine.
        let nonce = (self.next_nonce)();
        let shared = self.shared_box.as_ref()?;
        let bundle = encrypt_bytes_with_nonce(plaintext.as_bytes(), shared, &nonce)?;
        Some(base64::encode_standard(&bundle))
    }

    fn decrypt_text(&self, raw: &str) -> Option<String> {
        let shared = self.shared_box.as_ref()?;
        let bundle = base64::decode(raw)?;
        let plaintext = decrypt_bytes(&bundle, shared)?;
        Some(String::from_utf8_lossy(&plaintext).into_owned())
    }

    fn decrypt_binary(&self, bytes: &[u8]) -> Option<Vec<u8>> {
        let shared = self.shared_box.as_ref()?;
        decrypt_bytes(bytes, shared)
    }

    fn track_decrypt_failure(&mut self) -> Vec<E2eeEffect> {
        self.consecutive_failures += 1;
        if self.consecutive_failures >= MAX_CONSECUTIVE_DECRYPT_FAILURES {
            vec![error(4003, "Too many decryption failures")]
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

    fn client_encrypt_text(shared: &SharedBox, plaintext: &str, nonce_byte: u8) -> String {
        let mut nonce = [0u8; NONCE_BYTES];
        nonce[0] = nonce_byte;
        base64::encode_standard(&encrypt_bytes_with_nonce(plaintext.as_bytes(), shared, &nonce).unwrap())
    }

    fn hello_frame(public_key: &[u8]) -> String {
        format!(r#"{{"type":"e2ee_hello","publicKeyB64":"{}"}}"#, base64::encode_standard(public_key))
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
        let attacker = derive_shared_box(
            &key_pair_from_seed(&[8u8; 32]).unwrap().secret_key,
            &key_pair_from_seed(&[9u8; 32]).unwrap().public_key,
        )
        .unwrap();
        let frame = client_encrypt_text(&attacker, "attack", 3);
        let effects = ctx.channel.handle_raw_message(RawMessage::Text(&frame));
        assert_eq!(effects, Vec::new());
    }

    #[test]
    fn closes_after_too_many_consecutive_decrypt_failures() {
        let mut ctx = setup();
        do_handshake(&mut ctx);
        let bad = derive_shared_box(
            &key_pair_from_seed(&[8u8; 32]).unwrap().secret_key,
            &key_pair_from_seed(&[9u8; 32]).unwrap().public_key,
        )
        .unwrap();

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
        let bad = derive_shared_box(
            &key_pair_from_seed(&[8u8; 32]).unwrap().secret_key,
            &key_pair_from_seed(&[9u8; 32]).unwrap().public_key,
        )
        .unwrap();

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
}
