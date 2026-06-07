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

import { decryptBytes, deriveSharedKey } from '../../../src/shared/e2ee-crypto'

function toBytes(value: number[]): Uint8Array {
  return Uint8Array.from(value)
}

function fromBytes(bytes: Uint8Array): number[] {
  return Array.from(bytes)
}

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
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
