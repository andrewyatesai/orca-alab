// Why: Orca's remote runtime transports share one NaCl box format across
// desktop, CLI, and mobile pairing. The crypto core is now the orca-crypto Rust
// port compiled to wasm (byte-identical to tweetnacl, proven by orca-parity);
// the OS RNG (node:crypto) + base64 (Buffer) stay at this JS edge so the CLI
// never imports main-process modules. The renderer has a browser-flavoured twin
// (web-e2ee.ts) over the same wasm.
import { randomBytes } from 'node:crypto'
import {
  deriveSharedKey as wasmDeriveSharedKey,
  keyPairFromSeed as wasmKeyPairFromSeed,
  openWithSharedKey as wasmOpenWithSharedKey,
  sealWithSharedKey as wasmSealWithSharedKey
} from './crypto-wasm/node-crypto-wasm'

const NONCE_LENGTH = 24

export type BoxKeyPair = { publicKey: Uint8Array; secretKey: Uint8Array }

export function generateKeyPair(): BoxKeyPair {
  // The seed IS the X25519 secret key; the OS RNG owns the entropy.
  const packed = wasmKeyPairFromSeed(randomBytes(32))
  if (!packed) {
    throw new Error('Failed to generate key pair')
  }
  return { publicKey: packed.slice(0, 32), secretKey: packed.slice(32, 64) }
}

export function deriveSharedKey(ourSecretKey: Uint8Array, peerPublicKey: Uint8Array): Uint8Array {
  const sharedKey = wasmDeriveSharedKey(ourSecretKey, peerPublicKey)
  if (!sharedKey) {
    throw new Error('Invalid key: expected 32-byte secret and public keys')
  }
  return sharedKey
}

export function publicKeyFromBase64(b64: string): Uint8Array {
  const key = Uint8Array.from(Buffer.from(b64, 'base64'))
  if (key.length !== 32) {
    throw new Error(`Invalid public key: expected 32 bytes, got ${key.length}`)
  }
  return key
}

export function publicKeyToBase64(key: Uint8Array): string {
  return Buffer.from(key).toString('base64')
}

export function encrypt(plaintext: string, sharedKey: Uint8Array): string {
  const messageBytes = new TextEncoder().encode(plaintext)
  return Buffer.from(encryptBytes(messageBytes, sharedKey)).toString('base64')
}

export function decrypt(encrypted: string, sharedKey: Uint8Array): string | null {
  const bundle = Uint8Array.from(Buffer.from(encrypted, 'base64'))
  const plaintext = decryptBytes(bundle, sharedKey)
  return plaintext ? new TextDecoder().decode(plaintext) : null
}

export function encryptBytes(plaintext: Uint8Array, sharedKey: Uint8Array): Uint8Array {
  const nonce = randomBytes(NONCE_LENGTH)
  const bundle = wasmSealWithSharedKey(sharedKey, nonce, plaintext)
  if (!bundle) {
    throw new Error('Failed to encrypt: invalid shared key')
  }
  return bundle
}

export function decryptBytes(bundle: Uint8Array, sharedKey: Uint8Array): Uint8Array | null {
  return wasmOpenWithSharedKey(sharedKey, bundle) ?? null
}
