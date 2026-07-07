// The E2EE NaCl-box crypto for the Node processes (Electron main + CLI), driven
// by the orca-crypto Rust core compiled to wasm instead of the tweetnacl TS twin.
// The wasm bytes are base64-embedded so the main/CLI bundles stay self-contained;
// initSync is idempotent + lazy (Node compiles wasm synchronously). The OS RNG
// (nonces, key seeds) and base64 stay at the JS edge — these are the pure,
// deterministic primitives. Byte-identical to tweetnacl (proven by orca-parity).
import {
  initSync,
  keyPairFromSeed as wasmKeyPairFromSeed,
  deriveSharedKey as wasmDeriveSharedKey,
  sealWithSharedKey as wasmSealWithSharedKey,
  openWithSharedKey as wasmOpenWithSharedKey
} from './orca_crypto_wasm.js'
import { ORCA_CRYPTO_WASM_BASE64 } from './orca_crypto_wasm_bg.wasm.base64'

let inited = false
function ensureCryptoWasm(): void {
  if (inited) {
    return
  }
  // Buffer is a Uint8Array (BufferSource) initSync accepts. Node only — the
  // browser uses the async loader in the renderer.
  initSync({ module: Buffer.from(ORCA_CRYPTO_WASM_BASE64, 'base64') })
  inited = true
}

/** `publicKey (32) || secretKey (32)` for a 32-byte seed, or undefined. */
export function keyPairFromSeed(seed: Uint8Array): Uint8Array | undefined {
  ensureCryptoWasm()
  return wasmKeyPairFromSeed(seed)
}

/** `nacl.box.before` shared key, or undefined for non-32-byte keys. */
export function deriveSharedKey(
  ourSecretKey: Uint8Array,
  peerPublicKey: Uint8Array
): Uint8Array | undefined {
  ensureCryptoWasm()
  return wasmDeriveSharedKey(ourSecretKey, peerPublicKey)
}

/** `nonce || box` sealed under the raw shared key, or undefined. */
export function sealWithSharedKey(
  sharedKey: Uint8Array,
  nonce: Uint8Array,
  message: Uint8Array
): Uint8Array | undefined {
  ensureCryptoWasm()
  return wasmSealWithSharedKey(sharedKey, nonce, message)
}

/** Opened plaintext, or undefined on a short bundle / failed tag. */
export function openWithSharedKey(
  sharedKey: Uint8Array,
  bundle: Uint8Array
): Uint8Array | undefined {
  ensureCryptoWasm()
  return wasmOpenWithSharedKey(sharedKey, bundle)
}
