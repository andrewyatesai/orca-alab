// TS dispatch for the hook-command-source-policy parity module. The shared TS
// impl was DELETED (the Rust orca-core is the sole impl — main drives it via
// napi, the renderer via wasm), so this adapter drives the SAME wasm: the
// vectors' recorded goldens now pin that surface, and the harness's TS-vs-Rust
// diff degenerates to wasm-vs-binary (drift between the two Rust entry points
// would still surface here).
import { gitWasmOracle } from './orca-git-wasm-oracle'

export function dispatch(fn: string, input: unknown): unknown {
  return JSON.parse(
    gitWasmOracle().orcaDispatch('hook-command-source-policy', fn, JSON.stringify(input ?? null))
  )
}
