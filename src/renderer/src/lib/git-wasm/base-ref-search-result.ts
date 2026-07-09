// Legacy base-ref search-result derivation, driven by the Rust orca-core
// base_ref_search_result port in the orca-git wasm module (the shared TS impl
// was deleted). Both consumers call this via `refs.map(...)` AFTER an RPC
// await, so wasm is ready; the not-ready branch still returns a correct minimal
// object (identity — no known remote prefix stripped) rather than null, so a
// `.map` over refs can never yield null entries during the wasm-boot window.
import { isGitWasmReady } from './git-line-stats'
import { orcaDispatch } from './orca_git_wasm.js'
import type { BaseRefSearchResult } from '../../../../shared/types'

function op(fn: string, input: unknown): unknown | null {
  if (!isGitWasmReady()) return null
  return JSON.parse(orcaDispatch('base-ref-search-result', fn, JSON.stringify(input ?? null)))
}

export function legacyBaseRefSearchResult(refName: string): BaseRefSearchResult {
  const r = op('legacyBaseRefSearchResult', refName) as BaseRefSearchResult | null
  return r ?? { refName, localBranchName: refName }
}
