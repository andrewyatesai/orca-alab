// TS dispatch for the nacl-box parity module: maps the shared vector function
// names to the real `src/shared/e2ee-crypto.ts` exports so the harness compares
// the live TS reference against the Rust port.
//
// Byte buffers (Uint8Array / Vec<u8>) cross the vector boundary as plain number
// arrays so goldens stay valid JSON and compare structurally against the Rust
// side; this adapter converts at the edge and calls the real functions.
//
// `decryptBytes` needs a precomputed shared key. The Rust crate builds its
// `SharedBox` only from (secretKey, peerPublicKey) via `derive_shared_box`, so
// both halves derive the shared key the same way (`deriveSharedKey`) before
// decrypting — keeping the two adapters byte-for-byte symmetric.

import nacl from 'tweetnacl'
import { decryptBytes, deriveSharedKey } from '../../../src/shared/e2ee-crypto'

function toBytes(value: number[]): Uint8Array {
  return Uint8Array.from(value)
}

function fromBytes(bytes: Uint8Array): number[] {
  return Array.from(bytes)
}

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'encryptBytesWithNonce': {
      // The TS `encryptBytes` is `randomBytes(24)` + `nacl.box.after` + concat;
      // with the vector-injected nonce this reproduces its deterministic core
      // exactly (`nonce || box`), so live tweetnacl pins the Rust bundle bytes.
      const { message, nonce, ourSecretKey, peerPublicKey } = input as {
        message: number[]
        nonce: number[]
        ourSecretKey: number[]
        peerPublicKey: number[]
      }
      const sharedKey = deriveSharedKey(toBytes(ourSecretKey), toBytes(peerPublicKey))
      const nonceBytes = toBytes(nonce)
      if (nonceBytes.length !== nacl.box.nonceLength) {
        // Mirror the Rust None for a bad nonce length.
        return null
      }
      const ciphertext = nacl.box.after(toBytes(message), nonceBytes, sharedKey)
      const bundle = new Uint8Array(nonceBytes.length + ciphertext.length)
      bundle.set(nonceBytes)
      bundle.set(ciphertext, nonceBytes.length)
      return fromBytes(bundle)
    }
    case 'keyPairFromSeed': {
      const { seed } = input as { seed: number[] }
      if (seed.length !== 32) {
        return null
      }
      const pair = nacl.box.keyPair.fromSecretKey(toBytes(seed))
      return { publicKey: fromBytes(pair.publicKey), secretKey: fromBytes(pair.secretKey) }
    }
    case 'decryptBytes': {
      const { bundle, ourSecretKey, peerPublicKey } = input as {
        bundle: number[]
        ourSecretKey: number[]
        peerPublicKey: number[]
      }
      const sharedKey = deriveSharedKey(toBytes(ourSecretKey), toBytes(peerPublicKey))
      const plaintext = decryptBytes(toBytes(bundle), sharedKey)
      // decryptBytes returns null on a length/auth failure; success (even an
      // empty plaintext) is a truthy Uint8Array we surface as a number array.
      return plaintext ? fromBytes(plaintext) : null
    }
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
