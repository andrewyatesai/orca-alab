// The renderer's line-stats computation, driven by the orca-git Rust core
// compiled to wasm (rust/orca-git-wasm) — the same module the SSH relay embeds
// and the same `line_count.rs` the main process runs via napi. The renderer has
// no napi access (sandbox: true), so it loads the wasm via vite's `?url` asset
// + async init exactly like the aterm engine (no sync-compile on the Chromium
// main thread, no base64 bundle bloat).
import initGitWasm, {
  computeLineStats as wasmComputeLineStats,
  initSync,
  orcaDispatch as wasmOrcaDispatch
} from './orca_git_wasm.js'
import wasmUrl from './orca_git_wasm_bg.wasm?url'
import { setOrcaDispatchBinding } from '../../../../shared/orca-dispatch-seam'

export type DiffLineStats = { added: number; removed: number }

let ready = false
let startPromise: Promise<void> | null = null
const readyListeners = new Set<() => void>()

function markReady(): void {
  ready = true
  // Bind the shared dispatch seam now that wasm is initialised, so src/shared
  // modules cut over to Rust reach the core. Before this, tryOrcaDispatch returns
  // null and shared callers use their safe fallback. Fires in production
  // (startGitWasm) and tests (initGitWasmForTestFromBytes).
  setOrcaDispatchBinding((module, fn, inputJson) => wasmOrcaDispatch(module, fn, inputJson))
  for (const listener of readyListeners) {
    listener()
  }
}

/** Kick off the async wasm init (idempotent). Called once from the renderer
 *  bootstrap so the module is compiled long before any diff section renders. */
export function startGitWasm(): Promise<void> {
  startPromise ??= initGitWasm({ module_or_path: wasmUrl }).then(markReady)
  return startPromise
}

export function isGitWasmReady(): boolean {
  return ready
}

/** For useSyncExternalStore: re-render consumers when the wasm becomes ready. */
export function subscribeGitWasmReady(listener: () => void): () => void {
  readyListeners.add(listener)
  return () => {
    readyListeners.delete(listener)
  }
}

/** Test-only synchronous init from raw wasm bytes: vitest runs under Node,
 *  which has no main-thread sync-compile restriction. */
export function initGitWasmForTestFromBytes(bytes: Uint8Array): void {
  initSync({ module: bytes })
  markReady()
}

/**
 * Compute approximate added/removed line counts for a diff section (multiset
 * line matching in Rust). Returns null while the wasm is still initialising —
 * consumers fall back to the numstat-derived section counts and recompute via
 * `subscribeGitWasmReady` — and null for the >500k-char large-input guard
 * (splitting that in a React render would block the UI).
 */
export function computeLineStats(
  original: string,
  modified: string,
  status: string
): DiffLineStats | null {
  if (!ready) {
    return null
  }
  const json = wasmComputeLineStats(original, modified, status)
  return json === undefined ? null : (JSON.parse(json) as DiffLineStats)
}
