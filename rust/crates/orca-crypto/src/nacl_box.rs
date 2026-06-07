//! NaCl `box` E2EE, ported from `src/shared/e2ee-crypto.ts` (which used
//! `tweetnacl`). X25519 key agreement + XSalsa20-Poly1305 authenticated
//! encryption, wire-compatible with tweetnacl/libsodium so the existing desktop
//! / CLI / mobile peers interoperate.
//!
//! The bundle format matches the TS module: `nonce (24) || box`, where `box` is
//! libsodium combined mode (`tag (16) || ciphertext`). Nonces and key seeds are
//! injected by the caller (the IO edge owns the OS RNG), so this crate vendors
//! `crypto_box` *without* `getrandom` and stays deterministic + testable.

use crypto_box::{
    aead::{generic_array::GenericArray, Aead},
    PublicKey, SalsaBox, SecretKey,
};

pub const PUBLIC_KEY_BYTES: usize = 32;
pub const SECRET_KEY_BYTES: usize = 32;
pub const NONCE_BYTES: usize = 24;
/// Poly1305 authentication tag length prepended to the ciphertext.
pub const OVERHEAD_BYTES: usize = 16;

/// A precomputed NaCl `box` shared key (X25519 + HSalsa20), i.e. `nacl.box.before`.
pub struct SharedBox(SalsaBox);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyPair {
    pub public_key: [u8; PUBLIC_KEY_BYTES],
    pub secret_key: [u8; SECRET_KEY_BYTES],
}

fn array32(bytes: &[u8]) -> Option<[u8; 32]> {
    <[u8; 32]>::try_from(bytes).ok()
}

/// Derive the keypair for a 32-byte secret seed (the caller supplies random
/// bytes from its injected RNG). `None` if the seed is not 32 bytes.
pub fn key_pair_from_seed(seed: &[u8]) -> Option<KeyPair> {
    let secret = SecretKey::from(array32(seed)?);
    Some(KeyPair { public_key: secret.public_key().to_bytes(), secret_key: secret.to_bytes() })
}

/// Precompute the shared box from our secret key and a peer's public key
/// (`nacl.box.before`). `None` if either key is not 32 bytes.
pub fn derive_shared_box(our_secret_key: &[u8], peer_public_key: &[u8]) -> Option<SharedBox> {
    let secret = SecretKey::from(array32(our_secret_key)?);
    let public = PublicKey::from(array32(peer_public_key)?);
    Some(SharedBox(SalsaBox::new(&public, &secret)))
}

/// Seal `plaintext` under `shared` with an explicit `nonce`, returning
/// `nonce || box`. `None` if the nonce is not 24 bytes or sealing fails.
pub fn encrypt_bytes_with_nonce(plaintext: &[u8], shared: &SharedBox, nonce: &[u8]) -> Option<Vec<u8>> {
    if nonce.len() != NONCE_BYTES {
        return None;
    }
    let ciphertext = shared.0.encrypt(GenericArray::from_slice(nonce), plaintext).ok()?;
    let mut bundle = Vec::with_capacity(NONCE_BYTES + ciphertext.len());
    bundle.extend_from_slice(nonce);
    bundle.extend_from_slice(&ciphertext);
    Some(bundle)
}

/// Open a `nonce || box` bundle. `None` if the bundle is too short or the
/// authentication tag fails (mirrors the TS `null` return).
pub fn decrypt_bytes(bundle: &[u8], shared: &SharedBox) -> Option<Vec<u8>> {
    if bundle.len() < NONCE_BYTES + OVERHEAD_BYTES {
        return None;
    }
    let (nonce, ciphertext) = bundle.split_at(NONCE_BYTES);
    shared.0.decrypt(GenericArray::from_slice(nonce), ciphertext).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Canonical NaCl `box` test vector (from the NaCl distribution `tests/box`).
    const ALICE_SK: &str = "77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a";
    const ALICE_PK: &str = "8520f0098930a754748b7ddcb43ef75a0dbf3a0d26381af4eba4a98eaa9b4e6a";
    const BOB_SK: &str = "5dab087e624a8a4b79e17f8b83800ee66f3bb1292618b6fd1c2f8b27ff88e0eb";
    const BOB_PK: &str = "de9edb7d7b7dc1b4d35b61c2ece435373f8343c85b78674dadfc7e146f882b4f";
    const NONCE: &str = "69696ee955b62b73cd62bda875fc73d68219e0036b7a0b37";
    const MESSAGE: &str = "be075fc53c81f2d5cf141316ebeb0c7b5228c52a4c62cbd44b66849b64244ffce5ecbaaf33bd751a1ac728d45e6c61296cdc3c01233561f41db66cce314adb310e3be8250c46f06dceea3a7fa1348057e2f6556ad6b1318a024a838f21af1fde048977eb48f59ffd4924ca1c60902e52f0a089bc76897040e082f937763848645e0705";
    const EXPECTED_BOX: &str = "f3ffc7703f9400e52a7dfb4b3d3305d98e993b9f48681273c29650ba32fc76ce48332ea7164d96a4476fb8c531a1186ac0dfc17c98dce87b4da7f011ec48c97271d2c20f9b928fe2270d6fb863d51738b48eeee314a7cc8ab932164548e526ae90224368517acfeabd6bb3732bc0e9da99832b61ca01b6de56244a9e88d5f9b37973f622a43d14a6599b1f654cb45a74e355a5";

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    #[test]
    fn derives_canonical_x25519_public_keys() {
        assert_eq!(key_pair_from_seed(&hex(ALICE_SK)).unwrap().public_key.to_vec(), hex(ALICE_PK));
        assert_eq!(key_pair_from_seed(&hex(BOB_SK)).unwrap().public_key.to_vec(), hex(BOB_PK));
    }

    #[test]
    fn matches_canonical_nacl_box_vector() {
        let shared = derive_shared_box(&hex(ALICE_SK), &hex(BOB_PK)).unwrap();
        let bundle = encrypt_bytes_with_nonce(&hex(MESSAGE), &shared, &hex(NONCE)).unwrap();
        // Bundle is `nonce || box`; the box must match tweetnacl byte-for-byte.
        assert_eq!(&bundle[..NONCE_BYTES], hex(NONCE).as_slice());
        assert_eq!(&bundle[NONCE_BYTES..], hex(EXPECTED_BOX).as_slice());
    }

    #[test]
    fn round_trips_and_interoperates_between_peers() {
        let alice = derive_shared_box(&hex(ALICE_SK), &hex(BOB_PK)).unwrap();
        let bob = derive_shared_box(&hex(BOB_SK), &hex(ALICE_PK)).unwrap();

        let sealed = encrypt_bytes_with_nonce(b"hello over the wire", &alice, &hex(NONCE)).unwrap();
        assert_eq!(decrypt_bytes(&sealed, &bob).unwrap(), b"hello over the wire");

        let other_nonce = hex("000102030405060708090a0b0c0d0e0f1011121314151617");
        let reply = encrypt_bytes_with_nonce(b"ack", &bob, &other_nonce).unwrap();
        assert_eq!(decrypt_bytes(&reply, &alice).unwrap(), b"ack");
    }

    #[test]
    fn rejects_tampered_or_short_bundles() {
        let shared = derive_shared_box(&hex(ALICE_SK), &hex(BOB_PK)).unwrap();
        let mut sealed = encrypt_bytes_with_nonce(b"secret", &shared, &hex(NONCE)).unwrap();
        let last = sealed.len() - 1;
        sealed[last] ^= 0x01;
        assert_eq!(decrypt_bytes(&sealed, &shared), None);
        assert_eq!(decrypt_bytes(&[0u8; NONCE_BYTES + OVERHEAD_BYTES - 1], &shared), None);
    }

    #[test]
    fn rejects_wrong_length_keys_and_nonces() {
        assert!(key_pair_from_seed(&[0u8; 31]).is_none());
        assert!(derive_shared_box(&[0u8; 31], &hex(BOB_PK)).is_none());
        assert!(derive_shared_box(&hex(ALICE_SK), &[0u8; 33]).is_none());
        let shared = derive_shared_box(&hex(ALICE_SK), &hex(BOB_PK)).unwrap();
        assert!(encrypt_bytes_with_nonce(b"x", &shared, &[0u8; 23]).is_none());
    }
}
