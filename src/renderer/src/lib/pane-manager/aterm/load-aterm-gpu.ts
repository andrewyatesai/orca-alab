import init, { AtermGpuTerminal } from './aterm_gpu_web.js'
import wasmUrl from './aterm_gpu_web_bg.wasm?url'
import { loadAtermFontBytes } from './load-aterm-font'

export type LoadedAtermGpu = {
  AtermGpuTerminal: typeof AtermGpuTerminal
  fontBytes: Uint8Array
}

// Why: the GPU wasm module and the font are immutable, shared assets; load them
// once and hand the same result to every pane that opens the aterm GPU path.
// Mirrors load-aterm.ts (the CPU path) — same font bytes, same ?url wasm asset,
// so both engines size cells identically and a GPU↔CPU fallback is seamless.
let loadPromise: Promise<LoadedAtermGpu> | null = null

async function loadAtermGpuOnce(): Promise<LoadedAtermGpu> {
  // Share the font fetch with the CPU loader (load-aterm-font) so the face is
  // fetched once and a GPU→CPU swap reuses these bytes instead of re-fetching.
  const [, fontBytes] = await Promise.all([init(wasmUrl), loadAtermFontBytes()])
  return { AtermGpuTerminal, fontBytes }
}

export async function loadAtermGpu(): Promise<LoadedAtermGpu> {
  loadPromise ??= loadAtermGpuOnce()
  return loadPromise
}
