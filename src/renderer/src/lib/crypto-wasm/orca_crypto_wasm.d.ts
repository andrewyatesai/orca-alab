/* tslint:disable */
/* eslint-disable */

/**
 * `nacl.box.before`: the 32-byte precomputed shared key from our secret key and
 * a peer's public key. `None` if either key is not 32 bytes.
 */
export function deriveSharedKey(our_secret_key: Uint8Array, peer_public_key: Uint8Array): Uint8Array | undefined;

/**
 * X25519 keypair from a 32-byte secret seed (`nacl.box.keyPair.fromSecretKey`).
 * Returns `publicKey (32) || secretKey (32)` — the JS edge slices it. `None`
 * (→ `undefined`) if the seed is not 32 bytes.
 */
export function keyPairFromSeed(seed: Uint8Array): Uint8Array | undefined;

/**
 * `nacl.box.open.after`: open a `nonce || box` bundle with the raw shared key.
 * `None` if the bundle is too short or the authentication tag fails.
 */
export function openWithSharedKey(shared_key: Uint8Array, bundle: Uint8Array): Uint8Array | undefined;

/**
 * `nacl.box.after`: seal `plaintext` under the raw shared key with an explicit
 * `nonce`, returning `nonce || box`. `None` on a bad nonce length or failure.
 */
export function sealWithSharedKey(shared_key: Uint8Array, nonce: Uint8Array, plaintext: Uint8Array): Uint8Array | undefined;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly deriveSharedKey: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly keyPairFromSeed: (a: number, b: number, c: number) => void;
    readonly openWithSharedKey: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly sealWithSharedKey: (a: number, b: number, c: number, d: number, e: number, f: number, g: number) => void;
    readonly __wbindgen_add_to_stack_pointer: (a: number) => number;
    readonly __wbindgen_export: (a: number, b: number) => number;
    readonly __wbindgen_export2: (a: number, b: number, c: number) => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
