// TS dispatch for the project-groups parity module. Every function here was cut
// over to the Rust core (main via napi), so this adapter drives the SAME wasm
// for all of them — the harness's TS-vs-Rust diff degenerates to wasm-vs-binary
// and the vectors' recorded goldens pin correctness.
// getEffectiveProjectGroupManualRank + UNGROUPED_PROJECT_GROUP_KEY are the only
// survivors in TS and aren't parity functions (render-hot comparator / const).
import { gitWasmOracle } from './orca-git-wasm-oracle'

const WASM_FUNCTIONS = new Set([
  'normalizeProjectGroupName',
  'createProjectGroup',
  'normalizeProjectGroups',
  'clearMissingProjectGroupMemberships',
  'getNextProjectGroupOrder',
  'getProjectGroupSubtreeIds'
])

export function dispatch(fn: string, input: unknown): unknown {
  if (!WASM_FUNCTIONS.has(fn)) {
    throw new Error(`unknown function ${fn}`)
  }
  return JSON.parse(
    gitWasmOracle().orcaDispatch('project-groups', fn, JSON.stringify(input ?? null))
  )
}
