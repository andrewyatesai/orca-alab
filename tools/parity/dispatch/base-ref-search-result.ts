// TS dispatch for the base-ref-search-result parity module. The shared TS
// derivation was DELETED (the Rust orca-core port is the sole impl — the
// renderer drives it via the orca-git wasm), so this adapter drives the SAME
// wasm: the vectors' recorded goldens now pin that surface absolutely, and the
// harness's TS-vs-Rust diff degenerates to wasm-vs-binary (drift between the
// two Rust entry points would still surface here).
import { gitWasmOracle } from './orca-git-wasm-oracle'

export function dispatch(fn: string, input: unknown): unknown {
  return JSON.parse(
    gitWasmOracle().orcaDispatch('base-ref-search-result', fn, JSON.stringify(input ?? null))
  )
}
