// TS dispatch for the agent-notification-id parity module. The shared TS
// derivation was DELETED (the Rust orca-core is the sole impl — the renderer
// drives it via wasm), so this adapter drives the SAME wasm: the vectors'
// recorded goldens now pin that surface absolutely, and the harness's TS-vs-Rust
// diff degenerates to wasm-vs-binary (drift between the two Rust entry points
// would still surface here). `null` (no id) round-trips through JSON.parse.
import { gitWasmOracle } from './orca-git-wasm-oracle'

export function dispatch(fn: string, input: unknown): unknown {
  return JSON.parse(
    gitWasmOracle().orcaDispatch('agent-notification-id', fn, JSON.stringify(input ?? null))
  )
}
