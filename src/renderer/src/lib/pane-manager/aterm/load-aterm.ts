import init, { AtermTerminal } from './aterm_wasm.js'
import wasmUrl from './aterm_wasm_bg.wasm?url'
import fontUrl from '@renderer/assets/fonts/jetbrains-mono.ttf?url'

export type LoadedAterm = {
  AtermTerminal: typeof AtermTerminal
  fontBytes: Uint8Array
  /** The wasm module's linear memory — ONE shared instance across all panes (the
   *  module is instantiated once). e2e memory bench reads buffer.byteLength to size
   *  per-pane growth; production code never needs it. */
  memory: WebAssembly.Memory
}

// Why: the wasm module and the font are both immutable, shared assets; load
// them once and hand the same result to every pane that opens the aterm path.
let loadPromise: Promise<LoadedAterm> | null = null

async function loadAtermOnce(): Promise<LoadedAterm> {
  const [initOutput, fontResponse] = await Promise.all([init(wasmUrl), fetch(fontUrl)])
  const fontBytes = new Uint8Array(await fontResponse.arrayBuffer())
  return { AtermTerminal, fontBytes, memory: initOutput.memory }
}

export async function loadAterm(): Promise<LoadedAterm> {
  loadPromise ??= loadAtermOnce()
  return loadPromise
}
