// TS dispatch for the commit-message-generation parity module. The shared TS
// bodies were DELETED (the Rust orca-agents core is the sole impl — napi in main,
// wasm in the renderer's dialog preview), so this adapter drives the SAME wasm:
// the vectors' recorded goldens pin that surface and the harness's TS-vs-Rust
// diff degenerates to wasm-vs-binary (drift between the two Rust entry points
// would still surface here).
import { gitWasmOracle } from './orca-git-wasm-oracle'

export function dispatch(fn: string, input: unknown): unknown {
  return JSON.parse(
    gitWasmOracle().orcaDispatch('commit-message-generation', fn, JSON.stringify(input ?? null))
  )
}
