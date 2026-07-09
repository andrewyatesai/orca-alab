// TS dispatch for the repo-icon parity module. The shared TS impl was gutted to
// types/data (the Rust repo-icon core is the sole impl — main drives it via
// napi, the renderer via wasm), so this adapter drives the SAME wasm: the
// vectors' recorded goldens pin that surface, and the harness's TS-vs-Rust diff
// degenerates to wasm-vs-binary. The wasm already emits the tri-state
// `sanitizeRepoIcon` result (the `__undefined__` sentinel / null / icon).
import { gitWasmOracle } from './orca-git-wasm-oracle'

export function dispatch(fn: string, input: unknown): unknown {
  return JSON.parse(gitWasmOracle().orcaDispatch('repo-icon', fn, JSON.stringify(input ?? null)))
}
