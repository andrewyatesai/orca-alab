//! Parity dispatch for `orca_crypto::nacl_box` vs `src/shared/e2ee-crypto.ts`.
//!
//! Byte buffers (`Vec<u8>` / `Uint8Array`) cross the vector boundary as plain
//! number arrays so the goldens stay valid JSON and compare structurally
//! against the TS side; this adapter converts at the edge.
//!
//! `decryptBytes` takes a precomputed shared key. This crate builds its
//! `SharedBox` only from (secret, peer-public) via `derive_shared_box`, so the
//! adapter derives it the same way the TS `deriveSharedKey` does, then decrypts.

use orca_crypto::{
    decrypt_bytes, derive_shared_box, encrypt_bytes_with_nonce, key_pair_from_seed,
    open_with_shared_key, seal_with_shared_key, shared_key_before,
};
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
        // Raw-key `nacl.box.before`: the 32-byte precomputed shared key the TS
        // boundary keeps passing around (so the wasm cutover stays API-stable).
        "deriveSharedKey" => {
            let our_secret = bytes_from_json(input.get("ourSecretKey"));
            let peer_public = bytes_from_json(input.get("peerPublicKey"));
            match shared_key_before(&our_secret, &peer_public) {
                // `key` is a zeroize-on-drop wrapper; borrow the raw bytes for JSON.
                Some(key) => bytes_to_json(key.as_slice()),
                None => Value::Null,
            }
        }
        // Raw-key seal/open (`nacl.box.after` / `open.after`) — the injected
        // nonce makes the seal deterministic, pinning `nonce || box` bytes.
        "sealWithSharedKey" => {
            let shared_key = bytes_from_json(input.get("sharedKey"));
            let nonce = bytes_from_json(input.get("nonce"));
            let message = bytes_from_json(input.get("message"));
            match seal_with_shared_key(&shared_key, &nonce, &message) {
                Some(bundle) => bytes_to_json(&bundle),
                None => Value::Null,
            }
        }
        "openWithSharedKey" => {
            let shared_key = bytes_from_json(input.get("sharedKey"));
            let bundle = bytes_from_json(input.get("bundle"));
            match open_with_shared_key(&shared_key, &bundle) {
                Some(plaintext) => bytes_to_json(&plaintext),
                None => Value::Null,
            }
        }
        // Deterministic encrypt-path pin: the vector injects the nonce (the TS
        // module's only nondeterminism is `nacl.randomBytes`), so the bundle is
        // byte-exact `nonce || box` on both sides.
        "encryptBytesWithNonce" => {
            let message = bytes_from_json(input.get("message"));
            let nonce = bytes_from_json(input.get("nonce"));
            let our_secret = bytes_from_json(input.get("ourSecretKey"));
            let peer_public = bytes_from_json(input.get("peerPublicKey"));
            match derive_shared_box(&our_secret, &peer_public) {
                Some(shared) => match encrypt_bytes_with_nonce(&message, &shared, &nonce) {
                    Some(bundle) => bytes_to_json(&bundle),
                    None => Value::Null,
                },
                None => json!({ "__parity_error__": "derive_shared_box: keys must be 32 bytes" }),
            }
        }
        // X25519 keypair from a 32-byte seed (`nacl.box.keyPair.fromSecretKey`).
        "keyPairFromSeed" => {
            let seed = bytes_from_json(input.get("seed"));
            match key_pair_from_seed(&seed) {
                Some(pair) => json!({
                    "publicKey": bytes_to_json(&pair.public_key),
                    "secretKey": bytes_to_json(&pair.secret_key),
                }),
                None => Value::Null,
            }
        }
        "decryptBytes" => {
            let bundle = bytes_from_json(input.get("bundle"));
            let our_secret = bytes_from_json(input.get("ourSecretKey"));
            let peer_public = bytes_from_json(input.get("peerPublicKey"));
            match derive_shared_box(&our_secret, &peer_public) {
                // TS `decryptBytes` returns null when the box fails to open (short
                // bundle or failed Poly1305 tag); `decrypt_bytes` returns None on
                // the same paths, so the null image matches.
                Some(shared) => match decrypt_bytes(&bundle, &shared) {
                    Some(plaintext) => bytes_to_json(&plaintext),
                    None => Value::Null,
                },
                // Vectors only carry 32-byte keys; a bad length is a vector bug.
                None => json!({ "__parity_error__": "derive_shared_box: keys must be 32 bytes" }),
            }
        }
        other => json!({ "__parity_error__": format!("unknown function {other}") }),
    }
}

fn bytes_to_json(bytes: &[u8]) -> Value {
    Value::Array(bytes.iter().map(|b| Value::from(*b)).collect())
}

fn bytes_from_json(value: Option<&Value>) -> Vec<u8> {
    value
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(|v| v.as_u64().map(|n| n as u8)).collect())
        .unwrap_or_default()
}
