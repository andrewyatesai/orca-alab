// TS dispatch for the tailnet-address parity module. The shared TS impl was
// DELETED (the Rust orca-core is the sole impl — main drives it via napi, the
// renderer via wasm), so this adapter drives the SAME wasm: the vectors'
// recorded goldens now pin that surface, and the harness's TS-vs-Rust diff
// degenerates to wasm-vs-binary (drift between the two Rust entry points would
// still surface here). The vector input is a bare string.
import { gitWasmOracle } from './orca-git-wasm-oracle'

export function dispatch(fn: string, input: unknown): unknown {
  return JSON.parse(gitWasmOracle().orcaDispatch('tailnet-address', fn, JSON.stringify(input ?? null)))
}
