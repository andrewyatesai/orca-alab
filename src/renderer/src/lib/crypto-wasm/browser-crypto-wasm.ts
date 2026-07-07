// The renderer's E2EE NaCl-box crypto, driven by the orca-crypto Rust core
// compiled to wasm — the same module the Node main/CLI processes embed and the
// same primitives proven byte-identical to tweetnacl by orca-parity. The
// renderer has no napi access (sandbox: true), so it loads the wasm via vite's
// `?url` asset + async init like the aterm/orca-git engines.
//
// Unlike the git-wasm renderer wrappers (which return null until ready and let
// consumers fall back), crypto CANNOT degrade — you can't skip encryption. The
// init is kicked off eagerly at renderer bootstrap (startCryptoWasm), long
// before any remote WebSocket connects; the accessors throw if called before
// ready, which fails a premature connection safely (never sends plaintext).
import initCryptoWasm, {
  deriveSharedKey as wasmDeriveSharedKey,
  keyPairFromSeed as wasmKeyPairFromSeed,
  openWithSharedKey as wasmOpenWithSharedKey,
  sealWithSharedKey as wasmSealWithSharedKey,
  initSync
} from './orca_crypto_wasm.js'
import wasmUrl from './orca_crypto_wasm_bg.wasm?url'

let ready = false
let startPromise: Promise<void> | null = null

/** Kick off the async wasm init (idempotent). Called once from the renderer
 *  bootstrap so the crypto is compiled long before any remote handshake. */
export function startCryptoWasm(): Promise<void> {
  startPromise ??= initCryptoWasm(wasmUrl).then(() => {
    ready = true
  })
  return startPromise
}

export function isCryptoWasmReady(): boolean {
  return ready
}

function requireReady(): void {
  if (!ready) {
    // A connection this early is not a real user flow; failing it is safer than
    // any fallback that could weaken the box.
    throw new Error('E2EE crypto wasm is not initialised yet')
  }
}

/** `publicKey (32) || secretKey (32)` for a 32-byte seed, or undefined. */
export function keyPairFromSeed(seed: Uint8Array): Uint8Array | undefined {
  requireReady()
  return wasmKeyPairFromSeed(seed)
}

/** `nacl.box.before` shared key, or undefined for non-32-byte keys. */
export function deriveSharedKey(
  ourSecretKey: Uint8Array,
  peerPublicKey: Uint8Array
): Uint8Array | undefined {
  requireReady()
  return wasmDeriveSharedKey(ourSecretKey, peerPublicKey)
}

/** `nonce || box` sealed under the raw shared key, or undefined. wasm-bindgen
 *  copies the result into a fresh ArrayBuffer-backed view (a valid BufferSource
 *  for `ws.send`), which the loose lib `Uint8Array` type doesn't narrow to. */
export function sealWithSharedKey(
  sharedKey: Uint8Array,
  nonce: Uint8Array,
  message: Uint8Array
): Uint8Array<ArrayBuffer> | undefined {
  requireReady()
  return wasmSealWithSharedKey(sharedKey, nonce, message) as Uint8Array<ArrayBuffer> | undefined
}

/** Opened plaintext, or undefined on a short bundle / failed tag. */
export function openWithSharedKey(
  sharedKey: Uint8Array,
  bundle: Uint8Array
): Uint8Array<ArrayBuffer> | undefined {
  requireReady()
  return wasmOpenWithSharedKey(sharedKey, bundle) as Uint8Array<ArrayBuffer> | undefined
}

/** Test-only synchronous init from raw wasm bytes: vitest runs under Node,
 *  which has no main-thread sync-compile restriction. */
export function initCryptoWasmForTestFromBytes(bytes: Uint8Array): void {
  initSync({ module: bytes })
  ready = true
}
