import init, { AtermTerminal } from './aterm_wasm.js'
import wasmUrl from './aterm_wasm_bg.wasm?url'
import fontUrl from '@renderer/assets/fonts/jetbrains-mono.ttf?url'

export type LoadedAterm = {
  AtermTerminal: typeof AtermTerminal
  fontBytes: Uint8Array
}

// Why: the wasm module and the font are both immutable, shared assets; load
// them once and hand the same result to every pane that opens the aterm path.
let loadPromise: Promise<LoadedAterm> | null = null

async function loadAtermOnce(): Promise<LoadedAterm> {
  const [, fontResponse] = await Promise.all([init(wasmUrl), fetch(fontUrl)])
  const fontBytes = new Uint8Array(await fontResponse.arrayBuffer())
  return { AtermTerminal, fontBytes }
}

export async function loadAterm(): Promise<LoadedAterm> {
  loadPromise ??= loadAtermOnce()
  return loadPromise
}
