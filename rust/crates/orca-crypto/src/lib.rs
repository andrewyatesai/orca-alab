//! `orca-crypto` — cryptographic primitives for Orca's secure transports.
//!
//! Starts with NaCl `box` (X25519 + XSalsa20-Poly1305) for the E2EE remote
//! runtime, wire-compatible with the `tweetnacl` format the mobile/CLI/desktop
//! peers already speak. Over vendored, stripped `crypto_box`.

pub mod nacl_box;

pub use nacl_box::{
    derive_shared_box, decrypt_bytes, encrypt_bytes_with_nonce, key_pair_from_seed, KeyPair,
    SharedBox, NONCE_BYTES, OVERHEAD_BYTES, PUBLIC_KEY_BYTES, SECRET_KEY_BYTES,
};
