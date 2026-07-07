//! `orca-crypto-wasm` — the app's E2EE crypto substrate.
//!
//! The E2EE remote-runtime transport historically ran two hand-maintained
//! `tweetnacl` TS twins (`src/shared/e2ee-crypto.ts` for the Node main/CLI
//! processes, `src/renderer/src/web/web-e2ee.ts` for the browser). This crate
//! compiles the SAME pure `orca-crypto` NaCl-box functions to
//! `wasm32-unknown-unknown`, so every process encrypts through the identical
//! code — one source of truth, byte-identical to tweetnacl/libsodium.
//!
//! Scope is the raw-shared-key variant (`nacl.box.before`/`after`/`open`), which
//! keeps the TS API stable: derive a 32-byte shared key once, then seal/open with
//! it. The OS RNG (nonces, key seeds) and base64 stay at the JS edge — this crate
//! is deterministic given its inputs.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

/// X25519 keypair from a 32-byte secret seed (`nacl.box.keyPair.fromSecretKey`).
/// Returns `publicKey (32) || secretKey (32)` — the JS edge slices it. `None`
/// (→ `undefined`) if the seed is not 32 bytes.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen(js_name = "keyPairFromSeed"))]
pub fn key_pair_from_seed(seed: &[u8]) -> Option<Vec<u8>> {
    let pair = orca_crypto::key_pair_from_seed(seed)?;
    let mut out = Vec::with_capacity(64);
    out.extend_from_slice(&pair.public_key);
    out.extend_from_slice(&pair.secret_key);
    Some(out)
}

/// `nacl.box.before`: the 32-byte precomputed shared key from our secret key and
/// a peer's public key. `None` if either key is not 32 bytes.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen(js_name = "deriveSharedKey"))]
pub fn derive_shared_key(our_secret_key: &[u8], peer_public_key: &[u8]) -> Option<Vec<u8>> {
    orca_crypto::shared_key_before(our_secret_key, peer_public_key).map(|key| key.to_vec())
}

/// `nacl.box.after`: seal `plaintext` under the raw shared key with an explicit
/// `nonce`, returning `nonce || box`. `None` on a bad nonce length or failure.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen(js_name = "sealWithSharedKey"))]
pub fn seal_with_shared_key(shared_key: &[u8], nonce: &[u8], plaintext: &[u8]) -> Option<Vec<u8>> {
    orca_crypto::seal_with_shared_key(shared_key, nonce, plaintext)
}

/// `nacl.box.open.after`: open a `nonce || box` bundle with the raw shared key.
/// `None` if the bundle is too short or the authentication tag fails.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen(js_name = "openWithSharedKey"))]
pub fn open_with_shared_key(shared_key: &[u8], bundle: &[u8]) -> Option<Vec<u8>> {
    orca_crypto::open_with_shared_key(shared_key, bundle)
}
