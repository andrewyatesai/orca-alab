//! Parity dispatch for `orca_crypto::nacl_box` vs `src/shared/e2ee-crypto.ts`.
//!
//! Byte buffers (`Vec<u8>` / `Uint8Array`) cross the vector boundary as plain
//! number arrays so the goldens stay valid JSON and compare structurally
//! against the TS side; this adapter converts at the edge.
//!
//! `decryptBytes` takes a precomputed shared key. This crate builds its
//! `SharedBox` only from (secret, peer-public) via `derive_shared_box`, so the
//! adapter derives it the same way the TS `deriveSharedKey` does, then decrypts.

use orca_crypto::{decrypt_bytes, derive_shared_box};
use serde_json::{json, Value};

pub fn dispatch(function: &str, input: &Value) -> Value {
    match function {
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
