// TS dispatch for the agent-tab-title parity module. The shared TS derivation
// was DELETED (the Rust orca-text core is the sole impl — the renderer drives
// it via wasm), so this adapter drives the same wasm: the vectors' recorded
// goldens now pin that surface absolutely.
import { gitWasmOracle } from './orca-git-wasm-oracle'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'deriveGeneratedTabTitle':
      // The TS returned null for no-title; the wasm returns undefined.
      return gitWasmOracle().deriveGeneratedTabTitle(input as string) ?? null
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
