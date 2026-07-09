// TS dispatch for the repo-badge-color parity module. The shared TS impl was
// gutted to types/data (the Rust repo-badge-color core is the sole impl — main
// drives it via napi, the renderer via wasm), so this adapter drives the SAME
// wasm: the vectors' recorded goldens pin that surface, and the harness's
// TS-vs-Rust diff degenerates to wasm-vs-binary (drift between the two Rust
// entry points would still surface here).
import { gitWasmOracle } from './orca-git-wasm-oracle'

export function dispatch(fn: string, input: unknown): unknown {
  return JSON.parse(
    gitWasmOracle().orcaDispatch('repo-badge-color', fn, JSON.stringify(input ?? null))
  )
}
