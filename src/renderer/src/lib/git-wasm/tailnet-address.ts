// Renderer tailnet-address classifier, driven by the Rust orca-core core in the
// orca-git wasm module (the shared TS impl was deleted). The sole consumer uses
// this inside a `.find()` with a `?? interfaces[0]` fallback, so returning false
// during the ~tens-of-ms wasm boot window just makes the find miss and degrades
// to the first interface (never broken). The vector input is a bare string.
import { isGitWasmReady } from './git-line-stats'
import { orcaDispatch } from './orca_git_wasm.js'

function op(fn: string, input: unknown): unknown | null {
  if (!isGitWasmReady()) return null
  return JSON.parse(orcaDispatch('tailnet-address', fn, JSON.stringify(input ?? null)))
}

export function isTailnetIPv4Address(address: string): boolean {
  const r = op('isTailnetIPv4Address', address) as boolean | null
  return r ?? false
}
