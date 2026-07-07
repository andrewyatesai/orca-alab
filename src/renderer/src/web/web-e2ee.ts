// The renderer's E2EE NaCl-box crypto (browser flavour of src/shared/e2ee-crypto.ts).
// The crypto core is the orca-crypto Rust port compiled to wasm (byte-identical
// to tweetnacl, proven by orca-parity); the browser RNG (crypto.getRandomValues)
// and base64 (atob/btoa) stay at this JS edge. The wasm is eager-inited at
// renderer bootstrap via startCryptoWasm().
import {
  deriveSharedKey as wasmDeriveSharedKey,
  keyPairFromSeed as wasmKeyPairFromSeed,
  openWithSharedKey as wasmOpenWithSharedKey,
  sealWithSharedKey as wasmSealWithSharedKey
} from '@/lib/crypto-wasm/browser-crypto-wasm'

const NONCE_LENGTH = 24

export type BoxKeyPair = { publicKey: Uint8Array; secretKey: Uint8Array }

function randomBytes(length: number): Uint8Array {
  const bytes = new Uint8Array(length)
  globalThis.crypto.getRandomValues(bytes)
  return bytes
}

export function generateKeyPair(): BoxKeyPair {
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
  const key = base64ToBytes(b64)
  if (key.length !== 32) {
    throw new Error(`Invalid public key: expected 32 bytes, got ${key.length}`)
  }
  return key
}

export function publicKeyToBase64(key: Uint8Array): string {
  return bytesToBase64(key)
}

export function encrypt(plaintext: string, sharedKey: Uint8Array): string {
  return bytesToBase64(encryptBytes(new TextEncoder().encode(plaintext), sharedKey))
}

export function decrypt(encrypted: string, sharedKey: Uint8Array): string | null {
  const plaintext = decryptBytes(base64ToBytes(encrypted), sharedKey)
  return plaintext ? new TextDecoder().decode(plaintext) : null
}

export function encryptBytes(plaintext: Uint8Array, sharedKey: Uint8Array): Uint8Array<ArrayBuffer> {
  const bundle = wasmSealWithSharedKey(sharedKey, randomBytes(NONCE_LENGTH), plaintext)
  if (!bundle) {
    throw new Error('Failed to encrypt: invalid shared key')
  }
  return bundle
}

export function decryptBytes(
  bundle: Uint8Array,
  sharedKey: Uint8Array
): Uint8Array<ArrayBuffer> | null {
  return wasmOpenWithSharedKey(sharedKey, bundle) ?? null
}

function base64ToBytes(value: string): Uint8Array {
  const binary = window.atob(value)
  const bytes = new Uint8Array(binary.length)
  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index)
  }
  return bytes
}

function bytesToBase64(bytes: Uint8Array): string {
  const chunkSize = 0x8000
  let binary = ''
  for (let offset = 0; offset < bytes.length; offset += chunkSize) {
    const chunk = bytes.subarray(offset, offset + chunkSize)
    binary += String.fromCharCode(...chunk)
  }
  return window.btoa(binary)
}
