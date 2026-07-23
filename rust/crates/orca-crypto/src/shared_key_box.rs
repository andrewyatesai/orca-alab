//! NaCl `box` primitives that expose the raw 32-byte precomputed shared key
//! (`nacl.box.before`), so the TS boundary can keep passing an opaque shared key
//! around (matching `deriveSharedKey` -> `encryptBytes`/`decryptBytes`) instead
//! of re-deriving per call or threading a stateful handle across the wasm edge.
//!
//! `beforenm` = HSalsa20(X25519(sk, pk)); sealing/opening use the raw key with
//! XSalsa20-Poly1305 (== `nacl.box.after` / `nacl.secretbox`). These are the same
//! primitives `crypto_box::SalsaBox` composes internally — the tests prove this
//! path is byte-identical to the SharedBox path (and to tweetnacl).

use crypto_secretbox::{
    aead::{Aead, KeyInit},
    Key, Nonce, XSalsa20Poly1305,
};
use curve25519_dalek::montgomery::MontgomeryPoint;
use salsa20::{cipher::consts::U10, hsalsa};
use zeroize::{Zeroize, Zeroizing};

use crate::nacl_box::{NONCE_BYTES, OVERHEAD_BYTES};

fn array32(bytes: &[u8]) -> Option<[u8; 32]> {
    <[u8; 32]>::try_from(bytes).ok()
}

/// `nacl.box.before`: the precomputed shared key HSalsa20(X25519(sk, pk)).
/// `None` if either key is not 32 bytes.
pub fn shared_key_before(our_secret_key: &[u8], peer_public_key: &[u8]) -> Option<Zeroizing<[u8; 32]>> {
    let mut secret = array32(our_secret_key)?;
    let public = array32(peer_public_key)?;
    // X25519: mul_clamped applies the standard scalar clamping (clear low 3 bits
    // of byte 0, clear high bit + set bit 254 of byte 31).
    let mut shared_point = MontgomeryPoint(public).mul_clamped(secret);
    // beforenm KDF: HSalsa20 over the DH output with a zero 16-byte input — the
    // same derivation crypto_box's Salsa20 `Kdf` performs.
    let mut derived = hsalsa::<U10>(Key::from_slice(&shared_point.0), &Default::default());
    let mut key = [0u8; 32];
    key.copy_from_slice(&derived);
    // Wipe the secret-scalar copy, DH output, and KDF result; only the returned
    // (zeroize-on-drop) shared key escapes this frame.
    secret.zeroize();
    shared_point.0.zeroize();
    derived.as_mut_slice().zeroize();
    Some(Zeroizing::new(key))
}

/// `nacl.box.after` / `nacl.secretbox`: seal `plaintext` under the raw shared key
/// with an explicit `nonce`, returning `nonce || box`. `None` if the nonce is not
/// 24 bytes or sealing fails.
pub fn seal_with_shared_key(shared_key: &[u8], nonce: &[u8], plaintext: &[u8]) -> Option<Vec<u8>> {
    let mut key = array32(shared_key)?;
    if nonce.len() != NONCE_BYTES {
        return None;
    }
    let cipher = XSalsa20Poly1305::new(Key::from_slice(&key));
    // The cipher owns its own (zeroizing) key copy now; wipe ours.
    key.zeroize();
    let ciphertext = cipher.encrypt(Nonce::from_slice(nonce), plaintext).ok()?;
    let mut bundle = Vec::with_capacity(NONCE_BYTES + ciphertext.len());
    bundle.extend_from_slice(nonce);
    bundle.extend_from_slice(&ciphertext);
    Some(bundle)
}

/// `nacl.box.open.after`: open a `nonce || box` bundle with the raw shared key.
/// `None` if the bundle is too short or the authentication tag fails (mirrors the
/// TS `null` return).
pub fn open_with_shared_key(shared_key: &[u8], bundle: &[u8]) -> Option<Vec<u8>> {
    let mut key = array32(shared_key)?;
    if bundle.len() < NONCE_BYTES + OVERHEAD_BYTES {
        return None;
    }
    let (nonce, ciphertext) = bundle.split_at(NONCE_BYTES);
    let cipher = XSalsa20Poly1305::new(Key::from_slice(&key));
    // The cipher owns its own (zeroizing) key copy now; wipe ours.
    key.zeroize();
    cipher.decrypt(Nonce::from_slice(nonce), ciphertext).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nacl_box::{derive_shared_box, encrypt_bytes_with_nonce};

    // Canonical NaCl `box` vectors (NaCl distribution `tests/box`).
    const ALICE_SK: &str = "77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a";
    const BOB_PK: &str = "de9edb7d7b7dc1b4d35b61c2ece435373f8343c85b78674dadfc7e146f882b4f";
    const BOB_SK: &str = "5dab087e624a8a4b79e17f8b83800ee66f3bb1292618b6fd1c2f8b27ff88e0eb";
    const ALICE_PK: &str = "8520f0098930a754748b7ddcb43ef75a0dbf3a0d26381af4eba4a98eaa9b4e6a";
    const NONCE: &str = "69696ee955b62b73cd62bda875fc73d68219e0036b7a0b37";
    // The canonical NaCl "firstkey": beforenm(BOB_PK, ALICE_SK).
    const FIRSTKEY: &str = "1b27556473e985d462cd51197a9a46c76009549eac6474f206c4ee0844f68389";

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    #[test]
    fn beforenm_matches_canonical_nacl_firstkey() {
        assert_eq!(shared_key_before(&hex(ALICE_SK), &hex(BOB_PK)).unwrap().to_vec(), hex(FIRSTKEY));
        // Symmetric: both peers derive the same key.
        assert_eq!(
            shared_key_before(&hex(BOB_SK), &hex(ALICE_PK)).unwrap(),
            shared_key_before(&hex(ALICE_SK), &hex(BOB_PK)).unwrap()
        );
    }

    #[test]
    fn shared_key_is_returned_zeroize_on_drop_without_changing_the_bytes() {
        // Pins the zeroizing return type (compile-time) and that wrapping did not
        // change the derived beforenm bytes.
        let key: Zeroizing<[u8; 32]> = shared_key_before(&hex(ALICE_SK), &hex(BOB_PK)).unwrap();
        assert_eq!(key.as_slice(), hex(FIRSTKEY).as_slice());
    }

    #[test]
    fn raw_key_path_is_byte_identical_to_the_shared_box_path() {
        // The definitive equivalence: sealing via the exposed raw key must equal
        // crypto_box's SalsaBox output (already proven == tweetnacl EXPECTED_BOX).
        let key = shared_key_before(&hex(ALICE_SK), &hex(BOB_PK)).unwrap();
        let via_raw = seal_with_shared_key(key.as_slice(), &hex(NONCE), b"hello over the wire").unwrap();
        let shared = derive_shared_box(&hex(ALICE_SK), &hex(BOB_PK)).unwrap();
        let via_box = encrypt_bytes_with_nonce(b"hello over the wire", &shared, &hex(NONCE)).unwrap();
        assert_eq!(via_raw, via_box);
    }

    #[test]
    fn round_trips_and_interoperates_between_peers() {
        let alice = shared_key_before(&hex(ALICE_SK), &hex(BOB_PK)).unwrap();
        let bob = shared_key_before(&hex(BOB_SK), &hex(ALICE_PK)).unwrap();
        let sealed = seal_with_shared_key(alice.as_slice(), &hex(NONCE), b"secret payload").unwrap();
        assert_eq!(open_with_shared_key(bob.as_slice(), &sealed).unwrap(), b"secret payload");
    }

    #[test]
    fn rejects_tampered_short_bundles_and_bad_lengths() {
        let key = shared_key_before(&hex(ALICE_SK), &hex(BOB_PK)).unwrap();
        let mut sealed = seal_with_shared_key(key.as_slice(), &hex(NONCE), b"x").unwrap();
        let last = sealed.len() - 1;
        sealed[last] ^= 0x01;
        assert_eq!(open_with_shared_key(key.as_slice(), &sealed), None);
        assert_eq!(open_with_shared_key(key.as_slice(), &[0u8; NONCE_BYTES + OVERHEAD_BYTES - 1]), None);
        assert!(shared_key_before(&[0u8; 31], &hex(BOB_PK)).is_none());
        assert!(seal_with_shared_key(key.as_slice(), &[0u8; 23], b"x").is_none());
    }
}
