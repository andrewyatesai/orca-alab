// TS dispatch for the github-pr-merge-methods parity module. The shared TS impl
// was gutted to types + label data (the Rust github-pr-merge-methods core is the
// sole impl — main drives it via napi, the renderer via wasm), so this adapter
// drives the SAME wasm: the vectors' recorded goldens now pin that surface, and
// the harness's TS-vs-Rust diff degenerates to wasm-vs-binary.
import { gitWasmOracle } from './orca-git-wasm-oracle'

export function dispatch(fn: string, input: unknown): unknown {
  return JSON.parse(
    gitWasmOracle().orcaDispatch('github-pr-merge-methods', fn, JSON.stringify(input ?? null))
  )
}
