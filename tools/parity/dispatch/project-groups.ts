// TS dispatch for the project-groups parity module. getNextProjectGroupOrder +
// getProjectGroupSubtreeIds were cut over to the Rust core (main via napi, the
// renderer via wasm), so this adapter drives the SAME wasm for them — the
// harness's TS-vs-Rust diff degenerates to wasm-vs-binary and the vectors'
// recorded goldens pin correctness. normalizeProjectGroupName stays live TS
// (still the production impl; create/normalize/clear-membership stay in TS too).
import { normalizeProjectGroupName } from '../../../src/shared/project-groups'
import { gitWasmOracle } from './orca-git-wasm-oracle'

export function dispatch(fn: string, input: unknown): unknown {
  switch (fn) {
    case 'normalizeProjectGroupName': {
      // Absent `fallback` is passed as `undefined`, so the TS default param applies.
      const { name, fallback } = input as { name: string; fallback?: string }
      return normalizeProjectGroupName(name, fallback)
    }
    case 'getNextProjectGroupOrder':
    case 'getProjectGroupSubtreeIds':
      // getProjectGroupSubtreeIds already returns a sorted array from Rust.
      return JSON.parse(
        gitWasmOracle().orcaDispatch('project-groups', fn, JSON.stringify(input ?? null))
      )
    default:
      throw new Error(`unknown function ${fn}`)
  }
}
